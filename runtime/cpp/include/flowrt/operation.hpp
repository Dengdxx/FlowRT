#pragma once

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <flowrt/service.hpp>
#include <flowrt/wire.hpp>
#include <functional>
#include <limits>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <string_view>
#include <utility>
#include <vector>

namespace flowrt {

/**
 * @brief 唯一标识一次 Operation invocation。
 */
struct OperationId {
    std::uint64_t operation_key = 0;  ///< Operation endpoint canonical name 的稳定 key。
    std::uint64_t client_id = 0;      ///< 发起方 runtime/client 标识。
    std::uint64_t sequence = 0;       ///< 发起方内单调递增序号。

    friend constexpr bool operator==(const OperationId &, const OperationId &) noexcept = default;

    static constexpr std::size_t wire_size() noexcept { return sizeof(std::uint64_t) * 3U; }

    void encode_wire(std::span<std::uint8_t> output) const {
        ensure_wire_size(wire_size(), output.size());
        write_wire_le(output, 0U, operation_key);
        write_wire_le(output, 8U, client_id);
        write_wire_le(output, 16U, sequence);
    }

    static OperationId decode_wire(std::span<const std::uint8_t> input) {
        ensure_wire_size(wire_size(), input.size());
        return OperationId{
            .operation_key = read_wire_le<std::uint64_t>(input, 0U),
            .client_id = read_wire_le<std::uint64_t>(input, 8U),
            .sequence = read_wire_le<std::uint64_t>(input, 16U),
        };
    }
};

/**
 * @brief Operation control authority owner。
 */
struct OperationOwner {
    std::uint64_t scope_key = 0;  ///< 控制域 key。
    std::uint64_t owner_key = 0;  ///< owner key。

    friend constexpr bool operator==(const OperationOwner &,
                                     const OperationOwner &) noexcept = default;

    static constexpr std::size_t wire_size() noexcept { return sizeof(std::uint64_t) * 2U; }

    void encode_wire(std::span<std::uint8_t> output) const {
        ensure_wire_size(wire_size(), output.size());
        write_wire_le(output, 0U, scope_key);
        write_wire_le(output, 8U, owner_key);
    }

    static OperationOwner decode_wire(std::span<const std::uint8_t> input) {
        ensure_wire_size(wire_size(), input.size());
        return OperationOwner{
            .scope_key = read_wire_le<std::uint64_t>(input, 0U),
            .owner_key = read_wire_le<std::uint64_t>(input, 8U),
        };
    }
};

/**
 * @brief Operation 状态机状态。
 */
enum class OperationState : std::uint8_t {
    Idle = 0,             ///< 没有 active invocation。
    Starting = 1,         ///< start request 已接受，尚未进入用户 handler。
    Running = 2,          ///< 用户 handler 正在执行。
    CancelRequested = 3,  ///< 已请求 cooperative cancel。
    Succeeded = 4,        ///< 用户 handler 成功完成。
    Failed = 5,           ///< 用户 handler 或 runtime 执行失败。
    Cancelled = 6,        ///< 用户 handler 响应 cancel 请求并结束。
    TimedOut = 7,         ///< Operation 超时。
};

namespace operation_wire_detail {

inline void encode_state(OperationState state, std::span<std::uint8_t> output) {
    ensure_wire_size(sizeof(std::uint8_t), output.size());
    write_wire_le(output, 0U, static_cast<std::uint8_t>(state));
}

inline OperationState decode_state(std::span<const std::uint8_t> input) {
    ensure_wire_size(sizeof(std::uint8_t), input.size());
    const auto value = read_wire_le<std::uint8_t>(input, 0U);
    switch (value) {
        case 0U:
            return OperationState::Idle;
        case 1U:
            return OperationState::Starting;
        case 2U:
            return OperationState::Running;
        case 3U:
            return OperationState::CancelRequested;
        case 4U:
            return OperationState::Succeeded;
        case 5U:
            return OperationState::Failed;
        case 6U:
            return OperationState::Cancelled;
        case 7U:
            return OperationState::TimedOut;
        default:
            throw WireCodecError("operation state discriminant is unknown");
    }
}

inline std::uint64_t timeout_ms_from_duration(std::chrono::milliseconds timeout) noexcept {
    if (timeout.count() <= 0) {
        return 0U;
    }
    return static_cast<std::uint64_t>(timeout.count());
}

inline std::chrono::milliseconds duration_from_timeout_ms(std::uint64_t timeout_ms) noexcept {
    using Rep = std::chrono::milliseconds::rep;
    const auto max_timeout = static_cast<std::uint64_t>(std::numeric_limits<Rep>::max());
    return std::chrono::milliseconds{
        static_cast<Rep>(std::min(timeout_ms, max_timeout)),
    };
}

}  // namespace operation_wire_detail

/**
 * @brief Operation runtime 错误。
 */
enum class OperationError : std::uint8_t {
    Ok = 0,                 ///< 操作成功。
    InvalidTransition = 1,  ///< 状态转换不合法。
    InvalidPolicy = 2,      ///< policy 字段非法。
};

/**
 * @brief Operation control authority 错误。
 */
enum class OperationControlError : std::uint8_t {
    Ok = 0,
    InvalidTransition = 1,
    InvalidPolicy = 2,
    Busy = 3,
    OwnerConflict = 4,
    StaleInvocation = 5,
    AlreadyTerminal = 6,
};

/**
 * @brief Operation client 调用错误。
 */
enum class OperationClientError : std::uint8_t {
    Timeout = 0,
    Unavailable = 1,
    Busy = 2,
    Rejected = 3,
    Cancelled = 4,
    Backend = 5,
    WouldDeadlock = 6,
    HandlerError = 7,
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
           state == OperationState::Cancelled || state == OperationState::TimedOut;
}

inline std::string_view to_string(OperationState state) noexcept {
    switch (state) {
        case OperationState::Idle:
            return "idle";
        case OperationState::Starting:
            return "starting";
        case OperationState::Running:
            return "running";
        case OperationState::CancelRequested:
            return "cancel_requested";
        case OperationState::Succeeded:
            return "succeeded";
        case OperationState::Failed:
            return "failed";
        case OperationState::Cancelled:
            return "cancelled";
        case OperationState::TimedOut:
            return "timed_out";
    }
    return "unknown";
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

inline std::string_view to_string(OperationControlError error) noexcept {
    switch (error) {
        case OperationControlError::Ok:
            return "Ok";
        case OperationControlError::InvalidTransition:
            return "InvalidTransition";
        case OperationControlError::InvalidPolicy:
            return "InvalidPolicy";
        case OperationControlError::Busy:
            return "Busy";
        case OperationControlError::OwnerConflict:
            return "OwnerConflict";
        case OperationControlError::StaleInvocation:
            return "StaleInvocation";
        case OperationControlError::AlreadyTerminal:
            return "AlreadyTerminal";
    }
    return "Unknown";
}

inline std::uint64_t monotonic_time_ms() noexcept {
    return static_cast<std::uint64_t>(std::chrono::duration_cast<std::chrono::milliseconds>(
                                          std::chrono::steady_clock::now().time_since_epoch())
                                          .count());
}

inline OperationClientError operation_client_error_from_service_error(ServiceError error) noexcept {
    switch (error) {
        case ServiceError::Timeout:
        case ServiceError::DeadlineExceeded:
            return OperationClientError::Timeout;
        case ServiceError::Unavailable:
            return OperationClientError::Unavailable;
        case ServiceError::Busy:
            return OperationClientError::Busy;
        case ServiceError::Rejected:
            return OperationClientError::Rejected;
        case ServiceError::Cancelled:
            return OperationClientError::Cancelled;
        case ServiceError::Backend:
        case ServiceError::Protocol:
            return OperationClientError::Backend;
        case ServiceError::WouldDeadlock:
            return OperationClientError::WouldDeadlock;
        case ServiceError::HandlerError:
        case ServiceError::Ok:
            return OperationClientError::HandlerError;
    }
    return OperationClientError::HandlerError;
}

/**
 * @brief Operation client typed result。
 */
template <typename T>
class OperationClientResult {
   public:
    static OperationClientResult ok(T value) {
        OperationClientResult result;
        result.value_ = std::move(value);
        return result;
    }

    static OperationClientResult err(OperationClientError error) {
        OperationClientResult result;
        result.error_ = error;
        return result;
    }

    constexpr bool is_ok() const noexcept { return value_.has_value(); }

    constexpr bool is_err() const noexcept { return !is_ok(); }

    constexpr OperationClientError error_code() const noexcept { return error_; }

    const std::optional<T> &value() const noexcept { return value_; }

   private:
    std::optional<T> value_;
    OperationClientError error_{OperationClientError::HandlerError};
};

template <typename T>
OperationClientResult<T> operation_client_result_from_service(ServiceResult<T> result) {
    if (result.is_ok()) {
        auto value = std::move(result).take_value();
        if (value.has_value()) {
            return OperationClientResult<T>::ok(std::move(*value));
        }
        return OperationClientResult<T>::err(OperationClientError::HandlerError);
    }
    return OperationClientResult<T>::err(
        operation_client_error_from_service_error(result.error_code()));
}

/**
 * @brief Operation policy。
 */
struct OperationPolicy {
    std::chrono::milliseconds timeout{30000};  ///< invocation 超时时间。
    OperationConcurrencyPolicy concurrency{OperationConcurrencyPolicy::Reject};  ///< 并发策略。
    OperationPreemptPolicy preempt{OperationPreemptPolicy::Reject};              ///< 抢占策略。
    std::uint32_t queue_depth = 8;                                               ///< 等待队列深度。
    std::uint32_t max_in_flight = 1;                ///< 最大 in-flight 数。
    std::chrono::milliseconds result_retention{0};  ///< 终态快照保留时间。

    /**
     * @brief 构造并校验 Operation policy。
     */
    static std::optional<OperationPolicy> make(
        std::chrono::milliseconds timeout, OperationConcurrencyPolicy concurrency,
        OperationPreemptPolicy preempt, std::uint32_t queue_depth, std::uint32_t max_in_flight,
        std::chrono::milliseconds result_retention = std::chrono::milliseconds{0}) noexcept {
        if (timeout.count() <= 0 || queue_depth == 0U || max_in_flight == 0U) {
            return std::nullopt;
        }
        return OperationPolicy{
            .timeout = timeout,
            .concurrency = concurrency,
            .preempt = preempt,
            .queue_depth = queue_depth,
            .max_in_flight = max_in_flight,
            .result_retention = result_retention,
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

    static constexpr std::size_t wire_size() noexcept { return sizeof(std::uint64_t) * 6U; }

    void encode_wire(std::span<std::uint8_t> output) const {
        ensure_wire_size(wire_size(), output.size());
        write_wire_le(output, 0U, started);
        write_wire_le(output, 8U, succeeded);
        write_wire_le(output, 16U, failed);
        write_wire_le(output, 24U, canceled);
        write_wire_le(output, 32U, timeout);
        write_wire_le(output, 40U, preempted);
    }

    static OperationHealthSnapshot decode_wire(std::span<const std::uint8_t> input) {
        ensure_wire_size(wire_size(), input.size());
        return OperationHealthSnapshot{
            .started = read_wire_le<std::uint64_t>(input, 0U),
            .succeeded = read_wire_le<std::uint64_t>(input, 8U),
            .failed = read_wire_le<std::uint64_t>(input, 16U),
            .canceled = read_wire_le<std::uint64_t>(input, 24U),
            .timeout = read_wire_le<std::uint64_t>(input, 32U),
            .preempted = read_wire_le<std::uint64_t>(input, 40U),
        };
    }
};

/**
 * @brief Operation 健康计数器。
 */
class OperationHealthCounters {
   public:
    OperationHealthCounters() = default;
    OperationHealthCounters(const OperationHealthCounters &) = delete;
    OperationHealthCounters &operator=(const OperationHealthCounters &) = delete;

    OperationHealthCounters(OperationHealthCounters &&other) noexcept {
        assign_snapshot(other.snapshot());
    }

    OperationHealthCounters &operator=(OperationHealthCounters &&other) noexcept {
        if (this != &other) {
            assign_snapshot(other.snapshot());
        }
        return *this;
    }

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
            case OperationState::Cancelled:
                ++canceled_;
                break;
            case OperationState::TimedOut:
                ++timeout_;
                break;
            case OperationState::Idle:
            case OperationState::Starting:
            case OperationState::CancelRequested:
                break;
        }
    }

    /** @brief 记录一次抢占事件。 */
    void record_preempted() noexcept { ++preempted_; }

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
    void assign_snapshot(OperationHealthSnapshot snapshot) noexcept {
        started_.store(snapshot.started, std::memory_order_relaxed);
        succeeded_.store(snapshot.succeeded, std::memory_order_relaxed);
        failed_.store(snapshot.failed, std::memory_order_relaxed);
        canceled_.store(snapshot.canceled, std::memory_order_relaxed);
        timeout_.store(snapshot.timeout, std::memory_order_relaxed);
        preempted_.store(snapshot.preempted, std::memory_order_relaxed);
    }

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
    OperationOwner owner{};
    OperationState state{OperationState::Idle};
    bool cancel_requested = false;
    std::uint64_t deadline_ms = 0;
    OperationHealthSnapshot health{};

    static constexpr std::size_t wire_size() noexcept {
        return OperationId::wire_size() + OperationOwner::wire_size() + sizeof(std::uint8_t) +
               sizeof(std::uint8_t) + sizeof(std::uint64_t) + OperationHealthSnapshot::wire_size();
    }

    void encode_wire(std::span<std::uint8_t> output) const {
        ensure_wire_size(wire_size(), output.size());
        id.encode_wire(output.subspan(0U, OperationId::wire_size()));
        owner.encode_wire(output.subspan(24U, OperationOwner::wire_size()));
        operation_wire_detail::encode_state(state, output.subspan(40U, 1U));
        write_wire_le(output, 41U, cancel_requested);
        write_wire_le(output, 42U, deadline_ms);
        health.encode_wire(output.subspan(50U, OperationHealthSnapshot::wire_size()));
    }

    static OperationStatusSnapshot decode_wire(std::span<const std::uint8_t> input) {
        ensure_wire_size(wire_size(), input.size());
        return OperationStatusSnapshot{
            .id = OperationId::decode_wire(input.subspan(0U, OperationId::wire_size())),
            .owner = OperationOwner::decode_wire(input.subspan(24U, OperationOwner::wire_size())),
            .state = operation_wire_detail::decode_state(input.subspan(40U, 1U)),
            .cancel_requested = read_wire_le<bool>(input, 41U),
            .deadline_ms = read_wire_le<std::uint64_t>(input, 42U),
            .health = OperationHealthSnapshot::decode_wire(
                input.subspan(50U, OperationHealthSnapshot::wire_size())),
        };
    }
};

/**
 * @brief Operation start 请求被 runtime 接受后的响应。
 */
struct OperationStartAck {
    OperationId id{};
    OperationOwner owner{};
    std::uint64_t deadline_ms = 0;
    bool accepted = false;

    static constexpr std::size_t wire_size() noexcept {
        return OperationId::wire_size() + OperationOwner::wire_size() + sizeof(std::uint64_t) +
               sizeof(std::uint8_t);
    }

    void encode_wire(std::span<std::uint8_t> output) const {
        ensure_wire_size(wire_size(), output.size());
        id.encode_wire(output.subspan(0U, OperationId::wire_size()));
        owner.encode_wire(output.subspan(24U, OperationOwner::wire_size()));
        write_wire_le(output, 40U, deadline_ms);
        write_wire_le(output, 48U, accepted);
    }

    static OperationStartAck decode_wire(std::span<const std::uint8_t> input) {
        ensure_wire_size(wire_size(), input.size());
        return OperationStartAck{
            .id = OperationId::decode_wire(input.subspan(0U, OperationId::wire_size())),
            .owner = OperationOwner::decode_wire(input.subspan(24U, OperationOwner::wire_size())),
            .deadline_ms = read_wire_le<std::uint64_t>(input, 40U),
            .accepted = read_wire_le<bool>(input, 48U),
        };
    }

    static constexpr OperationStartAck accepted_ack(OperationId value) noexcept {
        return OperationStartAck{
            .id = value,
            .owner = OperationOwner{.scope_key = 0, .owner_key = value.client_id},
            .deadline_ms = 0,
            .accepted = true,
        };
    }

    static constexpr OperationStartAck accepted_with_authority(OperationId value,
                                                               OperationOwner owner,
                                                               std::uint64_t deadline_ms) noexcept {
        return OperationStartAck{
            .id = value,
            .owner = owner,
            .deadline_ms = deadline_ms,
            .accepted = true,
        };
    }
};

/**
 * @brief Operation start 内部 lowering 请求。
 */
template <typename T>
struct OperationStartRequest {
    T goal{};
    OperationOwner owner{};
    std::chrono::milliseconds timeout{0};

    std::size_t encoded_frame_size() const { return 24U + detail::encoded_frame_size(goal); }

    void encode_frame(std::span<std::uint8_t> output) const {
        ensure_wire_size(encoded_frame_size(), output.size());
        owner.encode_wire(output.subspan(0U, OperationOwner::wire_size()));
        write_wire_le(output, 16U, operation_wire_detail::timeout_ms_from_duration(timeout));
        detail::encode_frame(goal, output.subspan(24U));
    }

    static OperationStartRequest decode_frame(std::span<const std::uint8_t> input) {
        if (input.size() < 24U) {
            throw WireCodecError(24U, input.size());
        }
        return OperationStartRequest{
            .goal = detail::decode_frame<T>(input.subspan(24U)),
            .owner = OperationOwner::decode_wire(input.subspan(0U, OperationOwner::wire_size())),
            .timeout = operation_wire_detail::duration_from_timeout_ms(
                read_wire_le<std::uint64_t>(input, 16U)),
        };
    }
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
 * @brief Operation progress 发布器。
 */
template <typename T>
class OperationProgressPublisher {
   public:
    explicit OperationProgressPublisher(OperationId id) : id_(id) {}
    OperationProgressPublisher(OperationId id, std::function<void(OperationId, std::uint64_t)> hook)
        : id_(id),
          progress_hook_([hook = std::move(hook)](OperationId id, std::uint64_t sequence,
                                                  std::optional<std::vector<std::uint8_t>>) {
              hook(id, sequence);
          }) {}
    OperationProgressPublisher(
        OperationId id,
        std::function<void(OperationId, std::uint64_t, std::optional<std::vector<std::uint8_t>>)>
            hook)
        : id_(id), progress_hook_(std::move(hook)) {}

    void publish(T value) {
        const auto sequence = next_sequence_++;
        std::optional<std::vector<std::uint8_t>> payload;
        if constexpr (CanonicalTransportMessage<T>) {
            payload.emplace(detail::encoded_frame_size(value));
            detail::encode_frame(value, std::span<std::uint8_t>{payload->data(), payload->size()});
        }
        if (progress_hook_) {
            progress_hook_(id_, sequence, payload);
        }
        events_.push_back(
            OperationProgress<T>{.id = id_, .sequence = sequence, .value = std::move(value)});
    }

    const std::vector<OperationProgress<T>> &events() const noexcept { return events_; }

    std::vector<OperationProgress<T>> drain() {
        auto events = std::move(events_);
        events_.clear();
        return events;
    }

   private:
    OperationId id_{};
    std::uint64_t next_sequence_ = 0;
    std::vector<OperationProgress<T>> events_;
    std::function<void(OperationId, std::uint64_t, std::optional<std::vector<std::uint8_t>>)>
        progress_hook_;
};

/**
 * @brief Operation server handler 的 typed 结果。
 */
template <typename T>
class OperationHandlerResult {
   public:
    enum class Kind : std::uint8_t {
        Succeeded,
        Failed,
        Canceled,
    };

    static OperationHandlerResult succeeded(T value) {
        OperationHandlerResult result;
        result.kind_ = Kind::Succeeded;
        result.value_ = std::move(value);
        return result;
    }

    static OperationHandlerResult failed() {
        OperationHandlerResult result;
        result.kind_ = Kind::Failed;
        return result;
    }

    static OperationHandlerResult canceled() {
        OperationHandlerResult result;
        result.kind_ = Kind::Canceled;
        return result;
    }

    constexpr Kind kind() const noexcept { return kind_; }

    const std::optional<T> &value() const noexcept { return value_; }

   private:
    Kind kind_{Kind::Failed};
    std::optional<T> value_;
};

/**
 * @brief Operation runtime event 类型。
 */
enum class OperationRuntimeEventKind : std::uint8_t {
    StateChanged,
    Progress,
    Result,
    Error,
};

/**
 * @brief Operation runtime event。
 */
struct OperationRuntimeEvent {
    OperationId id{};
    OperationRuntimeEventKind kind{OperationRuntimeEventKind::StateChanged};
    std::optional<OperationState> state;
    std::optional<std::uint64_t> sequence;
    std::optional<std::vector<std::uint8_t>> payload;
    std::optional<std::string_view> message;
    std::optional<std::uint64_t> retention_ms;
};

/**
 * @brief Operation 生命周期状态机。
 */
class OperationLifecycle {
   public:
    OperationLifecycle(OperationId id, OperationPolicy policy)
        : id_(id),
          owner_(OperationOwner{.scope_key = 0, .owner_key = id.client_id}),
          deadline_ms_(static_cast<std::uint64_t>(policy.timeout.count())),
          policy_(policy),
          valid_policy_(policy.valid()),
          state_(OperationState::Starting) {}

    OperationLifecycle(const OperationLifecycle &) = delete;
    OperationLifecycle &operator=(const OperationLifecycle &) = delete;
    OperationLifecycle(OperationLifecycle &&) noexcept = default;
    OperationLifecycle &operator=(OperationLifecycle &&) noexcept = default;

    OperationLifecycle(OperationId id, OperationPolicy policy, OperationOwner owner,
                       std::uint64_t deadline_ms)
        : id_(id),
          owner_(owner),
          deadline_ms_(deadline_ms),
          policy_(policy),
          valid_policy_(policy.valid()),
          state_(OperationState::Starting) {}

    /** @brief 返回 invocation ID。 */
    constexpr OperationId id() const noexcept { return id_; }

    /** @brief 返回 policy。 */
    constexpr OperationPolicy policy() const noexcept { return policy_; }

    /** @brief 返回 owner。 */
    constexpr OperationOwner owner() const noexcept { return owner_; }

    /** @brief 返回 absolute deadline（runtime monotonic 毫秒）。 */
    constexpr std::uint64_t deadline_ms() const noexcept { return deadline_ms_; }

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
        if (state_ == OperationState::Starting || state_ == OperationState::Running) {
            return transition(OperationState::CancelRequested);
        }
        if (state_ == OperationState::CancelRequested) {
            return OperationError::Ok;
        }
        return OperationError::InvalidTransition;
    }

    /** @brief 返回当前状态快照。 */
    OperationStatusSnapshot snapshot() const noexcept {
        return OperationStatusSnapshot{
            .id = id_,
            .owner = owner_,
            .state = state_,
            .cancel_requested = cancel_token_.is_canceled(),
            .deadline_ms = deadline_ms_,
            .health = health_.snapshot(),
        };
    }

    /** @brief 供 control authority 校验 retained 状态补终态。 */
    static constexpr bool valid_transition_for_control(OperationState from,
                                                       OperationState to) noexcept {
        return valid_transition(from, to);
    }

   private:
    static constexpr bool valid_transition(OperationState from, OperationState to) noexcept {
        return (from == OperationState::Idle && to == OperationState::Starting) ||
               (from == OperationState::Starting &&
                (to == OperationState::Running || to == OperationState::CancelRequested ||
                 to == OperationState::Failed || to == OperationState::TimedOut)) ||
               (from == OperationState::Running &&
                (to == OperationState::Succeeded || to == OperationState::Failed ||
                 to == OperationState::CancelRequested || to == OperationState::TimedOut)) ||
               (from == OperationState::CancelRequested &&
                (to == OperationState::Cancelled || to == OperationState::Failed ||
                 to == OperationState::TimedOut));
    }

    OperationId id_{};
    OperationOwner owner_{};
    std::uint64_t deadline_ms_ = 0;
    OperationPolicy policy_{};
    bool valid_policy_ = true;
    OperationState state_{OperationState::Starting};
    OperationCancelToken cancel_token_;
    OperationHealthCounters health_;
};

/**
 * @brief Operation start result。
 */
class OperationStartResult {
   public:
    static OperationStartResult ok(OperationStartAck ack) {
        OperationStartResult result;
        result.value_ = ack;
        return result;
    }

    static OperationStartResult err(OperationControlError error) {
        OperationStartResult result;
        result.error_ = error;
        return result;
    }

    bool has_value() const noexcept { return value_.has_value(); }
    explicit operator bool() const noexcept { return has_value(); }
    const OperationStartAck &value() const noexcept { return *value_; }
    const OperationStartAck *operator->() const noexcept { return &*value_; }
    OperationControlError error() const noexcept { return error_; }

   private:
    std::optional<OperationStartAck> value_;
    OperationControlError error_{OperationControlError::Ok};
};

/**
 * @brief Operation status query result。
 */
class OperationStatusResult {
   public:
    static OperationStatusResult ok(OperationStatusSnapshot snapshot) {
        OperationStatusResult result;
        result.value_ = snapshot;
        return result;
    }

    static OperationStatusResult err(OperationControlError error) {
        OperationStatusResult result;
        result.error_ = error;
        return result;
    }

    bool has_value() const noexcept { return value_.has_value(); }
    explicit operator bool() const noexcept { return has_value(); }
    const OperationStatusSnapshot &value() const noexcept { return *value_; }
    const OperationStatusSnapshot *operator->() const noexcept { return &*value_; }
    OperationControlError error() const noexcept { return error_; }

   private:
    std::optional<OperationStatusSnapshot> value_;
    OperationControlError error_{OperationControlError::Ok};
};

/**
 * @brief Single-owner Operation control state。
 */
class OperationControl {
   public:
    OperationControl(std::uint64_t operation_key, OperationPolicy policy)
        : operation_key_(operation_key), policy_(policy) {}

    OperationStartResult start(OperationOwner owner, std::uint64_t now_ms) {
        return start_with_timeout(owner, now_ms, policy_.timeout);
    }

    OperationStartResult start_with_timeout(OperationOwner owner, std::uint64_t now_ms,
                                            std::chrono::milliseconds timeout) {
        std::lock_guard<std::mutex> lock(mutex_);
        if (active_count() > 0U) {
            if (lifecycle_->owner() != owner) {
                return OperationStartResult::err(OperationControlError::OwnerConflict);
            }
            const auto sequence = next_sequence_++;
            const OperationId id{
                .operation_key = operation_key_,
                .client_id = owner.owner_key,
                .sequence = sequence,
            };
            const auto timeout_ms =
                static_cast<std::uint64_t>(std::max<std::int64_t>(1, timeout.count()));
            const auto deadline_ms = now_ms + timeout_ms;
            OperationLifecycle pending{id, policy_, owner, deadline_ms};
            if (!policy_.valid()) {
                return OperationStartResult::err(OperationControlError::InvalidPolicy);
            }
            if (policy_.preempt == OperationPreemptPolicy::CancelRunning) {
                const auto cancel_error = preempt_active_invocations();
                if (cancel_error != OperationControlError::Ok) {
                    return OperationStartResult::err(cancel_error);
                }
                lifecycle_ = std::move(pending);
                handler_active_ = true;
                push_state_event(id, OperationState::Starting);
                return OperationStartResult::ok(
                    OperationStartAck::accepted_with_authority(id, owner, deadline_ms));
            }
            if (active_count() < policy_.max_in_flight) {
                in_flight_.push_back(std::move(pending));
                push_state_event(id, OperationState::Starting);
                return OperationStartResult::ok(
                    OperationStartAck::accepted_with_authority(id, owner, deadline_ms));
            }
            if (policy_.concurrency == OperationConcurrencyPolicy::Queue) {
                if (queue_.size() >= policy_.queue_depth) {
                    return OperationStartResult::err(OperationControlError::Busy);
                }
                queue_.push_back(std::move(pending));
                push_state_event(id, OperationState::Starting);
                return OperationStartResult::ok(
                    OperationStartAck::accepted_with_authority(id, owner, deadline_ms));
            }
            return OperationStartResult::err(OperationControlError::Busy);
        }
        const auto sequence = next_sequence_++;
        const OperationId id{
            .operation_key = operation_key_,
            .client_id = owner.owner_key,
            .sequence = sequence,
        };
        const auto timeout_ms =
            static_cast<std::uint64_t>(std::max<std::int64_t>(1, timeout.count()));
        const auto deadline_ms = now_ms + timeout_ms;
        if (!policy_.valid()) {
            return OperationStartResult::err(OperationControlError::InvalidPolicy);
        }
        lifecycle_.emplace(id, policy_, owner, deadline_ms);
        handler_active_ = true;
        push_state_event(id, OperationState::Starting);
        return OperationStartResult::ok(
            OperationStartAck::accepted_with_authority(id, owner, deadline_ms));
    }

    OperationControlError mark_running(OperationId id) noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        auto *lifecycle = current(id);
        if (lifecycle == nullptr) {
            return OperationControlError::StaleInvocation;
        }
        const auto error = map_error(lifecycle->transition(OperationState::Running));
        if (error == OperationControlError::Ok) {
            health_.record_state(OperationState::Running);
            push_state_event(id, OperationState::Running);
        }
        return error;
    }

    OperationControlError request_cancel(OperationId id, OperationOwner owner) noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        auto *lifecycle = current(id);
        if (lifecycle == nullptr) {
            return OperationControlError::StaleInvocation;
        }
        if (lifecycle->owner() != owner) {
            return OperationControlError::OwnerConflict;
        }
        if (is_terminal(lifecycle->state())) {
            return OperationControlError::AlreadyTerminal;
        }
        const auto error = map_error(lifecycle->request_cancel());
        if (error == OperationControlError::Ok) {
            push_state_event(id, OperationState::CancelRequested);
        }
        return error;
    }

    OperationControlError complete(OperationId id, OperationState terminal_state) noexcept {
        return complete_with_payload(id, terminal_state, std::nullopt);
    }

    OperationControlError complete_with_payload(
        OperationId id, OperationState terminal_state,
        std::optional<std::vector<std::uint8_t>> payload) noexcept {
        return complete_with_payload_at(id, terminal_state, std::move(payload),
                                        monotonic_time_ms());
    }

    OperationControlError complete_at(OperationId id, OperationState terminal_state,
                                      std::uint64_t completed_at_ms) noexcept {
        return complete_with_payload_at(id, terminal_state, std::nullopt, completed_at_ms);
    }

    OperationControlError complete_with_payload_at(OperationId id, OperationState terminal_state,
                                                   std::optional<std::vector<std::uint8_t>> payload,
                                                   std::uint64_t completed_at_ms) noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        if (!is_terminal(terminal_state)) {
            return OperationControlError::InvalidTransition;
        }
        auto active = remove_active(id);
        if (active.has_value()) {
            auto lifecycle = std::move(active->lifecycle);
            const auto was_primary = active->was_primary;
            if (is_terminal(lifecycle.state())) {
                handler_active_ = false;
                restore_active(std::move(lifecycle), was_primary);
                return OperationControlError::AlreadyTerminal;
            }
            const auto error = map_error(lifecycle.transition(terminal_state));
            handler_active_ = false;
            if (error != OperationControlError::Ok) {
                restore_active(std::move(lifecycle), was_primary);
                return error;
            }
            health_.record_state(terminal_state);
            push_result_event(id, terminal_state, std::move(payload));
            push_state_event(id, terminal_state);
            const auto snapshot = snapshot_with_health(lifecycle);
            retain_terminal_snapshot(snapshot, completed_at_ms);
            if (was_primary) {
                promote_next_active();
            }
            promote_queued_until_capacity();
            return OperationControlError::Ok;
        }
        auto retained = std::find_if(
            retained_results_.begin(), retained_results_.end(),
            [id](const RetainedOperationStatus &value) { return value.snapshot.id == id; });
        if (retained != retained_results_.end()) {
            const auto from = retained->snapshot.state;
            if (is_terminal(from)) {
                return OperationControlError::AlreadyTerminal;
            }
            if (!OperationLifecycle::valid_transition_for_control(from, terminal_state)) {
                return OperationControlError::InvalidTransition;
            }
            health_.record_state(terminal_state);
            retained->snapshot.state = terminal_state;
            retained->snapshot.health = health_.snapshot();
            retained->expires_at_ms = retention_deadline(completed_at_ms);
            const auto should_remove = !retained->expires_at_ms.has_value();
            push_result_event(id, terminal_state, std::move(payload));
            push_state_event(id, terminal_state);
            if (should_remove) {
                retained_results_.erase(retained);
            }
            return OperationControlError::Ok;
        }
        return OperationControlError::StaleInvocation;
    }

    bool check_deadline(std::uint64_t now_ms) noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        std::vector<OperationId> due;
        if (lifecycle_.has_value() && !is_terminal(lifecycle_->state()) &&
            now_ms >= lifecycle_->deadline_ms()) {
            due.push_back(lifecycle_->id());
        }
        for (const auto &lifecycle : in_flight_) {
            if (!is_terminal(lifecycle.state()) && now_ms >= lifecycle.deadline_ms()) {
                due.push_back(lifecycle.id());
            }
        }
        for (const auto &lifecycle : queue_) {
            if (!is_terminal(lifecycle.state()) && now_ms >= lifecycle.deadline_ms()) {
                due.push_back(lifecycle.id());
            }
        }
        bool changed = false;
        for (const auto id : due) {
            changed = timeout_active(id, now_ms) || timeout_queued(id, now_ms) || changed;
        }
        return changed;
    }

    void publish_progress(OperationId id, std::uint64_t sequence) noexcept {
        publish_progress_with_payload(id, sequence, std::nullopt);
    }

    void publish_progress_with_payload(OperationId id, std::uint64_t sequence,
                                       std::optional<std::vector<std::uint8_t>> payload) noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        auto *lifecycle = current(id);
        if (lifecycle == nullptr || is_terminal(lifecycle->state())) {
            return;
        }
        events_.push_back(OperationRuntimeEvent{
            .id = id,
            .kind = OperationRuntimeEventKind::Progress,
            .state = std::nullopt,
            .sequence = sequence,
            .payload = std::move(payload),
            .message = std::nullopt,
            .retention_ms = std::nullopt,
        });
    }

    std::optional<OperationCancelToken> cancel_token() const noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        if (!lifecycle_.has_value()) {
            return std::nullopt;
        }
        return lifecycle_->cancel_token();
    }

    std::optional<OperationCancelToken> cancel_token_for(OperationId id) const noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        if (lifecycle_.has_value() && lifecycle_->id() == id) {
            return lifecycle_->cancel_token();
        }
        const auto found = std::find_if(
            in_flight_.begin(), in_flight_.end(),
            [id](const OperationLifecycle &lifecycle) { return lifecycle.id() == id; });
        if (found == in_flight_.end()) {
            return std::nullopt;
        }
        return found->cancel_token();
    }

    bool ready_to_run(OperationId id) const noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        if (lifecycle_.has_value() && lifecycle_->id() == id &&
            lifecycle_->state() == OperationState::Starting) {
            return true;
        }
        return std::any_of(
            in_flight_.begin(), in_flight_.end(), [id](const OperationLifecycle &lifecycle) {
                return lifecycle.id() == id && lifecycle.state() == OperationState::Starting;
            });
    }

    OperationStatusSnapshot snapshot() const noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        if (!lifecycle_.has_value()) {
            return OperationStatusSnapshot{
                .id = OperationId{.operation_key = operation_key_, .client_id = 0, .sequence = 0},
                .owner = OperationOwner{},
                .state = OperationState::Idle,
                .cancel_requested = false,
                .deadline_ms = 0,
                .health = health_.snapshot(),
            };
        }
        auto snapshot = lifecycle_->snapshot();
        snapshot.health = health_.snapshot();
        return snapshot;
    }

    OperationStatusResult status(OperationId id) const noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        if (lifecycle_.has_value() && lifecycle_->id() == id) {
            return OperationStatusResult::ok(snapshot_with_health(*lifecycle_));
        }
        const auto active = std::find_if(
            in_flight_.begin(), in_flight_.end(),
            [id](const OperationLifecycle &lifecycle) { return lifecycle.id() == id; });
        if (active != in_flight_.end()) {
            return OperationStatusResult::ok(snapshot_with_health(*active));
        }
        const auto queued = std::find_if(
            queue_.begin(), queue_.end(),
            [id](const OperationLifecycle &lifecycle) { return lifecycle.id() == id; });
        if (queued != queue_.end()) {
            return OperationStatusResult::ok(snapshot_with_health(*queued));
        }
        const auto retained = std::find_if(
            retained_results_.begin(), retained_results_.end(),
            [id](const RetainedOperationStatus &value) { return value.snapshot.id == id; });
        if (retained != retained_results_.end()) {
            return OperationStatusResult::ok(retained->snapshot);
        }
        return OperationStatusResult::err(OperationControlError::StaleInvocation);
    }

    std::size_t queued_len() const noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        return queue_.size();
    }

    void evict_retained_results(std::uint64_t now_ms) noexcept {
        std::lock_guard<std::mutex> lock(mutex_);
        retained_results_.erase(std::remove_if(retained_results_.begin(), retained_results_.end(),
                                               [now_ms](const RetainedOperationStatus &retained) {
                                                   return retained.expires_at_ms.has_value() &&
                                                          now_ms > *retained.expires_at_ms;
                                               }),
                                retained_results_.end());
    }

    std::vector<OperationRuntimeEvent> drain_events() {
        std::lock_guard<std::mutex> lock(mutex_);
        auto events = std::move(events_);
        events_.clear();
        return events;
    }

   private:
    static OperationControlError map_error(OperationError error) noexcept {
        switch (error) {
            case OperationError::Ok:
                return OperationControlError::Ok;
            case OperationError::InvalidTransition:
                return OperationControlError::InvalidTransition;
            case OperationError::InvalidPolicy:
                return OperationControlError::InvalidPolicy;
        }
        return OperationControlError::InvalidTransition;
    }

    OperationLifecycle *current(OperationId id) noexcept {
        if (lifecycle_.has_value() && lifecycle_->id() == id) {
            return &*lifecycle_;
        }
        auto found = std::find_if(
            in_flight_.begin(), in_flight_.end(),
            [id](const OperationLifecycle &lifecycle) { return lifecycle.id() == id; });
        if (found == in_flight_.end()) {
            return nullptr;
        }
        return &*found;
    }

    struct RetainedOperationStatus {
        OperationStatusSnapshot snapshot{};
        std::optional<std::uint64_t> expires_at_ms;
    };

    struct ActiveEntry {
        OperationLifecycle lifecycle;
        bool was_primary = false;
    };

    struct QueuedEntry {
        OperationLifecycle lifecycle;
        std::size_t index = 0;
    };

    static bool same_id(OperationId left, OperationId right) noexcept { return left == right; }

    std::size_t active_count() const noexcept {
        return (lifecycle_.has_value() && !is_terminal(lifecycle_->state()) ? 1U : 0U) +
               static_cast<std::size_t>(std::count_if(in_flight_.begin(), in_flight_.end(),
                                                      [](const OperationLifecycle &lifecycle) {
                                                          return !is_terminal(lifecycle.state());
                                                      }));
    }

    std::optional<ActiveEntry> remove_active(OperationId id) {
        if (lifecycle_.has_value() && lifecycle_->id() == id) {
            auto lifecycle = std::move(*lifecycle_);
            lifecycle_.reset();
            return ActiveEntry{.lifecycle = std::move(lifecycle), .was_primary = true};
        }
        const auto found = std::find_if(
            in_flight_.begin(), in_flight_.end(),
            [id](const OperationLifecycle &lifecycle) { return lifecycle.id() == id; });
        if (found == in_flight_.end()) {
            return std::nullopt;
        }
        auto lifecycle = std::move(*found);
        in_flight_.erase(found);
        return ActiveEntry{.lifecycle = std::move(lifecycle), .was_primary = false};
    }

    std::optional<QueuedEntry> remove_queued(OperationId id) {
        const auto found = std::find_if(
            queue_.begin(), queue_.end(),
            [id](const OperationLifecycle &lifecycle) { return lifecycle.id() == id; });
        if (found == queue_.end()) {
            return std::nullopt;
        }
        const auto index = static_cast<std::size_t>(std::distance(queue_.begin(), found));
        auto lifecycle = std::move(*found);
        queue_.erase(found);
        return QueuedEntry{.lifecycle = std::move(lifecycle), .index = index};
    }

    void restore_queued(OperationLifecycle lifecycle, std::size_t index) {
        const auto bounded_index = std::min(index, queue_.size());
        queue_.insert(queue_.begin() + static_cast<std::ptrdiff_t>(bounded_index),
                      std::move(lifecycle));
    }

    void restore_active(OperationLifecycle lifecycle, bool was_primary) {
        if (was_primary) {
            lifecycle_ = std::move(lifecycle);
        } else {
            in_flight_.push_front(std::move(lifecycle));
        }
    }

    OperationStatusSnapshot snapshot_with_health(
        const OperationLifecycle &lifecycle) const noexcept {
        auto snapshot = lifecycle.snapshot();
        snapshot.health = health_.snapshot();
        return snapshot;
    }

    void push_result_event(OperationId id, OperationState terminal_state,
                           std::optional<std::vector<std::uint8_t>> payload) {
        events_.push_back(OperationRuntimeEvent{
            .id = id,
            .kind = terminal_state == OperationState::Failed ? OperationRuntimeEventKind::Error
                                                             : OperationRuntimeEventKind::Result,
            .state = terminal_state,
            .sequence = std::nullopt,
            .payload = std::move(payload),
            .message = std::nullopt,
            .retention_ms = result_retention_ms(),
        });
    }

    void push_state_event(OperationId id, OperationState state) {
        events_.push_back(OperationRuntimeEvent{
            .id = id,
            .kind = OperationRuntimeEventKind::StateChanged,
            .state = state,
            .sequence = std::nullopt,
            .payload = std::nullopt,
            .message = std::nullopt,
            .retention_ms = std::nullopt,
        });
    }

    void promote_next_active() {
        if (!lifecycle_.has_value() && !in_flight_.empty()) {
            lifecycle_ = std::move(in_flight_.front());
            in_flight_.pop_front();
        }
    }

    void promote_queued_until_capacity() {
        promote_next_active();
        while (active_count() < policy_.max_in_flight && !queue_.empty()) {
            auto next = std::move(queue_.front());
            queue_.pop_front();
            if (!lifecycle_.has_value()) {
                lifecycle_ = std::move(next);
            } else {
                in_flight_.push_back(std::move(next));
            }
        }
        handler_active_ = false;
    }

    OperationControlError preempt_active_invocations() {
        std::vector<OperationLifecycle> active;
        if (lifecycle_.has_value()) {
            active.push_back(std::move(*lifecycle_));
            lifecycle_.reset();
        }
        while (!in_flight_.empty()) {
            active.push_back(std::move(in_flight_.front()));
            in_flight_.pop_front();
        }
        for (auto &lifecycle : active) {
            if (is_terminal(lifecycle.state())) {
                continue;
            }
            const auto id = lifecycle.id();
            const auto error = map_error(lifecycle.request_cancel());
            if (error != OperationControlError::Ok) {
                restore_active(std::move(lifecycle), !lifecycle_.has_value());
                return error;
            }
            health_.record_preempted();
            retained_results_.push_back(RetainedOperationStatus{
                .snapshot = snapshot_with_health(lifecycle),
                .expires_at_ms = std::nullopt,
            });
            push_state_event(id, OperationState::CancelRequested);
        }
        handler_active_ = false;
        return OperationControlError::Ok;
    }

    bool timeout_active(OperationId id, std::uint64_t now_ms) {
        auto active = remove_active(id);
        if (!active.has_value()) {
            return false;
        }
        auto lifecycle = std::move(active->lifecycle);
        const auto was_primary = active->was_primary;
        if (is_terminal(lifecycle.state()) || now_ms < lifecycle.deadline_ms()) {
            restore_active(std::move(lifecycle), was_primary);
            return false;
        }
        lifecycle.cancel_token().request_cancel();
        if (lifecycle.transition(OperationState::TimedOut) != OperationError::Ok) {
            restore_active(std::move(lifecycle), was_primary);
            return false;
        }
        health_.record_state(OperationState::TimedOut);
        handler_active_ = false;
        const auto snapshot = snapshot_with_health(lifecycle);
        push_result_event(id, OperationState::TimedOut, std::nullopt);
        push_state_event(id, OperationState::TimedOut);
        retain_terminal_snapshot(snapshot, now_ms);
        if (was_primary) {
            promote_next_active();
        }
        promote_queued_until_capacity();
        return true;
    }

    bool timeout_queued(OperationId id, std::uint64_t now_ms) {
        auto queued = remove_queued(id);
        if (!queued.has_value()) {
            return false;
        }
        auto lifecycle = std::move(queued->lifecycle);
        const auto index = queued->index;
        if (is_terminal(lifecycle.state()) || now_ms < lifecycle.deadline_ms()) {
            restore_queued(std::move(lifecycle), index);
            return false;
        }
        lifecycle.cancel_token().request_cancel();
        if (lifecycle.transition(OperationState::TimedOut) != OperationError::Ok) {
            restore_queued(std::move(lifecycle), index);
            return false;
        }
        health_.record_state(OperationState::TimedOut);
        const auto snapshot = snapshot_with_health(lifecycle);
        push_result_event(id, OperationState::TimedOut, std::nullopt);
        push_state_event(id, OperationState::TimedOut);
        retain_terminal_snapshot(snapshot, now_ms);
        return true;
    }

    void retain_terminal_snapshot(OperationStatusSnapshot snapshot, std::uint64_t completed_at_ms) {
        const auto expires_at_ms = retention_deadline(completed_at_ms);
        if (!expires_at_ms.has_value()) {
            erase_retained(snapshot.id);
            return;
        }
        retained_results_.push_back(RetainedOperationStatus{
            .snapshot = snapshot,
            .expires_at_ms = expires_at_ms,
        });
    }

    std::optional<std::uint64_t> retention_deadline(std::uint64_t completed_at_ms) const noexcept {
        if (policy_.result_retention.count() <= 0) {
            return std::nullopt;
        }
        const auto retention_ms = static_cast<std::uint64_t>(policy_.result_retention.count());
        if (retention_ms > std::numeric_limits<std::uint64_t>::max() - completed_at_ms) {
            return std::numeric_limits<std::uint64_t>::max();
        }
        return completed_at_ms + retention_ms;
    }

    std::uint64_t result_retention_ms() const noexcept {
        if (policy_.result_retention.count() <= 0) {
            return 0U;
        }
        return static_cast<std::uint64_t>(policy_.result_retention.count());
    }

    void erase_retained(OperationId id) {
        retained_results_.erase(std::remove_if(retained_results_.begin(), retained_results_.end(),
                                               [id](const RetainedOperationStatus &retained) {
                                                   return same_id(retained.snapshot.id, id);
                                               }),
                                retained_results_.end());
    }

    std::uint64_t operation_key_ = 0;
    OperationPolicy policy_{};
    std::uint64_t next_sequence_ = 0;
    std::optional<OperationLifecycle> lifecycle_;
    std::deque<OperationLifecycle> in_flight_;
    std::deque<OperationLifecycle> queue_;
    bool handler_active_ = false;
    OperationHealthCounters health_;
    std::vector<RetainedOperationStatus> retained_results_;
    std::vector<OperationRuntimeEvent> events_;
    mutable std::mutex mutex_;
};

}  // namespace flowrt
