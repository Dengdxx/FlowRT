//! 可选 Zenoh transport 的 canonical-wire publish-subscribe endpoint。
//!
//! 该模块只在启用 `zenoh` feature 时编译。endpoint 在 transport 上发送 FlowRT canonical
//! wire bytes，不发送 Rust native struct 布局；用户组件接口不应直接依赖本模块或 Zenoh 类型。

use zenoh::{
    Config, Wait,
    handlers::{RingChannel, RingChannelHandler},
    pubsub::{Publisher, Subscriber},
    sample::Sample,
    session::Session,
};

use crate::{FrameCodec, Latest, OverflowPolicy, StaleConfig, StalePolicy, WireCodecError};

const PUBLISHED_AT_WIRE_SIZE: usize = std::mem::size_of::<u64>();
const FLOWRT_ZENOH_CONNECT: &str = "FLOWRT_ZENOH_CONNECT";
const FLOWRT_ZENOH_LISTEN: &str = "FLOWRT_ZENOH_LISTEN";
const FLOWRT_ZENOH_MODE: &str = "FLOWRT_ZENOH_MODE";
const FLOWRT_ZENOH_NO_MULTICAST: &str = "FLOWRT_ZENOH_NO_MULTICAST";

#[derive(Debug, Clone)]
struct ZenohReceived<T>
where
    T: Clone,
{
    published_at_ms: u64,
    payload: T,
}

/// Zenoh endpoint 操作失败。
///
/// 错误只暴露 FlowRT 操作上下文和稳定文本，不把 Zenoh 的错误类型泄漏到 generated shell 或
/// 用户组件边界。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZenohError {
    operation: &'static str,
    message: String,
}

impl ZenohError {
    fn new(operation: &'static str, message: impl std::fmt::Display) -> Self {
        Self {
            operation,
            message: message.to_string(),
        }
    }

    fn transport(operation: &'static str, error: impl std::fmt::Debug) -> Self {
        Self::new(operation, format!("{error:?}"))
    }

    fn codec(operation: &'static str, error: WireCodecError) -> Self {
        Self::new(operation, error)
    }

    /// 返回失败的 FlowRT endpoint 操作。
    pub fn operation(&self) -> &'static str {
        self.operation
    }

    /// 返回不含具体 Zenoh 类型的错误消息。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ZenohError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for ZenohError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZenohChannelKind {
    Latest,
    Fifo,
}

/// 打开 Zenoh publish-subscribe endpoint 时使用的 channel 配置。
///
/// 配置来自 Contract IR channel policy 的归一化结果。接收侧使用有界 ring handler，使
/// latest/depth=1 在消费者未及时读取时丢弃旧样本，而不阻塞 Zenoh 的接收回调。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZenohChannelConfig {
    kind: ZenohChannelKind,
    depth: usize,
    overflow: OverflowPolicy,
    stale: StaleConfig,
}

impl ZenohChannelConfig {
    /// 构造 latest channel 的默认配置。
    pub fn latest() -> Self {
        Self {
            kind: ZenohChannelKind::Latest,
            depth: 1,
            overflow: OverflowPolicy::DropOldest,
            stale: StaleConfig::default(),
        }
    }

    /// 构造 FIFO channel 配置；`depth` 为 0 时按 1 处理。
    ///
    /// 当前 Zenoh ring handler 只原生满足 `drop_oldest`；其他 policy 会在打开 endpoint 时
    /// 返回明确错误，避免 runtime 静默改变 Contract IR 语义。
    pub fn fifo(depth: usize, overflow: OverflowPolicy) -> Self {
        Self {
            kind: ZenohChannelKind::Fifo,
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

    fn validate(&self) -> Result<(), ZenohError> {
        if self.overflow != OverflowPolicy::DropOldest {
            return Err(ZenohError::new(
                "validate Zenoh channel config",
                format!(
                    "overflow policy `{}` is not supported by the nonblocking Zenoh ring handler",
                    overflow_policy_name(self.overflow)
                ),
            ));
        }
        Ok(())
    }

    fn is_latest(&self) -> bool {
        self.kind == ZenohChannelKind::Latest
    }
}

impl Default for ZenohChannelConfig {
    fn default() -> Self {
        Self::latest()
    }
}

/// 使用 canonical `FrameCodec` 的 Zenoh typed publish-subscribe endpoint。
///
/// `T` 的 native struct bytes 从不进入 transport。发送端先写入 little-endian runtime
/// 时间戳，再调用 `FrameCodec` 编码业务 payload；接收端按相同布局解码。该类型是 backend
/// 层实现细节，generated shell 可以使用它，用户组件接口不应暴露它。
pub struct ZenohPubSub<T>
where
    T: FrameCodec + Clone,
{
    publisher: Publisher<'static>,
    subscriber: Subscriber<RingChannelHandler<Sample>>,
    session: Session,
    config: ZenohChannelConfig,
    received: Option<ZenohReceived<T>>,
}

impl<T> ZenohPubSub<T>
where
    T: FrameCodec + Clone,
{
    /// 使用显式 channel 配置打开 Zenoh endpoint。
    ///
    /// `key_expr` 是 generated shell 根据 Contract IR channel stable ID 生成的 canonical key
    /// expression。session 网络连接由 FlowRT runtime 环境变量注入，不进入用户组件 API。
    pub fn open_with_config(
        key_expr: &str,
        config: ZenohChannelConfig,
    ) -> Result<Self, ZenohError> {
        config.validate()?;
        let session = zenoh::open(config_from_environment()?)
            .wait()
            .map_err(|error| ZenohError::transport("open Zenoh session", error))?;
        let subscriber = session
            .declare_subscriber(key_expr.to_owned())
            .with(RingChannel::new(config.depth()))
            .wait()
            .map_err(|error| ZenohError::transport("declare Zenoh subscriber", error))?;
        let publisher = session
            .declare_publisher(key_expr.to_owned())
            .wait()
            .map_err(|error| ZenohError::transport("declare Zenoh publisher", error))?;

        Ok(Self {
            publisher,
            subscriber,
            session,
            config,
            received: None,
        })
    }

    /// 带 FlowRT runtime 时间戳发布一个 canonical-wire payload。
    pub fn publish_at(&self, value: T, published_at_ms: u64) -> Result<(), ZenohError> {
        let frame = encode_frame(&value, published_at_ms)?;
        self.publisher
            .put(frame)
            .wait()
            .map_err(|error| ZenohError::transport("publish Zenoh sample", error))
    }

    /// 非阻塞接收当前可用的最新样本，并按 runtime 时间计算 freshness。
    ///
    /// 接收侧使用 Zenoh `RingChannel`；其回调在 ring 满时丢弃最旧样本，通知消费者时使用
    /// 非阻塞发送。latest channel 排空当前可用样本并保留最新值；FIFO channel 每次只消费
    /// 最旧的一项。两种路径都通过 `try_recv` 读取，不等待网络或新样本到达。
    pub fn receive_latest_at(&mut self, now_ms: u64) -> Result<Latest<'_, T>, ZenohError> {
        if self.config.is_latest() {
            while let Some(sample) = self.try_receive_sample()? {
                self.received = Some(decode_sample(sample)?);
            }
        } else if let Some(sample) = self.try_receive_sample()? {
            self.received = Some(decode_sample(sample)?);
        }

        let stale = self
            .received
            .as_ref()
            .map(|sample| {
                self.config
                    .stale()
                    .stale_at(Some(sample.published_at_ms), now_ms)
            })
            .unwrap_or(false);
        let value = if stale && self.config.stale().policy() == StalePolicy::Drop {
            None
        } else {
            self.received.as_ref().map(|sample| &sample.payload)
        };

        Ok(Latest::new(value, stale))
    }

    /// 判断底层 Zenoh session 是否仍处于打开状态。
    ///
    /// 返回 `true` 只表示本地 endpoint 可执行操作，不表示当前已有远端 subscriber 或 router。
    pub fn ready(&self) -> bool {
        !self.session.is_closed()
    }

    /// 返回 endpoint 的 channel 配置。
    pub fn config(&self) -> ZenohChannelConfig {
        self.config
    }

    fn try_receive_sample(&self) -> Result<Option<Sample>, ZenohError> {
        self.subscriber
            .try_recv()
            .map_err(|error| ZenohError::transport("receive Zenoh sample", error))
    }
}

fn overflow_policy_name(policy: OverflowPolicy) -> &'static str {
    match policy {
        OverflowPolicy::DropOldest => "drop_oldest",
        OverflowPolicy::DropNewest => "drop_newest",
        OverflowPolicy::Error => "error",
        OverflowPolicy::Block => "block",
    }
}

fn config_from_environment() -> Result<Config, ZenohError> {
    let mut config = Config::default();

    if let Ok(mode) = std::env::var(FLOWRT_ZENOH_MODE) {
        let mode = mode.trim();
        if !mode.is_empty() {
            insert_config_json5(&mut config, "mode", json_string(mode)?)?;
        }
    }

    if let Ok(listen) = std::env::var(FLOWRT_ZENOH_LISTEN)
        && let Some(json) = endpoint_list_json(&listen)?
    {
        insert_config_json5(&mut config, "listen/endpoints", json)?;
    }

    if let Ok(connect) = std::env::var(FLOWRT_ZENOH_CONNECT)
        && let Some(json) = endpoint_list_json(&connect)?
    {
        insert_config_json5(&mut config, "connect/endpoints", json)?;
    }

    if std::env::var(FLOWRT_ZENOH_NO_MULTICAST)
        .ok()
        .as_deref()
        .is_some_and(env_flag_enabled)
    {
        insert_config_json5(
            &mut config,
            "scouting/multicast/enabled",
            "false".to_string(),
        )?;
    }

    Ok(config)
}

fn insert_config_json5(config: &mut Config, key: &str, value: String) -> Result<(), ZenohError> {
    config
        .insert_json5(key, &value)
        .map_err(|error| ZenohError::transport("configure Zenoh session", error))
}

fn endpoint_list_json(raw: &str) -> Result<Option<String>, ZenohError> {
    let endpoints = raw
        .split(',')
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty())
        .collect::<Vec<_>>();
    if endpoints.is_empty() {
        return Ok(None);
    }
    serde_json::to_string(&endpoints)
        .map(Some)
        .map_err(|error| ZenohError::new("configure Zenoh session", error))
}

fn json_string(raw: &str) -> Result<String, ZenohError> {
    serde_json::to_string(raw).map_err(|error| ZenohError::new("configure Zenoh session", error))
}

fn env_flag_enabled(raw: &str) -> bool {
    matches!(raw, "1" | "true" | "TRUE" | "yes" | "on")
}

fn frame_size(payload_size: usize) -> Result<usize, ZenohError> {
    PUBLISHED_AT_WIRE_SIZE
        .checked_add(payload_size)
        .ok_or_else(|| ZenohError::new("size FlowRT Zenoh frame", "wire payload size overflow"))
}

fn encode_frame<T>(value: &T, published_at_ms: u64) -> Result<Vec<u8>, ZenohError>
where
    T: FrameCodec,
{
    let payload_size = value.encoded_frame_size();
    let mut frame = vec![0u8; frame_size(payload_size)?];
    frame[..PUBLISHED_AT_WIRE_SIZE].copy_from_slice(&published_at_ms.to_le_bytes());
    value
        .encode_frame(&mut frame[PUBLISHED_AT_WIRE_SIZE..])
        .map_err(|error| ZenohError::codec("encode FlowRT Zenoh payload", error))?;
    Ok(frame)
}

fn decode_frame<T>(frame: &[u8]) -> Result<ZenohReceived<T>, ZenohError>
where
    T: FrameCodec + Clone,
{
    if frame.len() < PUBLISHED_AT_WIRE_SIZE {
        return Err(ZenohError::new(
            "decode FlowRT Zenoh frame",
            format!(
                "expected at least {PUBLISHED_AT_WIRE_SIZE} bytes, got {} bytes",
                frame.len()
            ),
        ));
    }
    let published_at_ms = u64::from_le_bytes(
        frame[..PUBLISHED_AT_WIRE_SIZE]
            .try_into()
            .expect("fixed-size timestamp prefix was validated"),
    );
    let payload = T::decode_frame(&frame[PUBLISHED_AT_WIRE_SIZE..])
        .map_err(|error| ZenohError::codec("decode FlowRT Zenoh payload", error))?;
    Ok(ZenohReceived {
        published_at_ms,
        payload,
    })
}

fn decode_sample<T>(sample: Sample) -> Result<ZenohReceived<T>, ZenohError>
where
    T: FrameCodec + Clone,
{
    let bytes = sample.payload().to_bytes();
    decode_frame(&bytes)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::{OverflowPolicy, StaleConfig, StalePolicy, WireCodec, WireCodecError};

    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct PaddedMessage {
        tag: u8,
        value: u16,
    }

    impl WireCodec for PaddedMessage {
        const WIRE_SIZE: usize = 3;

        fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
            if output.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
            }
            output[0] = self.tag;
            output[1..].copy_from_slice(&self.value.to_le_bytes());
            Ok(())
        }

        fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
            if input.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
            }
            Ok(Self {
                tag: input[0],
                value: u16::from_le_bytes([input[1], input[2]]),
            })
        }
    }

    fn unique_key_expr(suffix: &str) -> String {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        format!(
            "flowrt/tests/zenoh/{}/{}/{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
            suffix
        )
    }

    #[test]
    fn channel_config_normalizes_latest_fifo_and_stale_policy() {
        let latest = ZenohChannelConfig::latest();
        assert_eq!(latest.depth(), 1);
        assert_eq!(latest.overflow(), OverflowPolicy::DropOldest);

        let stale = StaleConfig::new(Some(25), StalePolicy::Drop);
        let fifo = ZenohChannelConfig::fifo(0, OverflowPolicy::DropNewest).with_stale_config(stale);
        assert_eq!(fifo.depth(), 1);
        assert_eq!(fifo.overflow(), OverflowPolicy::DropNewest);
        assert_eq!(fifo.stale(), stale);
    }

    #[test]
    fn environment_endpoint_list_json_is_trimmed_and_escaped() {
        let json = endpoint_list_json(" tcp/127.0.0.1:7447, tcp/example\\\"host:7447 ")
            .expect("endpoint list JSON should encode")
            .expect("nonempty endpoint list should produce JSON");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            serde_json::json!(["tcp/127.0.0.1:7447", "tcp/example\\\"host:7447"])
        );
        assert_eq!(
            endpoint_list_json(" , \t ").expect("empty endpoint list should parse"),
            None
        );
        assert!(env_flag_enabled("true"));
        assert!(env_flag_enabled("1"));
        assert!(!env_flag_enabled("false"));
    }

    #[test]
    fn wire_frame_contains_timestamp_and_canonical_payload_without_native_padding() {
        let message = PaddedMessage {
            tag: 0x12,
            value: 0x3456,
        };

        let bytes = encode_frame(&message, 0x0102_0304_0506_0708)
            .expect("canonical zenoh frame should encode");
        assert_eq!(
            bytes,
            vec![
                0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0x12, 0x56, 0x34
            ]
        );
        assert_ne!(bytes.len(), 8 + std::mem::size_of::<PaddedMessage>());

        let decoded =
            decode_frame::<PaddedMessage>(&bytes).expect("canonical zenoh frame should decode");
        assert_eq!(decoded.published_at_ms, 0x0102_0304_0506_0708);
        assert_eq!(decoded.payload, message);
    }

    #[test]
    fn latest_endpoint_roundtrips_canonical_payload_without_blocking_receive() {
        let key_expr = unique_key_expr("latest-roundtrip");
        let mut endpoint =
            ZenohPubSub::<PaddedMessage>::open_with_config(&key_expr, ZenohChannelConfig::latest())
                .expect("zenoh endpoint should open");
        assert!(endpoint.ready());
        assert_eq!(endpoint.config(), ZenohChannelConfig::latest());

        endpoint
            .publish_at(PaddedMessage { tag: 1, value: 10 }, 100)
            .expect("first canonical payload should publish");
        endpoint
            .publish_at(PaddedMessage { tag: 2, value: 20 }, 110)
            .expect("second canonical payload should publish");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let latest = endpoint
                .receive_latest_at(111)
                .expect("nonblocking latest receive should succeed");
            if latest.as_ref() == Some(&PaddedMessage { tag: 2, value: 20 }) {
                assert!(!latest.stale());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "local zenoh roundtrip did not deliver the latest sample"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn latest_endpoint_applies_stale_drop_policy() {
        let key_expr = unique_key_expr("stale-drop");
        let config = ZenohChannelConfig::latest()
            .with_stale_config(StaleConfig::new(Some(10), StalePolicy::Drop));
        let mut endpoint = ZenohPubSub::<PaddedMessage>::open_with_config(&key_expr, config)
            .expect("zenoh endpoint should open");
        endpoint
            .publish_at(PaddedMessage { tag: 3, value: 30 }, 100)
            .expect("canonical payload should publish");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let latest = endpoint
                .receive_latest_at(111)
                .expect("nonblocking latest receive should succeed");
            if latest.stale() {
                assert!(latest.as_ref().is_none());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "local zenoh roundtrip did not deliver the stale sample"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn endpoint_rejects_overflow_policy_ring_handler_cannot_honor() {
        let result = ZenohPubSub::<PaddedMessage>::open_with_config(
            "not/opened/because/config/is/invalid",
            ZenohChannelConfig::fifo(4, OverflowPolicy::DropNewest),
        );

        let error = match result {
            Ok(_) => panic!("unsupported overflow policy must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.operation(), "validate Zenoh channel config");
        assert!(error.message().contains("drop_newest"));
    }

    #[test]
    fn fifo_endpoint_consumes_one_oldest_sample_per_receive() {
        let key_expr = unique_key_expr("fifo-order");
        let mut endpoint = ZenohPubSub::<PaddedMessage>::open_with_config(
            &key_expr,
            ZenohChannelConfig::fifo(2, OverflowPolicy::DropOldest),
        )
        .expect("zenoh FIFO endpoint should open");
        endpoint
            .publish_at(PaddedMessage { tag: 1, value: 10 }, 100)
            .expect("first FIFO payload should publish");
        endpoint
            .publish_at(PaddedMessage { tag: 2, value: 20 }, 110)
            .expect("second FIFO payload should publish");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let first = endpoint
                .receive_latest_at(111)
                .expect("nonblocking FIFO receive should succeed");
            if first.present() {
                assert_eq!(first.as_ref(), Some(&PaddedMessage { tag: 1, value: 10 }));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "local zenoh roundtrip did not deliver the first FIFO sample"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let second = endpoint
            .receive_latest_at(111)
            .expect("second nonblocking FIFO receive should succeed");
        assert_eq!(second.as_ref(), Some(&PaddedMessage { tag: 2, value: 20 }));
    }
}
