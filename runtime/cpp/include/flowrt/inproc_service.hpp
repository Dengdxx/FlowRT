#pragma once

#include <any>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <deque>
#include <flowrt/service.hpp>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

namespace flowrt {

/**
 * @brief inproc service 配置。
 *
 * 队列满时返回 `Busy`，不允许无界阻塞。默认 timeout 必须大于 0。
 */
struct InprocServiceConfig {
    std::size_t queue_depth = 32;    ///< 请求队列最大深度。0 表示拒绝新请求。
    std::size_t max_in_flight = 64;  ///< 并发处理中请求上限。0 表示拒绝新请求。
    std::uint64_t default_timeout_ms = 5000;  ///< 默认超时毫秒。0 按 5000 处理。
};

/**
 * @brief inproc service 调用统计。
 */
struct InprocServiceStats {
    std::uint64_t completed = 0;      ///< 成功完成次数。
    std::uint64_t timeouts = 0;       ///< 超时次数。
    std::uint64_t unavailable = 0;    ///< 服务不可用次数。
    std::uint64_t busy = 0;           ///< 队列满拒绝次数。
    std::uint64_t late_dropped = 0;   ///< 响应到达时 client 已丢弃的次数。
    std::uint64_t deadlocks = 0;      ///< 同 lane 死锁检测次数。
    std::uint64_t handler_error = 0;  ///< handler 抛异常或返回 HandlerError 次数。

    void reset() noexcept { *this = InprocServiceStats{}; }
};

namespace detail {

inline std::uint64_t monotonic_now_ms() noexcept {
    return static_cast<std::uint64_t>(std::chrono::duration_cast<std::chrono::milliseconds>(
                                          std::chrono::steady_clock::now().time_since_epoch())
                                          .count());
}

/**
 * @brief inproc service 内部共享状态。
 *
 * 由 `InprocServiceServer` 和 `InprocServiceClient` 通过 `shared_ptr` 共享。
 * 请求队列和响应通过 mutex + condition_variable 同步。
 */
struct InprocServiceState {
    mutable std::mutex mutex;
    std::condition_variable cv;
    std::deque<std::any> queue;
    std::function<void()> on_request_arrived;
    std::size_t queue_depth = 32;
    std::size_t max_in_flight = 64;
    std::uint64_t default_timeout_ms = 5000;
    std::uint64_t in_flight = 0;
    std::uint64_t session_counter = 0;
    bool available = true;  ///< server 存活标记。server 销毁后置 false。
    InprocServiceStats stats;
};

/**
 * @brief 类型擦除的调用队列条目。
 */
struct InprocCallEntry {
    std::any request;
    std::function<void(ServiceError, std::any &&, std::optional<std::string>)> deliver;
};

}  // namespace detail

// ── InprocServiceRegistry（声明在 server/client 之前）────────────────────────

/**
 * @brief inproc service registry。
 *
 * 管理已注册的 inproc service，供 client 查找 server 状态。
 * 单例模式，全局共享。server 构造时自动注册，析构时自动注销。
 */
class InprocServiceRegistry {
   public:
    static InprocServiceRegistry &instance() {
        static InprocServiceRegistry registry;
        return registry;
    }

    /**
     * @brief 注册 service。
     *
     * @param name service canonical name。
     * @param state server 共享状态。
     */
    void register_service(std::string name, std::shared_ptr<detail::InprocServiceState> state) {
        std::lock_guard lock(mutex_);
        services_[std::move(name)] = std::move(state);
    }

    /**
     * @brief 注销 service。
     */
    void unregister_service(std::string_view name) {
        std::lock_guard lock(mutex_);
        services_.erase(std::string(name));
    }

    /**
     * @brief 查询 service 是否已注册。
     */
    bool has_service(std::string_view name) const {
        std::lock_guard lock(mutex_);
        return services_.count(std::string(name)) > 0;
    }

    /**
     * @brief 查询 service 排队中的请求数量。
     *
     * service 未注册时返回 0。
     */
    std::size_t pending_count(std::string_view name) const {
        std::shared_ptr<detail::InprocServiceState> state;
        {
            std::lock_guard lock(mutex_);
            auto it = services_.find(std::string(name));
            if (it == services_.end()) {
                return 0;
            }
            state = it->second;
        }
        std::lock_guard state_lock(state->mutex);
        return state->queue.size();
    }

    /**
     * @brief 注销所有 service（用于测试清理）。
     */
    void clear() {
        std::lock_guard lock(mutex_);
        services_.clear();
    }

   private:
    InprocServiceRegistry() = default;

    mutable std::mutex mutex_;
    std::unordered_map<std::string, std::shared_ptr<detail::InprocServiceState>> services_;
};

// ── InprocServiceHandle ─────────────────────────────────────────────────────

/**
 * @brief inproc service 调用 handle。
 *
 * 非阻塞 future-like 对象。支持 `complete()` 阻塞等待和 `poll()` 非阻塞查询。
 * handle 可拷贝，多个 handle 实例共享同一调用状态。
 *
 * @tparam Resp 响应消息类型。
 */
template <typename Resp>
class InprocServiceHandle {
   public:
    /**
     * @brief 阻塞等待响应，带超时。
     *
     * 超时后标记 done 以阻止 late response 写入，并递增 stats.timeouts。
     *
     * @return 成功返回响应值，超时或错误返回 ServiceError。
     */
    ServiceResult<Resp> complete() {
        std::unique_lock lock(state_->mutex);
        if (state_->done) {
            return take_result();
        }
        const auto now_ms = detail::monotonic_now_ms();
        const auto remaining = state_->deadline_ms > now_ms ? state_->deadline_ms - now_ms : 0;
        if (remaining == 0) {
            state_->error = ServiceError::Timeout;
            state_->done = true;
            if (state_->stats) {
                ++state_->stats->timeouts;
            }
            return ServiceResult<Resp>::err(ServiceError::Timeout);
        }
        const auto abs_deadline =
            std::chrono::steady_clock::now() + std::chrono::milliseconds(remaining);
        if (!state_->cv.wait_until(lock, abs_deadline, [this] { return state_->done; })) {
            state_->error = ServiceError::Timeout;
            state_->done = true;
            if (state_->stats) {
                ++state_->stats->timeouts;
            }
            return ServiceResult<Resp>::err(ServiceError::Timeout);
        }
        return take_result();
    }

    /**
     * @brief `complete()` 的兼容别名。
     */
    ServiceResult<Resp> wait() { return complete(); }

    /**
     * @brief 非阻塞查询响应是否已就绪。
     *
     * 只返回 ready 状态，不搬走响应值；调用方使用 `complete()` 取得最终结果。
     */
    bool poll() const {
        std::lock_guard lock(state_->mutex);
        return state_->done;
    }

    /**
     * @brief 响应是否已就绪。
     */
    bool ready() const {
        std::lock_guard lock(state_->mutex);
        return state_->done;
    }

   private:
    template <typename R, typename S>
    friend class InprocServiceServer;

    template <typename R, typename S>
    friend class InprocServiceClient;

    struct State {
        mutable std::mutex mutex;
        std::condition_variable cv;
        std::optional<Resp> response;
        ServiceError error = ServiceError::Ok;
        std::optional<std::string> error_message;
        std::uint64_t deadline_ms = 0;
        bool done = false;
        InprocServiceStats *stats = nullptr;  ///< 指向 server 共享状态的统计计数器。
    };

   public:
    /**
     * @brief 构造一个已经就绪的错误 handle。
     *
     * 用于 generated shell 在 transport 尚不可用或调用提交阶段已经失败时，仍保持
     * `start_call()` 非阻塞 API 不抛异常。调用方随后 `poll()` 会立即返回 true，
     * `complete()` 会返回对应 `ServiceError`。
     */
    static InprocServiceHandle ready_error(ServiceError error) {
        auto state = std::make_shared<State>();
        state->error = error;
        state->done = true;
        return InprocServiceHandle(std::move(state));
    }

   private:
    explicit InprocServiceHandle(std::shared_ptr<State> state) : state_(std::move(state)) {}

    ServiceResult<Resp> take_result() {
        if (state_->error != ServiceError::Ok) {
            if (state_->error_message) {
                return ServiceResult<Resp>::err_with_message(state_->error,
                                                             std::move(*state_->error_message));
            }
            return ServiceResult<Resp>::err(state_->error);
        }
        return ServiceResult<Resp>::ok(std::move(*state_->response));
    }

    std::shared_ptr<State> state_;
};

// ── InprocServiceServer ─────────────────────────────────────────────────────

/**
 * @brief inproc service server。
 *
 * 注册 typed handler 到 `InprocServiceRegistry`。runtime 通过 `process_pending()` 驱动
 * server 处理排队中的请求，不依赖 tick polling。请求到达时可选触发回调通知 runtime。
 *
 * @tparam Req 请求消息类型。
 * @tparam Resp 响应消息类型。
 */
template <typename Req, typename Resp>
class InprocServiceServer {
   public:
    using Handler = std::function<ServiceResult<Resp>(const Req &)>;

    /**
     * @brief 构造 server 并自动注册到 registry。
     *
     * @param name service canonical name。
     * @param handler 请求处理函数。server 存活期间必须有效。
     * @param config service 配置。
     * @param on_request_arrived 请求到达时的可选回调（用于唤醒 server task）。
     */
    InprocServiceServer(std::string name, Handler handler, InprocServiceConfig config = {},
                        std::function<void()> on_request_arrived = nullptr)
        : name_(std::move(name)), handler_(std::move(handler)) {
        state_ = std::make_shared<detail::InprocServiceState>();
        state_->queue_depth = config.queue_depth;
        state_->max_in_flight = config.max_in_flight;
        state_->default_timeout_ms =
            config.default_timeout_ms == 0 ? 5000 : config.default_timeout_ms;
        state_->on_request_arrived = std::move(on_request_arrived);
        InprocServiceRegistry::instance().register_service(name_, state_);
    }

    /**
     * @brief 析构时自动从 registry 注销，并标记 service 不可用。
     */
    ~InprocServiceServer() {
        InprocServiceRegistry::instance().unregister_service(name_);
        std::lock_guard lock(state_->mutex);
        state_->available = false;
    }

    InprocServiceServer(const InprocServiceServer &) = delete;
    InprocServiceServer &operator=(const InprocServiceServer &) = delete;
    InprocServiceServer(InprocServiceServer &&) = delete;
    InprocServiceServer &operator=(InprocServiceServer &&) = delete;

    /**
     * @brief 处理排队中的请求。
     *
     * 每个请求在当前线程上调用 handler。如果 handler 抛出异常，该请求返回 HandlerError。
     * 不持有锁期间执行 handler，允许 handler 内部调用其他 service（但同 lane 会被检测）。
     *
     * @return 本次处理的请求数量。
     */
    std::size_t process_pending() {
        std::vector<detail::InprocCallEntry> entries;
        {
            std::lock_guard lock(state_->mutex);
            entries.reserve(state_->queue.size());
            while (!state_->queue.empty()) {
                entries.push_back(
                    std::move(*std::any_cast<detail::InprocCallEntry>(&state_->queue.front())));
                state_->queue.pop_front();
            }
        }

        for (auto &entry : entries) {
            ServiceError error = ServiceError::Ok;
            std::any response;
            std::optional<std::string> error_message;
            try {
                auto req = std::any_cast<Req>(std::move(entry.request));
                auto result = handler_(req);
                if (result.is_ok()) {
                    response = std::move(*std::move(result).take_value());
                } else {
                    error = result.error_code();
                    error_message = result.error_message();
                }
            } catch (const std::exception &e) {
                error = ServiceError::HandlerError;
                error_message = e.what();
            } catch (...) {
                error = ServiceError::HandlerError;
            }
            entry.deliver(error, std::move(response), std::move(error_message));
            {
                std::lock_guard lock(state_->mutex);
                if (state_->in_flight > 0) {
                    --state_->in_flight;
                }
                if (error == ServiceError::Ok) {
                    ++state_->stats.completed;
                } else if (error == ServiceError::HandlerError) {
                    ++state_->stats.handler_error;
                }
            }
        }
        return entries.size();
    }

    /** @brief 返回 service canonical name。 */
    const std::string &name() const noexcept { return name_; }

    /** @brief 返回当前排队中的请求数量。 */
    std::size_t pending_count() const noexcept {
        std::lock_guard lock(state_->mutex);
        return state_->queue.size();
    }

    /** @brief 返回当前处理中的请求数量。 */
    std::uint64_t in_flight_count() const noexcept {
        std::lock_guard lock(state_->mutex);
        return state_->in_flight;
    }

    /** @brief 返回调用统计的快照。 */
    InprocServiceStats stats() const noexcept {
        std::lock_guard lock(state_->mutex);
        return state_->stats;
    }

    /** @brief 返回内部共享状态（用于测试或高级场景）。 */
    std::shared_ptr<detail::InprocServiceState> shared_state() const noexcept { return state_; }

   private:
    template <typename R, typename S>
    friend class InprocServiceClient;

    std::string name_;
    Handler handler_;
    std::shared_ptr<detail::InprocServiceState> state_;
};

// ── InprocServiceClient ─────────────────────────────────────────────────────

/**
 * @brief inproc service client。
 *
 * 发起 typed request，返回非阻塞 `InprocServiceHandle`。支持 deadline-bound 调用，
 * 超时返回 `Timeout`。服务未注册返回 `Unavailable`，队列满返回 `Busy`。
 *
 * @tparam Req 请求消息类型。
 * @tparam Resp 响应消息类型。
 */
template <typename Req, typename Resp>
class InprocServiceClient {
   public:
    /**
     * @brief 从 server 引用构造 client。
     *
     * @param name 目标 service canonical name。
     * @param server 目标 server 引用。server 存活期间必须有效。
     * @param caller_lane 调用方所在 lane ID。0 表示不检测死锁。
     * @param server_lane server 所在 lane ID。0 表示不检测死锁。
     */
    InprocServiceClient(std::string name, const InprocServiceServer<Req, Resp> &server,
                        std::uint64_t caller_lane = 0, std::uint64_t server_lane = 0)
        : name_(std::move(name)),
          state_(server.state_),
          caller_lane_(caller_lane),
          server_lane_(server_lane) {}

    /**
     * @brief 从共享状态直接构造 client（用于测试或高级场景）。
     *
     * @param name 目标 service canonical name。
     * @param state server 共享状态。
     * @param caller_lane 调用方所在 lane ID。
     * @param server_lane server 所在 lane ID。
     */
    InprocServiceClient(std::string name, std::shared_ptr<detail::InprocServiceState> state,
                        std::uint64_t caller_lane, std::uint64_t server_lane)
        : name_(std::move(name)),
          state_(std::move(state)),
          caller_lane_(caller_lane),
          server_lane_(server_lane) {}

    /**
     * @brief 发起 deadline-bound 阻塞请求。
     *
     * @param request 请求消息。
     * @param timeout_ms 超时毫秒。0 会立即返回 `Timeout`。
     * @return 成功返回响应值，超时或错误返回 ServiceError。
     */
    ServiceResult<Resp> call(Req request) {
        return call(std::move(request), state_->default_timeout_ms);
    }

    ServiceResult<Resp> call(Req request, std::uint64_t timeout_ms) {
        auto handle = start_call(std::move(request), timeout_ms);
        return handle.complete();
    }

    /**
     * @brief 发起 deadline-bound 非阻塞请求。
     *
     * @param request 请求消息。
     * @param timeout_ms 超时毫秒。0 会立即返回 `Timeout`。
     * @return 非阻塞 handle，可 `wait()` 或 `poll()` 获取响应。
     */
    InprocServiceHandle<Resp> start_call(Req request) {
        return start_call(std::move(request), state_->default_timeout_ms);
    }

    InprocServiceHandle<Resp> start_call(Req request, std::uint64_t timeout_ms) {
        if (caller_lane_ != 0 && caller_lane_ == server_lane_) {
            auto handle = make_error_handle(ServiceError::WouldDeadlock);
            {
                std::lock_guard lock(state_->mutex);
                ++state_->stats.deadlocks;
            }
            return handle;
        }

        {
            std::lock_guard lock(state_->mutex);
            if (!state_->available) {
                ++state_->stats.unavailable;
                return make_error_handle(ServiceError::Unavailable);
            }
        }

        if (timeout_ms == 0) {
            auto handle = make_error_handle(ServiceError::Timeout);
            {
                std::lock_guard lock(state_->mutex);
                ++state_->stats.timeouts;
            }
            return handle;
        }

        const auto deadline_ms = detail::monotonic_now_ms() + timeout_ms;

        auto call_state = std::make_shared<typename InprocServiceHandle<Resp>::State>();
        call_state->deadline_ms = deadline_ms;
        call_state->stats = &state_->stats;

        {
            std::lock_guard lock(state_->mutex);
            if (!state_->available) {
                ++state_->stats.unavailable;
                call_state->error = ServiceError::Unavailable;
                call_state->done = true;
                return InprocServiceHandle<Resp>(call_state);
            }
            if (state_->queue.size() >= state_->queue_depth) {
                ++state_->stats.busy;
                call_state->error = ServiceError::Busy;
                call_state->done = true;
                return InprocServiceHandle<Resp>(call_state);
            }
            if (state_->in_flight >= state_->max_in_flight) {
                ++state_->stats.busy;
                call_state->error = ServiceError::Busy;
                call_state->done = true;
                return InprocServiceHandle<Resp>(call_state);
            }

            auto on_deliver = [call_state](ServiceError error, std::any &&response_any,
                                           std::optional<std::string> msg) {
                std::lock_guard inner(call_state->mutex);
                if (call_state->done) {
                    // late response：client 已超时或已取消，丢弃响应并计数。
                    if (call_state->stats) {
                        ++call_state->stats->late_dropped;
                    }
                    return;
                }
                call_state->error = error;
                call_state->error_message = std::move(msg);
                if (error == ServiceError::Ok && response_any.has_value()) {
                    try {
                        call_state->response = std::any_cast<Resp>(std::move(response_any));
                    } catch (...) {
                        call_state->error = ServiceError::Protocol;
                    }
                }
                call_state->done = true;
                call_state->cv.notify_all();
            };

            state_->queue.push_back(
                detail::InprocCallEntry{std::move(request), std::move(on_deliver)});
            ++state_->in_flight;
            if (state_->on_request_arrived) {
                state_->on_request_arrived();
            }
        }

        return InprocServiceHandle<Resp>(call_state);
    }

    /** @brief 返回目标 service canonical name。 */
    const std::string &name() const noexcept { return name_; }

   private:
    InprocServiceHandle<Resp> make_error_handle(ServiceError error) {
        auto call_state = std::make_shared<typename InprocServiceHandle<Resp>::State>();
        call_state->error = error;
        call_state->done = true;
        return InprocServiceHandle<Resp>(call_state);
    }

    std::string name_;
    std::shared_ptr<detail::InprocServiceState> state_;
    std::uint64_t caller_lane_;
    std::uint64_t server_lane_;
};

}  // namespace flowrt
