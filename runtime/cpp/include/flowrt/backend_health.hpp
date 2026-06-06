#pragma once

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <optional>
#include <string>
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
