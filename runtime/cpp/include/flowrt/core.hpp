#pragma once

#include <atomic>
#include <csignal>
#include <cstdint>
#include <memory>

namespace flowrt {

/**
 * @brief 组件回调和调度步骤的统一返回状态。
 *
 * 生成的 runtime shell 通过该状态决定是否继续当前调度循环。算法代码不应抛出异常来表达
 * FlowRT 语义错误；需要重试或停止时返回对应状态。
 */
enum class Status : std::uint8_t {
    Ok = 0,     ///< 本次步骤完成，调度器可以继续执行后续 tick。
    Retry = 1,  ///< 本次步骤未完成，调用方可按调度策略稍后重试。
    Error = 2,  ///< 本次步骤失败，调度器应停止当前运行序列并向上报告。
};

/**
 * @brief 返回成功状态的便捷函数。
 *
 * @return `Status::Ok`。
 */
constexpr Status ok() noexcept { return Status::Ok; }

/**
 * @brief runtime 传递给生命周期钩子和调度步骤的上下文。
 *
 * v0.1 暂不暴露资源句柄；保留该类型是为了后续承载 clock、logger、参数快照和 backend
 * 能力视图，同时保持用户接口稳定。
 */
struct Context {};

/**
 * @brief 调度循环可查询的关闭请求。
 *
 * token 可以由 Unix signal handler 驱动，也可以由测试或更高层 runtime 代码手动 request。
 * `is_requested()` 为 true 时，generated shell 应退出 tick loop，并继续执行 shutdown task 与
 * 生命周期清理。
 */
class ShutdownToken {
   public:
    ShutdownToken() : requested_(std::make_shared<std::atomic_bool>(false)) {}

    static ShutdownToken new_for_test() { return ShutdownToken{}; }

    void request() const noexcept {
        if (requested_) {
            requested_->store(true, std::memory_order_seq_cst);
        }
    }

    bool is_requested() const noexcept {
        const bool local_requested = requested_ && requested_->load(std::memory_order_seq_cst);
        const bool external_requested = external_signal_ != nullptr && *external_signal_ != 0;
        return local_requested || external_requested;
    }

   private:
    friend ShutdownToken install_signal_shutdown_token();

    explicit ShutdownToken(const volatile std::sig_atomic_t *external_signal)
        : requested_(std::make_shared<std::atomic_bool>(false)),
          external_signal_(external_signal) {}

    std::shared_ptr<std::atomic_bool> requested_;
    const volatile std::sig_atomic_t *external_signal_ = nullptr;
};

namespace detail {

inline volatile std::sig_atomic_t signal_shutdown_requested = 0;
inline std::atomic_bool signal_handlers_installed{false};

inline void handle_shutdown_signal(int) noexcept { signal_shutdown_requested = 1; }

inline void install_signal_handlers_once() noexcept {
    bool expected = false;
    if (!signal_handlers_installed.compare_exchange_strong(
            expected, true, std::memory_order_seq_cst, std::memory_order_seq_cst)) {
        return;
    }
    (void)std::signal(SIGINT, handle_shutdown_signal);
    (void)std::signal(SIGTERM, handle_shutdown_signal);
}

}  // namespace detail

/**
 * @brief 安装 SIGINT/SIGTERM handler，并返回进程信号驱动的 shutdown token。
 */
inline ShutdownToken install_signal_shutdown_token() {
    detail::install_signal_handlers_once();
    return ShutdownToken{std::addressof(detail::signal_shutdown_requested)};
}

}  // namespace flowrt
