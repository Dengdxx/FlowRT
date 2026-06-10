#pragma once

#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

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
 * @brief I/O boundary 声明资源的运行态状态。
 */
struct BoundaryResourceStatus {
    std::string name;
    std::string kind;
    bool ready = false;
    std::optional<std::string> message;
    std::optional<std::string> last_error;
    std::optional<std::uint64_t> updated_unix_ms;
};

/**
 * @brief 单个 I/O boundary component 的运行态健康状态。
 */
struct BoundaryStatus {
    std::string name;
    std::string component;
    bool ready = false;
    bool healthy = true;
    std::optional<std::string> last_error;
    std::vector<BoundaryResourceStatus> resources;
    std::optional<std::uint64_t> updated_unix_ms;
};

namespace detail {

inline std::uint64_t boundary_unix_time_ms() {
    const auto now = std::chrono::system_clock::now().time_since_epoch();
    const auto millis = std::chrono::duration_cast<std::chrono::milliseconds>(now).count();
    return millis < 0 ? 0U : static_cast<std::uint64_t>(millis);
}

}  // namespace detail

/**
 * @brief I/O boundary 组件可用的运行态上报上下文。
 *
 * 该类型只表达 FlowRT 的资源、readiness 和 health 语义，不暴露串口、SHM、网络或 backend
 * SDK 句柄。真实 I/O 仍由用户代码管理。
 */
class BoundaryContext {
   public:
    using Reporter = std::function<void(BoundaryStatus)>;

    BoundaryContext() = default;

    BoundaryContext(std::string instance, std::string component,
                    std::vector<BoundaryResourceStatus> resources, Reporter reporter)
        : status_(BoundaryStatus{
              .name = std::move(instance),
              .component = std::move(component),
              .resources = std::move(resources),
          }),
          reporter_(std::move(reporter)) {}

    [[nodiscard]] const std::string &instance() const noexcept { return status_.name; }

    [[nodiscard]] const std::string &component() const noexcept { return status_.component; }

    void mark_ready() {
        status_.ready = true;
        touch_and_report();
    }

    void mark_not_ready() {
        status_.ready = false;
        touch_and_report();
    }

    void report_healthy() {
        status_.healthy = true;
        status_.last_error.reset();
        touch_and_report();
    }

    void report_error(std::string error) {
        status_.healthy = false;
        status_.last_error = std::move(error);
        touch_and_report();
    }

    void mark_resource_ready(std::string_view resource) {
        auto &entry = resource_entry(resource);
        entry.ready = true;
        entry.message.reset();
        entry.last_error.reset();
        entry.updated_unix_ms = detail::boundary_unix_time_ms();
        touch_and_report();
    }

    void mark_resource_not_ready(std::string_view resource, std::string message) {
        auto &entry = resource_entry(resource);
        entry.ready = false;
        entry.message = std::move(message);
        entry.updated_unix_ms = detail::boundary_unix_time_ms();
        touch_and_report();
    }

    void report_resource_error(std::string_view resource, std::string error) {
        auto &entry = resource_entry(resource);
        entry.ready = false;
        entry.last_error = std::move(error);
        entry.updated_unix_ms = detail::boundary_unix_time_ms();
        status_.healthy = false;
        touch_and_report();
    }

   private:
    BoundaryResourceStatus &resource_entry(std::string_view resource) {
        for (auto &entry : status_.resources) {
            if (entry.name == resource) {
                return entry;
            }
        }
        status_.resources.push_back(BoundaryResourceStatus{.name = std::string{resource}});
        return status_.resources.back();
    }

    void touch_and_report() {
        status_.updated_unix_ms = detail::boundary_unix_time_ms();
        if (reporter_) {
            reporter_(status_);
        }
    }

    BoundaryStatus status_;
    Reporter reporter_;
};

/**
 * @brief runtime 传递给生命周期钩子和调度步骤的上下文。
 *
 * 普通组件看到空上下文；I/O boundary 组件会收到带 `BoundaryContext` 的上下文，用于上报
 * 资源、readiness 和 health。上下文不暴露底层 backend SDK。
 */
class Context {
   public:
    Context() = default;

    static Context for_boundary(BoundaryContext boundary) {
        Context context;
        context.boundary_ = std::move(boundary);
        return context;
    }

    [[nodiscard]] BoundaryContext *boundary() noexcept {
        return boundary_ ? std::addressof(*boundary_) : nullptr;
    }

    [[nodiscard]] const BoundaryContext *boundary() const noexcept {
        return boundary_ ? std::addressof(*boundary_) : nullptr;
    }

    [[nodiscard]] bool is_io_boundary() const noexcept { return boundary_.has_value(); }

   private:
    std::optional<BoundaryContext> boundary_;
};

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
