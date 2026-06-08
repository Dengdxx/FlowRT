//! FlowRT Operation runtime primitives。
//!
//! Operation 是 typed long-running command。runtime primitive 只负责状态机、policy、
//! cooperative cancel、progress carrier 和健康计数；backend 传输、codegen lowering 和 CLI
//! 控制面在更高层接入。

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::ServiceError;

/// 唯一标识一次 Operation invocation。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// Operation 状态机状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationState {
    /// start request 已接受，尚未进入用户 handler。
    Accepted,
    /// 用户 handler 正在执行。
    Running,
    /// 已请求 cooperative cancel，等待用户 handler 观察 token 并退出。
    Canceling,
    /// 用户 handler 成功完成。
    Succeeded,
    /// 用户 handler 或 runtime 执行失败。
    Failed,
    /// 用户 handler 响应 cancel 请求并结束。
    Canceled,
    /// Operation 超时。
    Timeout,
    /// 因 preempt policy 被新 invocation 抢占。
    Preempted,
}

impl OperationState {
    /// 判断状态是否为终态。
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Canceled | Self::Timeout | Self::Preempted
        )
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
        })
    }

    fn validate(self) -> Result<Self, OperationError> {
        Self::new(
            self.timeout,
            self.concurrency,
            self.preempt,
            self.queue_depth,
            self.max_in_flight,
        )
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OperationHealthSnapshot {
    pub started: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub canceled: u64,
    pub timeout: u64,
    pub preempted: u64,
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
            OperationState::Canceled => {
                self.canceled.fetch_add(1, Ordering::Relaxed);
            }
            OperationState::Timeout => {
                self.timeout.fetch_add(1, Ordering::Relaxed);
            }
            OperationState::Preempted => {
                self.preempted.fetch_add(1, Ordering::Relaxed);
            }
            OperationState::Accepted | OperationState::Canceling => {}
        }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationStatusSnapshot {
    pub id: OperationId,
    pub state: OperationState,
    pub cancel_requested: bool,
    pub health: OperationHealthSnapshot,
}

/// Operation start 请求被 runtime 接受后的响应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationStartAck {
    pub id: OperationId,
    pub accepted: bool,
}

impl OperationStartAck {
    /// 构造 accepted ack。
    pub const fn accepted(id: OperationId) -> Self {
        Self { id, accepted: true }
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

/// Operation progress 发布器。
///
/// 生成的 server handler 通过该类型发布 typed feedback。当前 inproc lowering 在 handler
/// 返回后统一取走事件；后续跨进程 backend 可把相同事件流接到 transport channel。
#[derive(Debug, Clone)]
pub struct OperationProgressPublisher<T> {
    id: OperationId,
    next_sequence: u64,
    events: Vec<OperationProgress<T>>,
}

impl<T> OperationProgressPublisher<T> {
    /// 构造指定 invocation 的 progress 发布器。
    pub const fn new(id: OperationId) -> Self {
        Self {
            id,
            next_sequence: 0,
            events: Vec::new(),
        }
    }

    /// 发布一条 progress event。
    pub fn publish(&mut self, value: T) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.events
            .push(OperationProgress::new(self.id, sequence, value));
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
    policy: OperationPolicy,
    state: OperationState,
    cancel_token: OperationCancelToken,
    health: OperationHealthCounters,
}

impl OperationLifecycle {
    /// 构造已接受但尚未运行的 Operation lifecycle。
    pub fn new(id: OperationId, policy: OperationPolicy) -> Result<Self, OperationError> {
        Ok(Self {
            id,
            policy: policy.validate()?,
            state: OperationState::Accepted,
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
            OperationState::Accepted | OperationState::Running => {
                self.transition(OperationState::Canceling)
            }
            OperationState::Canceling => Ok(()),
            state if state.is_terminal() => Err(OperationError::InvalidTransition {
                from: state,
                to: OperationState::Canceling,
            }),
            _ => Err(OperationError::InvalidTransition {
                from: self.state,
                to: OperationState::Canceling,
            }),
        }
    }

    /// 返回当前状态快照。
    pub fn snapshot(&self) -> OperationStatusSnapshot {
        OperationStatusSnapshot {
            id: self.id,
            state: self.state,
            cancel_requested: self.cancel_token.is_canceled(),
            health: self.health.snapshot(),
        }
    }
}

fn valid_transition(from: OperationState, to: OperationState) -> bool {
    use OperationState as S;
    matches!(
        (from, to),
        (S::Accepted, S::Running)
            | (S::Accepted, S::Canceling)
            | (S::Accepted, S::Failed)
            | (S::Accepted, S::Timeout)
            | (S::Accepted, S::Preempted)
            | (S::Running, S::Succeeded)
            | (S::Running, S::Failed)
            | (S::Running, S::Canceling)
            | (S::Running, S::Timeout)
            | (S::Running, S::Preempted)
            | (S::Canceling, S::Canceled)
            | (S::Canceling, S::Failed)
            | (S::Canceling, S::Timeout)
    )
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
}
