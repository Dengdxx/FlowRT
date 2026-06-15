#pragma once

#include <atomic>
#include <cstdint>
#include <flowrt/introspection/model.hpp>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <utility>
#include <vector>

namespace flowrt {

namespace detail {

struct IntrospectionProbeLatest {
    std::optional<std::vector<std::uint8_t>> payload;
    std::optional<std::uint64_t> published_at_ms;
    std::optional<std::size_t> max_payload_len;
};

struct IntrospectionProbeInner {
    std::atomic_uint64_t observer_count{0};
    std::atomic_uint64_t dropped_samples{0};
    std::atomic_uint64_t published_count{0};
    mutable std::mutex mutex;
    IntrospectionProbeLatest latest;
};

}  // namespace detail

class IntrospectionObserverGuard;

/**
 * @brief 单个 channel 的按需 echo 数据面 probe。
 */
class IntrospectionChannelProbe {
   public:
    IntrospectionChannelProbe() : IntrospectionChannelProbe(std::nullopt) {}

    explicit IntrospectionChannelProbe(std::optional<std::size_t> max_payload_len)
        : inner_(std::make_shared<detail::IntrospectionProbeInner>()) {
        inner_->latest.max_payload_len = max_payload_len;
        if (max_payload_len) {
            inner_->latest.payload = std::vector<std::uint8_t>{};
            inner_->latest.payload->reserve(*max_payload_len);
        }
    }

    bool enabled() const noexcept {
        return inner_->observer_count.load(std::memory_order_acquire) != 0U;
    }

    std::uint64_t active_count() const noexcept {
        return inner_->observer_count.load(std::memory_order_acquire);
    }

    std::uint64_t dropped_samples() const noexcept {
        return inner_->dropped_samples.load(std::memory_order_acquire);
    }

    void record_publish_event() const noexcept {
        std::uint64_t current = inner_->published_count.load(std::memory_order_acquire);
        while (current != UINT64_MAX) {
            if (inner_->published_count.compare_exchange_weak(
                    current, current + 1, std::memory_order_acq_rel, std::memory_order_acquire)) {
                break;
            }
        }
    }

    IntrospectionObserverGuard observe() const;

    IntrospectionProbeRecord try_record_bytes(const std::vector<std::uint8_t> &payload,
                                              std::optional<std::uint64_t> published_at_ms) const {
        return try_record_bytes(std::span<const std::uint8_t>{payload.data(), payload.size()},
                                published_at_ms);
    }

    IntrospectionProbeRecord try_record_bytes(std::span<const std::uint8_t> payload,
                                              std::optional<std::uint64_t> published_at_ms) const {
        if (!enabled()) {
            return IntrospectionProbeRecord{};
        }
        if (!inner_->mutex.try_lock()) {
            inner_->dropped_samples.fetch_add(1, std::memory_order_relaxed);
            return IntrospectionProbeRecord{.recorded = false, .dropped = true};
        }
        std::unique_lock<std::mutex> lock(inner_->mutex, std::adopt_lock);
        auto &latest = inner_->latest;
        if (latest.max_payload_len && payload.size() > *latest.max_payload_len) {
            inner_->dropped_samples.fetch_add(1, std::memory_order_relaxed);
            return IntrospectionProbeRecord{.recorded = false, .dropped = true};
        }
        auto &buffer = latest.payload ? *latest.payload : latest.payload.emplace();
        if (latest.max_payload_len && buffer.capacity() < *latest.max_payload_len) {
            buffer.reserve(*latest.max_payload_len);
        }
        if (latest.max_payload_len && buffer.capacity() < *latest.max_payload_len) {
            inner_->dropped_samples.fetch_add(1, std::memory_order_relaxed);
            return IntrospectionProbeRecord{.recorded = false, .dropped = true};
        }
        buffer.clear();
        buffer.insert(buffer.end(), payload.begin(), payload.end());
        latest.published_at_ms = published_at_ms;
        return IntrospectionProbeRecord{.recorded = true, .dropped = false};
    }

    void force_record_bytes(std::vector<std::uint8_t> payload,
                            std::optional<std::uint64_t> published_at_ms) const {
        record_publish_event();
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &latest = inner_->latest;
        latest.payload = std::move(payload);
        latest.published_at_ms = published_at_ms;
    }

    IntrospectionChannelSnapshot snapshot() const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return IntrospectionChannelSnapshot{
            .published_count = inner_->published_count.load(std::memory_order_acquire),
            .payload = inner_->latest.payload,
            .published_at_ms = inner_->latest.published_at_ms,
        };
    }

   private:
    friend class IntrospectionObserverGuard;

    std::shared_ptr<detail::IntrospectionProbeInner> inner_;
};

/**
 * @brief 连接作用域 observer guard，析构时自动关闭 probe。
 */
class IntrospectionObserverGuard {
   public:
    IntrospectionObserverGuard() = default;
    explicit IntrospectionObserverGuard(std::shared_ptr<detail::IntrospectionProbeInner> inner)
        : inner_(std::move(inner)) {
        if (inner_) {
            inner_->observer_count.fetch_add(1, std::memory_order_acq_rel);
        }
    }

    IntrospectionObserverGuard(const IntrospectionObserverGuard &) = delete;
    auto operator=(const IntrospectionObserverGuard &) -> IntrospectionObserverGuard & = delete;

    IntrospectionObserverGuard(IntrospectionObserverGuard &&other) noexcept
        : inner_(std::move(other.inner_)) {}

    auto operator=(IntrospectionObserverGuard &&other) noexcept -> IntrospectionObserverGuard & {
        if (this != std::addressof(other)) {
            release();
            inner_ = std::move(other.inner_);
        }
        return *this;
    }

    ~IntrospectionObserverGuard() { release(); }

   private:
    void release() noexcept {
        if (inner_) {
            std::uint64_t current = inner_->observer_count.load(std::memory_order_acquire);
            while (current != 0U) {
                if (inner_->observer_count.compare_exchange_weak(current, current - 1U,
                                                                 std::memory_order_acq_rel,
                                                                 std::memory_order_acquire)) {
                    break;
                }
            }
            inner_.reset();
        }
    }

    std::shared_ptr<detail::IntrospectionProbeInner> inner_;
};

inline IntrospectionObserverGuard IntrospectionChannelProbe::observe() const {
    return IntrospectionObserverGuard{inner_};
}

}  // namespace flowrt
