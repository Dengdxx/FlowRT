#pragma once

#include <atomic>
#include <chrono>
#include <cstdint>
#include <memory>
#include <optional>
#include <string_view>

namespace flowrt {

/**
 * @brief 唯一标识一次 Operation invocation。
 */
struct OperationId {
    std::uint64_t operation_key = 0;  ///< Operation endpoint canonical name 的稳定 key。
    std::uint64_t client_id = 0;      ///< 发起方 runtime/client 标识。
    std::uint64_t sequence = 0;       ///< 发起方内单调递增序号。

    friend constexpr bool operator==(const OperationId &, const OperationId &) noexcept = default;
};

/**
 * @brief Operation 状态机状态。
 */
enum class OperationState : std::uint8_t {
    Accepted = 0,  ///< start request 已接受，尚未进入用户 handler。
    Running = 1,   ///< 用户 handler 正在执行。
    Canceling = 2,  ///< 已请求 cooperative cancel，等待用户 handler 观察 token 并退出。
    Succeeded = 3,  ///< 用户 handler 成功完成。
    Failed = 4,     ///< 用户 handler 或 runtime 执行失败。
    Canceled = 5,   ///< 用户 handler 响应 cancel 请求并结束。
    Timeout = 6,    ///< Operation 超时。
    Preempted = 7,  ///< 因 preempt policy 被新 invocation 抢占。
};

/**
 * @brief Operation runtime 错误。
 */
enum class OperationError : std::uint8_t {
    Ok = 0,                 ///< 操作成功。
    InvalidTransition = 1,  ///< 状态转换不合法。
    InvalidPolicy = 2,      ///< policy 字段非法。
};

/**
 * @brief Operation 并发策略。
 */
enum class OperationConcurrencyPolicy : std::uint8_t {
    Reject = 0,  ///< 当前已有 invocation 时拒绝新 start request。
    Queue = 1,   ///< 当前已有 invocation 时按有界队列排队。
};

/**
 * @brief Operation 抢占策略。
 */
enum class OperationPreemptPolicy : std::uint8_t {
    Reject = 0,         ///< 不抢占正在运行的 invocation。
    CancelRunning = 1,  ///< 请求 cancel 当前 invocation 后启动新 invocation。
};

inline constexpr bool is_terminal(OperationState state) noexcept {
    return state == OperationState::Succeeded || state == OperationState::Failed ||
           state == OperationState::Canceled || state == OperationState::Timeout ||
           state == OperationState::Preempted;
}

inline std::string_view to_string(OperationState state) noexcept {
    switch (state) {
        case OperationState::Accepted:
            return "Accepted";
        case OperationState::Running:
            return "Running";
        case OperationState::Canceling:
            return "Canceling";
        case OperationState::Succeeded:
            return "Succeeded";
        case OperationState::Failed:
            return "Failed";
        case OperationState::Canceled:
            return "Canceled";
        case OperationState::Timeout:
            return "Timeout";
        case OperationState::Preempted:
            return "Preempted";
    }
    return "Unknown";
}

inline std::string_view to_string(OperationError error) noexcept {
    switch (error) {
        case OperationError::Ok:
            return "Ok";
        case OperationError::InvalidTransition:
            return "InvalidTransition";
        case OperationError::InvalidPolicy:
            return "InvalidPolicy";
    }
    return "Unknown";
}

/**
 * @brief Operation policy。
 */
struct OperationPolicy {
    std::chrono::milliseconds timeout{30000};  ///< invocation 超时时间。
    OperationConcurrencyPolicy concurrency{OperationConcurrencyPolicy::Reject};  ///< 并发策略。
    OperationPreemptPolicy preempt{OperationPreemptPolicy::Reject};  ///< 抢占策略。
    std::uint32_t queue_depth = 8;                                   ///< 等待队列深度。
    std::uint32_t max_in_flight = 1;                                 ///< 最大 in-flight 数。

    /**
     * @brief 构造并校验 Operation policy。
     */
    static std::optional<OperationPolicy> make(std::chrono::milliseconds timeout,
                                               OperationConcurrencyPolicy concurrency,
                                               OperationPreemptPolicy preempt,
                                               std::uint32_t queue_depth,
                                               std::uint32_t max_in_flight) noexcept {
        if (timeout.count() <= 0 || queue_depth == 0U || max_in_flight == 0U) {
            return std::nullopt;
        }
        return OperationPolicy{
            .timeout = timeout,
            .concurrency = concurrency,
            .preempt = preempt,
            .queue_depth = queue_depth,
            .max_in_flight = max_in_flight,
        };
    }

    /**
     * @brief 判断当前 policy 是否有效。
     */
    bool valid() const noexcept {
        return timeout.count() > 0 && queue_depth != 0U && max_in_flight != 0U;
    }
};

/**
 * @brief Cooperative cancel token。
 */
class OperationCancelToken {
   public:
    OperationCancelToken() : canceled_(std::make_shared<std::atomic_bool>(false)) {}

    /** @brief 请求用户 handler 在安全边界自行退出。 */
    void request_cancel() const noexcept { canceled_->store(true, std::memory_order_seq_cst); }

    /** @brief 查询是否已有 cancel 请求。 */
    bool is_canceled() const noexcept { return canceled_->load(std::memory_order_seq_cst); }

   private:
    std::shared_ptr<std::atomic_bool> canceled_;
};

/**
 * @brief Operation 健康计数快照。
 */
struct OperationHealthSnapshot {
    std::uint64_t started = 0;
    std::uint64_t succeeded = 0;
    std::uint64_t failed = 0;
    std::uint64_t canceled = 0;
    std::uint64_t timeout = 0;
    std::uint64_t preempted = 0;
};

/**
 * @brief Operation 健康计数器。
 */
class OperationHealthCounters {
   public:
    /** @brief 按状态进入事件记录计数。 */
    void record_state(OperationState state) noexcept {
        switch (state) {
            case OperationState::Running:
                ++started_;
                break;
            case OperationState::Succeeded:
                ++succeeded_;
                break;
            case OperationState::Failed:
                ++failed_;
                break;
            case OperationState::Canceled:
                ++canceled_;
                break;
            case OperationState::Timeout:
                ++timeout_;
                break;
            case OperationState::Preempted:
                ++preempted_;
                break;
            case OperationState::Accepted:
            case OperationState::Canceling:
                break;
        }
    }

    /** @brief 读取健康计数快照。 */
    OperationHealthSnapshot snapshot() const noexcept {
        return OperationHealthSnapshot{
            .started = started_.load(std::memory_order_relaxed),
            .succeeded = succeeded_.load(std::memory_order_relaxed),
            .failed = failed_.load(std::memory_order_relaxed),
            .canceled = canceled_.load(std::memory_order_relaxed),
            .timeout = timeout_.load(std::memory_order_relaxed),
            .preempted = preempted_.load(std::memory_order_relaxed),
        };
    }

   private:
    std::atomic_uint64_t started_{0};
    std::atomic_uint64_t succeeded_{0};
    std::atomic_uint64_t failed_{0};
    std::atomic_uint64_t canceled_{0};
    std::atomic_uint64_t timeout_{0};
    std::atomic_uint64_t preempted_{0};
};

/**
 * @brief Operation 状态快照。
 */
struct OperationStatusSnapshot {
    OperationId id{};
    OperationState state{OperationState::Accepted};
    bool cancel_requested = false;
    OperationHealthSnapshot health{};
};

/**
 * @brief Operation progress event carrier。
 */
template <typename T>
struct OperationProgress {
    OperationId id{};
    std::uint64_t sequence = 0;
    T value{};
};

/**
 * @brief Operation 生命周期状态机。
 */
class OperationLifecycle {
   public:
    OperationLifecycle(OperationId id, OperationPolicy policy)
        : id_(id),
          policy_(policy),
          valid_policy_(policy.valid()),
          state_(OperationState::Accepted) {}

    /** @brief 返回 invocation ID。 */
    constexpr OperationId id() const noexcept { return id_; }

    /** @brief 返回 policy。 */
    constexpr OperationPolicy policy() const noexcept { return policy_; }

    /** @brief 返回当前状态。 */
    constexpr OperationState state() const noexcept { return state_; }

    /** @brief 返回可共享给用户 handler 的 cancel token。 */
    OperationCancelToken cancel_token() const noexcept { return cancel_token_; }

    /** @brief 进入下一个状态。 */
    OperationError transition(OperationState to) noexcept {
        if (!valid_policy_) {
            return OperationError::InvalidPolicy;
        }
        if (!valid_transition(state_, to)) {
            return OperationError::InvalidTransition;
        }
        state_ = to;
        health_.record_state(to);
        return OperationError::Ok;
    }

    /** @brief 请求 cooperative cancel。 */
    OperationError request_cancel() noexcept {
        if (!valid_policy_) {
            return OperationError::InvalidPolicy;
        }
        cancel_token_.request_cancel();
        if (state_ == OperationState::Accepted || state_ == OperationState::Running) {
            return transition(OperationState::Canceling);
        }
        if (state_ == OperationState::Canceling) {
            return OperationError::Ok;
        }
        return OperationError::InvalidTransition;
    }

    /** @brief 返回当前状态快照。 */
    OperationStatusSnapshot snapshot() const noexcept {
        return OperationStatusSnapshot{
            .id = id_,
            .state = state_,
            .cancel_requested = cancel_token_.is_canceled(),
            .health = health_.snapshot(),
        };
    }

   private:
    static constexpr bool valid_transition(OperationState from, OperationState to) noexcept {
        return (from == OperationState::Accepted &&
                (to == OperationState::Running || to == OperationState::Canceling ||
                 to == OperationState::Failed || to == OperationState::Timeout ||
                 to == OperationState::Preempted)) ||
               (from == OperationState::Running &&
                (to == OperationState::Succeeded || to == OperationState::Failed ||
                 to == OperationState::Canceling || to == OperationState::Timeout ||
                 to == OperationState::Preempted)) ||
               (from == OperationState::Canceling &&
                (to == OperationState::Canceled || to == OperationState::Failed ||
                 to == OperationState::Timeout));
    }

    OperationId id_{};
    OperationPolicy policy_{};
    bool valid_policy_ = true;
    OperationState state_{OperationState::Accepted};
    OperationCancelToken cancel_token_{};
    OperationHealthCounters health_{};
};

}  // namespace flowrt
