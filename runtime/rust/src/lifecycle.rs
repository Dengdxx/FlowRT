//! per-instance 生命周期显式状态。
//!
//! 与 `runtime/cpp/include/flowrt/lifecycle.hpp` 的 `enum class LifecycleState` 逐值镜像；
//! 离散值是跨语言契约面，改动须同步 C++ 与 golden（见 CLAUDE.md pre-1.0 革新约束）。

/// 实例生命周期状态。`Degraded` 为 0.21.0 保留值，本切片不可达。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// 未初始化（on_init 前）。
    Uninitialized = 0,
    /// on_init 成功。
    Initialized = 1,
    /// on_start 成功，运行中。
    Running = 2,
    /// on_stop 成功。
    Stopped = 3,
    /// on_shutdown 成功，完成清理。
    ShutDown = 4,
    /// 生命周期某阶段失败。
    Faulted = 5,
    /// 保留：降级续跑（0.21.2 起可达）。
    Degraded = 6,
}

impl LifecycleState {
    /// 离散值，与 C++ 镜像一致。
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// canonical 小写名称，用于 diagnostics/status。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Uninitialized => "uninitialized",
            Self::Initialized => "initialized",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::ShutDown => "shutdown",
            Self::Faulted => "faulted",
            Self::Degraded => "degraded",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LifecycleState as L;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(L::Uninitialized.as_u8(), 0);
        assert_eq!(L::Initialized.as_u8(), 1);
        assert_eq!(L::Running.as_u8(), 2);
        assert_eq!(L::Stopped.as_u8(), 3);
        assert_eq!(L::ShutDown.as_u8(), 4);
        assert_eq!(L::Faulted.as_u8(), 5);
        assert_eq!(L::Degraded.as_u8(), 6);
    }

    #[test]
    fn canonical_strings_are_stable() {
        assert_eq!(L::Running.as_str(), "running");
        assert_eq!(L::Faulted.as_str(), "faulted");
        assert_eq!(L::ShutDown.as_str(), "shutdown");
    }
}
