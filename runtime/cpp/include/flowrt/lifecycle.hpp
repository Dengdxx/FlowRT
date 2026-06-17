#pragma once

#include <cstdint>
#include <string_view>

namespace flowrt {

/// per-instance 生命周期状态，与 runtime/rust/src/lifecycle.rs 逐值镜像。
enum class LifecycleState : std::uint8_t {
    Uninitialized = 0,  ///< on_init 前
    Initialized = 1,    ///< on_init 成功
    Running = 2,        ///< on_start 成功
    Stopped = 3,        ///< on_stop 成功
    ShutDown = 4,       ///< on_shutdown 成功
    Faulted = 5,        ///< 阶段失败
    Degraded = 6,       ///< 保留（0.21.2 起可达）
};

/// canonical 小写名称，与 Rust `LifecycleState::as_str` 一致。
constexpr std::string_view lifecycle_state_str(LifecycleState state) {
    switch (state) {
        case LifecycleState::Uninitialized:
            return "uninitialized";
        case LifecycleState::Initialized:
            return "initialized";
        case LifecycleState::Running:
            return "running";
        case LifecycleState::Stopped:
            return "stopped";
        case LifecycleState::ShutDown:
            return "shutdown";
        case LifecycleState::Faulted:
            return "faulted";
        case LifecycleState::Degraded:
            return "degraded";
    }
    return "uninitialized";
}

}  // namespace flowrt
