#pragma once

#include <cassert>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <limits>
#include <optional>
#include <utility>
#include <vector>

namespace flowrt {

/**
 * @brief 多传感器同步原语：把 N 路输入按 sample-time 对齐成同步集。
 *
 * v1 采用 latest-aligned approx-window 策略：当所有输入的最新样本落在一个
 * `tolerance` 窗口内时发射一组对齐样本，否则丢弃落后输入的陈旧 backlog 等待其
 * 追平。算法只依赖样本 sample-time，realtime 与 replay 行为一致，并与 Rust
 * `flowrt::synchronizer::Synchronizer` 位级对齐（同一事件序列产出相同同步集）。
 * 时间与 tolerance 使用同一整数单位（codegen 传入 ns）。late 样本一律丢弃。
 *
 * @tparam T 对齐投递给用户回调的 typed 样本。
 */
template <typename T>
class Synchronizer {
   public:
    /// 构造 `input_count` 路、每路容量 `capacity`、窗口宽度 `tolerance` 的同步器。
    Synchronizer(std::size_t input_count, std::size_t capacity, std::uint64_t tolerance)
        : tolerance_(tolerance),
          capacity_(capacity),
          buffers_(input_count),
          watermark_(input_count) {
        assert(input_count >= 1 && "synchronizer requires at least one input");
        assert(capacity >= 1 && "synchronizer buffer capacity must be positive");
    }

    /// 输入路数。
    std::size_t input_count() const { return buffers_.size(); }

    /// 第 `input` 路当前缓冲样本数（用于诊断与测试）。
    std::size_t buffered(std::size_t input) const { return buffers_[input].size(); }

    /**
     * @brief 接收第 `input` 路一个 sample-time 为 `time` 的样本。
     *
     * late 样本（时间不晚于该路上次发射窗口）按 DropLate 丢弃；buffer 满则丢最旧。
     */
    void push(std::size_t input, std::uint64_t time, T value) {
        if (watermark_[input].has_value() && time <= *watermark_[input]) {
            return;
        }
        auto &buffer = buffers_[input];
        if (buffer.size() == capacity_) {
            buffer.pop_front();
        }
        buffer.emplace_back(time, std::move(value));
    }

    /**
     * @brief 尝试发射一组对齐样本。
     * @return 每路一个样本，或 `std::nullopt`（暂无可发集）。
     */
    std::optional<std::vector<T>> poll() {
        for (;;) {
            for (const auto &buffer : buffers_) {
                if (buffer.empty()) {
                    return std::nullopt;
                }
            }
            std::uint64_t max_time = 0;
            std::uint64_t min_time = std::numeric_limits<std::uint64_t>::max();
            std::size_t laggard = 0;
            std::uint64_t laggard_time = std::numeric_limits<std::uint64_t>::max();
            for (std::size_t input = 0; input < buffers_.size(); ++input) {
                const std::uint64_t time = buffers_[input].back().first;
                if (time > max_time) {
                    max_time = time;
                }
                if (time < min_time) {
                    min_time = time;
                }
                if (time < laggard_time) {
                    laggard_time = time;
                    laggard = input;
                }
            }
            if (max_time - min_time <= tolerance_) {
                std::vector<T> set;
                set.reserve(buffers_.size());
                for (std::size_t input = 0; input < buffers_.size(); ++input) {
                    auto &buffer = buffers_[input];
                    auto entry = buffer.back();
                    watermark_[input] = entry.first;
                    buffer.clear();
                    set.push_back(std::move(entry.second));
                }
                return set;
            }
            buffers_[laggard].pop_front();
        }
    }

   private:
    std::uint64_t tolerance_;
    std::size_t capacity_;
    std::vector<std::deque<std::pair<std::uint64_t, T>>> buffers_;
    std::vector<std::optional<std::uint64_t>> watermark_;
};

}  // namespace flowrt
