//! FlowRT Operation runtime primitives。
//!
//! Operation 是 typed long-running command。runtime primitive 只负责状态机、policy、
//! cooperative cancel、progress carrier 和健康计数；backend 传输、codegen lowering 和 CLI
//! 控制面在更高层接入。

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(feature = "iox2")]
use crate::ZeroCopySend;
use crate::{FrameCodec, ServiceError, WireCodec, WireCodecError};

/// 唯一标识一次 Operation invocation。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "iox2", derive(ZeroCopySend))]
pub struct OperationId {
    /// Operation endpoint canonical name 的稳定 key。
    pub operation_key: u64,
    /// 发起方 runtime/client 标识。
    pub client_id: u64,
    /// 发起方内单调递增序号。
    pub sequence: u64,
}

impl OperationId {
    /// 构造 Operation invocation ID。
    pub const fn new(operation_key: u64, client_id: u64, sequence: u64) -> Self {
        Self {
            operation_key,
            client_id,
            sequence,
        }
    }
}

impl WireCodec for OperationId {
    const WIRE_SIZE: usize = u64::WIRE_SIZE * 3;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        self.operation_key.encode_wire(&mut output[0..8])?;
        self.client_id.encode_wire(&mut output[8..16])?;
        self.sequence.encode_wire(&mut output[16..24])?;
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        Ok(Self {
            operation_key: u64::decode_wire(&input[0..8])?,
            client_id: u64::decode_wire(&input[8..16])?,
            sequence: u64::decode_wire(&input[16..24])?,
        })
    }
}

/// Operation control authority owner。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "iox2", derive(ZeroCopySend))]
pub struct OperationOwner {
    /// 控制域 key；同一 Operation endpoint 使用同一 scope。
    pub scope_key: u64,
    /// owner key；默认由 generated client endpoint 派生。
    pub owner_key: u64,
}

impl OperationOwner {
    /// 构造 Operation owner。
    pub const fn new(scope_key: u64, owner_key: u64) -> Self {
        Self {
            scope_key,
            owner_key,
        }
    }
}

impl WireCodec for OperationOwner {
    const WIRE_SIZE: usize = u64::WIRE_SIZE * 2;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        self.scope_key.encode_wire(&mut output[0..8])?;
        self.owner_key.encode_wire(&mut output[8..16])?;
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        Ok(Self {
            scope_key: u64::decode_wire(&input[0..8])?,
            owner_key: u64::decode_wire(&input[8..16])?,
        })
    }
}

/// Operation 状态机状态。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "iox2", derive(ZeroCopySend))]
pub enum OperationState {
    /// 没有 active invocation。
    #[default]
    Idle,
    /// start request 已接受，尚未进入用户 handler。
    Starting,
    /// 用户 handler 正在执行。
    Running,
    /// 已请求 cooperative cancel，等待用户 handler 观察 token 并退出。
    CancelRequested,
    /// 用户 handler 成功完成。
    Succeeded,
    /// 用户 handler 或 runtime 执行失败。
    Failed,
    /// 用户 handler 响应 cancel 请求并结束。
    Cancelled,
    /// Operation 超时。
    TimedOut,
}

impl OperationState {
    /// 判断状态是否为终态。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }

    /// 返回 status/record 使用的 canonical snake_case 名称。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::CancelRequested => "cancel_requested",
            Self::Cancelled => "cancelled",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
        }
    }
}

impl WireCodec for OperationState {
    const WIRE_SIZE: usize = u8::WIRE_SIZE;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        let value = match self {
            Self::Idle => 0u8,
            Self::Starting => 1,
            Self::Running => 2,
            Self::CancelRequested => 3,
            Self::Succeeded => 4,
            Self::Failed => 5,
            Self::Cancelled => 6,
            Self::TimedOut => 7,
        };
        value.encode_wire(output)
    }

    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        match u8::decode_wire(input)? {
            0 => Ok(Self::Idle),
            1 => Ok(Self::Starting),
            2 => Ok(Self::Running),
            3 => Ok(Self::CancelRequested),
            4 => Ok(Self::Succeeded),
            5 => Ok(Self::Failed),
            6 => Ok(Self::Cancelled),
            7 => Ok(Self::TimedOut),
            _ => Err(WireCodecError::invalid_frame(
                "operation state discriminant is unknown",
            )),
        }
    }
}

/// Operation 并发策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationConcurrencyPolicy {
    /// 当前已有 invocation 时拒绝新 start request。
    Reject,
    /// 当前已有 invocation 时按有界队列排队。
    Queue,
}

/// Operation 抢占策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationPreemptPolicy {
    /// 不抢占正在运行的 invocation。
    Reject,
    /// 请求 cancel 当前 invocation 后启动新 invocation。
    CancelRunning,
}

/// Operation runtime 错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationError {
    /// 状态转换不合法。
    InvalidTransition {
        /// 当前状态。
        from: OperationState,
        /// 目标状态。
        to: OperationState,
    },
    /// policy 字段非法。
    InvalidPolicy(&'static str),
}

/// Operation control authority 错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationControlError {
    /// 操作成功。
    Ok,
    /// 状态转换不合法。
    InvalidTransition {
        /// 当前状态。
        from: OperationState,
        /// 目标状态。
        to: OperationState,
    },
    /// policy 字段非法。
    InvalidPolicy(&'static str),
    /// 当前 single-owner operation 已有同 owner active invocation。
    Busy {
        /// 当前 owner。
        active_owner: OperationOwner,
    },
    /// 当前 single-owner operation 已由其他 owner 控制。
    OwnerConflict {
        /// 当前 owner。
        active_owner: OperationOwner,
        /// 请求 owner。
        requested_owner: OperationOwner,
    },
    /// cancel/status/complete 指向了非当前 invocation。
    StaleInvocation {
        /// 请求中的 invocation id。
        requested: OperationId,
        /// 当前 invocation id。
        current: Option<OperationId>,
    },
    /// invocation 已进入终态。
    AlreadyTerminal {
        /// 当前终态。
        state: OperationState,
    },
}

impl std::fmt::Display for OperationControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::InvalidTransition { from, to } => {
                write!(f, "invalid operation transition from {from:?} to {to:?}")
            }
            Self::InvalidPolicy(field) => write!(f, "invalid operation policy field `{field}`"),
            Self::Busy { active_owner } => write!(
                f,
                "operation owner {} already has an active invocation",
                active_owner.owner_key
            ),
            Self::OwnerConflict {
                active_owner,
                requested_owner,
            } => write!(
                f,
                "operation owner conflict: active owner {} requested owner {}",
                active_owner.owner_key, requested_owner.owner_key
            ),
            Self::StaleInvocation { requested, current } => write!(
                f,
                "stale operation invocation {}:{}:{}, current={:?}",
                requested.operation_key, requested.client_id, requested.sequence, current
            ),
            Self::AlreadyTerminal { state } => {
                write!(
                    f,
                    "operation invocation already terminal: {}",
                    state.as_str()
                )
            }
        }
    }
}

impl std::error::Error for OperationControlError {}

impl From<OperationError> for OperationControlError {
    fn from(value: OperationError) -> Self {
        match value {
            OperationError::InvalidTransition { from, to } => Self::InvalidTransition { from, to },
            OperationError::InvalidPolicy(field) => Self::InvalidPolicy(field),
        }
    }
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTransition { from, to } => {
                write!(f, "invalid operation transition from {from:?} to {to:?}")
            }
            Self::InvalidPolicy(field) => write!(f, "invalid operation policy field `{field}`"),
        }
    }
}

impl std::error::Error for OperationError {}

/// Operation client 调用错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationClientError {
    Timeout,
    Unavailable,
    Busy,
    Rejected,
    Cancelled,
    Backend,
    WouldDeadlock,
    HandlerError,
}

impl OperationClientError {
    /// 从内部 service 错误映射到 Operation 用户错误。
    pub const fn from_service_error(error: ServiceError) -> Self {
        match error {
            ServiceError::Timeout | ServiceError::DeadlineExceeded => Self::Timeout,
            ServiceError::Unavailable => Self::Unavailable,
            ServiceError::Busy => Self::Busy,
            ServiceError::Rejected => Self::Rejected,
            ServiceError::Cancelled => Self::Cancelled,
            ServiceError::Backend | ServiceError::Protocol => Self::Backend,
            ServiceError::WouldDeadlock => Self::WouldDeadlock,
            ServiceError::HandlerError | ServiceError::Ok => Self::HandlerError,
        }
    }
}

/// Operation policy。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationPolicy {
    /// Operation invocation 超时时间。
    pub timeout: Duration,
    /// 并发 invocation 策略。
    pub concurrency: OperationConcurrencyPolicy,
    /// 抢占策略。
    pub preempt: OperationPreemptPolicy,
    /// 等待队列深度。
    pub queue_depth: u32,
    /// 最大同时 in-flight invocation 数。
    pub max_in_flight: u32,
    /// 终态结果快照保留时间；0 表示只在 active 生命周期内可见。
    pub result_retention: Duration,
}

impl OperationPolicy {
    /// 构造并校验 Operation policy。
    pub const fn new(
        timeout: Duration,
        concurrency: OperationConcurrencyPolicy,
        preempt: OperationPreemptPolicy,
        queue_depth: u32,
        max_in_flight: u32,
    ) -> Result<Self, OperationError> {
        if timeout.as_millis() == 0 {
            return Err(OperationError::InvalidPolicy("timeout_ms"));
        }
        if queue_depth == 0 {
            return Err(OperationError::InvalidPolicy("queue_depth"));
        }
        if max_in_flight == 0 {
            return Err(OperationError::InvalidPolicy("max_in_flight"));
        }
        Ok(Self {
            timeout,
            concurrency,
            preempt,
            queue_depth,
            max_in_flight,
            result_retention: Duration::ZERO,
        })
    }

    /// 返回设置了终态结果保留时间的 policy。
    pub const fn with_result_retention(
        mut self,
        result_retention: Duration,
    ) -> Result<Self, OperationError> {
        self.result_retention = result_retention;
        Ok(self)
    }

    fn validate(self) -> Result<Self, OperationError> {
        Self::new(
            self.timeout,
            self.concurrency,
            self.preempt,
            self.queue_depth,
            self.max_in_flight,
        )
        .and_then(|policy| policy.with_result_retention(self.result_retention))
    }
}

impl Default for OperationPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_millis(30_000),
            concurrency: OperationConcurrencyPolicy::Reject,
            preempt: OperationPreemptPolicy::Reject,
            queue_depth: 8,
            max_in_flight: 1,
            result_retention: Duration::ZERO,
        }
    }
}

/// Cooperative cancel token。
#[derive(Debug, Clone)]
pub struct OperationCancelToken {
    canceled: Arc<AtomicBool>,
}

impl OperationCancelToken {
    /// 构造未取消的 token。
    pub fn new() -> Self {
        Self {
            canceled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 请求用户 handler 在安全边界自行退出。
    pub fn request_cancel(&self) {
        self.canceled.store(true, Ordering::SeqCst);
    }

    /// 查询是否已有 cancel 请求。
    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::SeqCst)
    }
}

impl Default for OperationCancelToken {
    fn default() -> Self {
        Self::new()
    }
}

/// Operation 健康计数快照。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "iox2", derive(ZeroCopySend))]
pub struct OperationHealthSnapshot {
    pub started: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub canceled: u64,
    pub timeout: u64,
    /// 保留历史计数槽；当前 v0.13 lifecycle 不再产生 preempted 状态。
    pub preempted: u64,
}

impl WireCodec for OperationHealthSnapshot {
    const WIRE_SIZE: usize = u64::WIRE_SIZE * 6;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        self.started.encode_wire(&mut output[0..8])?;
        self.succeeded.encode_wire(&mut output[8..16])?;
        self.failed.encode_wire(&mut output[16..24])?;
        self.canceled.encode_wire(&mut output[24..32])?;
        self.timeout.encode_wire(&mut output[32..40])?;
        self.preempted.encode_wire(&mut output[40..48])?;
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        Ok(Self {
            started: u64::decode_wire(&input[0..8])?,
            succeeded: u64::decode_wire(&input[8..16])?,
            failed: u64::decode_wire(&input[16..24])?,
            canceled: u64::decode_wire(&input[24..32])?,
            timeout: u64::decode_wire(&input[32..40])?,
            preempted: u64::decode_wire(&input[40..48])?,
        })
    }
}

/// Operation 健康计数器。
#[derive(Debug, Default)]
pub struct OperationHealthCounters {
    started: AtomicU64,
    succeeded: AtomicU64,
    failed: AtomicU64,
    canceled: AtomicU64,
    timeout: AtomicU64,
    preempted: AtomicU64,
}

impl OperationHealthCounters {
    /// 按状态进入事件记录计数。
    pub fn record_state(&self, state: OperationState) {
        match state {
            OperationState::Running => {
                self.started.fetch_add(1, Ordering::Relaxed);
            }
            OperationState::Succeeded => {
                self.succeeded.fetch_add(1, Ordering::Relaxed);
            }
            OperationState::Failed => {
                self.failed.fetch_add(1, Ordering::Relaxed);
            }
            OperationState::Cancelled => {
                self.canceled.fetch_add(1, Ordering::Relaxed);
            }
            OperationState::TimedOut => {
                self.timeout.fetch_add(1, Ordering::Relaxed);
            }
            OperationState::Idle | OperationState::Starting | OperationState::CancelRequested => {}
        }
    }

    /// 记录一次抢占事件。
    pub fn record_preempted(&self) {
        self.preempted.fetch_add(1, Ordering::Relaxed);
    }

    /// 读取健康计数快照。
    pub fn snapshot(&self) -> OperationHealthSnapshot {
        OperationHealthSnapshot {
            started: self.started.load(Ordering::Relaxed),
            succeeded: self.succeeded.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            canceled: self.canceled.load(Ordering::Relaxed),
            timeout: self.timeout.load(Ordering::Relaxed),
            preempted: self.preempted.load(Ordering::Relaxed),
        }
    }
}

/// Operation 状态快照。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "iox2", derive(ZeroCopySend))]
pub struct OperationStatusSnapshot {
    pub id: OperationId,
    pub owner: OperationOwner,
    pub state: OperationState,
    pub cancel_requested: bool,
    pub deadline_ms: u64,
    pub health: OperationHealthSnapshot,
}

impl WireCodec for OperationStatusSnapshot {
    const WIRE_SIZE: usize = OperationId::WIRE_SIZE
        + OperationOwner::WIRE_SIZE
        + OperationState::WIRE_SIZE
        + bool::WIRE_SIZE
        + u64::WIRE_SIZE
        + OperationHealthSnapshot::WIRE_SIZE;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        self.id.encode_wire(&mut output[0..24])?;
        self.owner.encode_wire(&mut output[24..40])?;
        self.state.encode_wire(&mut output[40..41])?;
        self.cancel_requested.encode_wire(&mut output[41..42])?;
        self.deadline_ms.encode_wire(&mut output[42..50])?;
        self.health.encode_wire(&mut output[50..98])?;
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        Ok(Self {
            id: OperationId::decode_wire(&input[0..24])?,
            owner: OperationOwner::decode_wire(&input[24..40])?,
            state: OperationState::decode_wire(&input[40..41])?,
            cancel_requested: bool::decode_wire(&input[41..42])?,
            deadline_ms: u64::decode_wire(&input[42..50])?,
            health: OperationHealthSnapshot::decode_wire(&input[50..98])?,
        })
    }
}

/// Operation start 请求被 runtime 接受后的响应。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "iox2", derive(ZeroCopySend))]
pub struct OperationStartAck {
    pub id: OperationId,
    pub owner: OperationOwner,
    pub deadline_ms: u64,
    pub accepted: bool,
}

impl OperationStartAck {
    /// 构造 accepted ack。
    pub const fn accepted(id: OperationId) -> Self {
        Self {
            id,
            owner: OperationOwner {
                scope_key: 0,
                owner_key: id.client_id,
            },
            deadline_ms: 0,
            accepted: true,
        }
    }

    /// 构造带 control authority 元数据的 accepted ack。
    pub const fn accepted_with_authority(
        id: OperationId,
        owner: OperationOwner,
        deadline_ms: u64,
    ) -> Self {
        Self {
            id,
            owner,
            deadline_ms,
            accepted: true,
        }
    }
}

impl WireCodec for OperationStartAck {
    const WIRE_SIZE: usize =
        OperationId::WIRE_SIZE + OperationOwner::WIRE_SIZE + u64::WIRE_SIZE + bool::WIRE_SIZE;

    fn encode_wire(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, output.len()));
        }
        self.id.encode_wire(&mut output[0..24])?;
        self.owner.encode_wire(&mut output[24..40])?;
        self.deadline_ms.encode_wire(&mut output[40..48])?;
        self.accepted.encode_wire(&mut output[48..49])?;
        Ok(())
    }

    fn decode_wire(input: &[u8]) -> Result<Self, WireCodecError> {
        if input.len() != Self::WIRE_SIZE {
            return Err(WireCodecError::wrong_size(Self::WIRE_SIZE, input.len()));
        }
        Ok(Self {
            id: OperationId::decode_wire(&input[0..24])?,
            owner: OperationOwner::decode_wire(&input[24..40])?,
            deadline_ms: u64::decode_wire(&input[40..48])?,
            accepted: bool::decode_wire(&input[48..49])?,
        })
    }
}

/// Operation start 内部 lowering 请求。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OperationStartRequest<T> {
    pub goal: T,
    pub owner: OperationOwner,
    pub timeout_ms: u64,
}

#[cfg(feature = "iox2")]
unsafe impl<T: ZeroCopySend> ZeroCopySend for OperationStartRequest<T> {}

impl<T> OperationStartRequest<T> {
    /// 构造 start 请求 envelope。用户 API 仍只暴露 typed goal 和 timeout。
    pub const fn new(goal: T, owner: OperationOwner, timeout: Duration) -> Self {
        let millis = timeout.as_millis();
        Self {
            goal,
            owner,
            timeout_ms: if millis > u64::MAX as u128 {
                u64::MAX
            } else {
                millis as u64
            },
        }
    }

    /// 返回 start 请求 timeout。
    pub const fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }
}

impl<T> FrameCodec for OperationStartRequest<T>
where
    T: FrameCodec,
{
    fn encoded_frame_size(&self) -> usize {
        OperationOwner::WIRE_SIZE + u64::WIRE_SIZE + self.goal.encoded_frame_size()
    }

    fn encode_frame(&self, output: &mut [u8]) -> Result<(), WireCodecError> {
        if output.len() != self.encoded_frame_size() {
            return Err(WireCodecError::wrong_size(
                self.encoded_frame_size(),
                output.len(),
            ));
        }
        self.owner.encode_wire(&mut output[0..16])?;
        self.timeout_ms.encode_wire(&mut output[16..24])?;
        self.goal.encode_frame(&mut output[24..])?;
        Ok(())
    }

    fn decode_frame(input: &[u8]) -> Result<Self, WireCodecError> {
        let header_size = OperationOwner::WIRE_SIZE + u64::WIRE_SIZE;
        if input.len() < header_size {
            return Err(WireCodecError::wrong_size(header_size, input.len()));
        }
        Ok(Self {
            owner: OperationOwner::decode_wire(&input[0..16])?,
            timeout_ms: u64::decode_wire(&input[16..24])?,
            goal: T::decode_frame(&input[24..])?,
        })
    }
}

/// Operation progress event carrier。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationProgress<T> {
    pub id: OperationId,
    pub sequence: u64,
    pub value: T,
}

impl<T> OperationProgress<T> {
    /// 构造 progress event。
    pub const fn new(id: OperationId, sequence: u64, value: T) -> Self {
        Self {
            id,
            sequence,
            value,
        }
    }
}

/// Operation progress 进入 runtime observation log 的 hook。
pub type OperationProgressHook = dyn Fn(OperationId, u64, Option<Vec<u8>>) + Send + Sync;

/// Operation progress 发布器。
///
/// 生成的 server handler 通过该类型发布 typed feedback。当前 inproc lowering 在 handler
/// 返回后统一取走事件；后续跨进程 backend 可把相同事件流接到 transport channel。
#[derive(Clone)]
pub struct OperationProgressPublisher<T> {
    id: OperationId,
    next_sequence: u64,
    events: Vec<OperationProgress<T>>,
    progress_hook: Option<Arc<OperationProgressHook>>,
}

impl<T: std::fmt::Debug> std::fmt::Debug for OperationProgressPublisher<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationProgressPublisher")
            .field("id", &self.id)
            .field("next_sequence", &self.next_sequence)
            .field("events", &self.events)
            .finish_non_exhaustive()
    }
}

impl<T> OperationProgressPublisher<T> {
    /// 构造指定 invocation 的 progress 发布器。
    pub const fn new(id: OperationId) -> Self {
        Self {
            id,
            next_sequence: 0,
            events: Vec::new(),
            progress_hook: None,
        }
    }

    /// 构造带 runtime event hook 的 progress 发布器。
    pub fn with_hook(id: OperationId, progress_hook: Arc<OperationProgressHook>) -> Self {
        Self {
            id,
            next_sequence: 0,
            events: Vec::new(),
            progress_hook: Some(progress_hook),
        }
    }

    /// 借用当前已发布事件。
    pub fn events(&self) -> &[OperationProgress<T>] {
        &self.events
    }

    /// 取走当前已发布事件。
    pub fn drain(&mut self) -> Vec<OperationProgress<T>> {
        std::mem::take(&mut self.events)
    }
}

impl<T> OperationProgressPublisher<T>
where
    T: FrameCodec,
{
    /// 发布一条 progress event。
    pub fn publish(&mut self, value: T) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let payload = value.to_frame_vec().ok();
        if let Some(hook) = &self.progress_hook {
            hook(self.id, sequence, payload);
        }
        self.events
            .push(OperationProgress::new(self.id, sequence, value));
    }
}

/// Operation server handler 的 typed 结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationHandlerResult<T> {
    Succeeded(T),
    Failed,
    Canceled,
}

impl<T> OperationHandlerResult<T> {
    /// 构造成功结果。
    pub const fn succeeded(value: T) -> Self {
        Self::Succeeded(value)
    }

    /// 构造失败结果。
    pub const fn failed() -> Self {
        Self::Failed
    }

    /// 构造取消结果。
    pub const fn canceled() -> Self {
        Self::Canceled
    }
}

/// Operation 生命周期状态机。
#[derive(Debug)]
pub struct OperationLifecycle {
    id: OperationId,
    owner: OperationOwner,
    deadline_ms: u64,
    policy: OperationPolicy,
    state: OperationState,
    cancel_token: OperationCancelToken,
    health: OperationHealthCounters,
}

impl OperationLifecycle {
    /// 构造已接受但尚未运行的 Operation lifecycle。
    pub fn new(id: OperationId, policy: OperationPolicy) -> Result<Self, OperationError> {
        Self::new_with_authority(
            id,
            policy,
            OperationOwner::new(0, id.client_id),
            policy.timeout.as_millis() as u64,
        )
    }

    /// 构造带 owner/deadline 的 Operation lifecycle。
    pub fn new_with_authority(
        id: OperationId,
        policy: OperationPolicy,
        owner: OperationOwner,
        deadline_ms: u64,
    ) -> Result<Self, OperationError> {
        Ok(Self {
            id,
            owner,
            deadline_ms,
            policy: policy.validate()?,
            state: OperationState::Starting,
            cancel_token: OperationCancelToken::new(),
            health: OperationHealthCounters::default(),
        })
    }

    /// 返回 invocation ID。
    pub const fn id(&self) -> OperationId {
        self.id
    }

    /// 返回 policy。
    pub const fn policy(&self) -> OperationPolicy {
        self.policy
    }

    /// 返回当前状态。
    pub const fn state(&self) -> OperationState {
        self.state
    }

    /// 返回 owner。
    pub const fn owner(&self) -> OperationOwner {
        self.owner
    }

    /// 返回 absolute deadline（runtime monotonic 毫秒）。
    pub const fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }

    /// 返回可共享给用户 handler 的 cancel token。
    pub fn cancel_token(&self) -> OperationCancelToken {
        self.cancel_token.clone()
    }

    /// 进入下一个状态。
    pub fn transition(&mut self, to: OperationState) -> Result<(), OperationError> {
        let from = self.state;
        if !valid_transition(from, to) {
            return Err(OperationError::InvalidTransition { from, to });
        }
        self.state = to;
        self.health.record_state(to);
        Ok(())
    }

    /// 请求 cooperative cancel。
    pub fn request_cancel(&mut self) -> Result<(), OperationError> {
        self.cancel_token.request_cancel();
        match self.state {
            OperationState::Starting | OperationState::Running => {
                self.transition(OperationState::CancelRequested)
            }
            OperationState::CancelRequested => Ok(()),
            state if state.is_terminal() => Err(OperationError::InvalidTransition {
                from: state,
                to: OperationState::CancelRequested,
            }),
            _ => Err(OperationError::InvalidTransition {
                from: self.state,
                to: OperationState::CancelRequested,
            }),
        }
    }

    /// 返回当前状态快照。
    pub fn snapshot(&self) -> OperationStatusSnapshot {
        OperationStatusSnapshot {
            id: self.id,
            owner: self.owner,
            state: self.state,
            cancel_requested: self.cancel_token.is_canceled(),
            deadline_ms: self.deadline_ms,
            health: self.health.snapshot(),
        }
    }
}

fn valid_transition(from: OperationState, to: OperationState) -> bool {
    use OperationState as S;
    matches!(
        (from, to),
        (S::Idle, S::Starting)
            | (S::Starting, S::Running)
            | (S::Starting, S::CancelRequested)
            | (S::Starting, S::Failed)
            | (S::Starting, S::TimedOut)
            | (S::Running, S::Succeeded)
            | (S::Running, S::Failed)
            | (S::Running, S::CancelRequested)
            | (S::Running, S::TimedOut)
            | (S::CancelRequested, S::Cancelled)
            | (S::CancelRequested, S::Failed)
            | (S::CancelRequested, S::TimedOut)
    )
}

/// Operation runtime event 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationRuntimeEventKind {
    StateChanged,
    Progress,
    Result,
    Error,
}

/// Operation runtime event。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRuntimeEvent {
    pub id: OperationId,
    pub kind: OperationRuntimeEventKind,
    pub state: Option<OperationState>,
    pub sequence: Option<u64>,
    pub payload: Option<Vec<u8>>,
    pub message: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
struct RetainedOperationStatus {
    snapshot: OperationStatusSnapshot,
    expires_at_ms: Option<u64>,
}

/// Single-owner Operation control state。
#[derive(Debug)]
pub struct OperationControl {
    operation_key: u64,
    policy: OperationPolicy,
    next_sequence: u64,
    lifecycle: Option<OperationLifecycle>,
    in_flight: VecDeque<OperationLifecycle>,
    queue: VecDeque<OperationLifecycle>,
    handler_active: bool,
    health: OperationHealthCounters,
    retained_results: BTreeMap<OperationId, RetainedOperationStatus>,
    events: Vec<OperationRuntimeEvent>,
}

impl OperationControl {
    /// 构造空闲 Operation control state。
    pub fn new(operation_key: u64, policy: OperationPolicy) -> Self {
        Self {
            operation_key,
            policy,
            next_sequence: 0,
            lifecycle: None,
            in_flight: VecDeque::new(),
            queue: VecDeque::new(),
            handler_active: false,
            health: OperationHealthCounters::default(),
            retained_results: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    /// 使用 policy timeout 启动一次 invocation。
    pub fn start(
        &mut self,
        owner: OperationOwner,
        now_ms: u64,
    ) -> Result<OperationStartAck, OperationControlError> {
        self.start_with_timeout(owner, now_ms, self.policy.timeout)
    }

    /// 使用显式 timeout 启动一次 invocation。
    pub fn start_with_timeout(
        &mut self,
        owner: OperationOwner,
        now_ms: u64,
        timeout: Duration,
    ) -> Result<OperationStartAck, OperationControlError> {
        if self.active_count() > 0 {
            if let Some(active) = self.lifecycle.as_ref() {
                if active.owner() != owner {
                    return Err(OperationControlError::OwnerConflict {
                        active_owner: active.owner(),
                        requested_owner: owner,
                    });
                }
            }
        }

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let id = OperationId::new(self.operation_key, owner.owner_key, sequence);
        let timeout_ms = duration_millis_u64(timeout).max(1);
        let deadline_ms = now_ms.saturating_add(timeout_ms);
        let lifecycle =
            OperationLifecycle::new_with_authority(id, self.policy, owner, deadline_ms)?;
        let ack = OperationStartAck::accepted_with_authority(id, owner, deadline_ms);

        if self.active_count() > 0 {
            if self.policy.preempt == OperationPreemptPolicy::CancelRunning {
                self.preempt_active_invocations()?;
                self.lifecycle = Some(lifecycle);
                self.handler_active = true;
                self.push_state_event(id, OperationState::Starting);
                return Ok(ack);
            }
            if self.active_count() < self.policy.max_in_flight as usize {
                self.in_flight.push_back(lifecycle);
                self.push_state_event(id, OperationState::Starting);
                return Ok(ack);
            }
            if self.policy.concurrency == OperationConcurrencyPolicy::Queue {
                if self.queue.len() >= self.policy.queue_depth as usize {
                    return Err(OperationControlError::Busy {
                        active_owner: self
                            .lifecycle
                            .as_ref()
                            .map(OperationLifecycle::owner)
                            .unwrap_or(owner),
                    });
                }
                self.queue.push_back(lifecycle);
                self.push_state_event(id, OperationState::Starting);
                return Ok(ack);
            }
            if let Some(active) = self.lifecycle.as_ref() {
                return Err(OperationControlError::Busy {
                    active_owner: active.owner(),
                });
            }
        }

        self.lifecycle = Some(lifecycle);
        self.handler_active = true;
        self.push_state_event(id, OperationState::Starting);
        Ok(ack)
    }

    /// 标记 invocation 已进入用户 handler。
    pub fn mark_running(&mut self, id: OperationId) -> Result<(), OperationControlError> {
        let lifecycle = self.current_mut(id)?;
        lifecycle.transition(OperationState::Running)?;
        self.health.record_state(OperationState::Running);
        self.push_state_event(id, OperationState::Running);
        Ok(())
    }

    /// 请求 cooperative cancel。
    pub fn request_cancel(
        &mut self,
        id: OperationId,
        owner: OperationOwner,
    ) -> Result<OperationStatusSnapshot, OperationControlError> {
        let lifecycle = self.current_mut(id)?;
        if lifecycle.owner() != owner {
            return Err(OperationControlError::OwnerConflict {
                active_owner: lifecycle.owner(),
                requested_owner: owner,
            });
        }
        if lifecycle.state().is_terminal() {
            return Err(OperationControlError::AlreadyTerminal {
                state: lifecycle.state(),
            });
        }
        lifecycle.request_cancel()?;
        self.push_state_event(id, OperationState::CancelRequested);
        self.status(id)
    }

    /// 完成 invocation。若 runtime 已先进入 timeout 终态，释放 handler 并返回终态错误。
    pub fn complete(
        &mut self,
        id: OperationId,
        terminal_state: OperationState,
    ) -> Result<(), OperationControlError> {
        self.complete_with_payload(id, terminal_state, None)
    }

    /// 完成 invocation，并附带已编码的 typed result payload。
    pub fn complete_with_payload(
        &mut self,
        id: OperationId,
        terminal_state: OperationState,
        payload: Option<Vec<u8>>,
    ) -> Result<(), OperationControlError> {
        self.complete_with_payload_at(id, terminal_state, payload, monotonic_time_ms())
    }

    /// 完成 invocation，并用显式时间驱动 result retention。
    pub fn complete_at(
        &mut self,
        id: OperationId,
        terminal_state: OperationState,
        completed_at_ms: u64,
    ) -> Result<(), OperationControlError> {
        self.complete_with_payload_at(id, terminal_state, None, completed_at_ms)
    }

    /// 完成 invocation，并用显式时间驱动 result retention，同时携带 typed result payload。
    pub fn complete_with_payload_at(
        &mut self,
        id: OperationId,
        terminal_state: OperationState,
        payload: Option<Vec<u8>>,
        completed_at_ms: u64,
    ) -> Result<(), OperationControlError> {
        if !terminal_state.is_terminal() {
            let from = self
                .lifecycle
                .as_ref()
                .filter(|lifecycle| lifecycle.id() == id)
                .map(OperationLifecycle::state)
                .or_else(|| {
                    self.retained_results
                        .get(&id)
                        .map(|entry| entry.snapshot.state)
                })
                .unwrap_or(OperationState::Idle);
            return Err(OperationControlError::InvalidTransition {
                from,
                to: terminal_state,
            });
        }

        if let Some((mut lifecycle, was_primary)) = self.remove_active(id) {
            if lifecycle.state().is_terminal() {
                let state = lifecycle.state();
                self.handler_active = false;
                if was_primary {
                    self.lifecycle = Some(lifecycle);
                } else {
                    self.in_flight.push_front(lifecycle);
                }
                return Err(OperationControlError::AlreadyTerminal { state });
            }
            lifecycle.transition(terminal_state)?;
            self.health.record_state(terminal_state);
            self.handler_active = false;
            self.push_result_event(id, terminal_state, payload);
            self.push_state_event(id, terminal_state);
            let mut snapshot = lifecycle.snapshot();
            snapshot.health = self.health.snapshot();
            let should_keep_terminal_primary = was_primary
                && self.in_flight.is_empty()
                && self.queue.is_empty()
                && self.retention_deadline(completed_at_ms).is_none();
            if should_keep_terminal_primary {
                self.lifecycle = Some(lifecycle);
            } else {
                self.retain_terminal_snapshot(snapshot, completed_at_ms);
                if was_primary {
                    self.promote_next_active();
                }
                self.promote_queued_until_capacity();
            }
            return Ok(());
        }

        let expires_at_ms = self.retention_deadline(completed_at_ms);
        if let Some(entry) = self.retained_results.get_mut(&id) {
            let from = entry.snapshot.state;
            if from.is_terminal() {
                return Err(OperationControlError::AlreadyTerminal { state: from });
            }
            if !valid_transition(from, terminal_state) {
                return Err(OperationControlError::InvalidTransition {
                    from,
                    to: terminal_state,
                });
            }
            self.health.record_state(terminal_state);
            entry.snapshot.state = terminal_state;
            entry.snapshot.health = self.health.snapshot();
            entry.expires_at_ms = expires_at_ms;
            let should_remove = entry.expires_at_ms.is_none();
            let _ = entry;
            self.push_result_event(id, terminal_state, payload);
            self.push_state_event(id, terminal_state);
            if should_remove {
                self.retained_results.remove(&id);
            }
            return Ok(());
        }

        Err(OperationControlError::StaleInvocation {
            requested: id,
            current: self.lifecycle.as_ref().map(OperationLifecycle::id),
        })
    }

    /// 记录 progress publish。
    pub fn publish_progress(&mut self, id: OperationId, sequence: u64) {
        self.publish_progress_with_payload(id, sequence, None);
    }

    /// 记录带 typed feedback payload 的 progress publish。
    pub fn publish_progress_with_payload(
        &mut self,
        id: OperationId,
        sequence: u64,
        payload: Option<Vec<u8>>,
    ) {
        if self
            .lifecycle
            .as_ref()
            .is_some_and(|lifecycle| lifecycle.id() == id && !lifecycle.state().is_terminal())
            || self
                .in_flight
                .iter()
                .any(|lifecycle| lifecycle.id() == id && !lifecycle.state().is_terminal())
        {
            self.events.push(OperationRuntimeEvent {
                id,
                kind: OperationRuntimeEventKind::Progress,
                state: None,
                sequence: Some(sequence),
                payload,
                message: None,
            });
        }
    }

    /// 由 runtime/scheduler step 驱动 deadline。
    pub fn check_deadline(&mut self, now_ms: u64) -> bool {
        let mut due = Vec::new();
        if let Some(lifecycle) = self.lifecycle.as_ref() {
            if !lifecycle.state().is_terminal() && now_ms >= lifecycle.deadline_ms() {
                due.push(lifecycle.id());
            }
        }
        due.extend(self.in_flight.iter().filter_map(|lifecycle| {
            (!lifecycle.state().is_terminal() && now_ms >= lifecycle.deadline_ms())
                .then_some(lifecycle.id())
        }));
        due.extend(self.queue.iter().filter_map(|lifecycle| {
            (!lifecycle.state().is_terminal() && now_ms >= lifecycle.deadline_ms())
                .then_some(lifecycle.id())
        }));
        let mut changed = false;
        for id in due {
            changed |= self.timeout_active(id, now_ms) || self.timeout_queued(id, now_ms);
        }
        changed
    }

    /// 返回当前 cancel token。
    pub fn cancel_token(&self) -> Option<OperationCancelToken> {
        self.lifecycle
            .as_ref()
            .map(OperationLifecycle::cancel_token)
    }

    /// 返回指定 active invocation 的 cancel token。
    pub fn cancel_token_for(&self, id: OperationId) -> Option<OperationCancelToken> {
        self.lifecycle
            .as_ref()
            .filter(|lifecycle| lifecycle.id() == id)
            .map(OperationLifecycle::cancel_token)
            .or_else(|| {
                self.in_flight
                    .iter()
                    .find(|lifecycle| lifecycle.id() == id)
                    .map(OperationLifecycle::cancel_token)
            })
    }

    /// 判断 invocation 是否已经成为可进入用户 handler 的 active 状态。
    pub fn ready_to_run(&self, id: OperationId) -> bool {
        self.lifecycle.as_ref().is_some_and(|lifecycle| {
            lifecycle.id() == id && lifecycle.state() == OperationState::Starting
        }) || self
            .in_flight
            .iter()
            .any(|lifecycle| lifecycle.id() == id && lifecycle.state() == OperationState::Starting)
    }

    /// 返回当前状态快照。
    pub fn snapshot(&self) -> OperationStatusSnapshot {
        if let Some(lifecycle) = self.lifecycle.as_ref() {
            let mut snapshot = lifecycle.snapshot();
            snapshot.health = self.health.snapshot();
            snapshot
        } else {
            OperationStatusSnapshot {
                id: OperationId::new(self.operation_key, 0, 0),
                owner: OperationOwner::default(),
                state: OperationState::Idle,
                cancel_requested: false,
                deadline_ms: 0,
                health: self.health.snapshot(),
            }
        }
    }

    /// 返回指定 invocation 的状态快照。
    pub fn status(
        &self,
        id: OperationId,
    ) -> Result<OperationStatusSnapshot, OperationControlError> {
        if let Some(lifecycle) = self
            .lifecycle
            .as_ref()
            .filter(|lifecycle| lifecycle.id() == id)
        {
            let mut snapshot = lifecycle.snapshot();
            snapshot.health = self.health.snapshot();
            return Ok(snapshot);
        }
        if let Some(lifecycle) = self.in_flight.iter().find(|lifecycle| lifecycle.id() == id) {
            let mut snapshot = lifecycle.snapshot();
            snapshot.health = self.health.snapshot();
            return Ok(snapshot);
        }
        if let Some(lifecycle) = self.queue.iter().find(|lifecycle| lifecycle.id() == id) {
            let mut snapshot = lifecycle.snapshot();
            snapshot.health = self.health.snapshot();
            return Ok(snapshot);
        }
        if let Some(retained) = self.retained_results.get(&id) {
            return Ok(retained.snapshot);
        }
        Err(OperationControlError::StaleInvocation {
            requested: id,
            current: self.lifecycle.as_ref().map(OperationLifecycle::id),
        })
    }

    /// 返回等待队列中尚未进入 handler 的 invocation 数。
    pub fn queued_len(&self) -> usize {
        self.queue.len()
    }

    /// 清理已超过 result retention 的终态快照。
    pub fn evict_retained_results(&mut self, now_ms: u64) {
        self.retained_results.retain(|_, retained| {
            retained
                .expires_at_ms
                .is_none_or(|deadline| now_ms <= deadline)
        });
    }

    /// 取走 runtime events。
    pub fn drain_events(&mut self) -> Vec<OperationRuntimeEvent> {
        std::mem::take(&mut self.events)
    }

    fn current_mut(
        &mut self,
        id: OperationId,
    ) -> Result<&mut OperationLifecycle, OperationControlError> {
        let current = self.lifecycle.as_ref().map(OperationLifecycle::id);
        if current == Some(id) {
            return Ok(self
                .lifecycle
                .as_mut()
                .expect("checked current lifecycle must exist"));
        }
        if let Some(lifecycle) = self
            .in_flight
            .iter_mut()
            .find(|lifecycle| lifecycle.id() == id)
        {
            return Ok(lifecycle);
        }
        Err(OperationControlError::StaleInvocation {
            requested: id,
            current,
        })
    }

    fn push_state_event(&mut self, id: OperationId, state: OperationState) {
        self.events.push(OperationRuntimeEvent {
            id,
            kind: OperationRuntimeEventKind::StateChanged,
            state: Some(state),
            sequence: None,
            payload: None,
            message: None,
        });
    }

    fn push_result_event(
        &mut self,
        id: OperationId,
        terminal_state: OperationState,
        payload: Option<Vec<u8>>,
    ) {
        let kind = if terminal_state == OperationState::Failed {
            OperationRuntimeEventKind::Error
        } else {
            OperationRuntimeEventKind::Result
        };
        self.events.push(OperationRuntimeEvent {
            id,
            kind,
            state: Some(terminal_state),
            sequence: None,
            payload,
            message: None,
        });
    }

    fn active_count(&self) -> usize {
        usize::from(
            self.lifecycle
                .as_ref()
                .is_some_and(|lifecycle| !lifecycle.state().is_terminal()),
        ) + self
            .in_flight
            .iter()
            .filter(|lifecycle| !lifecycle.state().is_terminal())
            .count()
    }

    fn remove_active(&mut self, id: OperationId) -> Option<(OperationLifecycle, bool)> {
        if self.lifecycle.as_ref().map(OperationLifecycle::id) == Some(id) {
            return self.lifecycle.take().map(|lifecycle| (lifecycle, true));
        }
        let index = self
            .in_flight
            .iter()
            .position(|lifecycle| lifecycle.id() == id)?;
        self.in_flight
            .remove(index)
            .map(|lifecycle| (lifecycle, false))
    }

    fn remove_queued(&mut self, id: OperationId) -> Option<(usize, OperationLifecycle)> {
        let index = self
            .queue
            .iter()
            .position(|lifecycle| lifecycle.id() == id)?;
        self.queue.remove(index).map(|lifecycle| (index, lifecycle))
    }

    fn promote_next_active(&mut self) {
        if self.lifecycle.is_none() {
            self.lifecycle = self.in_flight.pop_front();
        }
    }

    fn promote_queued_until_capacity(&mut self) {
        self.promote_next_active();
        while self.active_count() < self.policy.max_in_flight as usize {
            let Some(next) = self.queue.pop_front() else {
                break;
            };
            if self.lifecycle.is_none() {
                self.lifecycle = Some(next);
            } else {
                self.in_flight.push_back(next);
            }
        }
        self.handler_active = false;
    }

    fn preempt_active_invocations(&mut self) -> Result<(), OperationControlError> {
        let mut active = Vec::new();
        if let Some(lifecycle) = self.lifecycle.take() {
            active.push(lifecycle);
        }
        active.extend(self.in_flight.drain(..));
        for mut lifecycle in active {
            if lifecycle.state().is_terminal() {
                continue;
            }
            let id = lifecycle.id();
            lifecycle.request_cancel()?;
            self.health.record_preempted();
            let mut snapshot = lifecycle.snapshot();
            snapshot.health = self.health.snapshot();
            self.retained_results.insert(
                id,
                RetainedOperationStatus {
                    snapshot,
                    expires_at_ms: None,
                },
            );
            self.push_state_event(id, OperationState::CancelRequested);
        }
        self.handler_active = false;
        Ok(())
    }

    fn timeout_active(&mut self, id: OperationId, now_ms: u64) -> bool {
        let Some((mut lifecycle, was_primary)) = self.remove_active(id) else {
            return false;
        };
        if lifecycle.state().is_terminal() || now_ms < lifecycle.deadline_ms() {
            if was_primary {
                self.lifecycle = Some(lifecycle);
            } else {
                self.in_flight.push_front(lifecycle);
            }
            return false;
        }
        lifecycle.cancel_token.request_cancel();
        if lifecycle.transition(OperationState::TimedOut).is_err() {
            if was_primary {
                self.lifecycle = Some(lifecycle);
            } else {
                self.in_flight.push_front(lifecycle);
            }
            return false;
        }
        self.health.record_state(OperationState::TimedOut);
        self.handler_active = false;
        let mut snapshot = lifecycle.snapshot();
        snapshot.health = self.health.snapshot();
        self.push_state_event(id, OperationState::TimedOut);
        let should_keep_terminal_primary = was_primary
            && self.in_flight.is_empty()
            && self.queue.is_empty()
            && self.retention_deadline(now_ms).is_none();
        if should_keep_terminal_primary {
            self.lifecycle = Some(lifecycle);
        } else {
            self.retain_terminal_snapshot(snapshot, now_ms);
            if was_primary {
                self.promote_next_active();
            }
            self.promote_queued_until_capacity();
        }
        true
    }

    fn timeout_queued(&mut self, id: OperationId, now_ms: u64) -> bool {
        let Some((index, mut lifecycle)) = self.remove_queued(id) else {
            return false;
        };
        if lifecycle.state().is_terminal() || now_ms < lifecycle.deadline_ms() {
            self.queue.insert(index, lifecycle);
            return false;
        }
        lifecycle.cancel_token.request_cancel();
        if lifecycle.transition(OperationState::TimedOut).is_err() {
            self.queue.insert(index, lifecycle);
            return false;
        }
        self.health.record_state(OperationState::TimedOut);
        let mut snapshot = lifecycle.snapshot();
        snapshot.health = self.health.snapshot();
        self.push_state_event(id, OperationState::TimedOut);
        self.retain_terminal_snapshot(snapshot, now_ms);
        true
    }

    fn retain_terminal_snapshot(
        &mut self,
        snapshot: OperationStatusSnapshot,
        completed_at_ms: u64,
    ) {
        let Some(expires_at_ms) = self.retention_deadline(completed_at_ms) else {
            self.retained_results.remove(&snapshot.id);
            return;
        };
        self.retained_results.insert(
            snapshot.id,
            RetainedOperationStatus {
                snapshot,
                expires_at_ms: Some(expires_at_ms),
            },
        );
    }

    fn retention_deadline(&self, completed_at_ms: u64) -> Option<u64> {
        let retention_ms = duration_millis_u64(self.policy.result_retention);
        if retention_ms == 0 {
            None
        } else {
            Some(completed_at_ms.saturating_add(retention_ms))
        }
    }
}

fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

/// 返回 runtime monotonic 毫秒，用于 Operation deadline 驱动。
pub fn monotonic_time_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    duration_millis_u64(START.get_or_init(Instant::now).elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_progress_publisher_assigns_monotonic_sequences() {
        let id = OperationId::new(1, 2, 3);
        let mut publisher = OperationProgressPublisher::new(id);
        publisher.publish(10u32);
        publisher.publish(20u32);

        assert_eq!(publisher.events()[0], OperationProgress::new(id, 0, 10));
        assert_eq!(publisher.events()[1], OperationProgress::new(id, 1, 20));
        assert_eq!(publisher.drain().len(), 2);
        assert!(publisher.events().is_empty());
    }

    #[test]
    fn operation_client_error_maps_service_error() {
        assert_eq!(
            OperationClientError::from_service_error(ServiceError::Backend),
            OperationClientError::Backend
        );
        assert_eq!(
            OperationClientError::from_service_error(ServiceError::WouldDeadlock),
            OperationClientError::WouldDeadlock
        );
        assert_eq!(
            OperationClientError::from_service_error(ServiceError::DeadlineExceeded),
            OperationClientError::Timeout
        );
    }

    #[test]
    fn operation_handler_result_constructors_are_typed() {
        assert_eq!(
            OperationHandlerResult::succeeded(7u32),
            OperationHandlerResult::Succeeded(7)
        );
        assert_eq!(
            OperationHandlerResult::<u32>::failed(),
            OperationHandlerResult::Failed
        );
        assert_eq!(
            OperationHandlerResult::<u32>::canceled(),
            OperationHandlerResult::Canceled
        );
    }

    #[cfg(feature = "iox2")]
    #[test]
    fn operation_control_plane_types_are_iox2_zero_copy() {
        use crate::ZeroCopySend;

        #[repr(C)]
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ZeroCopySend)]
        struct OperationZeroCopyGoal {
            value: u32,
        }

        fn assert_iox2_payload<T>()
        where
            T: std::fmt::Debug + Copy + Default + ZeroCopySend + 'static,
        {
        }

        assert_iox2_payload::<OperationId>();
        assert_iox2_payload::<OperationStartAck>();
        assert_iox2_payload::<OperationStatusSnapshot>();
        assert_iox2_payload::<OperationStartRequest<OperationZeroCopyGoal>>();

        let request = OperationStartRequest::new(
            OperationZeroCopyGoal { value: 7 },
            OperationOwner::new(11, 13),
            Duration::from_millis(123),
        );
        assert_eq!(request.timeout(), Duration::from_millis(123));
    }

    #[test]
    fn operation_control_records_owner_deadline_and_success_lifecycle() {
        let policy = OperationPolicy::new(
            Duration::from_millis(50),
            OperationConcurrencyPolicy::Reject,
            OperationPreemptPolicy::Reject,
            8,
            1,
        )
        .unwrap();
        let owner = OperationOwner::new(10, 20);
        let mut control = OperationControl::new(99, policy);

        assert_eq!(control.snapshot().state, OperationState::Idle);

        let ack = control.start(owner, 100).unwrap();
        assert!(ack.accepted);
        assert_eq!(ack.id, OperationId::new(99, owner.owner_key, 0));
        assert_eq!(ack.owner, owner);
        assert_eq!(ack.deadline_ms, 150);
        assert_eq!(control.snapshot().state, OperationState::Starting);

        control.mark_running(ack.id).unwrap();
        control.complete(ack.id, OperationState::Succeeded).unwrap();

        let snapshot = control.snapshot();
        assert_eq!(snapshot.state, OperationState::Succeeded);
        assert_eq!(snapshot.owner, owner);
        assert_eq!(snapshot.deadline_ms, 150);
        assert_eq!(snapshot.health.started, 1);
        assert_eq!(snapshot.health.succeeded, 1);
        let events = control.drain_events();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].state, Some(OperationState::Starting));
        assert_eq!(events[1].state, Some(OperationState::Running));
        assert_eq!(events[2].kind, OperationRuntimeEventKind::Result);
        assert_eq!(events[2].state, Some(OperationState::Succeeded));
        assert_eq!(events[3].state, Some(OperationState::Succeeded));
    }

    #[test]
    fn operation_control_rejects_second_owner_and_stale_cancel() {
        let policy = OperationPolicy::default();
        let owner_a = OperationOwner::new(10, 20);
        let owner_b = OperationOwner::new(10, 30);
        let mut control = OperationControl::new(99, policy);
        let ack = control.start(owner_a, 100).unwrap();
        control.mark_running(ack.id).unwrap();

        let conflict = control
            .start(owner_b, 101)
            .expect_err("second owner must not control active single-owner operation");
        assert_eq!(
            conflict,
            OperationControlError::OwnerConflict {
                active_owner: owner_a,
                requested_owner: owner_b,
            }
        );

        let stale_id = OperationId::new(99, owner_a.owner_key, 123);
        let stale = control
            .request_cancel(stale_id, owner_a)
            .expect_err("cancel must only affect the current invocation id");
        assert_eq!(
            stale,
            OperationControlError::StaleInvocation {
                requested: stale_id,
                current: Some(ack.id),
            }
        );
        assert_eq!(control.snapshot().state, OperationState::Running);
    }

    #[test]
    fn operation_control_timeout_is_runtime_tick_driven() {
        let policy = OperationPolicy::new(
            Duration::from_millis(5),
            OperationConcurrencyPolicy::Reject,
            OperationPreemptPolicy::Reject,
            8,
            1,
        )
        .unwrap();
        let owner = OperationOwner::new(10, 20);
        let mut control = OperationControl::new(99, policy);
        let ack = control.start(owner, 100).unwrap();
        control.mark_running(ack.id).unwrap();

        assert!(!control.check_deadline(104));
        assert_eq!(control.snapshot().state, OperationState::Running);
        assert!(control.check_deadline(105));

        let snapshot = control.snapshot();
        assert_eq!(snapshot.state, OperationState::TimedOut);
        assert!(snapshot.cancel_requested);
        assert_eq!(snapshot.health.timeout, 1);
        assert!(control.cancel_token().unwrap().is_canceled());
    }

    #[test]
    fn operation_control_handler_error_enters_failed() {
        let owner = OperationOwner::new(10, 20);
        let mut control = OperationControl::new(99, OperationPolicy::default());
        let ack = control.start(owner, 100).unwrap();
        control.mark_running(ack.id).unwrap();
        control.complete(ack.id, OperationState::Failed).unwrap();

        let snapshot = control.snapshot();
        assert_eq!(snapshot.state, OperationState::Failed);
        assert_eq!(snapshot.health.failed, 1);
    }
}
