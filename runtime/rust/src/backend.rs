use crate::{Context, ShutdownToken, Status, channel::BackendCapabilities};

/// runtime 当前认识的 backend 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// 单进程内存 backend，主要用于测试、CI 和最小 demo。
    Inproc,
    /// iceoryx2 backend，用于本机多进程高性能 dataflow。
    Iox2,
    /// zenoh backend，用于跨主机 copy transport dataflow。
    Zenoh,
}

/// backend 或 endpoint 当前健康状态。
///
/// 枚举值保持紧凑、稳定，方便未来 C ABI 直接映射为整数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendHealthState {
    /// 本地 endpoint 已打开，最近一次操作未发现错误。
    Ready,
    /// 本地 endpoint 可见错误，但仍允许后续恢复。
    Degraded,
    /// runtime 正在按重连策略等待或尝试恢复。
    Reconnecting,
    /// 重连预算耗尽或错误不可恢复。
    Failed,
}

/// backend 或 endpoint 的健康快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendHealthSnapshot {
    /// 当前健康状态。
    pub state: BackendHealthState,
    /// 最近一次稳定错误文本。
    pub last_error: Option<String>,
    /// 当前重连尝试序号，从 0 开始。
    pub attempt: u32,
    /// 下一次重试的 Unix 毫秒时间戳。
    pub next_retry_unix_ms: Option<u64>,
    /// 当前状态是否允许继续恢复。
    pub recoverable: bool,
}

impl BackendHealthSnapshot {
    /// 构造 ready 快照。
    pub fn ready() -> Self {
        Self {
            state: BackendHealthState::Ready,
            last_error: None,
            attempt: 0,
            next_retry_unix_ms: None,
            recoverable: false,
        }
    }
}

/// backend endpoint 的重连策略。
///
/// 使用毫秒和次数，而不是语言 runtime 特有类型，便于后续映射到 C ABI、Python binding 和其他
/// 语言 runtime。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectPolicy {
    initial_delay_ms: u64,
    max_delay_ms: u64,
    max_attempts: Option<u32>,
}

impl ReconnectPolicy {
    /// 构造指数退避策略。
    pub const fn new(initial_delay_ms: u64, max_delay_ms: u64, max_attempts: u32) -> Self {
        Self {
            initial_delay_ms,
            max_delay_ms,
            max_attempts: Some(max_attempts),
        }
    }

    /// 构造无限重试指数退避策略。
    pub const fn forever(initial_delay_ms: u64, max_delay_ms: u64) -> Self {
        Self {
            initial_delay_ms,
            max_delay_ms,
            max_attempts: None,
        }
    }

    /// 初次重试延迟，毫秒。
    pub const fn initial_delay_ms(&self) -> u64 {
        self.initial_delay_ms
    }

    /// 最大重试延迟，毫秒。
    pub const fn max_delay_ms(&self) -> u64 {
        self.max_delay_ms
    }

    /// 最大尝试次数；`None` 表示无限重试。
    pub const fn max_attempts(&self) -> Option<u32> {
        self.max_attempts
    }

    /// 指定 attempt 的退避延迟，毫秒。
    pub fn delay_for_attempt(&self, attempt: u32) -> u64 {
        let shift = attempt.min(63);
        let multiplier = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        self.initial_delay_ms
            .saturating_mul(multiplier)
            .min(self.max_delay_ms)
    }

    /// 判断当前 attempt 是否仍允许重试。
    pub fn can_retry(&self, attempt: u32) -> bool {
        self.max_attempts
            .map(|max_attempts| attempt < max_attempts)
            .unwrap_or(true)
    }
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self::forever(100, 5_000)
    }
}

/// 通用 backend health 状态机。
///
/// 该 tracker 不直接重连任何 transport，只统一记录状态转换。真实 zenoh/iox2 恢复逻辑会在后续
/// endpoint 层调用它。
#[derive(Debug, Clone)]
pub struct BackendHealthTracker {
    policy: ReconnectPolicy,
    snapshot: BackendHealthSnapshot,
}

impl BackendHealthTracker {
    /// 构造 ready 状态 tracker。
    pub fn new(policy: ReconnectPolicy) -> Self {
        Self {
            policy,
            snapshot: BackendHealthSnapshot::ready(),
        }
    }

    /// 返回重连策略。
    pub fn policy(&self) -> ReconnectPolicy {
        self.policy
    }

    /// 返回健康快照。
    pub fn snapshot(&self) -> BackendHealthSnapshot {
        self.snapshot.clone()
    }

    /// 标记 endpoint 恢复 ready。
    pub fn mark_ready(&mut self) {
        self.snapshot = BackendHealthSnapshot::ready();
    }

    /// 标记 endpoint 出现可恢复退化。
    pub fn mark_degraded(&mut self, error: impl Into<String>) {
        self.snapshot = BackendHealthSnapshot {
            state: BackendHealthState::Degraded,
            last_error: Some(error.into()),
            attempt: self.snapshot.attempt,
            next_retry_unix_ms: None,
            recoverable: true,
        };
    }

    /// 标记 endpoint 正在等待或尝试重连。
    pub fn mark_reconnecting(&mut self, attempt: u32, next_retry_unix_ms: u64) {
        self.snapshot = BackendHealthSnapshot {
            state: BackendHealthState::Reconnecting,
            last_error: self.snapshot.last_error.clone(),
            attempt,
            next_retry_unix_ms: Some(next_retry_unix_ms),
            recoverable: self.policy.can_retry(attempt),
        };
    }

    /// 标记 endpoint 恢复失败。
    pub fn mark_failed(&mut self, error: impl Into<String>, attempt: u32) {
        self.snapshot = BackendHealthSnapshot {
            state: BackendHealthState::Failed,
            last_error: Some(error.into()),
            attempt,
            next_retry_unix_ms: None,
            recoverable: false,
        };
    }
}

/// 调度器抽象边界。
///
/// 调度器负责驱动 generated runtime shell 的 tick，不负责用户算法逻辑。v0.1 使用同步 tick
/// 接口表达最小语义，后续可以在不改变组件接口的前提下替换为更完整的实时调度实现。
pub trait Scheduler {
    /// 连续运行固定数量的 tick。
    ///
    /// `step` 的第一个参数是 tick 序号，第二个参数是本轮共享 runtime context。全部 tick
    /// 成功时返回 `Status::Ok`；否则返回第一个非 OK 状态。
    fn run_ticks(
        &self,
        ticks: usize,
        step: &mut dyn FnMut(usize, &mut Context) -> Status,
    ) -> Status {
        self.run_ticks_until_shutdown(ticks, &ShutdownToken::new(), step)
    }

    /// 连续运行固定数量的 tick，但在 shutdown token 触发后提前停止。
    ///
    /// 提前停止不是错误；调用方仍应在返回 `Status::Ok` 后执行 shutdown task 和生命周期清理。
    fn run_ticks_until_shutdown(
        &self,
        ticks: usize,
        shutdown: &ShutdownToken,
        step: &mut dyn FnMut(usize, &mut Context) -> Status,
    ) -> Status;
}

/// runtime backend 抽象边界。
///
/// Backend 暴露能力集合和调度器，用于 generated shell 在不依赖具体通信库 API 的情况下绑定运行时。
pub trait Backend {
    /// 返回 backend 类型。
    fn kind(&self) -> BackendKind;

    /// 返回 backend capability 视图。
    fn capabilities(&self) -> BackendCapabilities;

    /// 返回 backend 提供的调度器。
    fn scheduler(&self) -> &dyn Scheduler;

    /// 返回 backend 自身的健康快照。
    fn health(&self) -> BackendHealthSnapshot {
        BackendHealthSnapshot::ready()
    }

    /// 返回 backend endpoint 默认重连策略。
    fn reconnect_policy(&self) -> ReconnectPolicy {
        ReconnectPolicy::default()
    }
}

/// 单进程同步调度器。
///
/// 该调度器按 tick 顺序直接调用步骤函数。它用于 v0.1 的 inproc demo 和测试，不承诺实时线程、
/// 优先级继承或跨进程同步。
#[derive(Debug, Default)]
pub struct InprocScheduler;

impl Scheduler for InprocScheduler {
    fn run_ticks(
        &self,
        ticks: usize,
        step: &mut dyn FnMut(usize, &mut Context) -> Status,
    ) -> Status {
        self.run_ticks_until_shutdown(ticks, &ShutdownToken::new(), step)
    }

    fn run_ticks_until_shutdown(
        &self,
        ticks: usize,
        shutdown: &ShutdownToken,
        step: &mut dyn FnMut(usize, &mut Context) -> Status,
    ) -> Status {
        let mut context = Context::default();
        let tick_sleep = configured_tick_sleep();
        for tick in 0..ticks {
            if shutdown.is_requested() {
                break;
            }
            match step(tick, &mut context) {
                Status::Ok => {}
                status => return status,
            }
            if let Some(duration) = tick_sleep {
                std::thread::sleep(duration);
            }
        }
        Status::Ok
    }
}

fn configured_tick_sleep() -> Option<std::time::Duration> {
    std::env::var("FLOWRT_TICK_SLEEP_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(std::time::Duration::from_millis)
}

const INPROC_CAPABILITIES: &[&str] = &[
    "abi:fixed_size_plain_data",
    "abi:variable_payload_frame",
    "layout:native_layout",
    "allocation:bounded",
    "allocation:unbounded_dynamic",
    "graph:static_graph",
    "trigger:periodic",
    "trigger:on_message",
    "trigger:startup",
    "trigger:shutdown",
    "timing:deadline_aware",
    "channel:latest",
    "channel:fifo",
    "overflow:drop_oldest",
    "overflow:drop_newest",
    "overflow:error",
    "overflow:block",
    "stale:warn",
    "stale:drop",
    "stale:hold_last",
    "stale:error",
    "topology:single_process",
    "transfer:copy",
    "observability:health",
];

const IOX2_CAPABILITIES: &[&str] = &[
    "abi:fixed_size_plain_data",
    "layout:native_layout",
    "allocation:bounded",
    "graph:static_graph",
    "trigger:periodic",
    "trigger:on_message",
    "trigger:startup",
    "trigger:shutdown",
    "timing:deadline_aware",
    "channel:latest",
    "channel:fifo",
    "overflow:drop_oldest",
    "overflow:drop_newest",
    "overflow:error",
    "overflow:block",
    "stale:warn",
    "stale:drop",
    "stale:hold_last",
    "stale:error",
    "topology:multi_process",
    "topology:single_host",
    "transfer:zero_copy",
    "transfer:loaned",
    "observability:health",
];

const ZENOH_CAPABILITIES: &[&str] = &[
    "abi:fixed_size_plain_data",
    "layout:native_layout",
    "allocation:bounded",
    "graph:static_graph",
    "trigger:periodic",
    "trigger:on_message",
    "trigger:startup",
    "trigger:shutdown",
    "timing:deadline_aware",
    "channel:latest",
    "channel:fifo",
    "overflow:drop_oldest",
    "stale:warn",
    "stale:drop",
    "stale:hold_last",
    "stale:error",
    "topology:multi_process",
    "topology:multi_host",
    "transfer:copy",
    "observability:health",
];

/// 单进程 backend 实现。
///
/// InprocBackend 使用进程内 channel 和同步调度器，适合测试、CI 和最小端到端 demo。
#[derive(Debug, Default)]
pub struct InprocBackend {
    scheduler: InprocScheduler,
}

impl InprocBackend {
    /// 构造默认 inproc backend。
    pub fn new() -> Self {
        Self::default()
    }
}

impl Backend for InprocBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Inproc
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::new(INPROC_CAPABILITIES)
    }

    fn scheduler(&self) -> &dyn Scheduler {
        &self.scheduler
    }
}

/// 构造默认 inproc backend。
pub fn inproc_backend() -> InprocBackend {
    InprocBackend::default()
}

/// iceoryx2 backend 的 capability 骨架。
///
/// 当前 Rust 侧已有可选 iox2 typed pub/sub endpoint；该 backend 对象负责表达 runtime 层的
/// backend 种类、capability 和调度边界。业务组件仍只应依赖 FlowRT runtime API。
#[derive(Debug, Default)]
pub struct Iox2Backend {
    scheduler: InprocScheduler,
}

impl Iox2Backend {
    /// 构造 iox2 backend capability 骨架。
    pub fn new() -> Self {
        Self::default()
    }
}

impl Backend for Iox2Backend {
    fn kind(&self) -> BackendKind {
        BackendKind::Iox2
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::new(IOX2_CAPABILITIES)
    }

    fn scheduler(&self) -> &dyn Scheduler {
        &self.scheduler
    }
}

/// 构造 iox2 backend capability 骨架。
pub fn iox2_backend() -> Iox2Backend {
    Iox2Backend::default()
}

/// zenoh backend 的 capability 骨架。
///
/// zenoh 用于跨主机 dataflow。该对象只表达 runtime 层的 backend 种类、capability 和调度边界；
/// 具体 channel transport 由 generated shell 内部绑定，业务组件仍只依赖 FlowRT runtime API。
#[derive(Debug, Default)]
pub struct ZenohBackend {
    scheduler: InprocScheduler,
}

impl ZenohBackend {
    /// 构造 zenoh backend capability 骨架。
    pub fn new() -> Self {
        Self::default()
    }
}

impl Backend for ZenohBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Zenoh
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::new(ZENOH_CAPABILITIES)
    }

    fn scheduler(&self) -> &dyn Scheduler {
        &self.scheduler
    }
}

/// 构造 zenoh backend capability 骨架。
pub fn zenoh_backend() -> ZenohBackend {
    ZenohBackend::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inproc_backend_reports_expected_capabilities() {
        let backend = inproc_backend();
        assert_eq!(backend.kind(), BackendKind::Inproc);
        let capabilities = backend.capabilities();
        assert!(capabilities.contains("channel:latest"));
        assert!(capabilities.contains("graph:static_graph"));
        assert!(capabilities.contains("timing:deadline_aware"));
        assert_eq!(
            capabilities.as_slice(),
            &[
                "abi:fixed_size_plain_data",
                "abi:variable_payload_frame",
                "layout:native_layout",
                "allocation:bounded",
                "allocation:unbounded_dynamic",
                "graph:static_graph",
                "trigger:periodic",
                "trigger:on_message",
                "trigger:startup",
                "trigger:shutdown",
                "timing:deadline_aware",
                "channel:latest",
                "channel:fifo",
                "overflow:drop_oldest",
                "overflow:drop_newest",
                "overflow:error",
                "overflow:block",
                "stale:warn",
                "stale:drop",
                "stale:hold_last",
                "stale:error",
                "topology:single_process",
                "transfer:copy",
                "observability:health",
            ]
        );
    }

    #[test]
    fn inproc_scheduler_stops_on_error() {
        let scheduler = InprocScheduler;
        let mut seen = 0usize;
        let status = scheduler.run_ticks(5, &mut |tick, _| {
            seen += 1;
            if tick == 2 { Status::Error } else { Status::Ok }
        });
        assert_eq!(seen, 3);
        assert_eq!(status, Status::Error);
    }

    #[test]
    fn inproc_scheduler_stops_when_shutdown_is_requested() {
        let scheduler = InprocScheduler;
        let shutdown = ShutdownToken::new_for_test();
        let mut seen = 0usize;

        let status = scheduler.run_ticks_until_shutdown(10, &shutdown, &mut |_tick, _context| {
            seen += 1;
            shutdown.request();
            Status::Ok
        });

        assert_eq!(seen, 1);
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn inproc_scheduler_honors_requested_tick_count() {
        let scheduler = InprocScheduler;
        let mut seen = 0usize;
        let status = scheduler.run_ticks(4, &mut |tick, _| {
            assert_eq!(tick, seen);
            seen += 1;
            Status::Ok
        });

        assert_eq!(seen, 4);
        assert_eq!(status, Status::Ok);
    }

    #[test]
    fn reconnect_policy_uses_bounded_exponential_backoff() {
        let policy = ReconnectPolicy::new(100, 1_000, 3);

        assert_eq!(policy.initial_delay_ms(), 100);
        assert_eq!(policy.max_delay_ms(), 1_000);
        assert_eq!(policy.max_attempts(), Some(3));
        assert_eq!(policy.delay_for_attempt(0), 100);
        assert_eq!(policy.delay_for_attempt(1), 200);
        assert_eq!(policy.delay_for_attempt(4), 1_000);
        assert!(policy.can_retry(2));
        assert!(!policy.can_retry(3));
    }

    #[test]
    fn backend_health_tracker_records_reconnect_progress() {
        let policy = ReconnectPolicy::new(100, 1_000, 3);
        let mut tracker = BackendHealthTracker::new(policy);

        assert_eq!(tracker.snapshot().state, BackendHealthState::Ready);
        assert_eq!(tracker.snapshot().attempt, 0);
        assert!(!tracker.snapshot().recoverable);

        tracker.mark_degraded("receive failed");
        assert_eq!(tracker.snapshot().state, BackendHealthState::Degraded);
        assert_eq!(
            tracker.snapshot().last_error.as_deref(),
            Some("receive failed")
        );
        assert!(tracker.snapshot().recoverable);

        tracker.mark_reconnecting(1, 500);
        assert_eq!(tracker.snapshot().state, BackendHealthState::Reconnecting);
        assert_eq!(tracker.snapshot().attempt, 1);
        assert_eq!(tracker.snapshot().next_retry_unix_ms, Some(500));
        assert!(tracker.snapshot().recoverable);

        tracker.mark_ready();
        assert_eq!(tracker.snapshot(), BackendHealthSnapshot::ready());

        tracker.mark_failed("retry budget exhausted", 3);
        assert_eq!(tracker.snapshot().state, BackendHealthState::Failed);
        assert_eq!(tracker.snapshot().attempt, 3);
        assert_eq!(
            tracker.snapshot().last_error.as_deref(),
            Some("retry budget exhausted")
        );
        assert!(!tracker.snapshot().recoverable);
    }

    #[test]
    fn backends_report_default_health_and_reconnect_policy() {
        let backend = zenoh_backend();

        assert_eq!(backend.health().state, BackendHealthState::Ready);
        assert_eq!(backend.reconnect_policy().initial_delay_ms(), 100);
        assert_eq!(backend.reconnect_policy().max_delay_ms(), 5_000);
        assert_eq!(backend.reconnect_policy().max_attempts(), None);
    }

    #[test]
    fn iox2_backend_reports_expected_capabilities() {
        let backend = iox2_backend();
        assert_eq!(backend.kind(), BackendKind::Iox2);
        let capabilities = backend.capabilities();
        assert!(capabilities.contains("topology:multi_process"));
        assert!(capabilities.contains("timing:deadline_aware"));
        assert_eq!(
            capabilities.as_slice(),
            &[
                "abi:fixed_size_plain_data",
                "layout:native_layout",
                "allocation:bounded",
                "graph:static_graph",
                "trigger:periodic",
                "trigger:on_message",
                "trigger:startup",
                "trigger:shutdown",
                "timing:deadline_aware",
                "channel:latest",
                "channel:fifo",
                "overflow:drop_oldest",
                "overflow:drop_newest",
                "overflow:error",
                "overflow:block",
                "stale:warn",
                "stale:drop",
                "stale:hold_last",
                "stale:error",
                "topology:multi_process",
                "topology:single_host",
                "transfer:zero_copy",
                "transfer:loaned",
                "observability:health",
            ]
        );
    }

    #[test]
    fn zenoh_backend_reports_expected_capabilities() {
        let backend = zenoh_backend();
        assert_eq!(backend.kind(), BackendKind::Zenoh);
        let capabilities = backend.capabilities();
        assert!(capabilities.contains("topology:multi_process"));
        assert!(capabilities.contains("topology:multi_host"));
        assert!(capabilities.contains("transfer:copy"));
        assert!(capabilities.contains("overflow:drop_oldest"));
        assert!(!capabilities.contains("overflow:drop_newest"));
        assert!(!capabilities.contains("overflow:error"));
        assert!(!capabilities.contains("overflow:block"));
        assert_eq!(
            capabilities.as_slice(),
            &[
                "abi:fixed_size_plain_data",
                "layout:native_layout",
                "allocation:bounded",
                "graph:static_graph",
                "trigger:periodic",
                "trigger:on_message",
                "trigger:startup",
                "trigger:shutdown",
                "timing:deadline_aware",
                "channel:latest",
                "channel:fifo",
                "overflow:drop_oldest",
                "stale:warn",
                "stale:drop",
                "stale:hold_last",
                "stale:error",
                "topology:multi_process",
                "topology:multi_host",
                "transfer:copy",
                "observability:health",
            ]
        );
    }
}
