//! 可选 iceoryx2 transport 的最小 typed pub/sub 支持。
//!
//! 该模块只在启用 `iox2` feature 时编译。它用于验证 FlowRT Message ABI plain-data
//! payload 可以通过 iceoryx2 传输；用户算法代码仍不应直接依赖本模块。

use std::{
    collections::BTreeMap,
    fmt::Debug,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use iceoryx2::port::{listener::Listener, notifier::Notifier};
use iceoryx2::prelude::*;
use iceoryx2::sample::Sample;

use crate::{
    BackendHealthSnapshot, BackendHealthState, BackendHealthTracker, Latest, OverflowPolicy,
    ReconnectPolicy, ServiceError, ServiceResult, StaleConfig, StalePolicy, service::fnv1a64,
};

type IpcNode = Node<ipc::Service>;
type IpcPublisher<T> = iceoryx2::port::publisher::Publisher<ipc::Service, T, FlowrtIox2Header>;
type IpcSubscriber<T> = iceoryx2::port::subscriber::Subscriber<ipc::Service, T, FlowrtIox2Header>;
type IpcNotifier = Notifier<ipc::Service>;
type IpcListener = Listener<ipc::Service>;
const IOX2_WAKE_EVENT_ID: usize = 4;

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

struct Iox2EndpointParts<T>
where
    T: std::fmt::Debug + ZeroCopySend + 'static,
{
    publisher: IpcPublisher<T>,
    subscriber: IpcSubscriber<T>,
    notifier: IpcNotifier,
    node: IpcNode,
}

struct Iox2WakeHandle {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

struct Iox2WakeListenerParts {
    listener: IpcListener,
    _node: IpcNode,
}

struct Iox2RecoveryRequest<'a> {
    operation: &'static str,
    service_name: &'a str,
    config: Iox2ChannelConfig,
    schedule_waiter: Option<&'a crate::ScheduleWaiter>,
}

struct Iox2RecoveryParts<'a, T>
where
    T: std::fmt::Debug + ZeroCopySend + 'static,
{
    publisher: &'a mut Option<IpcPublisher<T>>,
    subscriber: &'a mut Option<IpcSubscriber<T>>,
    notifier: &'a mut Option<IpcNotifier>,
    wake_handle: &'a mut Option<Iox2WakeHandle>,
    node: &'a mut Option<IpcNode>,
}

enum Iox2PublishOutcome {
    Sent,
    SentWakeFailed(Iox2Error),
}

impl Iox2WakeHandle {
    fn start(service_name: String, waiter: crate::ScheduleWaiter) -> Result<Self, Iox2Error> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker_name = format!("flowrt-iox2-wake-{service_name}");
        let worker = thread::Builder::new()
            .name(worker_name)
            .spawn(move || {
                let wake = match open_iox2_wake_listener(&service_name) {
                    Ok(wake) => {
                        let _ = ready_tx.send(Ok(()));
                        wake
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                while !worker_stop.load(Ordering::Acquire) {
                    match wake.listener.timed_wait_one(Duration::from_millis(50)) {
                        Ok(Some(_)) => waiter.notify_data(),
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
            })
            .map_err(|error| Iox2Error::new("failed to spawn iceoryx2 wake listener", error))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                stop,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                Err(error)
            }
            Err(mpsc::RecvError) => {
                stop.store(true, Ordering::Release);
                let _ = worker.join();
                Err(Iox2Error::new(
                    "iceoryx2 wake listener exited before ready",
                    "channel disconnected",
                ))
            }
        }
    }
}

impl Drop for Iox2WakeHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
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

    /// 构造 service request/response endpoint 的 FIFO QoS 配置。
    pub fn service_default() -> Self {
        Self::fifo(64, OverflowPolicy::DropOldest)
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
    notifier: Option<IpcNotifier>,
    wake_handle: Option<Iox2WakeHandle>,
    schedule_waiter: Option<crate::ScheduleWaiter>,
    node: Option<IpcNode>,
    config: Iox2ChannelConfig,
    stale: StaleConfig,
    health: BackendHealthTracker,
    received: Option<Iox2Received<T>>,
    revision: u64,
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
            notifier: Some(parts.notifier),
            wake_handle: None,
            schedule_waiter: None,
            node: Some(parts.node),
            config,
            stale: config.stale(),
            health: BackendHealthTracker::new(ReconnectPolicy::default()),
            received: None,
            revision: 0,
        })
    }

    /// 构造一个不可用 endpoint，用于 generated shell 在 startup open 失败后保留结构化状态。
    pub fn unavailable(
        service_name: &str,
        config: Iox2ChannelConfig,
        error: impl Into<String>,
    ) -> Self {
        let mut health = BackendHealthTracker::new(ReconnectPolicy::default());
        health.mark_failed(error.into(), 0);
        Self {
            service_name: service_name.to_string(),
            publisher: None,
            subscriber: None,
            notifier: None,
            wake_handle: None,
            schedule_waiter: None,
            node: None,
            config,
            stale: config.stale(),
            health,
            received: None,
            revision: 0,
        }
    }

    /// 通过 iceoryx2 loaned sample 发布一个值。
    pub fn publish(&mut self, value: T) -> Result<(), Iox2Error> {
        self.publish_at(value, 0)
    }

    /// 带 FlowRT runtime 时间戳发布一个值。
    pub fn publish_at(&mut self, value: T, published_at_ms: u64) -> Result<(), Iox2Error> {
        self.ensure_ready("publish iceoryx2 sample")?;
        match self.publish_slot(value, published_at_ms) {
            Ok(outcome) => self.finish_publish_outcome(outcome),
            Err(error) => {
                self.mark_transport_error(&error);
                self.recover_after_transport_error("publish iceoryx2 sample")?;
                self.publish_slot(value, published_at_ms)
                    .and_then(|outcome| self.finish_publish_outcome(outcome))
                    .inspect_err(|error| self.mark_transport_error(error))
            }
        }
    }

    /// 注册 scheduler 数据到达唤醒器。
    ///
    /// iceoryx2 v0.9 的 typed pub/sub subscriber 不直接暴露可附着到 WaitSet 的数据到达事件；
    /// FlowRT 保留该 API 作为 backend wake adapter 的稳定入口，后续会由 sideband event 或 SDK
    /// waitset adapter 驱动。
    pub fn set_schedule_waiter(&mut self, waiter: crate::ScheduleWaiter) {
        self.schedule_waiter = Some(waiter.clone());
        let _ = self.start_wake_listener(waiter);
    }

    fn finish_publish_outcome(&mut self, outcome: Iox2PublishOutcome) -> Result<(), Iox2Error> {
        match outcome {
            Iox2PublishOutcome::Sent => {
                self.health.mark_ready();
                Ok(())
            }
            Iox2PublishOutcome::SentWakeFailed(error) => {
                self.mark_transport_error(&error);
                Ok(())
            }
        }
    }

    fn publish_slot(
        &self,
        value: T,
        published_at_ms: u64,
    ) -> Result<Iox2PublishOutcome, Iox2Error> {
        {
            let publisher = self.publisher.as_ref().ok_or_else(|| {
                Iox2Error::new("publish iceoryx2 sample", "endpoint is not ready")
            })?;
            let sample = publisher
                .loan_uninit()
                .map_err(|error| Iox2Error::new("failed to loan iceoryx2 sample", error))?;
            let mut sample = sample;
            sample.user_header_mut().published_at_ms = published_at_ms;
            sample
                .write_payload(value)
                .send()
                .map_err(|error| Iox2Error::new("failed to send iceoryx2 sample", error))?;
        }

        match self.notify_wake() {
            Ok(()) => Ok(Iox2PublishOutcome::Sent),
            Err(error) => Ok(Iox2PublishOutcome::SentWakeFailed(error)),
        }
    }

    fn notify_wake(&self) -> Result<(), Iox2Error> {
        let notifier = self
            .notifier
            .as_ref()
            .ok_or_else(|| Iox2Error::new("notify iceoryx2 wake event", "endpoint is not ready"))?;
        notifier
            .notify_with_custom_event_id(EventId::new(IOX2_WAKE_EVENT_ID))
            .map(|_| ())
            .map_err(|error| Iox2Error::new("failed to notify iceoryx2 wake event", error))
    }

    fn start_wake_listener(&mut self, waiter: crate::ScheduleWaiter) -> Result<(), Iox2Error> {
        if self.wake_handle.is_some() {
            return Ok(());
        }
        match Iox2WakeHandle::start(self.service_name.clone(), waiter) {
            Ok(handle) => {
                self.wake_handle = Some(handle);
                Ok(())
            }
            Err(error) => {
                self.health
                    .mark_degraded(format!("iceoryx2 wake listener unavailable: {error}"));
                Err(error)
            }
        }
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
            self.revision = self.revision.saturating_add(1);
        }

        Ok(self.cached_latest_at(now_ms))
    }

    /// 接收当前 latest view，并同时返回接收后的修订号。
    pub fn receive_latest_with_revision_at(
        &mut self,
        now_ms: u64,
    ) -> Result<(Latest<'_, T>, u64), Iox2Error> {
        self.ensure_ready("receive iceoryx2 sample")?;
        if let Some(sample) = self.try_receive_sample_with_recovery()? {
            self.received = Some(Iox2Received {
                published_at_ms: sample.user_header().published_at_ms,
                payload: *sample,
            });
            self.revision = self.revision.saturating_add(1);
        }
        let revision = self.revision;
        Ok((self.cached_latest_at(now_ms), revision))
    }

    /// 返回最近一次已接收样本的 cached latest view，不触碰 transport。
    pub fn cached_latest_at(&self, now_ms: u64) -> Latest<'_, T> {
        let stale = self
            .received
            .map(|sample| self.stale.stale_at(Some(sample.published_at_ms), now_ms))
            .unwrap_or(false);
        let value = if stale && self.stale.policy() == StalePolicy::Drop {
            None
        } else {
            self.received.as_ref().map(|sample| &sample.payload)
        };

        Latest::new(value, stale)
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

    /// 返回接收侧已接受样本的修订号，用于调度器检测新到达数据。
    pub fn revision(&self) -> u64 {
        self.revision
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
            Iox2RecoveryRequest {
                operation,
                service_name: &self.service_name,
                config: self.config,
                schedule_waiter: self.schedule_waiter.as_ref(),
            },
            Iox2RecoveryParts {
                publisher: &mut self.publisher,
                subscriber: &mut self.subscriber,
                notifier: &mut self.notifier,
                wake_handle: &mut self.wake_handle,
                node: &mut self.node,
            },
            &mut self.health,
        )
    }

    #[cfg(test)]
    fn reset_transport_for_test(&mut self) {
        self.publisher = None;
        self.subscriber = None;
        self.notifier = None;
        self.wake_handle = None;
        self.node = None;
        self.health.mark_degraded("iceoryx2 endpoint reset by test");
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, ZeroCopySend)]
#[type_name("FlowRTIox2ServiceCorrelation")]
struct Iox2ServiceCorrelation {
    session_id: u64,
    sequence: u64,
    service_id: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("FlowRTIox2ServiceRequest")]
struct Iox2ServiceRequest<Req: Debug + Copy + ZeroCopySend + 'static> {
    correlation: Iox2ServiceCorrelation,
    payload: Req,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, ZeroCopySend)]
#[type_name("FlowRTIox2ServiceResponse")]
struct Iox2ServiceResponse<Resp: Debug + Copy + ZeroCopySend + 'static> {
    correlation: Iox2ServiceCorrelation,
    status: u16,
    _pad: [u8; 6],
    payload: Resp,
}

struct Iox2ServiceClientInner<Req, Resp>
where
    Req: Debug + Copy + ZeroCopySend + 'static,
    Resp: Debug + Copy + ZeroCopySend + Default + 'static,
{
    sequence: u64,
    request_tx: Iox2PubSub<Iox2ServiceRequest<Req>>,
    response_rx: Iox2PubSub<Iox2ServiceResponse<Resp>>,
    pending: BTreeMap<u64, Iox2ServiceResponse<Resp>>,
}

/// iox2 fixed-size Service client backend primitive。
pub struct Iox2ServiceClient<Req, Resp>
where
    Req: Debug + Copy + ZeroCopySend + 'static,
    Resp: Debug + Copy + ZeroCopySend + Default + 'static,
{
    service_name: String,
    session_id: u64,
    service_id: u64,
    inner: Mutex<Iox2ServiceClientInner<Req, Resp>>,
    unavailable: Option<String>,
}

impl<Req, Resp> Iox2ServiceClient<Req, Resp>
where
    Req: Debug + Copy + ZeroCopySend + 'static,
    Resp: Debug + Copy + ZeroCopySend + Default + 'static,
{
    /// 打开 iox2 service client。
    pub fn open(service_name: &str) -> Result<Self, Iox2Error> {
        let config = Iox2ChannelConfig::service_default();
        Ok(Self {
            service_name: service_name.to_string(),
            session_id: next_session_id(),
            service_id: stable_service_id(service_name),
            inner: Mutex::new(Iox2ServiceClientInner {
                sequence: 0,
                request_tx: Iox2PubSub::open_with_config(
                    &iox2_request_endpoint(service_name),
                    config,
                )?,
                response_rx: Iox2PubSub::open_with_config(
                    &iox2_response_endpoint(service_name),
                    config,
                )?,
                pending: BTreeMap::new(),
            }),
            unavailable: None,
        })
    }

    /// 构造不可用 client，用于缺少 iox2 feature/SDK 的 fail-fast 路径。
    pub fn unavailable(service_name: &str, error: impl Into<String>) -> Self {
        let config = Iox2ChannelConfig::service_default();
        let error = error.into();
        Self {
            service_name: service_name.to_string(),
            session_id: next_session_id(),
            service_id: stable_service_id(service_name),
            inner: Mutex::new(Iox2ServiceClientInner {
                sequence: 0,
                request_tx: Iox2PubSub::unavailable(
                    &iox2_request_endpoint(service_name),
                    config,
                    error.clone(),
                ),
                response_rx: Iox2PubSub::unavailable(
                    &iox2_response_endpoint(service_name),
                    config,
                    error.clone(),
                ),
                pending: BTreeMap::new(),
            }),
            unavailable: Some(error),
        }
    }

    /// 发起同步 request/response 调用。
    pub fn call(&self, request: Req, timeout_ms: u64) -> ServiceResult<Resp> {
        if let Some(error) = &self.unavailable {
            return ServiceResult::err_with_message(ServiceError::Unavailable, error.clone());
        }
        if timeout_ms == 0 {
            return ServiceResult::err(ServiceError::Timeout);
        }

        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let sequence = inner.sequence;
        inner.sequence = inner.sequence.wrapping_add(1);
        let correlation = Iox2ServiceCorrelation {
            session_id: self.session_id,
            sequence,
            service_id: self.service_id,
        };
        if inner
            .request_tx
            .publish(Iox2ServiceRequest {
                correlation,
                payload: request,
            })
            .is_err()
        {
            return ServiceResult::err(ServiceError::Backend);
        }

        if let Some(response) = inner.pending.remove(&sequence) {
            return decode_service_response(response);
        }

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            match inner.response_rx.receive() {
                Ok(Some(response))
                    if response.correlation.service_id == self.service_id
                        && response.correlation.session_id == self.session_id =>
                {
                    if response.correlation.sequence == sequence {
                        return decode_service_response(response);
                    }
                    inner
                        .pending
                        .insert(response.correlation.sequence, response);
                }
                Ok(Some(_)) | Ok(None) => {}
                Err(_) => return ServiceResult::err(ServiceError::Backend),
            }

            if Instant::now() >= deadline {
                return ServiceResult::err(ServiceError::Timeout);
            }
            let _ = inner.response_rx.poll_once(Duration::from_millis(1));
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// 返回 canonical service name。
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// 返回 backend health。
    pub fn health(&self) -> BackendHealthSnapshot {
        if let Some(error) = &self.unavailable {
            return BackendHealthSnapshot {
                state: BackendHealthState::Unsupported,
                last_error: Some(error.clone()),
                attempt: 0,
                next_retry_unix_ms: None,
                recoverable: false,
            };
        }
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .request_tx
            .health()
    }
}

/// iox2 fixed-size Service server backend primitive。
pub struct Iox2ServiceServer<Req, Resp>
where
    Req: Debug + Copy + ZeroCopySend + 'static,
    Resp: Debug + Copy + ZeroCopySend + Default + 'static,
{
    service_name: String,
    request_rx: Iox2PubSub<Iox2ServiceRequest<Req>>,
    response_tx: Iox2PubSub<Iox2ServiceResponse<Resp>>,
    max_in_flight: usize,
}

impl<Req, Resp> Iox2ServiceServer<Req, Resp>
where
    Req: Debug + Copy + ZeroCopySend + 'static,
    Resp: Debug + Copy + ZeroCopySend + Default + 'static,
{
    /// 打开 iox2 service server。
    pub fn open(service_name: &str, max_in_flight: usize) -> Result<Self, Iox2Error> {
        let config = Iox2ChannelConfig::service_default();
        Ok(Self {
            service_name: service_name.to_string(),
            request_rx: Iox2PubSub::open_with_config(&iox2_request_endpoint(service_name), config)?,
            response_tx: Iox2PubSub::open_with_config(
                &iox2_response_endpoint(service_name),
                config,
            )?,
            max_in_flight: max_in_flight.max(1),
        })
    }

    /// 注册 scheduler 数据到达唤醒器。
    pub fn set_schedule_waiter(&mut self, waiter: crate::ScheduleWaiter) {
        self.request_rx.set_schedule_waiter(waiter);
    }

    /// 由 scheduler hidden task drain request 并调用 handler。
    pub fn poll_requests(
        &mut self,
        mut handler: impl FnMut(Req) -> ServiceResult<Resp>,
    ) -> Result<usize, Iox2Error> {
        let mut handled = 0usize;
        while handled < self.max_in_flight {
            let Some(request) = self.request_rx.receive()? else {
                break;
            };
            let (status, payload) = match handler(request.payload) {
                ServiceResult::Ok(response) => (ServiceError::Ok as u16, response),
                ServiceResult::Err(code, _) => (code as u16, Resp::default()),
            };
            self.response_tx.publish(Iox2ServiceResponse {
                correlation: request.correlation,
                status,
                _pad: [0; 6],
                payload,
            })?;
            handled += 1;
        }
        Ok(handled)
    }

    /// 返回 canonical service name。
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// 返回 backend health。
    pub fn health(&self) -> BackendHealthSnapshot {
        self.request_rx.health()
    }
}

fn decode_service_response<Resp>(response: Iox2ServiceResponse<Resp>) -> ServiceResult<Resp>
where
    Resp: Debug + Copy + ZeroCopySend + Default + 'static,
{
    match ServiceError::from_abi(response.status) {
        Some(ServiceError::Ok) => ServiceResult::ok(response.payload),
        Some(code) => ServiceResult::err(code),
        None => ServiceResult::err(ServiceError::Protocol),
    }
}

fn iox2_request_endpoint(service_name: &str) -> String {
    format!("{service_name}/req")
}

fn iox2_response_endpoint(service_name: &str) -> String {
    format!("{service_name}/resp")
}

fn stable_service_id(service_name: &str) -> u64 {
    fnv1a64(service_name.as_bytes())
}

fn next_session_id() -> u64 {
    static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);
    NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
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
        .max_publishers(config.depth().max(8))
        .max_subscribers(config.depth().max(8))
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
    let event = node
        .service_builder(&service_name)
        .event()
        .open_or_create()
        .map_err(|error| Iox2Error::new("failed to open or create iceoryx2 wake event", error))?;
    let notifier = event
        .notifier_builder()
        .create()
        .map_err(|error| Iox2Error::new("failed to create iceoryx2 wake notifier", error))?;

    Ok(Iox2EndpointParts {
        publisher,
        subscriber,
        notifier,
        node,
    })
}

fn open_iox2_wake_listener(service_name: &str) -> Result<Iox2WakeListenerParts, Iox2Error> {
    let service_name = service_name
        .try_into()
        .map_err(|error| Iox2Error::new("invalid iceoryx2 wake service name", error))?;
    let node = NodeBuilder::new()
        .create::<ipc::Service>()
        .map_err(|error| Iox2Error::new("failed to create iceoryx2 wake node", error))?;
    let event = node
        .service_builder(&service_name)
        .event()
        .open_or_create()
        .map_err(|error| Iox2Error::new("failed to open or create iceoryx2 wake event", error))?;
    let listener = event
        .listener_builder()
        .create()
        .map_err(|error| Iox2Error::new("failed to create iceoryx2 wake listener", error))?;
    Ok(Iox2WakeListenerParts {
        listener,
        _node: node,
    })
}

fn recover_iox2_endpoint<T>(
    request: Iox2RecoveryRequest<'_>,
    parts: Iox2RecoveryParts<'_, T>,
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
            request.operation,
            "iceoryx2 endpoint reconnect backoff is active",
        ));
    }

    let attempt = snapshot.attempt;
    if !health.policy().can_retry(attempt) {
        health.mark_failed("iceoryx2 endpoint reconnect budget exhausted", attempt);
        return Err(Iox2Error::new(
            request.operation,
            "iceoryx2 endpoint reconnect budget exhausted",
        ));
    }

    let now_ms = unix_now_ms();
    health.mark_reconnecting(
        attempt,
        now_ms.saturating_add(health.policy().delay_for_attempt(attempt)),
    );
    *parts.wake_handle = None;
    *parts.publisher = None;
    *parts.subscriber = None;
    *parts.notifier = None;
    *parts.node = None;
    match open_iox2_parts(request.service_name, request.config) {
        Ok(reopened) => {
            let wake_handle = if let Some(waiter) = request.schedule_waiter {
                Some(Iox2WakeHandle::start(
                    request.service_name.to_string(),
                    waiter.clone(),
                )?)
            } else {
                None
            };
            *parts.publisher = Some(reopened.publisher);
            *parts.subscriber = Some(reopened.subscriber);
            *parts.notifier = Some(reopened.notifier);
            *parts.wake_handle = wake_handle;
            *parts.node = Some(reopened.node);
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
    use crate::{ServiceError, ServiceResult};

    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, ZeroCopySend)]
    struct Iox2SmokeMessage {
        timestamp: u64,
        x: f32,
        y: f32,
    }

    #[test]
    fn wake_listener_start_reports_open_failure() {
        let error = match Iox2WakeHandle::start(String::new(), crate::ScheduleWaiter::new()) {
            Ok(_) => panic!("invalid wake service name should be reported"),
            Err(error) => error,
        };

        assert!(
            error
                .message()
                .contains("invalid iceoryx2 wake service name"),
            "unexpected error: {}",
            error.message()
        );
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
    fn schedule_waiter_is_notified_when_peer_publishes_sample() {
        let service_name = "FlowRT/Smoke/ScheduleWake";
        let mut receiver = Iox2PubSub::<Iox2SmokeMessage>::open(service_name)
            .expect("receiver endpoint should open");
        let mut sender = Iox2PubSub::<Iox2SmokeMessage>::open(service_name)
            .expect("sender endpoint should open");
        let waiter = crate::ScheduleWaiter::new();
        receiver.set_schedule_waiter(waiter.clone());

        sender
            .publish_at(
                Iox2SmokeMessage {
                    timestamp: 41,
                    x: 7.0,
                    y: 8.0,
                },
                300,
            )
            .expect("sender should publish");

        let event = waiter.wait_until_after(
            0,
            Some(std::time::Instant::now() + Duration::from_millis(500)),
            &crate::ShutdownToken::new(),
        );

        assert_eq!(event, crate::ScheduleEvent::Data);
    }

    #[test]
    fn publish_does_not_retry_payload_when_wake_notify_fails() {
        let service_name = "FlowRT/Smoke/WakeFailureNoRetry";
        let config = Iox2ChannelConfig::fifo(4, crate::OverflowPolicy::DropOldest);
        let mut receiver = Iox2PubSub::<Iox2SmokeMessage>::open_with_config(service_name, config)
            .expect("receiver endpoint should open");
        let mut sender = Iox2PubSub::<Iox2SmokeMessage>::open_with_config(service_name, config)
            .expect("sender endpoint should open");
        sender.notifier = None;
        let message = Iox2SmokeMessage {
            timestamp: 51,
            x: 9.0,
            y: 10.0,
        };

        sender
            .publish_at(message, 400)
            .expect("payload send should succeed even when wake notify fails");

        assert_eq!(sender.health().state, BackendHealthState::Degraded);
        receiver
            .poll_once(Duration::from_millis(1))
            .expect("receiver node should poll after publish");
        assert_eq!(
            receiver
                .receive()
                .expect("receiver should read the published sample"),
            Some(message)
        );
        assert_eq!(
            receiver
                .receive()
                .expect("wake failure must not trigger payload retry"),
            None
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

    #[repr(C)]
    #[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend)]
    struct Iox2SvcReq {
        goal: u32,
    }

    #[repr(C)]
    #[derive(Debug, Clone, Copy, Default, PartialEq, ZeroCopySend)]
    struct Iox2SvcResp {
        accepted: u32,
    }

    fn unique_service_name(prefix: &str) -> String {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!(
            "FlowRT/Smoke/Service/{prefix}/{}/{}",
            std::process::id(),
            sequence
        )
    }

    #[test]
    fn iox2_service_request_response_roundtrips() {
        let name = unique_service_name("roundtrip");
        let server_name = name.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut server =
                Iox2ServiceServer::<Iox2SvcReq, Iox2SvcResp>::open(&server_name, 8).unwrap();
            ready_tx.send(()).unwrap();
            for _ in 0..50 {
                let _ = server.poll_requests(|req| {
                    ServiceResult::ok(Iox2SvcResp {
                        accepted: u32::from(req.goal % 2 == 0),
                    })
                });
                std::thread::sleep(Duration::from_millis(5));
            }
        });

        ready_rx.recv().unwrap();
        let client = Iox2ServiceClient::<Iox2SvcReq, Iox2SvcResp>::open(&name).unwrap();
        let response = client.call(Iox2SvcReq { goal: 4 }, 1000);
        assert_eq!(response, ServiceResult::Ok(Iox2SvcResp { accepted: 1 }));
        server.join().unwrap();
    }

    #[test]
    fn iox2_service_call_times_out_without_server() {
        let name = unique_service_name("timeout");
        let client = Iox2ServiceClient::<Iox2SvcReq, Iox2SvcResp>::open(&name).unwrap();

        let response = client.call(Iox2SvcReq { goal: 1 }, 50);

        assert_eq!(response.error_code(), ServiceError::Timeout);
    }

    #[test]
    fn iox2_service_unavailable_client_returns_unavailable() {
        let client = Iox2ServiceClient::<Iox2SvcReq, Iox2SvcResp>::unavailable("svc", "no sdk");

        let response = client.call(Iox2SvcReq { goal: 1 }, 50);

        assert_eq!(response.error_code(), ServiceError::Unavailable);
        assert_eq!(client.health().state, BackendHealthState::Unsupported);
    }

    #[test]
    fn iox2_service_multi_client_correlation_isolated() {
        let name = unique_service_name("multi-client");
        let server_name = name.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut server =
                Iox2ServiceServer::<Iox2SvcReq, Iox2SvcResp>::open(&server_name, 8).unwrap();
            ready_tx.send(()).unwrap();
            for _ in 0..100 {
                let _ = server.poll_requests(|req| {
                    ServiceResult::ok(Iox2SvcResp {
                        accepted: req.goal + 100,
                    })
                });
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        ready_rx.recv().unwrap();

        let name_a = name.clone();
        let name_b = name.clone();
        let left = std::thread::spawn(move || {
            let client = Iox2ServiceClient::<Iox2SvcReq, Iox2SvcResp>::open(&name_a).unwrap();
            client.call(Iox2SvcReq { goal: 7 }, 1000)
        });
        let right = std::thread::spawn(move || {
            let client = Iox2ServiceClient::<Iox2SvcReq, Iox2SvcResp>::open(&name_b).unwrap();
            client.call(Iox2SvcReq { goal: 9 }, 1000)
        });

        assert_eq!(
            left.join().unwrap(),
            ServiceResult::Ok(Iox2SvcResp { accepted: 107 })
        );
        assert_eq!(
            right.join().unwrap(),
            ServiceResult::Ok(Iox2SvcResp { accepted: 109 })
        );
        server.join().unwrap();
    }
}
