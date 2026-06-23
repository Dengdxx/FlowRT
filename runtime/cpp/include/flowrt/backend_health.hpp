#pragma once

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <utility>

namespace flowrt {

/**
 * @brief backend 或 endpoint 当前健康状态。
 *
 * 枚举值保持紧凑、稳定，方便未来 C ABI 直接映射为整数。
 */
enum class BackendHealthState : std::uint8_t {
    Ready = 0,          ///< 本地 endpoint 已打开，最近一次操作未发现错误。
    Degraded = 1,       ///< 本地 endpoint 可见错误，但仍允许后续恢复。
    Reconnecting = 2,   ///< runtime 正在按重连策略等待或尝试恢复。
    Failed = 3,         ///< 重连预算耗尽或错误不可恢复。
    Unsupported = 4,    ///< backend SDK 未编译进当前构建，配置错误不可恢复。
};

/**
 * @brief backend transport 错误的 FlowRT 语义分类。
 *
 * 底层 SDK 原始错误只用于诊断文本；route counter 映射以该枚举为主键。
 */
enum class TransportErrorKind : std::uint8_t {
    QueueFull = 0,          ///< backend 明确报告发送队列或 slot 已满。
    Backpressure = 1,       ///< backend 明确报告当前写入遇到背压。
    Timeout = 2,            ///< backend 操作超时。
    Disconnected = 3,       ///< transport session 或连接已断开。
    Unavailable = 4,        ///< endpoint、session 或资源当前不可用。
    PermissionDenied = 5,   ///< 权限不足。
    Unsupported = 6,        ///< 当前构建或 backend 不支持该操作。
    Codec = 7,              ///< payload 编解码失败。
    SchemaMismatch = 8,     ///< schema、type 或 frame layout 不匹配。
    ResourceExhausted = 9,  ///< backend 资源耗尽，但未能确认是队列满。
    Internal = 10,          ///< backend 内部错误。
    Unknown = 11,           ///< SDK 未提供可稳定分类的错误。
};

inline std::string_view transport_error_kind_str(TransportErrorKind kind) noexcept {
    switch (kind) {
        case TransportErrorKind::QueueFull:
            return "queue_full";
        case TransportErrorKind::Backpressure:
            return "backpressure";
        case TransportErrorKind::Timeout:
            return "timeout";
        case TransportErrorKind::Disconnected:
            return "disconnected";
        case TransportErrorKind::Unavailable:
            return "unavailable";
        case TransportErrorKind::PermissionDenied:
            return "permission_denied";
        case TransportErrorKind::Unsupported:
            return "unsupported";
        case TransportErrorKind::Codec:
            return "codec";
        case TransportErrorKind::SchemaMismatch:
            return "schema_mismatch";
        case TransportErrorKind::ResourceExhausted:
            return "resource_exhausted";
        case TransportErrorKind::Internal:
            return "internal";
        case TransportErrorKind::Unknown:
            return "unknown";
    }
    return "unknown";
}

inline bool transport_error_kind_recoverable(TransportErrorKind kind) noexcept {
    switch (kind) {
        case TransportErrorKind::PermissionDenied:
        case TransportErrorKind::Unsupported:
        case TransportErrorKind::Codec:
        case TransportErrorKind::SchemaMismatch:
        case TransportErrorKind::Internal:
            return false;
        case TransportErrorKind::QueueFull:
        case TransportErrorKind::Backpressure:
        case TransportErrorKind::Timeout:
        case TransportErrorKind::Disconnected:
        case TransportErrorKind::Unavailable:
        case TransportErrorKind::ResourceExhausted:
        case TransportErrorKind::Unknown:
            return true;
    }
    return true;
}

/**
 * @brief backend endpoint 的重连策略。
 *
 * 使用毫秒和次数，而不是语言 runtime 特有类型，便于后续映射到 C ABI、Python binding 和其他
 * 语言 runtime。
 */
struct ReconnectPolicy {
    std::uint64_t initial_delay_ms = 100;
    std::uint64_t max_delay_ms = 5000;
    std::optional<std::uint32_t> max_attempts = std::nullopt;

    /**
     * @brief 指定 attempt 的指数退避延迟，毫秒。
     */
    std::uint64_t delay_for_attempt(std::uint32_t attempt) const noexcept {
        std::uint64_t multiplier = 1U;
        for (std::uint32_t index = 0; index < attempt && multiplier <= UINT64_MAX / 2U; ++index) {
            multiplier *= 2U;
        }
        if (initial_delay_ms != 0U && multiplier > UINT64_MAX / initial_delay_ms) {
            return max_delay_ms;
        }
        return std::min(initial_delay_ms * multiplier, max_delay_ms);
    }

    /**
     * @brief 判断当前 attempt 是否仍允许重试。
     */
    bool can_retry(std::uint32_t attempt) const noexcept {
        return !max_attempts || attempt < *max_attempts;
    }
};

/**
 * @brief backend 或 endpoint 的健康快照。
 */
struct BackendHealthSnapshot {
    BackendHealthState state = BackendHealthState::Ready;
    std::optional<std::string> last_error = std::nullopt;
    std::uint32_t attempt = 0;
    std::optional<std::uint64_t> next_retry_unix_ms = std::nullopt;
    bool recoverable = false;

    /**
     * @brief 构造 ready 快照。
     */
    static BackendHealthSnapshot ready() { return BackendHealthSnapshot{}; }

    /**
     * @brief 构造 test-only backend_drop 故障注入快照。
     */
    static BackendHealthSnapshot fault_injection_backend_drop() {
        return BackendHealthSnapshot{
            .state = BackendHealthState::Degraded,
            .last_error = std::string{"fault_injection_backend_drop"},
            .attempt = 0,
            .next_retry_unix_ms = std::nullopt,
            .recoverable = true,
        };
    }

    friend bool operator==(const BackendHealthSnapshot &, const BackendHealthSnapshot &) = default;
};

inline std::uint64_t unix_now_ms() noexcept {
    const auto now = std::chrono::system_clock::now().time_since_epoch();
    return static_cast<std::uint64_t>(
        std::chrono::duration_cast<std::chrono::milliseconds>(now).count());
}

/**
 * @brief 通用 backend health 状态机。
 *
 * 该 tracker 不直接重连任何 transport，只统一记录状态转换。真实 zenoh/iox2 恢复逻辑会在
 * endpoint 层调用它。
 */
class BackendHealthTracker {
   public:
    explicit BackendHealthTracker(ReconnectPolicy policy) : policy_(policy) {}

    ReconnectPolicy policy() const noexcept { return policy_; }

    BackendHealthSnapshot snapshot() const { return snapshot_; }

    void mark_ready() { snapshot_ = BackendHealthSnapshot::ready(); }

    void mark_degraded(std::string error) {
        snapshot_ = BackendHealthSnapshot{
            .state = BackendHealthState::Degraded,
            .last_error = std::move(error),
            .attempt = snapshot_.attempt,
            .next_retry_unix_ms = std::nullopt,
            .recoverable = true,
        };
    }

    void mark_reconnecting(std::uint32_t attempt, std::uint64_t next_retry_unix_ms) {
        snapshot_ = BackendHealthSnapshot{
            .state = BackendHealthState::Reconnecting,
            .last_error = snapshot_.last_error,
            .attempt = attempt,
            .next_retry_unix_ms = next_retry_unix_ms,
            .recoverable = policy_.can_retry(attempt),
        };
    }

    void mark_failed(std::string error, std::uint32_t attempt) {
        snapshot_ = BackendHealthSnapshot{
            .state = BackendHealthState::Failed,
            .last_error = std::move(error),
            .attempt = attempt,
            .next_retry_unix_ms = std::nullopt,
            .recoverable = false,
        };
    }

   private:
    ReconnectPolicy policy_;
    BackendHealthSnapshot snapshot_;
};

}  // namespace flowrt
