//! 可选 iceoryx2 transport 的最小 typed pub/sub 支持。
//!
//! 该模块只在启用 `iox2` feature 时编译。它用于验证 FlowRT Message ABI plain-data
//! payload 可以通过 iceoryx2 传输；用户算法代码仍不应直接依赖本模块。

use std::time::Duration;

use iceoryx2::prelude::*;

use crate::{Latest, OverflowPolicy, StaleConfig, StalePolicy};

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
    publisher: IpcPublisher<T>,
    subscriber: IpcSubscriber<T>,
    node: IpcNode,
    stale: StaleConfig,
    received: Option<Iox2Received<T>>,
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

        Ok(Self {
            publisher,
            subscriber,
            node,
            stale: config.stale(),
            received: None,
        })
    }

    /// 通过 iceoryx2 loaned sample 发布一个值。
    pub fn publish(&self, value: T) -> Result<(), Iox2Error> {
        self.publish_at(value, 0)
    }

    /// 带 FlowRT runtime 时间戳发布一个值。
    pub fn publish_at(&self, value: T, published_at_ms: u64) -> Result<(), Iox2Error> {
        let sample = self
            .publisher
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
    pub fn receive(&self) -> Result<Option<T>, Iox2Error> {
        self.subscriber
            .receive()
            .map(|sample| sample.map(|sample| *sample))
            .map_err(|error| Iox2Error::new("failed to receive iceoryx2 sample", error))
    }

    /// 接收一个值，并根据 transport 时间戳暴露 freshness 状态。
    pub fn receive_latest_at(&mut self, now_ms: u64) -> Result<Latest<'_, T>, Iox2Error> {
        if let Some(sample) = self
            .subscriber
            .receive()
            .map_err(|error| Iox2Error::new("failed to receive iceoryx2 sample", error))?
        {
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

    /// 短暂等待，让 iceoryx2 推进本机 endpoint 状态。
    pub fn poll_once(&self, timeout: Duration) -> Result<(), Iox2Error> {
        self.node
            .wait(timeout)
            .map_err(|error| Iox2Error::new("failed to poll iceoryx2 node", error))
    }
}

/// 通过 iceoryx2 运行一个单进程 publish/subscribe roundtrip。
pub fn smoke_pubsub<T>(service_name: &str, value: T) -> Result<T, Iox2Error>
where
    T: std::fmt::Debug + Copy + ZeroCopySend + 'static,
{
    let endpoint = Iox2PubSub::<T>::open(service_name)?;
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
        let endpoint = Iox2PubSub::<Iox2SmokeMessage>::open("FlowRT/Smoke/TypedEndpoint")
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
        let endpoint = Iox2PubSub::<Iox2SmokeMessage>::open_with_config(
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
}
