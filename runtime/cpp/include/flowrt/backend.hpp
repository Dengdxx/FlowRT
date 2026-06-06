#pragma once

#include <algorithm>
#include <array>
#include <cerrno>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <flowrt/backend_health.hpp>
#include <flowrt/core.hpp>
#include <functional>
#include <span>
#include <string_view>
#include <thread>

namespace flowrt {

/**
 * @brief runtime 当前认识的 backend 类型。
 */
enum class BackendKind : std::uint8_t {
    Inproc = 0,  ///< 单进程内存 backend，主要用于测试、CI 和最小 demo。
    Iox2 = 1,    ///< iceoryx2 backend，用于本机多进程高性能 dataflow。
    Zenoh = 2,   ///< zenoh backend，用于跨主机 copy transport dataflow。
};

/**
 * @brief backend capability 的只读视图。
 *
 * capability 字符串来自 Contract IR/backend contract，例如 `channel:latest` 或
 * `topology:multi_process`。validator 使用同一套 capability 语义判断部署是否可满足。
 */
class BackendCapabilities {
   public:
    /**
     * @brief 从静态 capability 列表构造视图。
     *
     * @param items capability 字符串切片；调用方必须保证其生命周期覆盖本对象。
     */
    constexpr explicit BackendCapabilities(std::span<const std::string_view> items) noexcept
        : items_(items) {}

    /**
     * @brief 查询 backend 是否声明某项能力。
     *
     * @param capability capability 字符串。
     * @return 存在时返回 true。
     */
    bool contains(std::string_view capability) const noexcept {
        return std::find(items_.begin(), items_.end(), capability) != items_.end();
    }

    /**
     * @brief 返回完整 capability 列表。
     *
     * @return capability 字符串切片。
     */
    std::span<const std::string_view> items() const noexcept { return items_; }

   private:
    std::span<const std::string_view> items_;
};

/**
 * @brief 调度器抽象边界。
 *
 * 调度器负责驱动 generated runtime shell 的 tick，不负责用户算法逻辑。v0.1 使用同步 tick
 * 接口表达最小语义，后续可以在不改变组件接口的前提下替换为更完整的实时调度实现。
 */
class Scheduler {
   public:
    /**
     * @brief 单次调度步骤函数。
     *
     * 第一个参数是 tick 序号，第二个参数是本轮共享 runtime context。
     */
    using StepFn = std::function<Status(std::size_t, Context &)>;

    virtual ~Scheduler() = default;

    /**
     * @brief 连续运行固定数量的 tick。
     *
     * @param ticks 要运行的 tick 数量。
     * @param step 每个 tick 调用一次的步骤函数。
     * @return 全部 tick 成功时返回 `Status::Ok`；否则返回第一个非 OK 状态。
     */
    Status run_ticks(std::size_t ticks, StepFn step) const {
        return run_ticks_until_shutdown(ticks, ShutdownToken{}, std::move(step));
    }

    /**
     * @brief 连续运行固定数量的 tick，但在 shutdown token 触发后提前停止。
     *
     * 提前停止不是错误；调用方仍应在返回 `Status::Ok` 后执行 shutdown task 和生命周期清理。
     */
    virtual Status run_ticks_until_shutdown(std::size_t ticks, const ShutdownToken &shutdown,
                                            StepFn step) const = 0;
};

/**
 * @brief runtime backend 抽象边界。
 *
 * Backend 暴露能力集合和调度器，用于 generated shell 在不依赖具体通信库 API 的情况下绑定运行时。
 */
class Backend {
   public:
    virtual ~Backend() = default;

    /**
     * @brief 返回 backend 类型。
     */
    virtual BackendKind kind() const noexcept = 0;

    /**
     * @brief 返回 backend capability 视图。
     */
    virtual BackendCapabilities capabilities() const noexcept = 0;

    /**
     * @brief 返回 backend 提供的调度器。
     */
    virtual const Scheduler &scheduler() const noexcept = 0;

    /**
     * @brief 返回 backend 自身的健康快照。
     */
    virtual BackendHealthSnapshot health() const { return BackendHealthSnapshot::ready(); }

    /**
     * @brief 返回 backend endpoint 默认重连策略。
     */
    virtual ReconnectPolicy reconnect_policy() const noexcept { return ReconnectPolicy{}; }
};

/**
 * @brief 单进程同步调度器。
 *
 * 该调度器按 tick 顺序直接调用步骤函数。它用于 v0.1 的 inproc demo 和测试，不承诺实时线程、
 * 优先级继承或跨进程同步。
 */
class InprocScheduler final : public Scheduler {
   public:
    /**
     * @copydoc Scheduler::run_ticks
     */
    Status run_ticks_until_shutdown(std::size_t ticks, const ShutdownToken &shutdown,
                                    StepFn step) const override {
        Context context;
        const auto tick_sleep = configured_tick_sleep();
        for (std::size_t tick = 0; tick < ticks; ++tick) {
            if (shutdown.is_requested()) {
                break;
            }
            const auto status = step(tick, context);
            if (status != Status::Ok) {
                return status;
            }
            if (tick_sleep.count() > 0) {
                std::this_thread::sleep_for(tick_sleep);
            }
        }
        return Status::Ok;
    }

   private:
    static std::chrono::milliseconds configured_tick_sleep() noexcept {
        const auto *raw = std::getenv("FLOWRT_TICK_SLEEP_MS");
        if (raw == nullptr) {
            return std::chrono::milliseconds{0};
        }
        char *end = nullptr;
        errno = 0;
        const auto value = std::strtoull(raw, &end, 10);
        if (errno != 0 || end == raw || *end != '\0' || value == 0U) {
            return std::chrono::milliseconds{0};
        }
        return std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(value)};
    }
};

/**
 * @brief 单进程 backend 实现。
 *
 * InprocBackend 使用进程内 channel 和同步调度器，适合测试、CI 和最小端到端 demo。
 */
class InprocBackend final : public Backend {
   public:
    /**
     * @copydoc Backend::kind
     */
    BackendKind kind() const noexcept override { return BackendKind::Inproc; }

    /**
     * @copydoc Backend::capabilities
     */
    BackendCapabilities capabilities() const noexcept override {
        return BackendCapabilities{std::span<const std::string_view>(kCapabilities)};
    }

    /**
     * @copydoc Backend::scheduler
     */
    const Scheduler &scheduler() const noexcept override { return scheduler_; }

   private:
    static inline constexpr std::array<std::string_view, 22> kCapabilities = {
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
    };

    InprocScheduler scheduler_;
};

/**
 * @brief iceoryx2 backend 的 C++ capability 骨架。
 *
 * 该 backend 报告 iox2 capability，并继续复用同步调度器驱动 generated shell。具体 channel
 * transport 由 `flowrt::iox2::Iox2PubSub<T>` 在 shell 内部绑定；业务组件仍只应依赖 FlowRT
 * runtime API，不直接依赖 iox2 publisher/subscriber。
 */
class Iox2Backend final : public Backend {
   public:
    /**
     * @copydoc Backend::kind
     */
    BackendKind kind() const noexcept override { return BackendKind::Iox2; }

    /**
     * @copydoc Backend::capabilities
     */
    BackendCapabilities capabilities() const noexcept override {
        return BackendCapabilities{std::span<const std::string_view>(kCapabilities)};
    }

    /**
     * @copydoc Backend::scheduler
     */
    const Scheduler &scheduler() const noexcept override { return scheduler_; }

   private:
    static inline constexpr std::array<std::string_view, 24> kCapabilities = {
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
    };

    InprocScheduler scheduler_;
};

/**
 * @brief zenoh backend 的 C++ capability 骨架。
 *
 * 该 backend 报告跨主机 copy transport capability，并继续复用同步调度器驱动 generated shell。
 * 具体 channel transport 由后续 `flowrt::zenoh` endpoint 在 shell 内部绑定；业务组件仍只应依赖
 * FlowRT runtime API，不直接依赖 zenoh publisher/subscriber。
 */
class ZenohBackend final : public Backend {
   public:
    /**
     * @copydoc Backend::kind
     */
    BackendKind kind() const noexcept override { return BackendKind::Zenoh; }

    /**
     * @copydoc Backend::capabilities
     */
    BackendCapabilities capabilities() const noexcept override {
        return BackendCapabilities{std::span<const std::string_view>(kCapabilities)};
    }

    /**
     * @copydoc Backend::scheduler
     */
    const Scheduler &scheduler() const noexcept override { return scheduler_; }

   private:
    static inline constexpr std::array<std::string_view, 20> kCapabilities = {
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
    };

    InprocScheduler scheduler_;
};

/**
 * @brief 构造默认 inproc backend。
 *
 * @return 可直接传给 generated shell 的单进程 backend。
 */
inline InprocBackend inproc_backend() { return InprocBackend{}; }

/**
 * @brief 构造 iox2 backend capability 骨架。
 *
 * @return 可用于 capability 选择和后续 iox2 shell 绑定的 backend 对象。
 */
inline Iox2Backend iox2_backend() { return Iox2Backend{}; }

/**
 * @brief 构造 zenoh backend capability 骨架。
 *
 * @return 可用于 capability 选择和后续 zenoh shell 绑定的 backend 对象。
 */
inline ZenohBackend zenoh_backend() { return ZenohBackend{}; }

}  // namespace flowrt
