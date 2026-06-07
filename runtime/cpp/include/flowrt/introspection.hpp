#pragma once

#include <algorithm>
#include <atomic>
#include <cerrno>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <filesystem>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <system_error>
#include <thread>
#include <type_traits>
#include <unistd.h>
#include <utility>
#include <variant>
#include <vector>

namespace flowrt {

/// 当前 runtime introspection JSON-line 协议版本。
inline constexpr const char *INTROSPECTION_PROTOCOL_VERSION = "0.1";

/**
 * @brief CLI 连接 socket 后首先验证的进程身份。
 */
struct IntrospectionHandshake {
    std::string protocol_version;
    std::uint32_t pid = 0;
    std::uint64_t started_at_unix_ms = 0;
    std::string self_description_hash;
    std::string package;
    std::string process;
    std::string runtime;
};

/**
 * @brief 单个 channel 的运行态摘要。
 */
struct IntrospectionChannelStatus {
    std::string name;
    std::string message_type;
    std::uint64_t published_count = 0;
    std::optional<std::size_t> last_payload_len;
    std::uint64_t active_observers = 0;
    std::uint64_t dropped_samples = 0;
};

/**
 * @brief 单个 channel 的 latest raw ABI snapshot。
 */
struct IntrospectionChannelSnapshot {
    std::uint64_t published_count = 0;
    std::optional<std::vector<std::uint8_t>> payload;
    std::optional<std::uint64_t> published_at_ms;
};

/**
 * @brief 数据面 probe 的单次记录结果。
 */
struct IntrospectionProbeRecord {
    bool recorded = false;
    bool dropped = false;
};

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

/**
 * @brief 单个 service endpoint 的运行态健康状态。
 */
struct IntrospectionServiceStatus {
    std::string name;
    bool ready = true;
    std::uint64_t in_flight = 0;
    std::uint64_t queued = 0;
    std::uint64_t total_requests = 0;
    std::uint64_t timeout_count = 0;
    std::uint64_t busy_count = 0;
    std::uint64_t unavailable_count = 0;
    std::uint64_t late_drop_count = 0;
};

/**
 * @brief 单个 task 的调度健康快照。
 *
 * 由 generated shell 在 scheduler step 边界填充，反映 task 级调度质量。
 */
struct IntrospectionTaskHealth {
    std::string name;
    std::string lane;
    std::uint64_t deadline_missed = 0;
    std::uint64_t stale_input = 0;
    std::uint64_t backpressure = 0;
    std::uint64_t overflow = 0;
    std::uint64_t fairness_violations = 0;
    std::uint64_t run_count = 0;
    std::uint64_t success_count = 0;
    std::uint64_t consecutive_failures = 0;
    std::optional<std::uint64_t> last_run_ms;
    std::optional<std::uint64_t> last_success_ms;
};

/**
 * @brief 单个 lane 的调度健康快照。
 *
 * 反映 lane 级队列深度和公平性状态。
 */
struct IntrospectionLaneHealth {
    std::string name;
    std::uint64_t queue_depth = 0;
    std::uint64_t dispatched_count = 0;
    std::uint64_t fairness_violations = 0;
};

/**
 * @brief 运行态 status 快照。
 */
struct IntrospectionStatus {
    std::uint64_t tick_count = 0;
    std::vector<IntrospectionChannelStatus> channels;
    std::vector<IntrospectionServiceStatus> services;
    std::vector<IntrospectionTaskHealth> tasks;
    std::vector<IntrospectionLaneHealth> lanes;
};

/**
 * @brief generated shell 注册到 runtime 控制面的参数 schema。
 *
 * `current`、`min`、`max` 和 `choices` 使用合法 JSON 片段，避免 C++ runtime 依赖完整 JSON 库。
 */
struct IntrospectionParamSchema {
    std::string name;
    std::string ty;
    std::string update;
    std::string current;
    std::optional<std::string> min;
    std::optional<std::string> max;
    std::vector<std::string> choices;
};

/**
 * @brief runtime 参数状态快照。
 */
struct IntrospectionParamStatus {
    std::string name;
    std::string ty;
    std::string update;
    std::string current;
    std::optional<std::string> pending;
    std::optional<std::string> min;
    std::optional<std::string> max;
    std::vector<std::string> choices;
};

namespace detail {

inline std::uint64_t unix_time_ms() {
    const auto now = std::chrono::system_clock::now().time_since_epoch();
    const auto millis = std::chrono::duration_cast<std::chrono::milliseconds>(now).count();
    return millis < 0 ? 0U : static_cast<std::uint64_t>(millis);
}

inline std::string json_string(std::string_view value) {
    static constexpr char kHex[] = "0123456789abcdef";
    std::string output;
    output.reserve(value.size() + 2);
    output.push_back('"');
    for (const unsigned char byte : value) {
        switch (byte) {
            case '"':
                output.append("\\\"");
                break;
            case '\\':
                output.append("\\\\");
                break;
            case '\b':
                output.append("\\b");
                break;
            case '\f':
                output.append("\\f");
                break;
            case '\n':
                output.append("\\n");
                break;
            case '\r':
                output.append("\\r");
                break;
            case '\t':
                output.append("\\t");
                break;
            default:
                if (byte < 0x20U) {
                    output.append("\\u00");
                    output.push_back(kHex[(byte >> 4U) & 0x0FU]);
                    output.push_back(kHex[byte & 0x0FU]);
                } else {
                    output.push_back(static_cast<char>(byte));
                }
                break;
        }
    }
    output.push_back('"');
    return output;
}

inline std::string handshake_json(const IntrospectionHandshake &handshake) {
    std::string output;
    output.append("{\"protocol_version\":");
    output.append(json_string(handshake.protocol_version));
    output.append(",\"pid\":");
    output.append(std::to_string(handshake.pid));
    output.append(",\"started_at_unix_ms\":");
    output.append(std::to_string(handshake.started_at_unix_ms));
    output.append(",\"self_description_hash\":");
    output.append(json_string(handshake.self_description_hash));
    output.append(",\"package\":");
    output.append(json_string(handshake.package));
    output.append(",\"process\":");
    output.append(json_string(handshake.process));
    output.append(",\"runtime\":");
    output.append(json_string(handshake.runtime));
    output.push_back('}');
    return output;
}

inline std::string channel_status_json(const IntrospectionChannelStatus &channel) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(channel.name));
    output.append(",\"message_type\":");
    output.append(json_string(channel.message_type));
    output.append(",\"published_count\":");
    output.append(std::to_string(channel.published_count));
    output.append(",\"last_payload_len\":");
    output.append(channel.last_payload_len ? std::to_string(*channel.last_payload_len) : "null");
    output.append(",\"active_observers\":");
    output.append(std::to_string(channel.active_observers));
    output.append(",\"dropped_samples\":");
    output.append(std::to_string(channel.dropped_samples));
    output.push_back('}');
    return output;
}

inline std::string service_status_json(const IntrospectionServiceStatus &service) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(service.name));
    output.append(",\"ready\":");
    output.append(service.ready ? "true" : "false");
    output.append(",\"in_flight\":");
    output.append(std::to_string(service.in_flight));
    output.append(",\"queued\":");
    output.append(std::to_string(service.queued));
    output.append(",\"total_requests\":");
    output.append(std::to_string(service.total_requests));
    output.append(",\"timeout_count\":");
    output.append(std::to_string(service.timeout_count));
    output.append(",\"busy_count\":");
    output.append(std::to_string(service.busy_count));
    output.append(",\"unavailable_count\":");
    output.append(std::to_string(service.unavailable_count));
    output.append(",\"late_drop_count\":");
    output.append(std::to_string(service.late_drop_count));
    output.push_back('}');
    return output;
}

inline std::string optional_u64_json(const std::optional<std::uint64_t> &value) {
    return value ? std::to_string(*value) : "null";
}

inline std::string task_health_json(const IntrospectionTaskHealth &task) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(task.name));
    output.append(",\"lane\":");
    output.append(json_string(task.lane));
    output.append(",\"deadline_missed\":");
    output.append(std::to_string(task.deadline_missed));
    output.append(",\"stale_input\":");
    output.append(std::to_string(task.stale_input));
    output.append(",\"backpressure\":");
    output.append(std::to_string(task.backpressure));
    output.append(",\"overflow\":");
    output.append(std::to_string(task.overflow));
    output.append(",\"fairness_violations\":");
    output.append(std::to_string(task.fairness_violations));
    output.append(",\"run_count\":");
    output.append(std::to_string(task.run_count));
    output.append(",\"success_count\":");
    output.append(std::to_string(task.success_count));
    output.append(",\"consecutive_failures\":");
    output.append(std::to_string(task.consecutive_failures));
    output.append(",\"last_run_ms\":");
    output.append(optional_u64_json(task.last_run_ms));
    output.append(",\"last_success_ms\":");
    output.append(optional_u64_json(task.last_success_ms));
    output.push_back('}');
    return output;
}

inline std::string lane_health_json(const IntrospectionLaneHealth &lane) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(lane.name));
    output.append(",\"queue_depth\":");
    output.append(std::to_string(lane.queue_depth));
    output.append(",\"dispatched_count\":");
    output.append(std::to_string(lane.dispatched_count));
    output.append(",\"fairness_violations\":");
    output.append(std::to_string(lane.fairness_violations));
    output.push_back('}');
    return output;
}

inline std::string status_json(const IntrospectionStatus &status) {
    std::string output;
    output.append("{\"tick_count\":");
    output.append(std::to_string(status.tick_count));
    output.append(",\"channels\":[");
    for (std::size_t index = 0; index < status.channels.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(channel_status_json(status.channels[index]));
    }
    output.append("],\"processes\":[],\"services\":[");
    for (std::size_t index = 0; index < status.services.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(service_status_json(status.services[index]));
    }
    output.append("],\"tasks\":[");
    for (std::size_t index = 0; index < status.tasks.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(task_health_json(status.tasks[index]));
    }
    output.append("],\"lanes\":[");
    for (std::size_t index = 0; index < status.lanes.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(lane_health_json(status.lanes[index]));
    }
    output.append("]}");
    return output;
}

inline std::string optional_json_fragment(const std::optional<std::string> &value) {
    return value ? *value : "null";
}

inline std::string json_fragment_array(const std::vector<std::string> &values) {
    std::string output;
    output.push_back('[');
    for (std::size_t index = 0; index < values.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(values[index]);
    }
    output.push_back(']');
    return output;
}

inline std::string param_status_json(const IntrospectionParamStatus &param) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(param.name));
    output.append(",\"type\":");
    output.append(json_string(param.ty));
    output.append(",\"update\":");
    output.append(json_string(param.update));
    output.append(",\"current\":");
    output.append(param.current);
    output.append(",\"pending\":");
    output.append(optional_json_fragment(param.pending));
    output.append(",\"min\":");
    output.append(optional_json_fragment(param.min));
    output.append(",\"max\":");
    output.append(optional_json_fragment(param.max));
    output.append(",\"choices\":");
    output.append(json_fragment_array(param.choices));
    output.push_back('}');
    return output;
}

inline std::string param_list_response_json(const IntrospectionHandshake &handshake,
                                            const std::vector<IntrospectionParamStatus> &params) {
    std::string output;
    output.append("{\"response\":\"param_list\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"params\":[");
    for (std::size_t index = 0; index < params.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(param_status_json(params[index]));
    }
    output.append("]}");
    return output;
}

inline std::string param_value_response_json(const IntrospectionHandshake &handshake,
                                             const IntrospectionParamStatus &param) {
    std::string output;
    output.append("{\"response\":\"param_value\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"param\":");
    output.append(param_status_json(param));
    output.push_back('}');
    return output;
}

inline std::string self_description_response_json(const IntrospectionHandshake &handshake,
                                                  std::string_view json) {
    std::string output;
    output.append("{\"response\":\"self_description\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"json\":");
    output.append(json_string(json));
    output.push_back('}');
    return output;
}

inline std::string payload_json(const std::optional<std::vector<std::uint8_t>> &payload) {
    if (!payload) {
        return "null";
    }
    std::string output;
    output.push_back('[');
    for (std::size_t index = 0; index < payload->size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(std::to_string(static_cast<unsigned int>((*payload)[index])));
    }
    output.push_back(']');
    return output;
}

inline std::string channel_snapshot_json(const IntrospectionChannelSnapshot &channel) {
    std::string output;
    output.append("{\"published_count\":");
    output.append(std::to_string(channel.published_count));
    output.append(",\"payload\":");
    output.append(payload_json(channel.payload));
    output.append(",\"published_at_ms\":");
    output.append(channel.published_at_ms ? std::to_string(*channel.published_at_ms) : "null");
    output.push_back('}');
    return output;
}

inline std::string status_response_json(const IntrospectionHandshake &handshake,
                                        const IntrospectionStatus &status) {
    std::string output;
    output.append("{\"response\":\"status\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"status\":");
    output.append(status_json(status));
    output.push_back('}');
    return output;
}

inline std::string channel_snapshot_response_json(const IntrospectionHandshake &handshake,
                                                  const IntrospectionChannelSnapshot &channel) {
    std::string output;
    output.append("{\"response\":\"channel_snapshot\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"channel\":");
    output.append(channel_snapshot_json(channel));
    output.push_back('}');
    return output;
}

inline std::string observe_ready_response_json(const IntrospectionHandshake &handshake,
                                               const IntrospectionChannelStatus &channel) {
    std::string output;
    output.append("{\"response\":\"observe_ready\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"channel\":");
    output.append(channel_status_json(channel));
    output.push_back('}');
    return output;
}

inline std::string error_response_json(const IntrospectionHandshake &handshake,
                                       std::string_view message) {
    std::string output;
    output.append("{\"response\":\"error\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"message\":");
    output.append(json_string(message));
    output.push_back('}');
    return output;
}

inline bool json_whitespace(char byte) noexcept {
    return byte == ' ' || byte == '\t' || byte == '\n' || byte == '\r';
}

inline std::optional<std::size_t> find_json_string_value(std::string_view input,
                                                         std::string_view key, std::string &value) {
    const std::string needle = "\"" + std::string(key) + "\"";
    const auto key_pos = input.find(needle);
    if (key_pos == std::string_view::npos) {
        return std::nullopt;
    }
    std::size_t index = key_pos + needle.size();
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size() || input[index] != ':') {
        return std::nullopt;
    }
    ++index;
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size() || input[index] != '"') {
        return std::nullopt;
    }
    ++index;

    value.clear();
    while (index < input.size()) {
        const char byte = input[index++];
        if (byte == '"') {
            return index;
        }
        if (byte != '\\') {
            value.push_back(byte);
            continue;
        }
        if (index >= input.size()) {
            return std::nullopt;
        }
        const char escape = input[index++];
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

enum class IntrospectionRequestKind : std::uint8_t {
    Status = 0,
    SelfDescription = 1,
    ChannelSnapshot = 2,
    ObserveChannel = 3,
    ParamList = 4,
    ParamGet = 5,
    ParamSet = 6,
};

struct ParsedIntrospectionRequest {
    IntrospectionRequestKind kind = IntrospectionRequestKind::Status;
    std::string channel;
    std::string param_name;
    std::string param_value;
};

inline std::optional<std::size_t> find_json_value_fragment(std::string_view input,
                                                           std::string_view key,
                                                           std::string &value) {
    const std::string needle = "\"" + std::string(key) + "\"";
    const auto key_pos = input.find(needle);
    if (key_pos == std::string_view::npos) {
        return std::nullopt;
    }
    std::size_t index = key_pos + needle.size();
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size() || input[index] != ':') {
        return std::nullopt;
    }
    ++index;
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size()) {
        return std::nullopt;
    }
    const std::size_t start = index;
    bool in_string = false;
    bool escaped = false;
    int object_depth = 0;
    int array_depth = 0;
    while (index < input.size()) {
        const char byte = input[index];
        if (in_string) {
            if (escaped) {
                escaped = false;
            } else if (byte == '\\') {
                escaped = true;
            } else if (byte == '"') {
                in_string = false;
            }
            ++index;
            continue;
        }
        if (byte == '"') {
            in_string = true;
            ++index;
            continue;
        }
        if (byte == '{') {
            ++object_depth;
        } else if (byte == '}') {
            if (object_depth == 0 && array_depth == 0) {
                break;
            }
            --object_depth;
        } else if (byte == '[') {
            ++array_depth;
        } else if (byte == ']') {
            --array_depth;
        } else if (byte == ',' && object_depth == 0 && array_depth == 0) {
            break;
        }
        ++index;
    }
    value = std::string{input.substr(start, index - start)};
    while (!value.empty() && json_whitespace(value.back())) {
        value.pop_back();
    }
    return index;
}

inline std::optional<ParsedIntrospectionRequest> parse_introspection_request(
    std::string_view line) {
    std::string command;
    if (!find_json_string_value(line, "command", command)) {
        return std::nullopt;
    }
    if (command == "status") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::Status, {}};
    }
    if (command == "self_description") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::SelfDescription, {}};
    }
    if (command == "channel_snapshot") {
        std::string channel;
        if (!find_json_string_value(line, "channel", channel)) {
            return std::nullopt;
        }
        return ParsedIntrospectionRequest{
            IntrospectionRequestKind::ChannelSnapshot, std::move(channel), {}, {}};
    }
    if (command == "observe_channel") {
        std::string channel;
        if (!find_json_string_value(line, "channel", channel)) {
            return std::nullopt;
        }
        return ParsedIntrospectionRequest{
            IntrospectionRequestKind::ObserveChannel, std::move(channel), {}, {}};
    }
    if (command == "param_list") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::ParamList, {}, {}, {}};
    }
    if (command == "param_get") {
        std::string name;
        if (!find_json_string_value(line, "name", name)) {
            return std::nullopt;
        }
        return ParsedIntrospectionRequest{
            IntrospectionRequestKind::ParamGet, {}, std::move(name), {}};
    }
    if (command == "param_set") {
        std::string name;
        std::string value;
        if (!find_json_string_value(line, "name", name) ||
            !find_json_value_fragment(line, "value", value)) {
            return std::nullopt;
        }
        return ParsedIntrospectionRequest{
            IntrospectionRequestKind::ParamSet, {}, std::move(name), std::move(value)};
    }
    return std::nullopt;
}

inline bool write_all(int fd, std::string_view data) {
#if defined(MSG_NOSIGNAL)
    constexpr int send_flags = MSG_NOSIGNAL;
#else
    constexpr int send_flags = 0;
#endif
    std::size_t offset = 0;
    while (offset < data.size()) {
        const auto written = ::send(fd, data.data() + offset, data.size() - offset, send_flags);
        if (written < 0) {
            if (errno == EINTR) {
                continue;
            }
            return false;
        }
        if (written == 0) {
            return false;
        }
        offset += static_cast<std::size_t>(written);
    }
    return true;
}

inline void set_socket_timeout(int fd) {
    timeval timeout{};
    timeout.tv_sec = 1;
    timeout.tv_usec = 0;
    (void)::setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    (void)::setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
}

inline std::optional<std::string> read_line(int fd) {
    std::string line;
    char byte = '\0';
    while (line.size() < 65536U) {
        const auto received = ::read(fd, &byte, 1);
        if (received == 0) {
            break;
        }
        if (received < 0) {
            if (errno == EINTR) {
                continue;
            }
            return std::nullopt;
        }
        if (byte == '\n') {
            return line;
        }
        line.push_back(byte);
    }
    return line.empty() ? std::nullopt : std::optional<std::string>{std::move(line)};
}

}  // namespace detail

/**
 * @brief 生成 handshake 的输入元数据。
 */
struct IntrospectionIdentity {
    std::string self_description_hash;
    std::string package;
    std::string process;
    std::string runtime;

    /**
     * @brief 构造当前进程的 handshake。
     */
    IntrospectionHandshake handshake() const {
        return IntrospectionHandshake{
            .protocol_version = std::string{INTROSPECTION_PROTOCOL_VERSION},
            .pid = static_cast<std::uint32_t>(::getpid()),
            .started_at_unix_ms = detail::unix_time_ms(),
            .self_description_hash = self_description_hash,
            .package = package,
            .process = process,
            .runtime = runtime,
        };
    }
};

/**
 * @brief runtime shell 可共享更新的 introspection live 状态。
 */
class IntrospectionState {
   public:
    /**
     * @brief 构造空 live 状态。
     */
    IntrospectionState() : inner_(std::make_shared<Inner>()) {}

    /**
     * @brief 预注册 channel，使其在尚未发布样本时也出现在 status 中。
     */
    void register_channel(std::string name, std::string message_type) const {
        register_channel_with_probe_capacity(std::move(name), std::move(message_type),
                                             std::nullopt);
    }

    /**
     * @brief 预注册带有有界 probe snapshot 容量的 channel。
     */
    void register_channel_with_probe_capacity(std::string name, std::string message_type,
                                              std::optional<std::size_t> max_payload_len) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->channels.try_emplace(std::move(name),
                                     ChannelState{
                                         .message_type = std::move(message_type),
                                         .probe = IntrospectionChannelProbe{max_payload_len},
                                     });
    }

    /**
     * @brief 注册一个 runtime 参数，使 CLI 能查询并提交 pending 更新。
     */
    void register_param(IntrospectionParamSchema schema) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->params.try_emplace(std::move(schema.name), ParamState{
                                                               .type = std::move(schema.ty),
                                                               .update = std::move(schema.update),
                                                               .current = std::move(schema.current),
                                                               .pending = std::nullopt,
                                                               .min = std::move(schema.min),
                                                               .max = std::move(schema.max),
                                                               .choices = std::move(schema.choices),
                                                           });
    }

    /**
     * @brief 注册编译期 self-description JSON，供在线 CLI 自动发现和格式化。
     */
    void set_self_description_json(std::string json) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->self_description_json = std::move(json);
    }

    /**
     * @brief 返回当前 runtime 暴露的 self-description JSON。
     */
    std::optional<std::string> self_description_json() const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return inner_->self_description_json;
    }

    /**
     * @brief 增加 scheduler tick 计数。
     */
    void record_tick() const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        if (inner_->tick_count != UINT64_MAX) {
            ++inner_->tick_count;
        }
    }

    /**
     * @brief 记录 channel 发布的 raw ABI bytes。
     */
    void record_channel_publish_bytes(std::string name, std::string message_type,
                                      std::vector<std::uint8_t> payload,
                                      std::optional<std::uint64_t> published_at_ms) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto [iterator, _inserted] =
            inner_->channels.try_emplace(std::move(name), ChannelState{
                                                              .message_type = message_type,
                                                              .probe = IntrospectionChannelProbe{},
                                                          });
        auto &channel = iterator->second;
        channel.message_type = std::move(message_type);
        channel.probe.force_record_bytes(std::move(payload), published_at_ms);
    }

    /**
     * @brief 记录 channel 发布的 Message ABI 对象表示。
     */
    template <typename T>
    void record_channel_publish(std::string name, std::string message_type, const T &value,
                                std::optional<std::uint64_t> published_at_ms) const {
        static_assert(std::is_trivially_copyable_v<T>,
                      "FlowRT introspection payload snapshot requires trivially copyable values");
        std::vector<std::uint8_t> payload(sizeof(T));
        if (!payload.empty()) {
            std::memcpy(payload.data(), std::addressof(value), payload.size());
        }
        record_channel_publish_bytes(std::move(name), std::move(message_type), std::move(payload),
                                     published_at_ms);
    }

    /**
     * @brief 获取指定 channel 的 probe handle。
     */
    std::optional<IntrospectionChannelProbe> channel_probe(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto channel = inner_->channels.find(std::string{name});
        if (channel == inner_->channels.end()) {
            return std::nullopt;
        }
        return channel->second.probe;
    }

    /**
     * @brief 为指定 channel 建立连接作用域 observer guard。
     */
    std::optional<IntrospectionObserverGuard> observe_channel(std::string_view name) const {
        const auto probe = channel_probe(name);
        if (!probe) {
            return std::nullopt;
        }
        return probe->observe();
    }

    /**
     * @brief 返回指定 channel 的 active observer 数量。
     */
    std::optional<std::uint64_t> active_probe_count(std::string_view name) const {
        const auto probe = channel_probe(name);
        if (!probe) {
            return std::nullopt;
        }
        return probe->active_count();
    }

    /**
     * @brief 按需记录 channel 发布的 raw ABI bytes。
     */
    IntrospectionProbeRecord try_probe_channel_publish_bytes(
        std::string name, std::string message_type, const std::vector<std::uint8_t> &payload,
        std::optional<std::uint64_t> published_at_ms) const {
        return try_probe_channel_publish_bytes(
            std::move(name), std::move(message_type),
            std::span<const std::uint8_t>{payload.data(), payload.size()}, published_at_ms);
    }

    /**
     * @brief 按需记录 channel 发布的 raw ABI bytes。
     */
    IntrospectionProbeRecord try_probe_channel_publish_bytes(
        std::string name, std::string message_type, std::span<const std::uint8_t> payload,
        std::optional<std::uint64_t> published_at_ms) const {
        IntrospectionChannelProbe probe;
        {
            std::lock_guard<std::mutex> lock(inner_->mutex);
            auto [iterator, _inserted] = inner_->channels.try_emplace(
                std::move(name), ChannelState{
                                     .message_type = message_type,
                                     .probe = IntrospectionChannelProbe{},
                                 });
            iterator->second.message_type = std::move(message_type);
            probe = iterator->second.probe;
        }
        return probe.try_record_bytes(payload, published_at_ms);
    }

    /**
     * @brief 按需记录 channel 发布的 Message ABI 对象表示。
     */
    template <typename T>
    IntrospectionProbeRecord try_probe_channel_publish(
        std::string_view name, std::string message_type, const T &value,
        std::optional<std::uint64_t> published_at_ms) const {
        static_assert(std::is_trivially_copyable_v<T>,
                      "FlowRT introspection payload snapshot requires trivially copyable values");
        const auto probe = channel_probe(name);
        if (!probe || !probe->enabled()) {
            return IntrospectionProbeRecord{};
        }
        return try_probe_channel_publish_bytes(
            std::string{name}, std::move(message_type),
            std::span<const std::uint8_t>{reinterpret_cast<const std::uint8_t *>(&value),
                                          sizeof(T)},
            published_at_ms);
    }

    /**
     * @brief 预注册一个 service endpoint，使其在尚未收到请求时也出现在 status 中。
     */
    void register_service(std::string name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->services.try_emplace(name, IntrospectionServiceStatus{
                                                .name = name,
                                                .ready = true,
                                            });
    }

    /**
     * @brief 记录 service 运行态健康状态快照。
     */
    void record_service_health(IntrospectionServiceStatus status) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->services.insert_or_assign(status.name, std::move(status));
    }

    /**
     * @brief 记录 task 调度健康快照。
     */
    void record_task_health(IntrospectionTaskHealth health) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->tasks.insert_or_assign(health.name, std::move(health));
    }

    /**
     * @brief 记录 lane 调度健康快照。
     */
    void record_lane_health(IntrospectionLaneHealth health) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->lanes.insert_or_assign(health.name, std::move(health));
    }

    /**
     * @brief 返回当前 status 快照。
     */
    IntrospectionStatus status() const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        IntrospectionStatus snapshot;
        snapshot.tick_count = inner_->tick_count;
        snapshot.channels.reserve(inner_->channels.size());
        for (const auto &[name, channel] : inner_->channels) {
            const auto channel_snapshot = channel.probe.snapshot();
            snapshot.channels.push_back(IntrospectionChannelStatus{
                .name = name,
                .message_type = channel.message_type,
                .published_count = channel_snapshot.published_count,
                .last_payload_len =
                    channel_snapshot.payload
                        ? std::optional<std::size_t>{channel_snapshot.payload->size()}
                        : std::nullopt,
                .active_observers = channel.probe.active_count(),
                .dropped_samples = channel.probe.dropped_samples(),
            });
        }
        snapshot.services.reserve(inner_->services.size());
        for (const auto &[name, service] : inner_->services) {
            snapshot.services.push_back(service);
        }
        snapshot.tasks.reserve(inner_->tasks.size());
        for (const auto &[name, task] : inner_->tasks) {
            snapshot.tasks.push_back(task);
        }
        snapshot.lanes.reserve(inner_->lanes.size());
        for (const auto &[name, lane] : inner_->lanes) {
            snapshot.lanes.push_back(lane);
        }
        return snapshot;
    }

    /**
     * @brief 返回指定 channel 的 raw ABI snapshot。
     */
    std::optional<IntrospectionChannelSnapshot> channel_snapshot(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto channel = inner_->channels.find(std::string{name});
        if (channel == inner_->channels.end()) {
            return std::nullopt;
        }
        return channel->second.probe.snapshot();
    }

    /**
     * @brief 返回指定 channel 的运行态摘要。
     */
    std::optional<IntrospectionChannelStatus> channel_status(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto channel = inner_->channels.find(std::string{name});
        if (channel == inner_->channels.end()) {
            return std::nullopt;
        }
        const auto snapshot = channel->second.probe.snapshot();
        return IntrospectionChannelStatus{
            .name = std::string{name},
            .message_type = channel->second.message_type,
            .published_count = snapshot.published_count,
            .last_payload_len = snapshot.payload
                                    ? std::optional<std::size_t>{snapshot.payload->size()}
                                    : std::nullopt,
            .active_observers = channel->second.probe.active_count(),
            .dropped_samples = channel->second.probe.dropped_samples(),
        };
    }

    /**
     * @brief 返回参数状态列表。
     */
    std::vector<IntrospectionParamStatus> params() const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        std::vector<IntrospectionParamStatus> snapshot;
        snapshot.reserve(inner_->params.size());
        for (const auto &[name, param] : inner_->params) {
            snapshot.push_back(param_status(name, param));
        }
        return snapshot;
    }

    /**
     * @brief 返回单个参数状态。
     */
    std::optional<IntrospectionParamStatus> param(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto param = inner_->params.find(std::string{name});
        if (param == inner_->params.end()) {
            return std::nullopt;
        }
        return param_status(param->first, param->second);
    }

    /**
     * @brief 设置参数 pending 值。
     */
    std::variant<IntrospectionParamStatus, std::string> set_param_pending(std::string_view name,
                                                                          std::string value) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto param = inner_->params.find(std::string{name});
        if (param == inner_->params.end()) {
            return "unknown FlowRT parameter `" + std::string{name} + "`";
        }
        if (param->second.update != "on_tick") {
            return "FlowRT parameter `" + std::string{name} + "` is startup-only";
        }
        if (const auto error = validate_param_json_value(param->first, param->second, value)) {
            return *error;
        }
        param->second.pending = std::move(value);
        return param_status(param->first, param->second);
    }

    /**
     * @brief 读取参数 pending 值，主要用于测试和 generated shell 快速检查。
     */
    std::optional<std::string> pending_param(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto param = inner_->params.find(std::string{name});
        if (param == inner_->params.end()) {
            return std::nullopt;
        }
        return param->second.pending;
    }

    /**
     * @brief 取出并清空参数 pending 值。
     */
    std::optional<std::string> take_pending_param(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto param = inner_->params.find(std::string{name});
        if (param == inner_->params.end()) {
            return std::nullopt;
        }
        auto value = std::move(param->second.pending);
        param->second.pending = std::nullopt;
        return value;
    }

    /**
     * @brief 记录参数已经由 generated shell 应用为当前值。
     */
    void record_param_applied(std::string_view name, std::string value) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto param = inner_->params.find(std::string{name});
        if (param != inner_->params.end()) {
            param->second.current = std::move(value);
            param->second.pending = std::nullopt;
        }
    }

   private:
    struct ChannelState {
        std::string message_type;
        IntrospectionChannelProbe probe;
    };

    struct ParamState {
        std::string type;
        std::string update;
        std::string current;
        std::optional<std::string> pending;
        std::optional<std::string> min;
        std::optional<std::string> max;
        std::vector<std::string> choices;
    };

    struct Inner {
        std::mutex mutex;
        std::uint64_t tick_count = 0;
        std::optional<std::string> self_description_json;
        std::map<std::string, ChannelState> channels;
        std::map<std::string, ParamState> params;
        std::map<std::string, IntrospectionServiceStatus> services;
        std::map<std::string, IntrospectionTaskHealth> tasks;
        std::map<std::string, IntrospectionLaneHealth> lanes;
    };

    static IntrospectionParamStatus param_status(const std::string &name, const ParamState &param) {
        return IntrospectionParamStatus{
            .name = name,
            .ty = param.type,
            .update = param.update,
            .current = param.current,
            .pending = param.pending,
            .min = param.min,
            .max = param.max,
            .choices = param.choices,
        };
    }

    static std::optional<std::string> validate_param_json_value(const std::string &name,
                                                                const ParamState &param,
                                                                std::string_view value) {
        if (!json_value_matches_param_type(param.type, value)) {
            return "FlowRT parameter `" + name + "` expects `" + param.type + "` value";
        }
        if (param.min && compare_json_number(value, *param.min).value_or(0) < 0) {
            return "FlowRT parameter `" + name + "` is below minimum";
        }
        if (param.max && compare_json_number(value, *param.max).value_or(0) > 0) {
            return "FlowRT parameter `" + name + "` is above maximum";
        }
        if (!param.choices.empty() &&
            std::find(param.choices.begin(), param.choices.end(), value) == param.choices.end()) {
            return "FlowRT parameter `" + name + "` is not in declared enum choices";
        }
        return std::nullopt;
    }

    static bool json_value_matches_param_type(std::string_view type, std::string_view value) {
        if (type == "string") {
            return value.size() >= 2U && value.front() == '"' && value.back() == '"';
        }
        if (type == "bool") {
            return value == "true" || value == "false";
        }
        if (type == "f32" || type == "f64") {
            return parse_json_number(value).has_value();
        }
        if (type == "u8" || type == "u16" || type == "u32" || type == "u64") {
            const auto number = parse_json_number(value);
            return number && *number >= 0.0;
        }
        if (type == "i8" || type == "i16" || type == "i32" || type == "i64") {
            return parse_json_number(value).has_value();
        }
        return false;
    }

    static std::optional<double> parse_json_number(std::string_view value) {
        std::string owned{value};
        char *end = nullptr;
        errno = 0;
        const double parsed = std::strtod(owned.c_str(), &end);
        if (errno != 0 || end == owned.c_str() || *end != '\0') {
            return std::nullopt;
        }
        return parsed;
    }

    static std::optional<int> compare_json_number(std::string_view left, std::string_view right) {
        const auto left_number = parse_json_number(left);
        const auto right_number = parse_json_number(right);
        if (!left_number || !right_number) {
            return std::nullopt;
        }
        if (*left_number < *right_number) {
            return -1;
        }
        if (*left_number > *right_number) {
            return 1;
        }
        return 0;
    }

    std::shared_ptr<Inner> inner_;
};

/**
 * @brief 返回当前用户 runtime socket 目录。
 *
 * 优先使用 `$XDG_RUNTIME_DIR/flowrt`；没有时 fallback 到 `/tmp/flowrt.<uid>`，避免不同用户
 * 的同名 PID socket 互相污染。
 */
inline std::filesystem::path runtime_socket_dir() {
    if (const char *runtime_dir = std::getenv("XDG_RUNTIME_DIR"); runtime_dir != nullptr) {
        return std::filesystem::path(runtime_dir) / "flowrt";
    }
    return std::filesystem::path("/tmp") /
           ("flowrt." + std::to_string(static_cast<unsigned int>(::getuid())));
}

/**
 * @brief 返回指定 PID 的默认 runtime socket 路径。
 */
inline std::filesystem::path runtime_socket_path_for_pid(std::uint32_t pid) {
    return runtime_socket_dir() / (std::to_string(pid) + ".sock");
}

class IntrospectionServer;

namespace detail {

inline void handle_introspection_connection(int client_fd, const IntrospectionHandshake &handshake,
                                            const IntrospectionState &state) {
    const auto line = read_line(client_fd);
    std::string response;
    if (!line) {
        response = error_response_json(handshake, "invalid FlowRT introspection request");
    } else if (const auto request = parse_introspection_request(*line)) {
        switch (request->kind) {
            case IntrospectionRequestKind::Status:
                response = status_response_json(handshake, state.status());
                break;
            case IntrospectionRequestKind::SelfDescription: {
                const auto json = state.self_description_json();
                response = json ? self_description_response_json(handshake, *json)
                                : error_response_json(handshake,
                                                      "FlowRT self-description is not registered");
                break;
            }
            case IntrospectionRequestKind::ChannelSnapshot: {
                const auto channel = state.channel_snapshot(request->channel);
                response = channel ? channel_snapshot_response_json(handshake, *channel)
                                   : error_response_json(handshake, "unknown FlowRT channel");
                break;
            }
            case IntrospectionRequestKind::ObserveChannel: {
                auto guard = state.observe_channel(request->channel);
                const auto channel = state.channel_status(request->channel);
                if (!guard || !channel) {
                    response = error_response_json(handshake, "unknown FlowRT channel");
                    break;
                }
                response = observe_ready_response_json(handshake, *channel);
                response.push_back('\n');
                (void)write_all(client_fd, response);
                while (true) {
                    const auto keepalive = read_line(client_fd);
                    if (!keepalive) {
                        break;
                    }
                }
                return;
            }
            case IntrospectionRequestKind::ParamList:
                response = param_list_response_json(handshake, state.params());
                break;
            case IntrospectionRequestKind::ParamGet: {
                const auto param = state.param(request->param_name);
                response = param ? param_value_response_json(handshake, *param)
                                 : error_response_json(handshake, "unknown FlowRT parameter `" +
                                                                      request->param_name + "`");
                break;
            }
            case IntrospectionRequestKind::ParamSet: {
                const auto result =
                    state.set_param_pending(request->param_name, request->param_value);
                if (std::holds_alternative<IntrospectionParamStatus>(result)) {
                    response = param_value_response_json(
                        handshake, std::get<IntrospectionParamStatus>(result));
                } else {
                    response = error_response_json(handshake, std::get<std::string>(result));
                }
                break;
            }
        }
    } else {
        response = error_response_json(handshake, "invalid FlowRT introspection request");
    }
    response.push_back('\n');
    (void)write_all(client_fd, response);
}

inline bool unix_socket_accepts_connection(const std::filesystem::path &path) noexcept {
    const int fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        return false;
    }

    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    const auto path_string = path.string();
    if (path_string.size() >= sizeof(address.sun_path)) {
        ::close(fd);
        return false;
    }
    std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", path_string.c_str());
    const bool connected =
        ::connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) == 0;
    ::close(fd);
    return connected;
}

}  // namespace detail

/**
 * @brief 已启动的 introspection 服务。
 *
 * 该对象拥有 Unix socket listener 线程，并在析构时停止 listener、删除 socket 文件。
 */
class IntrospectionServer {
   public:
    IntrospectionServer() = default;
    IntrospectionServer(const IntrospectionServer &) = delete;
    auto operator=(const IntrospectionServer &) -> IntrospectionServer & = delete;

    IntrospectionServer(IntrospectionServer &&other) noexcept
        : path_(std::move(other.path_)),
          handle_(std::move(other.handle_)),
          stop_(std::move(other.stop_)) {
        other.path_.clear();
    }

    auto operator=(IntrospectionServer &&other) noexcept -> IntrospectionServer & {
        if (this != std::addressof(other)) {
            stop();
            path_ = std::move(other.path_);
            handle_ = std::move(other.handle_);
            stop_ = std::move(other.stop_);
            other.path_.clear();
        }
        return *this;
    }

    ~IntrospectionServer() { stop(); }

    /**
     * @brief 返回服务 socket 路径。
     */
    const std::filesystem::path &path() const noexcept { return path_; }

   private:
    friend std::optional<IntrospectionServer> spawn_status_server_at(
        std::filesystem::path path, IntrospectionHandshake handshake, IntrospectionState state);

    IntrospectionServer(std::filesystem::path path, std::thread handle,
                        std::shared_ptr<std::atomic_bool> stop)
        : path_(std::move(path)), handle_(std::move(handle)), stop_(std::move(stop)) {}

    void stop() noexcept {
        if (stop_) {
            stop_->store(true, std::memory_order_relaxed);
        }
        if (!path_.empty()) {
            std::error_code ignored;
            std::filesystem::remove(path_, ignored);
        }
        if (handle_.joinable()) {
            handle_.join();
        }
        stop_.reset();
        path_.clear();
    }

    std::filesystem::path path_;
    std::thread handle_;
    std::shared_ptr<std::atomic_bool> stop_;
};

/**
 * @brief 在指定路径启动最小 introspection status 服务，主要用于测试和后续 generated shell 接入。
 */
inline std::optional<IntrospectionServer> spawn_status_server_at(std::filesystem::path path,
                                                                 IntrospectionHandshake handshake,
                                                                 IntrospectionState state) {
    std::error_code filesystem_error;
    if (const auto parent = path.parent_path(); !parent.empty()) {
        std::filesystem::create_directories(parent, filesystem_error);
        if (filesystem_error) {
            return std::nullopt;
        }
    }
    if (std::filesystem::exists(path, filesystem_error)) {
        if (detail::unix_socket_accepts_connection(path)) {
            return std::nullopt;
        }
        filesystem_error.clear();
        std::filesystem::remove(path, filesystem_error);
        if (filesystem_error) {
            return std::nullopt;
        }
    }

    const int listener_fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
    if (listener_fd < 0) {
        return std::nullopt;
    }

    auto close_listener = [listener_fd]() { ::close(listener_fd); };
    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    const auto path_string = path.string();
    if (path_string.size() >= sizeof(address.sun_path)) {
        close_listener();
        return std::nullopt;
    }
    std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", path_string.c_str());

    if (::bind(listener_fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) != 0) {
        close_listener();
        return std::nullopt;
    }
    if (::listen(listener_fd, 16) != 0) {
        close_listener();
        std::filesystem::remove(path, filesystem_error);
        return std::nullopt;
    }
    const int flags = ::fcntl(listener_fd, F_GETFL, 0);
    if (flags < 0 || ::fcntl(listener_fd, F_SETFL, flags | O_NONBLOCK) != 0) {
        close_listener();
        std::filesystem::remove(path, filesystem_error);
        return std::nullopt;
    }

    auto stop = std::make_shared<std::atomic_bool>(false);
    auto thread_stop = stop;
    std::thread handle;
    try {
        handle = std::thread([listener_fd, thread_stop, handshake = std::move(handshake),
                              state = std::move(state)]() mutable {
            while (!thread_stop->load(std::memory_order_relaxed)) {
                const int client_fd = ::accept(listener_fd, nullptr, nullptr);
                if (client_fd >= 0) {
                    detail::set_socket_timeout(client_fd);
                    try {
                        std::thread([client_fd, handshake, state]() {
                            detail::handle_introspection_connection(client_fd, handshake, state);
                            ::close(client_fd);
                        }).detach();
                    } catch (...) {
                        ::close(client_fd);
                    }
                    continue;
                }
                if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) {
                    std::this_thread::sleep_for(std::chrono::milliseconds{10});
                    continue;
                }
                break;
            }
            ::close(listener_fd);
        });
    } catch (...) {
        close_listener();
        std::filesystem::remove(path, filesystem_error);
        return std::nullopt;
    }

    return IntrospectionServer{std::move(path), std::move(handle), std::move(stop)};
}

/**
 * @brief 用当前进程 PID 命名 socket 并启动最小 introspection status 服务。
 */
inline std::optional<IntrospectionServer> spawn_status_server(IntrospectionIdentity identity,
                                                              IntrospectionState state) {
    auto handshake = identity.handshake();
    auto path = runtime_socket_path_for_pid(handshake.pid);
    return spawn_status_server_at(std::move(path), std::move(handshake), std::move(state));
}

}  // namespace flowrt
