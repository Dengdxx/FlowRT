#pragma once

#include <algorithm>
#include <cstdint>
#include <deque>
#include <flowrt/core.hpp>
#include <fstream>
#include <optional>
#include <set>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

/**
 * @file replay.hpp
 * @brief 运行时原生确定性回放 (v0.17.1)：C++ 侧镜像 Rust runtime 的 ReplayDriver。
 *
 * ReplayDriver 让 C++ runtime 自己拥有回放事件时间线并按确定性网格步进，取代 v0.16.0 经
 * introspection socket 由外部 wall-clock 节奏逐事件注入的回放路径。给定调度器算出的下一个
 * periodic deadline，driver 在「下一个事件时间」与「下一个 periodic 网格点」之间取较早者推进
 * 逻辑时钟，从而在两个事件之间逐周期触发 periodic task，回放结果只取决于事件序列、与回放
 * 物理快慢无关。
 *
 * 语义与字节级行为与 Rust `flowrt::ReplayDriver` 对齐（见 runtime/rust/src/time_driver.rs），
 * 由两侧 conformance 测试用同一事件序列断言一致。external_stepped 的 SteppedDriver/StepController
 * 留待 v0.18.0 与 Rust 同步点亮。
 *
 * 时间线来源由生成 shell 在更外层装配：C++ 没有 MCAP 解析能力，`flowrt run --replay` 在启动
 * C++ 生成 shell 前把 MCAP 规范化为 JSONL 时间线（见 flowrt-record write_replay_timeline_jsonl），
 * 本头文件只解析该 JSONL 并对抽象时间线步进。
 */

namespace flowrt {

namespace replay_detail {

/// 在 JSONL 行内按 key 提取 unsigned 整数值。key 以独立 `"key":` 形态匹配，避免误命中后缀同名键。
inline std::optional<std::uint64_t> parse_u64_field(std::string_view line, std::string_view key) {
    const std::string needle = "\"" + std::string{key} + "\":";
    const auto pos = line.find(needle);
    if (pos == std::string_view::npos) {
        return std::nullopt;
    }
    std::size_t index = pos + needle.size();
    while (index < line.size() && (line[index] == ' ' || line[index] == '\t')) {
        ++index;
    }
    if (index >= line.size() || line[index] < '0' || line[index] > '9') {
        return std::nullopt;
    }
    std::uint64_t value = 0;
    while (index < line.size() && line[index] >= '0' && line[index] <= '9') {
        value = value * 10U + static_cast<std::uint64_t>(line[index] - '0');
        ++index;
    }
    return value;
}

/// 在 JSONL 行内按 key 提取字符串值。处理标准短转义；不支持 `\\u`（回放 target 是 canonical
/// 名称，正常不含转义），与 introspection request parser 的容错口径一致。
inline std::optional<std::string> parse_string_field(std::string_view line, std::string_view key) {
    const std::string needle = "\"" + std::string{key} + "\":";
    const auto pos = line.find(needle);
    if (pos == std::string_view::npos) {
        return std::nullopt;
    }
    std::size_t index = pos + needle.size();
    while (index < line.size() && (line[index] == ' ' || line[index] == '\t')) {
        ++index;
    }
    if (index >= line.size() || line[index] != '"') {
        return std::nullopt;
    }
    ++index;
    std::string value;
    while (index < line.size()) {
        const char byte = line[index++];
        if (byte == '"') {
            return value;
        }
        if (byte != '\\') {
            value.push_back(byte);
            continue;
        }
        if (index >= line.size()) {
            return std::nullopt;
        }
        const char escape = line[index++];
        switch (escape) {
            case '"':
            case '\\':
            case '/':
                value.push_back(escape);
                break;
            case 'b':
                value.push_back('\b');
                break;
            case 'f':
                value.push_back('\f');
                break;
            case 'n':
                value.push_back('\n');
                break;
            case 'r':
                value.push_back('\r');
                break;
            case 't':
                value.push_back('\t');
                break;
            default:
                return std::nullopt;
        }
    }
    return std::nullopt;
}

/// 在 JSONL 行内按 key 提取 0..=255 的整数数组（payload 字节）。空数组合法。
inline std::optional<std::vector<std::uint8_t>> parse_u8_array_field(std::string_view line,
                                                                     std::string_view key) {
    const std::string needle = "\"" + std::string{key} + "\":";
    const auto pos = line.find(needle);
    if (pos == std::string_view::npos) {
        return std::nullopt;
    }
    std::size_t index = pos + needle.size();
    while (index < line.size() && (line[index] == ' ' || line[index] == '\t')) {
        ++index;
    }
    if (index >= line.size() || line[index] != '[') {
        return std::nullopt;
    }
    ++index;
    std::vector<std::uint8_t> values;
    while (index < line.size()) {
        while (index < line.size() &&
               (line[index] == ' ' || line[index] == '\t' || line[index] == ',')) {
            ++index;
        }
        if (index < line.size() && line[index] == ']') {
            return values;
        }
        if (index >= line.size() || line[index] < '0' || line[index] > '9') {
            return std::nullopt;
        }
        std::uint32_t byte = 0;
        while (index < line.size() && line[index] >= '0' && line[index] <= '9') {
            byte = byte * 10U + static_cast<std::uint32_t>(line[index] - '0');
            if (byte > 255U) {
                return std::nullopt;
            }
            ++index;
        }
        values.push_back(static_cast<std::uint8_t>(byte));
    }
    return std::nullopt;
}

}  // namespace replay_detail

/**
 * @brief 回放时间线的一条记录：在某逻辑毫秒把一段 wire payload 注入某目标 channel/boundary。
 *
 * 与 flowrt-record `ReplayTimelineEntry` 对应，是 JSONL 回放源每行的解析结果。
 */
struct ReplayTimelineEntry {
    std::uint64_t time_ms = 0;
    std::string target;
    std::vector<std::uint8_t> payload;
    /// sensor sample-time（毫秒），声明 timestamp 源的消息录制时填充；为空回退 receive-time。
    std::optional<std::uint64_t> sample_time_ms;
};

/**
 * @brief 单条回放事件：在某逻辑毫秒把一段 wire payload 注入某个 boundary input 或 channel。
 *
 * 与 Rust `flowrt::ReplayEvent` 对应。`sample_time_ms` 由声明 timestamp 源的消息在录制时填充
 * （v0.18.0 起）；为空时 event-time 回放回退到 `time_ms` receive-time。
 */
struct ReplayEvent {
    std::uint64_t time_ms = 0;
    std::string target;
    std::vector<std::uint8_t> payload;
    std::optional<std::uint64_t> sample_time_ms;

    /// event-time 回放使用的有效逻辑时间：有 sample-time 用之，否则回退 receive-time。
    [[nodiscard]] std::uint64_t effective_time_ms() const noexcept {
        return sample_time_ms.value_or(time_ms);
    }
};

/// 一次步进的分类结果，与 Rust `flowrt::Step` 对应。
enum class Step : std::uint8_t {
    /// 推进到一个 periodic 网格点，本步无新数据。
    Timer,
    /// 推进到一个事件时间，已暂存该时刻全部待注入事件（用 take_pending_events 取走后注入）。
    Data,
    /// 时间线已耗尽或收到 shutdown，回放结束。
    Shutdown,
};

/**
 * @brief 运行时原生回放驱动：拥有事件时间线并确定性步进逻辑时钟。
 *
 * 与 Rust `flowrt::ReplayDriver` 行为对齐：逻辑时钟单调不退；同一时刻的多个事件一次性暂存。
 */
class ReplayDriver {
   public:
    ReplayDriver() = default;

    /// 从按时间升序的事件时间线构造 driver。调用方负责保证时间线已排序。
    explicit ReplayDriver(std::vector<ReplayEvent> timeline)
        : timeline_(std::make_move_iterator(timeline.begin()),
                    std::make_move_iterator(timeline.end())) {}

    /// 当前逻辑毫秒时间。
    [[nodiscard]] std::uint64_t now_ms() const noexcept { return now_ms_; }

    /**
     * @brief 在「下一个事件时间」与传入的「下一个 periodic deadline」之间取较早者推进逻辑时钟。
     *
     * 命中事件时间返回 Step::Data 并暂存该时刻全部事件；命中 periodic 网格点返回 Step::Timer；
     * 时间线耗尽返回 Step::Shutdown。
     */
    Step step(std::optional<std::uint64_t> next_periodic_deadline_ms) {
        if (timeline_.empty()) {
            return Step::Shutdown;
        }
        const std::uint64_t next_event_ms = timeline_.front().effective_time_ms();
        const std::uint64_t target = next_periodic_deadline_ms.has_value()
                                         ? std::min(next_event_ms, *next_periodic_deadline_ms)
                                         : next_event_ms;
        now_ms_ = std::max(now_ms_, target);
        if (target != next_event_ms) {
            return Step::Timer;
        }
        while (!timeline_.empty() && timeline_.front().effective_time_ms() == target) {
            pending_.push_back(std::move(timeline_.front()));
            timeline_.pop_front();
        }
        return Step::Data;
    }

    /// 推进到下一个调度步；shutdown 已请求时直接返回 Step::Shutdown。
    Step next_step(std::optional<std::uint64_t> next_periodic_deadline_ms,
                   const ShutdownToken &shutdown) {
        if (shutdown.is_requested()) {
            return Step::Shutdown;
        }
        return step(next_periodic_deadline_ms);
    }

    /// 取走上一次 Step::Data 暂存的待注入事件。
    [[nodiscard]] std::vector<ReplayEvent> take_pending_events() {
        return std::exchange(pending_, std::vector<ReplayEvent>{});
    }

   private:
    std::deque<ReplayEvent> timeline_;
    std::uint64_t now_ms_ = 0;
    std::vector<ReplayEvent> pending_;
};

/**
 * @brief 把回放时间线条目过滤并映射为按时间升序的 boundary 激励事件。
 *
 * 只保留 target 属于 boundary_inputs 的条目；其余（内部 channel sample）被忽略——确定性回放
 * 只重放外部边界激励，由 runtime 重新推导下游。输入需已按时间升序。
 */
inline std::vector<ReplayEvent> boundary_replay_events(
    const std::vector<ReplayTimelineEntry> &entries, const std::set<std::string> &boundary_inputs) {
    std::vector<ReplayEvent> events;
    for (const auto &entry : entries) {
        if (boundary_inputs.find(entry.target) == boundary_inputs.end()) {
            continue;
        }
        events.push_back(ReplayEvent{
            .time_ms = entry.time_ms,
            .target = entry.target,
            .payload = entry.payload,
            .sample_time_ms = entry.sample_time_ms,
        });
    }
    return events;
}

/**
 * @brief 从 JSONL 回放源构造只含 boundary 激励的 ReplayDriver。
 *
 * 读取失败（打开失败、行解析失败）返回错误说明字符串，由生成 shell 决定 fail-fast，不静默
 * 吞掉错误后用空时间线伪装成功回放。成功返回构造好的 ReplayDriver。
 */
inline std::variant<ReplayDriver, std::string> replay_driver_from_timeline_file(
    std::string_view path, const std::set<std::string> &boundary_inputs) {
    std::ifstream file{std::string{path}};
    if (!file.is_open()) {
        return "open FlowRT replay source `" + std::string{path} + "`";
    }
    std::vector<ReplayTimelineEntry> entries;
    std::string line;
    std::size_t line_number = 0;
    while (std::getline(file, line)) {
        ++line_number;
        if (line.find_first_not_of(" \t\r\n") == std::string::npos) {
            continue;
        }
        auto time_ms = replay_detail::parse_u64_field(line, "time_ms");
        auto target = replay_detail::parse_string_field(line, "target");
        auto payload = replay_detail::parse_u8_array_field(line, "payload");
        if (!time_ms.has_value() || !target.has_value() || !payload.has_value()) {
            return "parse FlowRT replay timeline `" + std::string{path} + "` line " +
                   std::to_string(line_number);
        }
        entries.push_back(ReplayTimelineEntry{
            .time_ms = *time_ms,
            .target = std::move(*target),
            .payload = std::move(*payload),
            .sample_time_ms = replay_detail::parse_u64_field(line, "sample_time_ms"),
        });
    }
    return ReplayDriver{boundary_replay_events(entries, boundary_inputs)};
}

}  // namespace flowrt
