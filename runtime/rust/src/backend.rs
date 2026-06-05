use crate::{Context, Status, channel::BackendCapabilities};

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
        let mut context = Context::default();
        let tick_count = configured_run_ticks(ticks);
        let tick_sleep = configured_tick_sleep();
        for tick in 0..tick_count {
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

fn configured_run_ticks(default_ticks: usize) -> usize {
    std::env::var("FLOWRT_RUN_TICKS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|ticks| *ticks > 0)
        .unwrap_or(default_ticks)
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
