//! 可选 Zenoh transport 的 canonical-wire publish-subscribe 和 service endpoint。
//!
//! 该模块只在启用 `zenoh` feature 时编译。endpoint 在 transport 上发送 FlowRT canonical
//! wire bytes，不发送 Rust native struct 布局；用户组件接口不应直接依赖本模块或 Zenoh 类型。

use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicUsize, Ordering},
    },
};

use zenoh::{
    Config, Wait,
    pubsub::{Publisher, Subscriber},
    query::{Query, Queryable},
    session::Session,
};

use crate::{
    BackendHealthSnapshot, BackendHealthState, BackendHealthTracker, Deadline, FrameCodec, Latest,
    OverflowPolicy, ReconnectPolicy, RequestId, ScheduleWaiter, ServiceError, ServiceFrameHeader,
    ServiceResult, StaleConfig, StalePolicy, WireCodecError, decode_service_frame,
    encode_service_frame, fnv1a64,
};

const PUBLISHED_AT_WIRE_SIZE: usize = std::mem::size_of::<u64>();
const SERVICE_TRANSPORT_TIMEOUT_GRACE_MS: u64 = 1000;
const DEFAULT_ZENOH_SERVICE_MAX_IN_FLIGHT: usize = 64;
const FLOWRT_ZENOH_CONNECT: &str = "FLOWRT_ZENOH_CONNECT";
const FLOWRT_ZENOH_LISTEN: &str = "FLOWRT_ZENOH_LISTEN";
const FLOWRT_ZENOH_MODE: &str = "FLOWRT_ZENOH_MODE";
const FLOWRT_ZENOH_NO_MULTICAST: &str = "FLOWRT_ZENOH_NO_MULTICAST";

fn service_key_expr(service_name: &str) -> String {
    let mut encoded = String::with_capacity(service_name.len());
    for byte in service_name.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'-' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("_x{byte:02X}_")),
        }
    }
    format!("flowrt/service/{encoded}/request")
}

#[derive(Debug, Clone)]
struct ZenohReceived<T>
where
    T: Clone,
{
    published_at_ms: u64,
    payload: T,
}

struct ZenohEndpointParts {
    publisher: Publisher<'static>,
    subscriber: Subscriber<()>,
    session: Session,
}

#[derive(Debug)]
struct ZenohInbox {
    frames: Mutex<VecDeque<Vec<u8>>>,
    schedule_waiter: Mutex<Option<ScheduleWaiter>>,
    depth: usize,
}

impl ZenohInbox {
    fn new(depth: usize) -> Self {
        Self {
            frames: Mutex::new(VecDeque::with_capacity(depth)),
            schedule_waiter: Mutex::new(None),
            depth,
        }
    }

    fn push(&self, frame: Vec<u8>) {
        {
            let mut frames = lock_recover(&self.frames);
            if frames.len() >= self.depth {
                frames.pop_front();
            }
            frames.push_back(frame);
        }
        if let Some(waiter) = lock_recover(&self.schedule_waiter).clone() {
            waiter.notify_data();
        }
    }

    fn pop(&self) -> Option<Vec<u8>> {
        lock_recover(&self.frames).pop_front()
    }

    fn set_schedule_waiter(&self, waiter: ScheduleWaiter) {
        *lock_recover(&self.schedule_waiter) = Some(waiter);
    }
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
    key_expr: String,
    publisher: Option<Publisher<'static>>,
    subscriber: Option<Subscriber<()>>,
    session: Option<Session>,
    inbox: Arc<ZenohInbox>,
    config: ZenohChannelConfig,
    health: BackendHealthTracker,
    received: Option<ZenohReceived<T>>,
    revision: u64,
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
        let inbox = Arc::new(ZenohInbox::new(config.depth()));
        let parts = open_zenoh_parts(key_expr, config, Arc::clone(&inbox))?;

        Ok(Self {
            key_expr: key_expr.to_string(),
            publisher: Some(parts.publisher),
            subscriber: Some(parts.subscriber),
            session: Some(parts.session),
            inbox,
            config,
            health: BackendHealthTracker::new(ReconnectPolicy::default()),
            received: None,
            revision: 0,
        })
    }

    /// 构造一个不可用 endpoint，用于 generated shell 在 startup open 失败后保留结构化状态。
    pub fn unavailable(
        key_expr: &str,
        config: ZenohChannelConfig,
        error: impl Into<String>,
    ) -> Self {
        let depth = config.depth();
        let mut health = BackendHealthTracker::new(ReconnectPolicy::default());
        health.mark_failed(error.into(), 0);
        Self {
            key_expr: key_expr.to_string(),
            publisher: None,
            subscriber: None,
            session: None,
            inbox: Arc::new(ZenohInbox::new(depth)),
            config,
            health,
            received: None,
            revision: 0,
        }
    }

    /// 注册 scheduler 数据到达唤醒器。
    ///
    /// Zenoh 接收 callback 只把 canonical frame 放入有界 inbox，并通过该 waiter 通知 generated
    /// scheduler；用户回调仍在 FlowRT scheduler 线程中同步执行。
    pub fn set_schedule_waiter(&mut self, waiter: ScheduleWaiter) {
        self.inbox.set_schedule_waiter(waiter);
    }

    /// 带 FlowRT runtime 时间戳发布一个 canonical-wire payload。
    pub fn publish_at(&mut self, value: T, published_at_ms: u64) -> Result<(), ZenohError> {
        self.ensure_ready("publish Zenoh sample")?;
        let frame = encode_frame(&value, published_at_ms)?;
        let publisher = self
            .publisher
            .as_ref()
            .ok_or_else(|| ZenohError::new("publish Zenoh sample", "endpoint is not ready"))?;
        match publisher.put(frame).wait() {
            Ok(()) => {
                self.health.mark_ready();
                Ok(())
            }
            Err(error) => {
                let original = ZenohError::transport("publish Zenoh sample", error);
                self.mark_transport_error(&original);
                self.recover_after_transport_error("publish Zenoh sample")?;
                let frame = encode_frame(&value, published_at_ms)?;
                let publisher = self.publisher.as_ref().ok_or_else(|| {
                    ZenohError::new("publish Zenoh sample", "endpoint is not ready")
                })?;
                publisher
                    .put(frame)
                    .wait()
                    .map_err(|error| ZenohError::transport("publish Zenoh sample", error))
                    .inspect(|_| self.health.mark_ready())
                    .inspect_err(|error| self.mark_transport_error(error))
            }
        }
    }

    /// 非阻塞接收当前可用的最新样本，并按 runtime 时间计算 freshness。
    ///
    /// 接收侧使用 Zenoh `RingChannel`；其回调在 ring 满时丢弃最旧样本，通知消费者时使用
    /// 非阻塞发送。latest channel 排空当前可用样本并保留最新值；FIFO channel 每次只消费
    /// 最旧的一项。两种路径都通过 `try_recv` 读取，不等待网络或新样本到达。
    pub fn receive_latest_at(&mut self, now_ms: u64) -> Result<Latest<'_, T>, ZenohError> {
        self.ensure_ready("receive Zenoh sample")?;
        if self.config.is_latest() {
            while let Some(sample) = self.try_receive_sample_with_recovery()? {
                self.received = Some(decode_sample(sample)?);
                self.revision = self.revision.saturating_add(1);
            }
        } else if let Some(sample) = self.try_receive_sample_with_recovery()? {
            self.received = Some(decode_sample(sample)?);
            self.revision = self.revision.saturating_add(1);
        }

        Ok(self.cached_latest_at(now_ms))
    }

    /// 接收当前 latest view，并同时返回接收后的修订号。
    pub fn receive_latest_with_revision_at(
        &mut self,
        now_ms: u64,
    ) -> Result<(Latest<'_, T>, u64), ZenohError> {
        self.ensure_ready("receive Zenoh sample")?;
        if self.config.is_latest() {
            while let Some(sample) = self.try_receive_sample_with_recovery()? {
                self.received = Some(decode_sample(sample)?);
                self.revision = self.revision.saturating_add(1);
            }
        } else if let Some(sample) = self.try_receive_sample_with_recovery()? {
            self.received = Some(decode_sample(sample)?);
            self.revision = self.revision.saturating_add(1);
        }
        let revision = self.revision;
        Ok((self.cached_latest_at(now_ms), revision))
    }

    /// 返回最近一次已接收样本的 cached latest view，不触碰 transport。
    pub fn cached_latest_at(&self, now_ms: u64) -> Latest<'_, T> {
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

        Latest::new(value, stale)
    }

    /// 判断底层 Zenoh session 是否仍处于打开状态。
    ///
    /// 返回 `true` 只表示本地 endpoint 可执行操作，不表示当前已有远端 subscriber 或 router。
    pub fn ready(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| !session.is_closed())
    }

    /// 返回 endpoint 健康快照。
    pub fn health(&self) -> BackendHealthSnapshot {
        if !self.ready() && self.health.snapshot().state == BackendHealthState::Ready {
            return BackendHealthSnapshot {
                state: BackendHealthState::Degraded,
                last_error: Some("Zenoh session is closed".to_string()),
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

    /// 返回 endpoint 的 channel 配置。
    pub fn config(&self) -> ZenohChannelConfig {
        self.config
    }

    /// 返回接收侧已接受样本的修订号，用于调度器检测新到达数据。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn try_receive_sample(&self) -> Result<Option<Vec<u8>>, ZenohError> {
        Ok(self.inbox.pop())
    }

    fn try_receive_sample_with_recovery(&mut self) -> Result<Option<Vec<u8>>, ZenohError> {
        match self.try_receive_sample() {
            Ok(sample) => {
                self.health.mark_ready();
                Ok(sample)
            }
            Err(error) => {
                self.mark_transport_error(&error);
                self.recover_after_transport_error("receive Zenoh sample")?;
                self.try_receive_sample()
                    .inspect(|_| self.health.mark_ready())
                    .inspect_err(|error| self.mark_transport_error(error))
            }
        }
    }

    fn ensure_ready(&mut self, operation: &'static str) -> Result<(), ZenohError> {
        if self.ready() {
            return Ok(());
        }
        if self.health.snapshot().state != BackendHealthState::Reconnecting {
            let error = ZenohError::new(operation, "Zenoh session is closed");
            self.mark_transport_error(&error);
        }
        self.recover_after_transport_error(operation)
    }

    fn mark_transport_error(&mut self, error: &ZenohError) {
        self.health.mark_degraded(error.to_string());
    }

    fn recover_after_transport_error(&mut self, operation: &'static str) -> Result<(), ZenohError> {
        let snapshot = self.health.snapshot();
        if let Some(next_retry_unix_ms) = snapshot.next_retry_unix_ms
            && snapshot.state == BackendHealthState::Reconnecting
            && unix_now_ms() < next_retry_unix_ms
        {
            return Err(ZenohError::new(
                operation,
                "Zenoh endpoint reconnect backoff is active",
            ));
        }

        let attempt = snapshot.attempt;
        if !self.health.policy().can_retry(attempt) {
            self.health
                .mark_failed("Zenoh endpoint reconnect budget exhausted", attempt);
            return Err(ZenohError::new(
                operation,
                "Zenoh endpoint reconnect budget exhausted",
            ));
        }

        let now_ms = unix_now_ms();
        self.health.mark_reconnecting(
            attempt,
            now_ms.saturating_add(self.health.policy().delay_for_attempt(attempt)),
        );
        match open_zenoh_parts(&self.key_expr, self.config, Arc::clone(&self.inbox)) {
            Ok(parts) => {
                self.publisher = Some(parts.publisher);
                self.subscriber = Some(parts.subscriber);
                self.session = Some(parts.session);
                self.health.mark_ready();
                Ok(())
            }
            Err(error) => {
                let next_attempt = attempt.saturating_add(1);
                if self.health.policy().can_retry(next_attempt) {
                    self.health.mark_reconnecting(
                        next_attempt,
                        now_ms.saturating_add(self.health.policy().delay_for_attempt(next_attempt)),
                    );
                } else {
                    self.health.mark_failed(error.to_string(), next_attempt);
                }
                Err(error)
            }
        }
    }
}

fn open_zenoh_parts(
    key_expr: &str,
    _config: ZenohChannelConfig,
    inbox: Arc<ZenohInbox>,
) -> Result<ZenohEndpointParts, ZenohError> {
    let session = zenoh::open(config_from_environment()?)
        .wait()
        .map_err(|error| ZenohError::transport("open Zenoh session", error))?;
    let callback_inbox = Arc::clone(&inbox);
    let subscriber = session
        .declare_subscriber(key_expr.to_owned())
        .callback(move |sample| {
            callback_inbox.push(sample.payload().to_bytes().to_vec());
        })
        .wait()
        .map_err(|error| ZenohError::transport("declare Zenoh subscriber", error))?;
    let publisher = session
        .declare_publisher(key_expr.to_owned())
        .wait()
        .map_err(|error| ZenohError::transport("declare Zenoh publisher", error))?;
    Ok(ZenohEndpointParts {
        publisher,
        subscriber,
        session,
    })
}

/// 从 `FLOWRT_ZENOH_*` 环境变量打开一个 zenoh session。
///
/// 供生成 runtime shell 在进程内为 zenoh service client/server 创建一个共享 session；
/// 网络配置由 FlowRT runtime 环境注入，不进入用户组件 API。
pub fn open_session_from_environment() -> Result<Session, ZenohError> {
    zenoh::open(config_from_environment()?)
        .wait()
        .map_err(|error| ZenohError::transport("open Zenoh session", error))
}

fn overflow_policy_name(policy: OverflowPolicy) -> &'static str {
    match policy {
        OverflowPolicy::DropOldest => "drop_oldest",
        OverflowPolicy::DropNewest => "drop_newest",
        OverflowPolicy::Error => "error",
        OverflowPolicy::Block => "block",
    }
}

/// 从 FLOWRT_ZENOH_* 环境变量构建 zenoh session 配置。
pub fn config_from_environment() -> Result<Config, ZenohError> {
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

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
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

fn decode_sample<T>(frame: Vec<u8>) -> Result<ZenohReceived<T>, ZenohError>
where
    T: FrameCodec + Clone,
{
    decode_frame(&frame)
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn deadline_from_service_header(header: &ServiceFrameHeader) -> Deadline {
    Deadline {
        timeout_ms: header.timeout_ms,
        absolute_deadline_ms: header.absolute_deadline_ms,
    }
}

fn validate_service_response_header(
    header: &ServiceFrameHeader,
    request_id: RequestId,
    correlation_id: u64,
    schema_hash: u64,
) -> Result<(), &'static str> {
    if header.service_id != request_id.service_id
        || header.session_id != request_id.session_id
        || header.sequence != request_id.sequence
    {
        return Err("response request id mismatch");
    }
    if header.correlation_id != correlation_id {
        return Err("response correlation id mismatch");
    }
    if header.schema_hash != schema_hash {
        return Err("response schema hash mismatch");
    }
    Ok(())
}

fn service_transport_timeout(timeout_ms: u64) -> std::time::Duration {
    std::time::Duration::from_millis(timeout_ms.saturating_add(SERVICE_TRANSPORT_TIMEOUT_GRACE_MS))
}

fn service_error_degrades_health(error: ServiceError) -> bool {
    matches!(
        error,
        ServiceError::Timeout
            | ServiceError::Unavailable
            | ServiceError::DeadlineExceeded
            | ServiceError::Protocol
            | ServiceError::Backend
    )
}

fn record_service_result<T>(health: &Mutex<BackendHealthTracker>, result: &ServiceResult<T>) {
    match result {
        ServiceResult::Ok(_) => lock_recover(health).mark_ready(),
        ServiceResult::Err(error, message) if service_error_degrades_health(*error) => {
            let detail = message
                .as_deref()
                .unwrap_or_else(|| service_error_health_message(*error));
            lock_recover(health).mark_degraded(detail);
        }
        ServiceResult::Err(_, _) => {}
    }
}

fn service_error_health_message(error: ServiceError) -> &'static str {
    match error {
        ServiceError::Timeout => "zenoh service timeout",
        ServiceError::Unavailable => "zenoh service unavailable",
        ServiceError::DeadlineExceeded => "zenoh service deadline exceeded",
        ServiceError::Protocol => "zenoh service protocol error",
        ServiceError::Backend => "zenoh service backend error",
        _ => "zenoh service error",
    }
}

fn service_result_from_response_error_code<T>(
    error_code: u16,
    error_msg: &[u8],
) -> ServiceResult<T> {
    let message = if error_msg.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(error_msg).to_string())
    };
    match ServiceError::from_abi(error_code) {
        Some(error) => ServiceResult::Err(error, message),
        None => {
            let message = match message {
                Some(message) => format!("unknown service error code {error_code}: {message}"),
                None => format!("unknown service error code {error_code}"),
            };
            ServiceResult::err_with_message(ServiceError::Protocol, message)
        }
    }
}

/// Zenoh service client。
///
/// 使用 zenoh query 实现 request/response 语义。client 发送 query，server 通过 queryable
/// 接收并回复。client 不持有 session 所有权，session 生命周期由调用方管理。
pub struct ZenohServiceClient<Req: FrameCodec + Clone, Resp: FrameCodec + Clone> {
    service_name: String,
    service_id: u64,
    key_expr: String,
    session: Session,
    health: Mutex<BackendHealthTracker>,
    session_id: u64,
    sequence: Mutex<u64>,
    _phantom: std::marker::PhantomData<(Req, Resp)>,
}

impl<Req: FrameCodec + Clone, Resp: FrameCodec + Clone> ZenohServiceClient<Req, Resp> {
    /// 使用已有 session 打开 zenoh service client。
    pub fn open(service_name: &str, session: Session) -> Self {
        let service_id = fnv1a64(service_name.as_bytes());
        let key_expr = service_key_expr(service_name);

        Self {
            service_name: service_name.to_string(),
            service_id,
            key_expr,
            session,
            health: Mutex::new(BackendHealthTracker::new(ReconnectPolicy::default())),
            session_id: rand::random(),
            sequence: Mutex::new(0),
            _phantom: std::marker::PhantomData,
        }
    }

    /// 发送请求并等待响应。
    pub fn call(&self, request: Req, timeout_ms: u64) -> ServiceResult<Resp> {
        let result = self.call_inner(request, timeout_ms);
        record_service_result(&self.health, &result);
        result
    }

    fn call_inner(&self, request: Req, timeout_ms: u64) -> ServiceResult<Resp> {
        let now_ms = unix_now_ms();
        let deadline = match Deadline::new(timeout_ms, now_ms) {
            Some(d) => d,
            None => return ServiceResult::err(ServiceError::Timeout),
        };

        let sequence = {
            let mut seq = lock_recover(&self.sequence);
            let s = *seq;
            *seq = s.wrapping_add(1);
            s
        };

        let request_id = RequestId {
            session_id: self.session_id,
            sequence,
            service_id: self.service_id,
        };

        let payload = match request.to_frame_vec() {
            Ok(p) => p,
            Err(e) => {
                return ServiceResult::err_with_message(
                    ServiceError::Protocol,
                    format!("encode request payload: {e}"),
                );
            }
        };

        let correlation_id = 0;
        let schema_hash = 0;
        let header = ServiceFrameHeader::request(request_id, deadline, correlation_id, schema_hash);
        let frame = match encode_service_frame(&header, &payload, &[]) {
            Ok(f) => f,
            Err(e) => {
                return ServiceResult::err_with_message(
                    ServiceError::Protocol,
                    format!("encode service frame: {e}"),
                );
            }
        };

        let receiver = match self
            .session
            .get(&self.key_expr)
            .with(zenoh::handlers::FifoChannel::new(10))
            .payload(zenoh::bytes::ZBytes::from(frame))
            .timeout(service_transport_timeout(timeout_ms))
            .wait()
        {
            Ok(receiver) => receiver,
            Err(e) => {
                return ServiceResult::err_with_message(
                    ServiceError::Backend,
                    format!("zenoh query failed: {e}"),
                );
            }
        };

        let reply = match receiver.recv_timeout(std::time::Duration::from_millis(timeout_ms)) {
            Ok(Some(reply)) => reply,
            Ok(None) => {
                return ServiceResult::err(ServiceError::Timeout);
            }
            Err(_) => {
                // zenoh query timeout closes the channel, treat as service timeout
                return ServiceResult::err(ServiceError::Timeout);
            }
        };

        let sample = match reply.result() {
            Ok(sample) => sample,
            Err(reply_err) => {
                let err_payload = reply_err.payload().to_bytes().to_vec();
                if let Ok((resp_header, _, error_msg)) = decode_service_frame(&err_payload) {
                    if let Err(reason) = validate_service_response_header(
                        &resp_header,
                        request_id,
                        correlation_id,
                        schema_hash,
                    ) {
                        return ServiceResult::err_with_message(ServiceError::Protocol, reason);
                    }
                    return service_result_from_response_error_code(
                        resp_header.error_code,
                        &error_msg,
                    );
                }
                // zenoh query timeout returns an empty ReplyError with default encoding
                return ServiceResult::err(ServiceError::Timeout);
            }
        };

        let reply_payload = sample.payload().to_bytes().to_vec();
        let (resp_header, resp_payload, resp_error_msg) = match decode_service_frame(&reply_payload)
        {
            Ok(f) => f,
            Err(e) => {
                return ServiceResult::err_with_message(
                    ServiceError::Protocol,
                    format!("decode response frame: {e}"),
                );
            }
        };

        if let Err(reason) =
            validate_service_response_header(&resp_header, request_id, correlation_id, schema_hash)
        {
            return ServiceResult::err_with_message(ServiceError::Protocol, reason);
        }

        if resp_header.error_code != ServiceError::Ok as u16 {
            return service_result_from_response_error_code(
                resp_header.error_code,
                &resp_error_msg,
            );
        }

        match Resp::decode_frame(&resp_payload) {
            Ok(resp) => ServiceResult::ok(resp),
            Err(e) => ServiceResult::err_with_message(
                ServiceError::Protocol,
                format!("decode response payload: {e}"),
            ),
        }
    }

    /// 返回 service 名称。
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// 返回 endpoint 健康快照。
    pub fn health(&self) -> BackendHealthSnapshot {
        lock_recover(&self.health).snapshot()
    }
}

/// Zenoh service server 的运行时配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZenohServiceConfig {
    max_in_flight: usize,
}

impl ZenohServiceConfig {
    /// 设置同时执行的 handler 数量上限；0 会归一为 1。
    pub fn with_max_in_flight(mut self, max_in_flight: usize) -> Self {
        self.max_in_flight = max_in_flight.max(1);
        self
    }

    /// 返回同时执行的 handler 数量上限。
    pub const fn max_in_flight(&self) -> usize {
        self.max_in_flight
    }
}

impl Default for ZenohServiceConfig {
    fn default() -> Self {
        Self {
            max_in_flight: DEFAULT_ZENOH_SERVICE_MAX_IN_FLIGHT,
        }
    }
}

struct ZenohServiceInFlightGuard {
    counter: Arc<AtomicUsize>,
}

impl ZenohServiceInFlightGuard {
    fn try_acquire(counter: &Arc<AtomicUsize>, limit: usize) -> Option<Self> {
        let limit = limit.max(1);
        let mut current = counter.load(Ordering::Acquire);
        loop {
            if current >= limit {
                return None;
            }
            match counter.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(Self {
                        counter: Arc::clone(counter),
                    });
                }
                Err(next) => current = next,
            }
        }
    }
}

impl Drop for ZenohServiceInFlightGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Zenoh service server。
///
/// 使用 zenoh queryable 实现 request/response 语义。server 注册 queryable，接收请求并回复。
/// server 不持有 session 所有权，session 生命周期由调用方管理。
pub struct ZenohServiceServer<
    Req: FrameCodec + Clone + Send + 'static,
    Resp: FrameCodec + Clone + Send + 'static,
> {
    service_name: String,
    #[allow(dead_code)]
    service_id: u64,
    #[allow(dead_code)]
    key_expr: String,
    _queryable: Queryable<()>,
    #[allow(dead_code)]
    handler: Arc<dyn Fn(Req) -> ServiceResult<Resp> + Send + Sync + 'static>,
    health: Arc<Mutex<BackendHealthTracker>>,
    in_flight: Arc<AtomicUsize>,
    max_in_flight: usize,
}

impl<Req: FrameCodec + Clone + Send + 'static, Resp: FrameCodec + Clone + Send + 'static>
    ZenohServiceServer<Req, Resp>
{
    /// 使用已有 session 打开 zenoh service server。
    pub fn open<F>(service_name: &str, session: Session, handler: F) -> Result<Self, ZenohError>
    where
        F: Fn(Req) -> ServiceResult<Resp> + Send + Sync + 'static,
    {
        Self::open_with_config(
            service_name,
            session,
            ZenohServiceConfig::default(),
            handler,
        )
    }

    /// 使用已有 session 和显式配置打开 zenoh service server。
    pub fn open_with_config<F>(
        service_name: &str,
        session: Session,
        config: ZenohServiceConfig,
        handler: F,
    ) -> Result<Self, ZenohError>
    where
        F: Fn(Req) -> ServiceResult<Resp> + Send + Sync + 'static,
    {
        let service_id = fnv1a64(service_name.as_bytes());
        let key_expr = service_key_expr(service_name);
        let max_in_flight = config.max_in_flight();

        let handler = Arc::new(handler);
        let handler_clone = Arc::clone(&handler);
        let service_id_clone = service_id;
        let key_expr_clone = key_expr.clone();
        let key_expr_for_callback = key_expr.clone();
        let health = Arc::new(Mutex::new(BackendHealthTracker::new(
            ReconnectPolicy::default(),
        )));
        let health_clone = Arc::clone(&health);
        let in_flight = Arc::new(AtomicUsize::new(0));
        let in_flight_clone = Arc::clone(&in_flight);

        let queryable = session
            .declare_queryable(&key_expr)
            .callback(move |query: Query| {
                let reply_ke = query.key_expr().clone();

                let payload = match query.payload() {
                    Some(p) => p.to_bytes().to_vec(),
                    None => {
                        let header = ServiceFrameHeader::response(
                            RequestId {
                                session_id: 0,
                                sequence: 0,
                                service_id: service_id_clone,
                            },
                            Deadline::new(1000, unix_now_ms())
                                .unwrap_or_else(|| Deadline::new(1000, 0).unwrap()),
                            0,
                            0,
                            ServiceError::Protocol,
                        );
                        let frame = encode_service_frame(&header, &[], b"empty request payload")
                            .unwrap_or_default();
                        lock_recover(&health_clone).mark_degraded("empty request payload");
                        let _ = query
                            .reply(reply_ke, zenoh::bytes::ZBytes::from(frame))
                            .wait();
                        return;
                    }
                };

                let (req_header, req_payload, _) = match decode_service_frame(&payload) {
                    Ok(f) => f,
                    Err(e) => {
                        let header = ServiceFrameHeader::response(
                            RequestId {
                                session_id: 0,
                                sequence: 0,
                                service_id: service_id_clone,
                            },
                            Deadline::new(1000, unix_now_ms())
                                .unwrap_or_else(|| Deadline::new(1000, 0).unwrap()),
                            0,
                            0,
                            ServiceError::Protocol,
                        );
                        let msg = format!("decode request frame: {e}");
                        let frame =
                            encode_service_frame(&header, &[], msg.as_bytes()).unwrap_or_default();
                        lock_recover(&health_clone).mark_degraded(msg);
                        let _ = query
                            .reply(reply_ke, zenoh::bytes::ZBytes::from(frame))
                            .wait();
                        return;
                    }
                };
                let now_ms = unix_now_ms();
                let deadline = deadline_from_service_header(&req_header);
                if req_header.service_id != service_id_clone {
                    let header = ServiceFrameHeader::response(
                        RequestId {
                            session_id: req_header.session_id,
                            sequence: req_header.sequence,
                            service_id: service_id_clone,
                        },
                        deadline,
                        req_header.correlation_id,
                        req_header.schema_hash,
                        ServiceError::Protocol,
                    );
                    let frame = encode_service_frame(&header, &[], b"request service id mismatch")
                        .unwrap_or_default();
                    lock_recover(&health_clone).mark_degraded("request service id mismatch");
                    let _ = query
                        .reply(reply_ke, zenoh::bytes::ZBytes::from(frame))
                        .wait();
                    return;
                }
                if deadline.expired(now_ms) {
                    let header = ServiceFrameHeader::response(
                        RequestId {
                            session_id: req_header.session_id,
                            sequence: req_header.sequence,
                            service_id: service_id_clone,
                        },
                        deadline,
                        req_header.correlation_id,
                        req_header.schema_hash,
                        ServiceError::Timeout,
                    );
                    let frame = encode_service_frame(&header, &[], b"request deadline expired")
                        .unwrap_or_default();
                    lock_recover(&health_clone).mark_degraded("request deadline expired");
                    let _ = query
                        .reply(reply_ke, zenoh::bytes::ZBytes::from(frame))
                        .wait();
                    return;
                }

                let request = match Req::decode_frame(&req_payload) {
                    Ok(r) => r,
                    Err(e) => {
                        let header = ServiceFrameHeader::response(
                            RequestId {
                                session_id: req_header.session_id,
                                sequence: req_header.sequence,
                                service_id: service_id_clone,
                            },
                            deadline,
                            req_header.correlation_id,
                            req_header.schema_hash,
                            ServiceError::Protocol,
                        );
                        let msg = format!("decode request payload: {e}");
                        let frame =
                            encode_service_frame(&header, &[], msg.as_bytes()).unwrap_or_default();
                        lock_recover(&health_clone).mark_degraded(msg);
                        let _ = query
                            .reply(reply_ke, zenoh::bytes::ZBytes::from(frame))
                            .wait();
                        return;
                    }
                };

                let remaining_ms = deadline.absolute_deadline_ms.saturating_sub(unix_now_ms());
                let Some(in_flight_guard) =
                    ZenohServiceInFlightGuard::try_acquire(&in_flight_clone, max_in_flight)
                else {
                    let header = ServiceFrameHeader::response(
                        RequestId {
                            session_id: req_header.session_id,
                            sequence: req_header.sequence,
                            service_id: service_id_clone,
                        },
                        deadline,
                        req_header.correlation_id,
                        req_header.schema_hash,
                        ServiceError::Busy,
                    );
                    let frame = encode_service_frame(
                        &header,
                        &[],
                        b"zenoh service max in-flight requests reached",
                    )
                    .unwrap_or_default();
                    lock_recover(&health_clone)
                        .mark_degraded("zenoh service max in-flight requests reached");
                    let _ = query
                        .reply(reply_ke, zenoh::bytes::ZBytes::from(frame))
                        .wait();
                    return;
                };
                let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
                let handler_for_worker = Arc::clone(&handler_clone);
                let health_for_worker = Arc::clone(&health_clone);
                let worker = std::thread::Builder::new()
                    .name(format!("flowrt-zenoh-service-{key_expr_for_callback}"))
                    .spawn(move || {
                        let _in_flight_guard = in_flight_guard;
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            handler_for_worker(request)
                        }))
                        .unwrap_or_else(|_| {
                            ServiceResult::err_with_message(
                                ServiceError::HandlerError,
                                "service handler panicked",
                            )
                        });
                        record_service_result(&health_for_worker, &result);
                        let _ = result_tx.send(result);
                    });
                if let Err(err) = worker {
                    let message = format!("failed to spawn zenoh service handler: {err}");
                    lock_recover(&health_clone).mark_degraded(message.clone());
                    let header = ServiceFrameHeader::response(
                        RequestId {
                            session_id: req_header.session_id,
                            sequence: req_header.sequence,
                            service_id: service_id_clone,
                        },
                        deadline,
                        req_header.correlation_id,
                        req_header.schema_hash,
                        ServiceError::Backend,
                    );
                    let frame =
                        encode_service_frame(&header, &[], message.as_bytes()).unwrap_or_default();
                    let _ = query
                        .reply(reply_ke, zenoh::bytes::ZBytes::from(frame))
                        .wait();
                    return;
                }

                let result = match result_rx
                    .recv_timeout(std::time::Duration::from_millis(remaining_ms))
                {
                    Ok(result) => result,
                    Err(_) => {
                        let header = ServiceFrameHeader::response(
                            RequestId {
                                session_id: req_header.session_id,
                                sequence: req_header.sequence,
                                service_id: service_id_clone,
                            },
                            deadline,
                            req_header.correlation_id,
                            req_header.schema_hash,
                            ServiceError::Timeout,
                        );
                        let frame = encode_service_frame(
                            &header,
                            &[],
                            b"request deadline expired while handler was running",
                        )
                        .unwrap_or_default();
                        lock_recover(&health_clone)
                            .mark_degraded("request deadline expired while handler was running");
                        let _ = query
                            .reply(reply_ke, zenoh::bytes::ZBytes::from(frame))
                            .wait();
                        return;
                    }
                };
                let (error_code, response_payload, error_msg) = match result {
                    ServiceResult::Ok(resp) => {
                        let payload = resp.to_frame_vec().unwrap_or_default();
                        (ServiceError::Ok, payload, Vec::new())
                    }
                    ServiceResult::Err(code, msg) => {
                        let msg_bytes = msg.unwrap_or_default().into_bytes();
                        (code, Vec::new(), msg_bytes)
                    }
                };

                let header = ServiceFrameHeader::response(
                    RequestId {
                        session_id: req_header.session_id,
                        sequence: req_header.sequence,
                        service_id: service_id_clone,
                    },
                    deadline,
                    req_header.correlation_id,
                    req_header.schema_hash,
                    error_code,
                );
                let frame = encode_service_frame(&header, &response_payload, &error_msg)
                    .unwrap_or_default();
                if let Err(error) = query
                    .reply(reply_ke, zenoh::bytes::ZBytes::from(frame))
                    .wait()
                {
                    lock_recover(&health_clone)
                        .mark_degraded(format!("zenoh service reply failed: {error:?}"));
                }
            })
            .wait()
            .map_err(|error| ZenohError::transport("declare Zenoh queryable", error))?;

        Ok(Self {
            service_name: service_name.to_string(),
            service_id,
            key_expr: key_expr_clone,
            _queryable: queryable,
            handler,
            health,
            in_flight,
            max_in_flight,
        })
    }

    /// 返回 service 名称。
    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    /// 返回 endpoint 健康快照。
    pub fn health(&self) -> BackendHealthSnapshot {
        lock_recover(&self.health).snapshot()
    }

    /// 返回当前正在执行的 handler 数量。
    pub fn in_flight_count(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }

    /// 返回同时执行的 handler 数量上限。
    pub const fn max_in_flight(&self) -> usize {
        self.max_in_flight
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::AtomicUsize,
        sync::atomic::{AtomicU64, Ordering},
        thread,
        time::{Duration, Instant},
    };

    use super::*;
    use crate::{
        BackendHealthState, OverflowPolicy, StaleConfig, StalePolicy, WireCodec, WireCodecError,
    };

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

    #[repr(C)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct DecodeFailingMessage;

    impl WireCodec for DecodeFailingMessage {
        const WIRE_SIZE: usize = 1;

        fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
            if output.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
            }
            output[0] = 0xFF;
            Ok(())
        }

        fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
            if input.len() != Self::WIRE_SIZE {
                return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
            }
            Err(WireCodecError::invalid_frame(
                "intentional decode failure for health regression",
            ))
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
        let _zenoh_guard = crate::zenoh_test_guard();
        let key_expr = unique_key_expr("latest-roundtrip");
        let mut endpoint =
            ZenohPubSub::<PaddedMessage>::open_with_config(&key_expr, ZenohChannelConfig::latest())
                .expect("zenoh endpoint should open");
        assert!(endpoint.ready());
        assert_eq!(endpoint.health().state, BackendHealthState::Ready);
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
        let _zenoh_guard = crate::zenoh_test_guard();
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
        let _zenoh_guard = crate::zenoh_test_guard();
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

    #[test]
    fn endpoint_can_receive_after_peer_endpoint_restarts() {
        let _zenoh_guard = crate::zenoh_test_guard();
        let key_expr = unique_key_expr("peer-restart");
        let mut receiver =
            ZenohPubSub::<PaddedMessage>::open_with_config(&key_expr, ZenohChannelConfig::latest())
                .expect("receiver endpoint should open");
        {
            let mut sender = ZenohPubSub::<PaddedMessage>::open_with_config(
                &key_expr,
                ZenohChannelConfig::latest(),
            )
            .expect("first sender endpoint should open");
            publish_until_latest(
                &mut sender,
                &mut receiver,
                PaddedMessage { tag: 1, value: 10 },
                100,
                101,
            );
        }

        let mut sender =
            ZenohPubSub::<PaddedMessage>::open_with_config(&key_expr, ZenohChannelConfig::latest())
                .expect("restarted sender endpoint should open");
        publish_until_latest(
            &mut sender,
            &mut receiver,
            PaddedMessage { tag: 2, value: 20 },
            110,
            111,
        );
    }

    #[test]
    fn endpoint_recovers_after_local_session_is_closed() {
        let _zenoh_guard = crate::zenoh_test_guard();
        let key_expr = unique_key_expr("local-recovery");
        let mut endpoint =
            ZenohPubSub::<PaddedMessage>::open_with_config(&key_expr, ZenohChannelConfig::latest())
                .expect("endpoint should open before forced close");
        assert_eq!(endpoint.health().state, BackendHealthState::Ready);

        endpoint
            .session
            .as_ref()
            .expect("endpoint should hold a live session before forced close")
            .close()
            .wait()
            .expect("test should be able to close the local zenoh session");
        assert!(!endpoint.ready());

        endpoint
            .publish_at(PaddedMessage { tag: 7, value: 70 }, 700)
            .expect("publish should reopen a locally closed zenoh session");

        assert!(endpoint.ready());
        assert_eq!(endpoint.health().state, BackendHealthState::Ready);

        let expected = PaddedMessage { tag: 7, value: 70 };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let latest = endpoint
                .receive_latest_at(701)
                .expect("reopened zenoh endpoint should receive latest view");
            if latest.as_ref() == Some(&expected) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "reopened zenoh endpoint did not receive its recovered sample"
            );
            endpoint
                .publish_at(expected, 700)
                .expect("reopened zenoh endpoint should keep publishing");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn decode_errors_do_not_mark_endpoint_reconnecting() {
        let _zenoh_guard = crate::zenoh_test_guard();
        let key_expr = unique_key_expr("decode-error-health");
        let mut endpoint = ZenohPubSub::<DecodeFailingMessage>::open_with_config(
            &key_expr,
            ZenohChannelConfig::latest(),
        )
        .expect("endpoint should open");

        endpoint
            .publish_at(DecodeFailingMessage, 800)
            .expect("invalid payload should still publish");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match endpoint.receive_latest_at(801) {
                Ok(_) => {}
                Err(error) => {
                    assert_eq!(error.operation(), "decode FlowRT Zenoh payload");
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "local zenoh roundtrip did not deliver invalid payload"
            );
            thread::sleep(Duration::from_millis(10));
        }

        assert_eq!(endpoint.health().state, BackendHealthState::Ready);
    }

    #[test]
    fn service_in_flight_guard_enforces_bound_and_releases_on_drop() {
        let counter = Arc::new(AtomicUsize::new(0));
        let first = ZenohServiceInFlightGuard::try_acquire(&counter, 1)
            .expect("first request should acquire slot");

        assert!(ZenohServiceInFlightGuard::try_acquire(&counter, 1).is_none());
        assert_eq!(counter.load(Ordering::Acquire), 1);

        drop(first);

        assert_eq!(counter.load(Ordering::Acquire), 0);
        let second = ZenohServiceInFlightGuard::try_acquire(&counter, 1)
            .expect("slot should be reusable after guard drops");
        drop(second);
    }

    #[test]
    fn unknown_service_error_code_is_not_classified_as_backend() {
        let result: ServiceResult<()> =
            service_result_from_response_error_code(99, b"future service error");

        assert_eq!(result.error_code(), ServiceError::Protocol);
        assert_eq!(
            result.error_message(),
            Some("unknown service error code 99: future service error")
        );
    }

    #[test]
    fn service_key_expr_escapes_underscore_to_avoid_collisions() {
        assert_ne!(service_key_expr("a/b"), service_key_expr("a_x2F_b"));
        assert_eq!(
            service_key_expr("a_x2F_b"),
            "flowrt/service/a_x5F_x2F_x5F_b/request"
        );
    }

    fn publish_until_latest(
        sender: &mut ZenohPubSub<PaddedMessage>,
        receiver: &mut ZenohPubSub<PaddedMessage>,
        expected: PaddedMessage,
        published_at_ms: u64,
        now_ms: u64,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            sender
                .publish_at(expected, published_at_ms)
                .expect("sender should publish while waiting for peer discovery");
            let latest = receiver
                .receive_latest_at(now_ms)
                .expect("nonblocking latest receive should succeed");
            if latest.as_ref() == Some(&expected) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "local zenoh roundtrip did not deliver expected sample after peer restart"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
