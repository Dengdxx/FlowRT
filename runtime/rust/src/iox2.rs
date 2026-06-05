//! 可选 iceoryx2 transport 的最小 typed pub/sub 支持。
//!
//! 该模块只在启用 `iox2` feature 时编译。它用于验证 FlowRT Message ABI plain-data
//! payload 可以通过 iceoryx2 传输；用户算法代码仍不应直接依赖本模块。

use std::time::Duration;

use iceoryx2::prelude::*;
use iceoryx2::sample::Sample;

use crate::{
    BackendHealthSnapshot, BackendHealthState, BackendHealthTracker, Latest, OverflowPolicy,
    ReconnectPolicy, StaleConfig, StalePolicy, WireCodecError,
};

type IpcNode = Node<ipc::Service>;
type IpcPublisher<T> = iceoryx2::port::publisher::Publisher<ipc::Service, T, FlowrtIox2Header>;
type IpcSubscriber<T> = iceoryx2::port::subscriber::Subscriber<ipc::Service, T, FlowrtIox2Header>;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, ZeroCopySend)]
#[type_name("FlowRTIox2Header")]
struct FlowrtIox2Header {
    published_at_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct Iox2Received<T>
where
    T: Copy,
{
    published_at_ms: u64,
    payload: T,
}

#[derive(Debug, Clone)]
struct Iox2FrameReceived<T>
where
    T: Clone,
{
    published_at_ms: u64,
    payload: T,
}

struct Iox2EndpointParts<T>
where
    T: std::fmt::Debug + ZeroCopySend + 'static,
{
    publisher: IpcPublisher<T>,
    subscriber: IpcSubscriber<T>,
    node: IpcNode,
}

/// 可选 iceoryx2 transport helper 返回的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iox2Error {
    message: String,
}

impl Iox2Error {
    fn new(context: &str, error: impl std::fmt::Debug) -> Self {
        Self {
            message: format!("{context}: {error:?}"),
        }
    }

    /// 返回错误消息。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for Iox2Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Iox2Error {}

/// iox2 有界变长消息 transport slot。
///
/// 生成的变长消息本体可以包含 `Vec`/`String` 等动态所有权字段，不能直接作为 iox2 typed
/// payload。codegen 会为每个变长消息生成一个固定最大容量的 slot，并通过该 trait 在用户
/// 消息和 canonical frame bytes 之间转换。
pub trait Iox2FrameSlot<T>: Copy {
    /// 将用户消息编码成固定容量 slot。
    fn try_from_message(value: &T) -> Result<Self, WireCodecError>;

    /// 将 slot 中的 canonical frame 解码回用户消息。
    fn decode_message(&self) -> Result<T, WireCodecError>;
}

/// 打开 iceoryx2 publish-subscribe endpoint 时使用的 QoS 配置。
///
/// 配置来自 Contract IR channel policy 的归一化结果。当前映射覆盖 depth、overflow 和
/// freshness intent，后续 backend contract 扩展时应继续保持从语义到 transport 参数的显式映射。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Iox2ChannelConfig {
    depth: usize,
    overflow: OverflowPolicy,
    stale: StaleConfig,
}

impl Iox2ChannelConfig {
    /// 构造 latest channel 的默认 QoS 配置。
    pub fn latest() -> Self {
        Self {
            depth: 1,
            overflow: OverflowPolicy::DropOldest,
            stale: StaleConfig::default(),
        }
    }

    /// 构造 FIFO channel 的 QoS 配置；`depth` 为 0 时按 1 处理。
    pub fn fifo(depth: usize, overflow: OverflowPolicy) -> Self {
        Self {
            depth: depth.max(1),
            overflow,
            stale: StaleConfig::default(),
        }
    }

    /// 设置 freshness 配置。
    pub fn with_stale_config(mut self, stale: StaleConfig) -> Self {
        self.stale = stale;
        self
    }

    /// 返回归一化后的 channel depth。
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// 返回 overflow policy。
    pub fn overflow(&self) -> OverflowPolicy {
        self.overflow
    }

    /// 返回 stale-data 配置。
    pub fn stale(&self) -> StaleConfig {
        self.stale
    }

    fn safe_overflow(&self) -> bool {
        !matches!(self.overflow, OverflowPolicy::Block)
    }

    fn backpressure_strategy(&self) -> BackpressureStrategy {
        match self.overflow {
            OverflowPolicy::Block => BackpressureStrategy::RetryUntilDelivered,
            OverflowPolicy::DropOldest | OverflowPolicy::DropNewest | OverflowPolicy::Error => {
                BackpressureStrategy::DiscardData
            }
        }
    }
}

impl Default for Iox2ChannelConfig {
    fn default() -> Self {
        Self::latest()
    }
}

/// FlowRT ABI plain-data payload 的 typed publish-subscribe endpoint。
///
/// `T` 必须是满足 FlowRT Message ABI v0.1 的 plain-data 类型，并实现 iceoryx2 的
/// `ZeroCopySend`。该类型是 backend 层实现细节，生成 shell 可以使用它，用户组件接口不应暴露它。
pub struct Iox2PubSub<T>
where
    T: std::fmt::Debug + Copy + ZeroCopySend + 'static,
{
    service_name: String,
    publisher: Option<IpcPublisher<T>>,
    subscriber: Option<IpcSubscriber<T>>,
    node: Option<IpcNode>,
    config: Iox2ChannelConfig,
    stale: StaleConfig,
    health: BackendHealthTracker,
    received: Option<Iox2Received<T>>,
}

/// 使用固定容量 canonical frame slot 的 iox2 publish-subscribe endpoint。
///
/// `T` 是用户组件看到的结构化消息类型；`S` 是 codegen 生成的 fixed-size iox2 payload slot。
/// 该类型保持用户 API 与 transport ABI 解耦，并让 bounded variable frame 可以继续走 iox2
/// typed IPC service。
pub struct Iox2FramePubSub<T, S>
where
    T: Clone,
    S: Iox2FrameSlot<T> + std::fmt::Debug + Copy + ZeroCopySend + 'static,
{
    service_name: String,
    publisher: Option<IpcPublisher<S>>,
    subscriber: Option<IpcSubscriber<S>>,
    node: Option<IpcNode>,
    config: Iox2ChannelConfig,
    stale: StaleConfig,
    health: BackendHealthTracker,
    received: Option<Iox2FrameReceived<T>>,
}

impl<T, S> Iox2FramePubSub<T, S>
where
    T: Clone,
    S: Iox2FrameSlot<T> + std::fmt::Debug + Copy + ZeroCopySend + 'static,
{
    /// 使用显式 QoS 配置打开或创建一个本机 IPC publish-subscribe service。
    pub fn open_with_config(
        service_name: &str,
        config: Iox2ChannelConfig,
    ) -> Result<Self, Iox2Error> {
        let parts = open_iox2_parts(service_name, config)?;

        Ok(Self {
            service_name: service_name.to_string(),
            publisher: Some(parts.publisher),
            subscriber: Some(parts.subscriber),
            node: Some(parts.node),
            config,
            stale: config.stale(),
            health: BackendHealthTracker::new(ReconnectPolicy::default()),
            received: None,
        })
    }

    /// 带 FlowRT runtime 时间戳发布一个结构化变长消息。
    pub fn publish_at(&mut self, value: T, published_at_ms: u64) -> Result<(), Iox2Error> {
        let slot = S::try_from_message(&value)
            .map_err(|error| Iox2Error::new("encode FlowRT iox2 frame payload", error))?;
        self.ensure_ready("publish iceoryx2 frame sample")?;
        match self.publish_slot(slot, published_at_ms) {
            Ok(()) => {
                self.health.mark_ready();
                Ok(())
            }
            Err(error) => {
                self.mark_transport_error(&error);
                self.recover_after_transport_error("publish iceoryx2 frame sample")?;
                self.publish_slot(slot, published_at_ms)
                    .inspect(|_| self.health.mark_ready())
                    .inspect_err(|error| self.mark_transport_error(error))
            }
        }
    }

    fn publish_slot(&self, slot: S, published_at_ms: u64) -> Result<(), Iox2Error> {
        let publisher = self.publisher.as_ref().ok_or_else(|| {
            Iox2Error::new("publish iceoryx2 frame sample", "endpoint is not ready")
        })?;
        let sample = publisher
            .loan_uninit()
            .map_err(|error| Iox2Error::new("failed to loan iceoryx2 frame sample", error))?;
        let mut sample = sample;
        sample.user_header_mut().published_at_ms = published_at_ms;
        sample
            .write_payload(slot)
            .send()
            .map_err(|error| Iox2Error::new("failed to send iceoryx2 frame sample", error))?;
        Ok(())
    }

    /// 接收一个结构化消息，并根据 transport 时间戳暴露 freshness 状态。
    pub fn receive_latest_at(&mut self, now_ms: u64) -> Result<Latest<'_, T>, Iox2Error> {
        self.ensure_ready("receive iceoryx2 frame sample")?;
        if let Some(sample) = self.try_receive_frame_with_recovery()? {
            self.received = Some(Iox2FrameReceived {
                published_at_ms: sample.user_header().published_at_ms,
                payload: (*sample)
                    .decode_message()
                    .map_err(|error| Iox2Error::new("decode FlowRT iox2 frame payload", error))?,
            });
        }

        let stale = self
            .received
            .as_ref()
            .map(|sample| self.stale.stale_at(Some(sample.published_at_ms), now_ms))
            .unwrap_or(false);
        let value = if stale && self.stale.policy() == StalePolicy::Drop {
            None
        } else {
            self.received.as_ref().map(|sample| &sample.payload)
        };

        Ok(Latest::new(value, stale))
    }

    /// 返回 endpoint 的 QoS 配置。
    pub fn config(&self) -> Iox2ChannelConfig {
        self.config
    }

    /// 判断底层 iox2 endpoint 是否已经打开。
    pub fn ready(&self) -> bool {
        self.node.is_some() && self.publisher.is_some() && self.subscriber.is_some()
    }

    /// 返回 endpoint 健康快照。
    pub fn health(&self) -> BackendHealthSnapshot {
        if !self.ready() && self.health.snapshot().state == BackendHealthState::Ready {
            return BackendHealthSnapshot {
                state: BackendHealthState::Degraded,
                last_error: Some("iceoryx2 endpoint is not ready".to_string()),
                attempt: 0,
                next_retry_unix_ms: None,
                recoverable: true,
            };
        }
        self.health.snapshot()
    }

    /// 返回 endpoint 重连策略。
    pub fn reconnect_policy(&self) -> ReconnectPolicy {
        self.health.policy()
    }

    /// 短暂等待，让 iceoryx2 推进本机 endpoint 状态。
    pub fn poll_once(&self, timeout: Duration) -> Result<(), Iox2Error> {
        self.node
            .as_ref()
            .ok_or_else(|| Iox2Error::new("poll iceoryx2 frame node", "endpoint is not ready"))?
            .wait(timeout)
            .map_err(|error| Iox2Error::new("failed to poll iceoryx2 frame node", error))
    }

    fn try_receive_frame(
        &self,
    ) -> Result<Option<Sample<ipc::Service, S, FlowrtIox2Header>>, Iox2Error> {
        let subscriber = self.subscriber.as_ref().ok_or_else(|| {
            Iox2Error::new("receive iceoryx2 frame sample", "endpoint is not ready")
        })?;
        subscriber
            .receive()
            .map_err(|error| Iox2Error::new("failed to receive iceoryx2 frame sample", error))
    }

    fn try_receive_frame_with_recovery(
        &mut self,
    ) -> Result<Option<Sample<ipc::Service, S, FlowrtIox2Header>>, Iox2Error> {
        match self.try_receive_frame() {
            Ok(sample) => {
                self.health.mark_ready();
                Ok(sample)
            }
            Err(error) => {
                self.mark_transport_error(&error);
                self.recover_after_transport_error("receive iceoryx2 frame sample")?;
                self.try_receive_frame()
                    .inspect(|_| self.health.mark_ready())
                    .inspect_err(|error| self.mark_transport_error(error))
            }
        }
    }

    fn ensure_ready(&mut self, operation: &'static str) -> Result<(), Iox2Error> {
        if self.ready() {
            return Ok(());
        }
        if self.health.snapshot().state != BackendHealthState::Reconnecting {
            self.health
                .mark_degraded(format!("{operation}: endpoint is not ready"));
        }
        self.recover_after_transport_error(operation)
    }

    fn mark_transport_error(&mut self, error: &Iox2Error) {
        self.health.mark_degraded(error.to_string());
    }

    fn recover_after_transport_error(&mut self, operation: &'static str) -> Result<(), Iox2Error> {
        recover_iox2_endpoint(
            operation,
            &self.service_name,
            self.config,
            &mut self.publisher,
            &mut self.subscriber,
            &mut self.node,
            &mut self.health,
        )
    }
}

impl<T> Iox2PubSub<T>
where
    T: std::fmt::Debug + Copy + ZeroCopySend + 'static,
{
    /// 打开或创建一个本机 IPC publish-subscribe service。
    pub fn open(service_name: &str) -> Result<Self, Iox2Error> {
        Self::open_with_config(service_name, Iox2ChannelConfig::latest())
    }

    /// 使用显式 QoS 配置打开或创建一个本机 IPC publish-subscribe service。
    pub fn open_with_config(
        service_name: &str,
        config: Iox2ChannelConfig,
    ) -> Result<Self, Iox2Error> {
        let parts = open_iox2_parts(service_name, config)?;

        Ok(Self {
            service_name: service_name.to_string(),
            publisher: Some(parts.publisher),
            subscriber: Some(parts.subscriber),
            node: Some(parts.node),
            config,
            stale: config.stale(),
            health: BackendHealthTracker::new(ReconnectPolicy::default()),
            received: None,
        })
    }

    /// 通过 iceoryx2 loaned sample 发布一个值。
    pub fn publish(&mut self, value: T) -> Result<(), Iox2Error> {
        self.publish_at(value, 0)
    }

    /// 带 FlowRT runtime 时间戳发布一个值。
    pub fn publish_at(&mut self, value: T, published_at_ms: u64) -> Result<(), Iox2Error> {
        self.ensure_ready("publish iceoryx2 sample")?;
        match self.publish_slot(value, published_at_ms) {
            Ok(()) => {
                self.health.mark_ready();
                Ok(())
            }
            Err(error) => {
                self.mark_transport_error(&error);
                self.recover_after_transport_error("publish iceoryx2 sample")?;
                self.publish_slot(value, published_at_ms)
                    .inspect(|_| self.health.mark_ready())
                    .inspect_err(|error| self.mark_transport_error(error))
            }
        }
    }

    fn publish_slot(&self, value: T, published_at_ms: u64) -> Result<(), Iox2Error> {
        let publisher = self
            .publisher
            .as_ref()
            .ok_or_else(|| Iox2Error::new("publish iceoryx2 sample", "endpoint is not ready"))?;
        let sample = publisher
            .loan_uninit()
            .map_err(|error| Iox2Error::new("failed to loan iceoryx2 sample", error))?;
        let mut sample = sample;
        sample.user_header_mut().published_at_ms = published_at_ms;
        sample
            .write_payload(value)
            .send()
            .map_err(|error| Iox2Error::new("failed to send iceoryx2 sample", error))?;
        Ok(())
    }

    /// 如果有可用样本，则接收一个值。
    pub fn receive(&mut self) -> Result<Option<T>, Iox2Error> {
        self.ensure_ready("receive iceoryx2 sample")?;
        self.try_receive_sample_with_recovery()
            .map(|sample| sample.map(|sample| *sample))
    }

    /// 接收一个值，并根据 transport 时间戳暴露 freshness 状态。
    pub fn receive_latest_at(&mut self, now_ms: u64) -> Result<Latest<'_, T>, Iox2Error> {
        self.ensure_ready("receive iceoryx2 sample")?;
        if let Some(sample) = self.try_receive_sample_with_recovery()? {
            self.received = Some(Iox2Received {
                published_at_ms: sample.user_header().published_at_ms,
                payload: *sample,
            });
        }

        let stale = self
            .received
            .map(|sample| self.stale.stale_at(Some(sample.published_at_ms), now_ms))
            .unwrap_or(false);
        let value = if stale && self.stale.policy() == StalePolicy::Drop {
            None
        } else {
            self.received.as_ref().map(|sample| &sample.payload)
        };

        Ok(Latest::new(value, stale))
    }

    /// 返回 endpoint 的 QoS 配置。
    pub fn config(&self) -> Iox2ChannelConfig {
        self.config
    }

    /// 判断底层 iox2 endpoint 是否已经打开。
    pub fn ready(&self) -> bool {
        self.node.is_some() && self.publisher.is_some() && self.subscriber.is_some()
    }

    /// 返回 endpoint 健康快照。
    pub fn health(&self) -> BackendHealthSnapshot {
        if !self.ready() && self.health.snapshot().state == BackendHealthState::Ready {
            return BackendHealthSnapshot {
                state: BackendHealthState::Degraded,
                last_error: Some("iceoryx2 endpoint is not ready".to_string()),
                attempt: 0,
                next_retry_unix_ms: None,
                recoverable: true,
            };
        }
        self.health.snapshot()
    }

    /// 返回 endpoint 重连策略。
    pub fn reconnect_policy(&self) -> ReconnectPolicy {
        self.health.policy()
    }

    /// 短暂等待，让 iceoryx2 推进本机 endpoint 状态。
    pub fn poll_once(&self, timeout: Duration) -> Result<(), Iox2Error> {
        self.node
            .as_ref()
            .ok_or_else(|| Iox2Error::new("poll iceoryx2 node", "endpoint is not ready"))?
            .wait(timeout)
            .map_err(|error| Iox2Error::new("failed to poll iceoryx2 node", error))
    }

    fn try_receive_sample(
        &self,
    ) -> Result<Option<Sample<ipc::Service, T, FlowrtIox2Header>>, Iox2Error> {
        let subscriber = self
            .subscriber
            .as_ref()
            .ok_or_else(|| Iox2Error::new("receive iceoryx2 sample", "endpoint is not ready"))?;
        subscriber
            .receive()
            .map_err(|error| Iox2Error::new("failed to receive iceoryx2 sample", error))
    }

    fn try_receive_sample_with_recovery(
        &mut self,
    ) -> Result<Option<Sample<ipc::Service, T, FlowrtIox2Header>>, Iox2Error> {
        match self.try_receive_sample() {
            Ok(sample) => {
                self.health.mark_ready();
                Ok(sample)
            }
            Err(error) => {
                self.mark_transport_error(&error);
                self.recover_after_transport_error("receive iceoryx2 sample")?;
                self.try_receive_sample()
                    .inspect(|_| self.health.mark_ready())
                    .inspect_err(|error| self.mark_transport_error(error))
            }
        }
    }

    fn ensure_ready(&mut self, operation: &'static str) -> Result<(), Iox2Error> {
        if self.ready() {
            return Ok(());
        }
        if self.health.snapshot().state != BackendHealthState::Reconnecting {
            self.health
                .mark_degraded(format!("{operation}: endpoint is not ready"));
        }
        self.recover_after_transport_error(operation)
    }

    fn mark_transport_error(&mut self, error: &Iox2Error) {
        self.health.mark_degraded(error.to_string());
    }

    fn recover_after_transport_error(&mut self, operation: &'static str) -> Result<(), Iox2Error> {
        recover_iox2_endpoint(
            operation,
            &self.service_name,
            self.config,
            &mut self.publisher,
            &mut self.subscriber,
            &mut self.node,
            &mut self.health,
        )
    }

    #[cfg(test)]
    fn reset_transport_for_test(&mut self) {
        self.publisher = None;
        self.subscriber = None;
        self.node = None;
        self.health.mark_degraded("iceoryx2 endpoint reset by test");
    }
}

fn open_iox2_parts<T>(
    service_name: &str,
    config: Iox2ChannelConfig,
) -> Result<Iox2EndpointParts<T>, Iox2Error>
where
    T: std::fmt::Debug + ZeroCopySend + 'static,
{
    let service_name = service_name
        .try_into()
        .map_err(|error| Iox2Error::new("invalid iceoryx2 service name", error))?;
    let node = NodeBuilder::new()
        .create::<ipc::Service>()
        .map_err(|error| Iox2Error::new("failed to create iceoryx2 node", error))?;
    let service = node
        .service_builder(&service_name)
        .publish_subscribe::<T>()
        .user_header::<FlowrtIox2Header>()
        .enable_safe_overflow(config.safe_overflow())
        .history_size(config.depth())
        .subscriber_max_buffer_size(config.depth())
        .open_or_create()
        .map_err(|error| {
            Iox2Error::new("failed to open or create iceoryx2 pubsub service", error)
        })?;
    let publisher = service
        .publisher_builder()
        .backpressure_strategy(config.backpressure_strategy())
        .create()
        .map_err(|error| Iox2Error::new("failed to create iceoryx2 publisher", error))?;
    let subscriber = service
        .subscriber_builder()
        .buffer_size(config.depth())
        .create()
        .map_err(|error| Iox2Error::new("failed to create iceoryx2 subscriber", error))?;

    Ok(Iox2EndpointParts {
        publisher,
        subscriber,
        node,
    })
}

fn recover_iox2_endpoint<T>(
    operation: &'static str,
    service_name: &str,
    config: Iox2ChannelConfig,
    publisher: &mut Option<IpcPublisher<T>>,
    subscriber: &mut Option<IpcSubscriber<T>>,
    node: &mut Option<IpcNode>,
    health: &mut BackendHealthTracker,
) -> Result<(), Iox2Error>
where
    T: std::fmt::Debug + ZeroCopySend + 'static,
{
    let snapshot = health.snapshot();
    if let Some(next_retry_unix_ms) = snapshot.next_retry_unix_ms
        && snapshot.state == BackendHealthState::Reconnecting
        && unix_now_ms() < next_retry_unix_ms
    {
        return Err(Iox2Error::new(
            operation,
            "iceoryx2 endpoint reconnect backoff is active",
        ));
    }

    let attempt = snapshot.attempt;
    if !health.policy().can_retry(attempt) {
        health.mark_failed("iceoryx2 endpoint reconnect budget exhausted", attempt);
        return Err(Iox2Error::new(
            operation,
            "iceoryx2 endpoint reconnect budget exhausted",
        ));
    }

    let now_ms = unix_now_ms();
    health.mark_reconnecting(
        attempt,
        now_ms.saturating_add(health.policy().delay_for_attempt(attempt)),
    );
    *publisher = None;
    *subscriber = None;
    *node = None;
    match open_iox2_parts(service_name, config) {
        Ok(parts) => {
            *publisher = Some(parts.publisher);
            *subscriber = Some(parts.subscriber);
            *node = Some(parts.node);
            health.mark_ready();
            Ok(())
        }
        Err(error) => {
            let next_attempt = attempt.saturating_add(1);
            if health.policy().can_retry(next_attempt) {
                health.mark_reconnecting(
                    next_attempt,
                    now_ms.saturating_add(health.policy().delay_for_attempt(next_attempt)),
                );
            } else {
                health.mark_failed(error.to_string(), next_attempt);
            }
            Err(error)
        }
    }
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// 通过 iceoryx2 运行一个单进程 publish/subscribe roundtrip。
pub fn smoke_pubsub<T>(service_name: &str, value: T) -> Result<T, Iox2Error>
where
    T: std::fmt::Debug + Copy + ZeroCopySend + 'static,
{
    let mut endpoint = Iox2PubSub::<T>::open(service_name)?;
    endpoint.publish(value)?;
    endpoint.poll_once(Duration::from_millis(1))?;
    match endpoint.receive()? {
        Some(sample) => Ok(sample),
        None => Err(Iox2Error {
            message: "iceoryx2 smoke sample was not received".to_string(),
        }),
    }
}

/// 通过 iceoryx2 运行一个单进程 `u64` publish/subscribe roundtrip。
pub fn smoke_pubsub_u64(service_name: &str, value: u64) -> Result<u64, Iox2Error> {
    smoke_pubsub(service_name, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, ZeroCopySend)]
    struct Iox2SmokeMessage {
        timestamp: u64,
        x: f32,
        y: f32,
    }

    #[test]
    fn flowrt_iox2_header_has_stable_cross_language_type_name() {
        assert_eq!(unsafe { FlowrtIox2Header::type_name() }, "FlowRTIox2Header");
    }

    #[test]
    fn smoke_pubsub_u64_roundtrips_payload() {
        let received = smoke_pubsub_u64("FlowRT/Smoke/PubSubU64", 42)
            .expect("iceoryx2 pubsub smoke should roundtrip");
        assert_eq!(received, 42);
    }

    #[test]
    fn smoke_pubsub_roundtrips_flowrt_abi_message() {
        let message = Iox2SmokeMessage {
            timestamp: 7,
            x: 1.25,
            y: -2.5,
        };
        let received = smoke_pubsub("FlowRT/Smoke/AbiMessage", message)
            .expect("iceoryx2 pubsub should roundtrip a FlowRT ABI plain-data message");
        assert_eq!(received, message);
    }

    #[test]
    fn typed_pubsub_endpoint_roundtrips_flowrt_abi_message() {
        let mut endpoint = Iox2PubSub::<Iox2SmokeMessage>::open("FlowRT/Smoke/TypedEndpoint")
            .expect("typed iceoryx2 endpoint should open");
        let message = Iox2SmokeMessage {
            timestamp: 11,
            x: 3.5,
            y: -8.25,
        };

        endpoint
            .publish(message)
            .expect("typed iceoryx2 endpoint should publish");
        let received = endpoint
            .receive()
            .expect("typed iceoryx2 endpoint should receive")
            .expect("typed iceoryx2 endpoint should have a sample");

        assert_eq!(received, message);
    }

    #[test]
    fn typed_pubsub_endpoint_accepts_fifo_qos_config() {
        let mut endpoint = Iox2PubSub::<Iox2SmokeMessage>::open_with_config(
            "FlowRT/Smoke/FifoQos",
            Iox2ChannelConfig::fifo(8, crate::OverflowPolicy::DropOldest),
        )
        .expect("typed iceoryx2 endpoint should open with FIFO QoS");

        let message = Iox2SmokeMessage {
            timestamp: 12,
            x: 5.0,
            y: -1.0,
        };
        endpoint
            .publish(message)
            .expect("typed iceoryx2 endpoint should publish with FIFO QoS");

        assert_eq!(
            endpoint
                .receive()
                .expect("typed iceoryx2 endpoint should receive with FIFO QoS"),
            Some(message)
        );
    }

    #[test]
    fn typed_pubsub_endpoint_marks_received_payload_stale() {
        let mut endpoint = Iox2PubSub::<Iox2SmokeMessage>::open_with_config(
            "FlowRT/Smoke/StaleWarn",
            Iox2ChannelConfig::latest()
                .with_stale_config(crate::StaleConfig::new(Some(10), crate::StalePolicy::Warn)),
        )
        .expect("typed iceoryx2 endpoint should open with stale config");
        let message = Iox2SmokeMessage {
            timestamp: 13,
            x: 8.0,
            y: -3.0,
        };

        endpoint
            .publish_at(message, 100)
            .expect("typed iceoryx2 endpoint should publish with timestamp");
        let received = endpoint
            .receive_latest_at(111)
            .expect("typed iceoryx2 endpoint should receive latest view");

        assert_eq!(received.as_ref(), Some(&message));
        assert!(received.stale());
    }

    #[test]
    fn typed_pubsub_endpoint_drops_stale_payload_when_policy_is_drop() {
        let mut endpoint = Iox2PubSub::<Iox2SmokeMessage>::open_with_config(
            "FlowRT/Smoke/StaleDrop",
            Iox2ChannelConfig::latest()
                .with_stale_config(crate::StaleConfig::new(Some(10), crate::StalePolicy::Drop)),
        )
        .expect("typed iceoryx2 endpoint should open with stale drop config");
        let message = Iox2SmokeMessage {
            timestamp: 14,
            x: 9.0,
            y: -4.0,
        };

        endpoint
            .publish_at(message, 100)
            .expect("typed iceoryx2 endpoint should publish with timestamp");
        let received = endpoint
            .receive_latest_at(111)
            .expect("typed iceoryx2 endpoint should receive latest view");

        assert!(received.as_ref().is_none());
        assert!(received.stale());
    }

    #[test]
    fn typed_pubsub_endpoint_exposes_hold_last_stale_policy_configuration() {
        let endpoint = Iox2PubSub::<Iox2SmokeMessage>::open_with_config(
            "FlowRT/Smoke/StaleHoldLast",
            Iox2ChannelConfig::latest().with_stale_config(crate::StaleConfig::new(
                Some(10),
                crate::StalePolicy::HoldLast,
            )),
        )
        .expect("typed iceoryx2 endpoint should open with hold-last config");

        assert_eq!(endpoint.stale.policy(), crate::StalePolicy::HoldLast);
        assert_eq!(endpoint.stale.max_age_ms(), Some(10));
    }

    #[test]
    fn typed_pubsub_endpoint_exposes_block_overflow_configuration() {
        let endpoint = Iox2PubSub::<Iox2SmokeMessage>::open_with_config(
            "FlowRT/Smoke/OverflowBlock",
            Iox2ChannelConfig::fifo(0, crate::OverflowPolicy::Block),
        )
        .expect("typed iceoryx2 endpoint should open with block overflow config");

        assert_eq!(endpoint.config().depth(), 1);
        assert_eq!(endpoint.config().overflow(), crate::OverflowPolicy::Block);
    }

    #[test]
    fn typed_pubsub_endpoint_receives_after_peer_endpoint_restarts() {
        let service_name = "FlowRT/Smoke/PeerRestart";
        let mut receiver = Iox2PubSub::<Iox2SmokeMessage>::open(service_name)
            .expect("receiver endpoint should open");
        {
            let mut sender = Iox2PubSub::<Iox2SmokeMessage>::open(service_name)
                .expect("first sender endpoint should open");
            let message = Iox2SmokeMessage {
                timestamp: 21,
                x: 1.0,
                y: 2.0,
            };
            sender
                .publish_at(message, 100)
                .expect("first sender should publish");
            receiver
                .poll_once(Duration::from_millis(1))
                .expect("receiver node should poll after first publish");
            assert_eq!(
                receiver
                    .receive_latest_at(101)
                    .expect("receiver should receive from first sender")
                    .as_ref(),
                Some(&message)
            );
        }

        let mut sender = Iox2PubSub::<Iox2SmokeMessage>::open(service_name)
            .expect("restarted sender endpoint should open");
        let message = Iox2SmokeMessage {
            timestamp: 22,
            x: 3.0,
            y: 4.0,
        };
        sender
            .publish_at(message, 110)
            .expect("restarted sender should publish");
        receiver
            .poll_once(Duration::from_millis(1))
            .expect("receiver node should poll after restarted publish");
        assert_eq!(
            receiver
                .receive_latest_at(111)
                .expect("receiver should receive from restarted sender")
                .as_ref(),
            Some(&message)
        );
    }

    #[test]
    fn typed_pubsub_endpoint_recovers_after_local_transport_is_reset() {
        let mut endpoint = Iox2PubSub::<Iox2SmokeMessage>::open("FlowRT/Smoke/TypedRecovery")
            .expect("typed endpoint should open before forced reset");
        assert_eq!(endpoint.health().state, BackendHealthState::Ready);

        endpoint.reset_transport_for_test();
        assert_eq!(endpoint.health().state, BackendHealthState::Degraded);

        let message = Iox2SmokeMessage {
            timestamp: 31,
            x: 5.0,
            y: 6.0,
        };
        endpoint
            .publish_at(message, 200)
            .expect("publish should reopen a reset iox2 endpoint");

        assert_eq!(endpoint.health().state, BackendHealthState::Ready);
    }

    #[derive(Debug, Clone, PartialEq)]
    struct FrameSmokeMessage {
        value: u32,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy, ZeroCopySend)]
    #[type_name("FlowRTIox2DecodeFailingSlot")]
    struct DecodeFailingSlot {
        value: u32,
    }

    impl Iox2FrameSlot<FrameSmokeMessage> for DecodeFailingSlot {
        fn try_from_message(value: &FrameSmokeMessage) -> Result<Self, WireCodecError> {
            Ok(Self { value: value.value })
        }

        fn decode_message(&self) -> Result<FrameSmokeMessage, WireCodecError> {
            Err(WireCodecError::invalid_frame("test decode failure"))
        }
    }

    #[test]
    fn frame_decode_errors_do_not_mark_endpoint_reconnecting() {
        let mut endpoint =
            Iox2FramePubSub::<FrameSmokeMessage, DecodeFailingSlot>::open_with_config(
                "FlowRT/Smoke/FrameDecodeHealth",
                Iox2ChannelConfig::latest(),
            )
            .expect("frame endpoint should open");

        endpoint
            .publish_at(FrameSmokeMessage { value: 1 }, 300)
            .expect("invalid frame payload should still publish");
        let error = endpoint
            .receive_latest_at(301)
            .expect_err("decode failure should be returned to caller");

        assert!(error.message().contains("decode FlowRT iox2 frame payload"));
        assert_eq!(endpoint.health().state, BackendHealthState::Ready);
    }
}
