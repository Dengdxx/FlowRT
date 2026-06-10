#pragma once

#include <algorithm>
#include <atomic>
#include <cerrno>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <flowrt/core.hpp>
#include <fcntl.h>
#include <filesystem>
#include <limits>
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
/// 单个 runtime introspection server 允许同时处理的客户端连接数。
inline constexpr std::size_t MAX_INTROSPECTION_CLIENT_THREADS = 64U;
/// 单个 runtime introspection server 允许同时保持的 observe 长连接数。
inline constexpr std::size_t MAX_INTROSPECTION_OBSERVERS = 32U;

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

/**
 * @brief recorder 的单次运行态事件。
 *
 * C++ runtime 先暴露与 Rust recorder 对齐的内存队列 tap。CLI record 写盘由后续任务接入，
 * 这里不启动后台线程，也不在 recorder 关闭时复制 payload。
 */
struct IntrospectionRecorderEvent {
    std::uint16_t schema_version = 1;
    std::string event_kind;
    std::string package;
    std::string process;
    std::uint32_t runtime_pid = 0;
    std::string selfdesc_hash;
    std::uint64_t monotonic_ns = 0;
    std::uint64_t wall_unix_ns = 0;
    std::uint64_t sequence = 0;
    std::string entity_kind;
    std::string entity_name;
    std::optional<std::string> entity_instance;
    std::optional<std::string> entity_task;
    std::optional<std::string> entity_type_name;
    std::string payload_encoding;
    std::string payload_schema;
    std::vector<std::uint8_t> payload;
};

/**
 * @brief recorder 状态快照，进入 introspection status。
 */
struct IntrospectionRecorderStatus {
    bool enabled = false;
    std::optional<std::string> output;
    std::uint64_t dropped_count = 0;
    std::uint64_t bytes_written = 0;
    std::vector<std::string> active_filters;
    std::uint64_t queued_events = 0;
};

/**
 * @brief recorder 启动参数。socket 控制面会用当前 handshake 填充身份字段。
 */
struct IntrospectionRecorderStart {
    std::optional<std::string> output;
    std::vector<std::string> filters;
    std::size_t queue_depth = 1024;
    std::string package;
    std::string process;
    std::uint32_t runtime_pid = 0;
    std::string self_description_hash;
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
 * @brief 单个 Operation endpoint 的运行态健康状态。
 */
struct IntrospectionOperationStatus {
    std::string name;
    bool ready = true;
    std::uint64_t running = 0;
    std::uint64_t queued = 0;
    std::vector<std::string> current_operation_ids;
    std::uint64_t total_started = 0;
    std::uint64_t succeeded_count = 0;
    std::uint64_t failed_count = 0;
    std::uint64_t canceled_count = 0;
    std::uint64_t timeout_count = 0;
    std::uint64_t preempted_count = 0;
    std::optional<std::uint64_t> last_transition_ms;
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
    std::vector<BoundaryStatus> io_boundaries;
    std::vector<IntrospectionServiceStatus> services;
    std::vector<IntrospectionOperationStatus> operations;
    std::vector<IntrospectionTaskHealth> tasks;
    std::vector<IntrospectionLaneHealth> lanes;
    IntrospectionRecorderStatus recorder;
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

inline std::uint64_t unix_time_ns() {
    const auto now = std::chrono::system_clock::now().time_since_epoch();
    const auto nanos = std::chrono::duration_cast<std::chrono::nanoseconds>(now).count();
    return nanos < 0 ? 0U : static_cast<std::uint64_t>(nanos);
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

inline std::string boundary_resource_status_json(const BoundaryResourceStatus &resource) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(resource.name));
    output.append(",\"kind\":");
    output.append(json_string(resource.kind));
    output.append(",\"ready\":");
    output.append(resource.ready ? "true" : "false");
    output.append(",\"message\":");
    output.append(resource.message ? json_string(*resource.message) : "null");
    output.append(",\"last_error\":");
    output.append(resource.last_error ? json_string(*resource.last_error) : "null");
    output.append(",\"updated_unix_ms\":");
    output.append(resource.updated_unix_ms ? std::to_string(*resource.updated_unix_ms) : "null");
    output.push_back('}');
    return output;
}

inline std::string boundary_status_json(const BoundaryStatus &boundary) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(boundary.name));
    output.append(",\"component\":");
    output.append(json_string(boundary.component));
    output.append(",\"ready\":");
    output.append(boundary.ready ? "true" : "false");
    output.append(",\"healthy\":");
    output.append(boundary.healthy ? "true" : "false");
    output.append(",\"last_error\":");
    output.append(boundary.last_error ? json_string(*boundary.last_error) : "null");
    output.append(",\"resources\":[");
    for (std::size_t index = 0; index < boundary.resources.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(boundary_resource_status_json(boundary.resources[index]));
    }
    output.append("],\"updated_unix_ms\":");
    output.append(boundary.updated_unix_ms ? std::to_string(*boundary.updated_unix_ms) : "null");
    output.push_back('}');
    return output;
}

inline std::string optional_u64_json(const std::optional<std::uint64_t> &value) {
    return value ? std::to_string(*value) : "null";
}

inline std::string json_string_array(const std::vector<std::string> &values) {
    std::string output;
    output.push_back('[');
    for (std::size_t index = 0; index < values.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(json_string(values[index]));
    }
    output.push_back(']');
    return output;
}

inline std::string recorder_status_json(const IntrospectionRecorderStatus &recorder) {
    std::string output;
    output.append("{\"enabled\":");
    output.append(recorder.enabled ? "true" : "false");
    output.append(",\"output\":");
    output.append(recorder.output ? json_string(*recorder.output) : "null");
    output.append(",\"dropped_count\":");
    output.append(std::to_string(recorder.dropped_count));
    output.append(",\"bytes_written\":");
    output.append(std::to_string(recorder.bytes_written));
    output.append(",\"active_filters\":");
    output.append(json_string_array(recorder.active_filters));
    output.append(",\"queued_events\":");
    output.append(std::to_string(recorder.queued_events));
    output.push_back('}');
    return output;
}

inline std::string operation_status_json(const IntrospectionOperationStatus &operation) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(operation.name));
    output.append(",\"ready\":");
    output.append(operation.ready ? "true" : "false");
    output.append(",\"running\":");
    output.append(std::to_string(operation.running));
    output.append(",\"queued\":");
    output.append(std::to_string(operation.queued));
    output.append(",\"current_operation_ids\":");
    output.append(json_string_array(operation.current_operation_ids));
    output.append(",\"total_started\":");
    output.append(std::to_string(operation.total_started));
    output.append(",\"succeeded_count\":");
    output.append(std::to_string(operation.succeeded_count));
    output.append(",\"failed_count\":");
    output.append(std::to_string(operation.failed_count));
    output.append(",\"canceled_count\":");
    output.append(std::to_string(operation.canceled_count));
    output.append(",\"timeout_count\":");
    output.append(std::to_string(operation.timeout_count));
    output.append(",\"preempted_count\":");
    output.append(std::to_string(operation.preempted_count));
    output.append(",\"last_transition_ms\":");
    output.append(optional_u64_json(operation.last_transition_ms));
    output.push_back('}');
    return output;
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
    output.append(",\"recorder\":");
    output.append(recorder_status_json(status.recorder));
    output.append(",\"channels\":[");
    for (std::size_t index = 0; index < status.channels.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(channel_status_json(status.channels[index]));
    }
    output.append("],\"processes\":[],\"io_boundaries\":[");
    for (std::size_t index = 0; index < status.io_boundaries.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(boundary_status_json(status.io_boundaries[index]));
    }
    output.append("],\"services\":[");
    for (std::size_t index = 0; index < status.services.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(service_status_json(status.services[index]));
    }
    output.append("],\"operations\":[");
    for (std::size_t index = 0; index < status.operations.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(operation_status_json(status.operations[index]));
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

inline std::string operation_value_response_json(const IntrospectionHandshake &handshake,
                                                 const IntrospectionOperationStatus &operation) {
    std::string output;
    output.append("{\"response\":\"operation_value\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"operation\":");
    output.append(operation_status_json(operation));
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

inline std::string recorder_entity_json(const IntrospectionRecorderEvent &event) {
    std::string output;
    output.append("{\"kind\":");
    output.append(json_string(event.entity_kind));
    output.append(",\"name\":");
    output.append(json_string(event.entity_name));
    if (event.entity_instance) {
        output.append(",\"instance\":");
        output.append(json_string(*event.entity_instance));
    }
    if (event.entity_task) {
        output.append(",\"task\":");
        output.append(json_string(*event.entity_task));
    }
    if (event.entity_type_name) {
        output.append(",\"type_name\":");
        output.append(json_string(*event.entity_type_name));
    }
    output.push_back('}');
    return output;
}

inline std::string recorder_event_json(const IntrospectionRecorderEvent &event) {
    std::string output;
    output.append("{\"schema_version\":");
    output.append(std::to_string(event.schema_version));
    output.append(",\"event_kind\":");
    output.append(json_string(event.event_kind));
    output.append(",\"package\":");
    output.append(json_string(event.package));
    output.append(",\"process\":");
    output.append(json_string(event.process));
    output.append(",\"runtime_pid\":");
    output.append(std::to_string(event.runtime_pid));
    output.append(",\"selfdesc_hash\":");
    output.append(json_string(event.selfdesc_hash));
    output.append(",\"monotonic_ns\":");
    output.append(std::to_string(event.monotonic_ns));
    output.append(",\"wall_unix_ns\":");
    output.append(std::to_string(event.wall_unix_ns));
    output.append(",\"sequence\":");
    output.append(std::to_string(event.sequence));
    output.append(",\"entity\":");
    output.append(recorder_entity_json(event));
    output.append(",\"payload_encoding\":");
    output.append(json_string(event.payload_encoding));
    output.append(",\"payload_schema\":");
    output.append(json_string(event.payload_schema));
    output.append(",\"payload\":");
    output.append(payload_json(std::optional<std::vector<std::uint8_t>>{event.payload}));
    output.push_back('}');
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

inline std::string recorder_value_response_json(const IntrospectionHandshake &handshake,
                                                const IntrospectionRecorderStatus &recorder) {
    std::string output;
    output.append("{\"response\":\"recorder_value\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"recorder\":");
    output.append(recorder_status_json(recorder));
    output.push_back('}');
    return output;
}

inline std::string recorder_events_response_json(
    const IntrospectionHandshake &handshake, const IntrospectionRecorderStatus &recorder,
    const std::vector<IntrospectionRecorderEvent> &events) {
    std::string output;
    output.append("{\"response\":\"recorder_events\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"recorder\":");
    output.append(recorder_status_json(recorder));
    output.append(",\"events\":[");
    for (std::size_t index = 0; index < events.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(recorder_event_json(events[index]));
    }
    output.append("]}");
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
    OperationCancel = 7,
    RecorderStart = 8,
    RecorderStop = 9,
    RecorderDrain = 10,
};

struct ParsedIntrospectionRequest {
    IntrospectionRequestKind kind = IntrospectionRequestKind::Status;
    std::string channel;
    std::string param_name;
    std::string param_value;
    std::string operation_id;
    std::optional<std::string> recorder_output;
    std::vector<std::string> recorder_filters;
    std::optional<std::size_t> recorder_queue_depth;
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

inline std::optional<std::size_t> find_json_unsigned_value(std::string_view input,
                                                           std::string_view key) {
    std::string fragment;
    if (!find_json_value_fragment(input, key, fragment)) {
        return std::nullopt;
    }
    if (fragment.empty() || std::any_of(fragment.begin(), fragment.end(),
                                        [](char byte) { return byte < '0' || byte > '9'; })) {
        return std::nullopt;
    }
    errno = 0;
    char *end = nullptr;
    const auto parsed = std::strtoull(fragment.c_str(), &end, 10);
    if (errno != 0 || end == fragment.c_str() || *end != '\0') {
        return std::nullopt;
    }
    return static_cast<std::size_t>(parsed);
}

inline std::optional<std::vector<std::string>> find_json_string_array_value(std::string_view input,
                                                                            std::string_view key) {
    std::string fragment;
    if (!find_json_value_fragment(input, key, fragment)) {
        return std::nullopt;
    }
    std::size_t index = 0;
    while (index < fragment.size() && json_whitespace(fragment[index])) {
        ++index;
    }
    if (index >= fragment.size() || fragment[index] != '[') {
        return std::nullopt;
    }
    ++index;
    std::vector<std::string> values;
    while (index < fragment.size()) {
        while (index < fragment.size() && json_whitespace(fragment[index])) {
            ++index;
        }
        if (index < fragment.size() && fragment[index] == ']') {
            ++index;
            while (index < fragment.size() && json_whitespace(fragment[index])) {
                ++index;
            }
            return index == fragment.size() ? std::optional<std::vector<std::string>>{values}
                                            : std::nullopt;
        }
        if (index >= fragment.size() || fragment[index] != '"') {
            return std::nullopt;
        }
        ++index;
        std::string value;
        while (index < fragment.size()) {
            const char byte = fragment[index++];
            if (byte == '"') {
                break;
            }
            if (byte != '\\') {
                value.push_back(byte);
                continue;
            }
            if (index >= fragment.size()) {
                return std::nullopt;
            }
            const char escape = fragment[index++];
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
        values.push_back(std::move(value));
        while (index < fragment.size() && json_whitespace(fragment[index])) {
            ++index;
        }
        if (index < fragment.size() && fragment[index] == ',') {
            ++index;
            continue;
        }
        if (index < fragment.size() && fragment[index] == ']') {
            continue;
        }
        return std::nullopt;
    }
    return std::nullopt;
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
    if (command == "operation_cancel") {
        std::string operation_id;
        if (!find_json_string_value(line, "operation_id", operation_id)) {
            return std::nullopt;
        }
        return ParsedIntrospectionRequest{
            IntrospectionRequestKind::OperationCancel, {}, {}, {}, std::move(operation_id)};
    }
    if (command == "recorder_start") {
        std::string output;
        const auto output_end = find_json_string_value(line, "output", output);
        auto filters =
            find_json_string_array_value(line, "filters").value_or(std::vector<std::string>{});
        ParsedIntrospectionRequest request{IntrospectionRequestKind::RecorderStart, {}, {}, {}, {}};
        if (output_end) {
            request.recorder_output = std::move(output);
        }
        request.recorder_filters = std::move(filters);
        request.recorder_queue_depth = find_json_unsigned_value(line, "queue_depth");
        return request;
    }
    if (command == "recorder_stop") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::RecorderStop, {}, {}, {}};
    }
    if (command == "recorder_drain") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::RecorderDrain, {}, {}, {}};
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

enum class ReadLineStatus {
    Line,
    Closed,
    Timeout,
    Error,
};

struct ReadLineResult {
    ReadLineStatus status = ReadLineStatus::Closed;
    std::string line;
};

inline ReadLineResult read_line_result(int fd) {
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
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                return ReadLineResult{ReadLineStatus::Timeout, {}};
            }
            return ReadLineResult{ReadLineStatus::Error, {}};
        }
        if (byte == '\n') {
            return ReadLineResult{ReadLineStatus::Line, std::move(line)};
        }
        line.push_back(byte);
    }
    if (!line.empty()) {
        return ReadLineResult{ReadLineStatus::Line, std::move(line)};
    }
    return ReadLineResult{ReadLineStatus::Closed, {}};
}

inline std::optional<std::string> read_line(int fd) {
    auto result = read_line_result(fd);
    if (result.status == ReadLineStatus::Line) {
        return std::move(result.line);
    }
    return std::nullopt;
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
        std::uint64_t tick_count = 0;
        {
            std::lock_guard<std::mutex> lock(inner_->mutex);
            if (inner_->tick_count != UINT64_MAX) {
                ++inner_->tick_count;
            }
            tick_count = inner_->tick_count;
        }
        record_json_event("runtime", "clock", "flowrt.clock", "clock_event", "clock_tick",
                          "{\"tick_count\":" + std::to_string(tick_count) + "}", std::nullopt);
    }

    /**
     * @brief 启动 runtime recorder。
     */
    IntrospectionRecorderStatus start_recorder(IntrospectionRecorderStart start) const {
        if (start.queue_depth == 0U) {
            start.queue_depth = 1U;
        }
        start.filters = normalize_recorder_filters(std::move(start.filters));

        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->recorder.events.clear();
        inner_->recorder.dropped_count = 0;
        inner_->recorder.bytes_written = 0;
        inner_->recorder.sequence = 0;
        inner_->recorder.output = std::move(start.output);
        inner_->recorder.filters = std::move(start.filters);
        inner_->recorder.queue_depth = start.queue_depth;
        inner_->recorder.package = std::move(start.package);
        inner_->recorder.process = std::move(start.process);
        inner_->recorder.runtime_pid = start.runtime_pid;
        inner_->recorder.self_description_hash = std::move(start.self_description_hash);
        inner_->recorder_enabled.store(true, std::memory_order_release);
        return recorder_status_locked();
    }

    /**
     * @brief 停止 runtime recorder；已暂存事件保留到调用方 drain。
     */
    IntrospectionRecorderStatus stop_recorder() const {
        inner_->recorder_enabled.store(false, std::memory_order_release);
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->recorder.output = std::nullopt;
        inner_->recorder.filters.clear();
        return recorder_status_locked();
    }

    /**
     * @brief 取走 recorder 暂存事件。
     */
    std::vector<IntrospectionRecorderEvent> drain_recorder_events() const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto events = std::move(inner_->recorder.events);
        inner_->recorder.events.clear();
        return events;
    }

    /**
     * @brief 判断指定 channel 是否会被当前 recorder 采集。
     */
    bool recorder_enabled_for_channel(std::string_view name) const {
        if (!inner_->recorder_enabled.load(std::memory_order_acquire)) {
            return false;
        }
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return recorder_filter_matches(inner_->recorder.filters, "channel", name);
    }

    /**
     * @brief 判断指定 descriptor resource 是否会被当前 recorder 采集。
     */
    bool recorder_enabled_for_descriptor(std::string_view resource_id) const {
        if (!inner_->recorder_enabled.load(std::memory_order_acquire)) {
            return false;
        }
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return recorder_filter_matches(inner_->recorder.filters, "descriptor", resource_id);
    }

    /**
     * @brief 记录 frame descriptor / side-channel lease 事件，不复制真实 payload。
     */
    IntrospectionProbeRecord record_frame_descriptor_event(std::string_view name,
                                                           const FrameDescriptor &descriptor,
                                                           FrameLeaseStatus status,
                                                           bool payload_recording) const {
        (void)name;
        if (!inner_->recorder_enabled.load(std::memory_order_acquire)) {
            return IntrospectionProbeRecord{};
        }
        const auto payload = frame_descriptor_payload_json(descriptor, status, payload_recording);
        const auto payload_bytes = string_bytes(payload);
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return record_event_locked("descriptor", descriptor.resource().resource_id,
                                   "descriptor_event", "resource",
                                   descriptor.resource().resource_id, "FrameDescriptor", "json",
                                   "flowrt.descriptor.frame.v1", payload_bytes, std::nullopt);
    }

    /**
     * @brief 按需记录 channel 样本到 recorder。
     */
    IntrospectionProbeRecord try_record_channel_sample_bytes(
        std::string_view name, std::string_view message_type,
        const std::vector<std::uint8_t> &payload,
        std::optional<std::uint64_t> published_at_ms) const {
        return try_record_channel_sample_bytes(
            name, message_type, std::span<const std::uint8_t>{payload.data(), payload.size()},
            published_at_ms);
    }

    /**
     * @brief 按需记录 channel 样本到 recorder。
     */
    IntrospectionProbeRecord try_record_channel_sample_bytes(
        std::string_view name, std::string_view message_type, std::span<const std::uint8_t> payload,
        std::optional<std::uint64_t> published_at_ms) const {
        if (!inner_->recorder_enabled.load(std::memory_order_acquire)) {
            return IntrospectionProbeRecord{};
        }
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return record_event_locked("channel", name, "channel_sample", "channel", name, message_type,
                                   "raw_abi", message_type, payload,
                                   published_at_ms
                                       ? std::optional<std::uint64_t>{*published_at_ms * 1000000U}
                                       : std::nullopt);
    }

    /**
     * @brief 记录 channel 发布的 raw ABI bytes。
     */
    void record_channel_publish_bytes(std::string name, std::string message_type,
                                      std::vector<std::uint8_t> payload,
                                      std::optional<std::uint64_t> published_at_ms) const {
        try_record_channel_sample_bytes(name, message_type, payload, published_at_ms);
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
        const std::string channel_name = name;
        const std::string channel_message_type = message_type;
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
        const auto probe_record = probe.try_record_bytes(payload, published_at_ms);
        const auto recorder_record = try_record_channel_sample_bytes(
            channel_name, channel_message_type, payload, published_at_ms);
        return IntrospectionProbeRecord{
            .recorded = probe_record.recorded || recorder_record.recorded,
            .dropped = probe_record.dropped || recorder_record.dropped,
        };
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
        if ((!probe || !probe->enabled()) && !recorder_enabled_for_channel(name)) {
            return IntrospectionProbeRecord{};
        }
        return try_probe_channel_publish_bytes(
            std::string{name}, std::move(message_type),
            std::span<const std::uint8_t>{reinterpret_cast<const std::uint8_t *>(&value),
                                          sizeof(T)},
            published_at_ms);
    }

    /**
     * @brief 预注册一个 I/O boundary，使其在尚未上报 health 前也出现在 status 中。
     */
    void register_io_boundary(std::string name, std::string component,
                              std::vector<BoundaryResourceStatus> resources) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        BoundaryStatus status{
            .name = std::move(name),
            .component = std::move(component),
            .resources = std::move(resources),
        };
        const auto key = status.name;
        inner_->io_boundaries.try_emplace(key, std::move(status));
    }

    /**
     * @brief 记录 I/O boundary 运行态健康状态。
     */
    void record_io_boundary_health(BoundaryStatus status) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto key = status.name;
        inner_->io_boundaries.insert_or_assign(key, std::move(status));
    }

    /**
     * @brief 预注册一个 service endpoint，使其在尚未收到请求时也出现在 status 中。
     */
    void register_service(std::string name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->services.try_emplace(name, IntrospectionServiceStatus{
                                               .name = name,
                                               .ready = false,
                                           });
    }

    /**
     * @brief 标记预注册 service 已完成 lifecycle startup，可被 readiness gate 视为可用。
     */
    void mark_service_ready(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        if (auto it = inner_->services.find(std::string(name)); it != inner_->services.end()) {
            it->second.ready = true;
        }
    }

    /**
     * @brief 记录 service 运行态健康状态快照。
     */
    void record_service_health(IntrospectionServiceStatus status) const {
        const auto payload = "{\"ready\":" + std::string(status.ready ? "true" : "false") +
                             ",\"in_flight\":" + std::to_string(status.in_flight) +
                             ",\"queued\":" + std::to_string(status.queued) +
                             ",\"total_requests\":" + std::to_string(status.total_requests) + "}";
        const auto name = status.name;
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->services.insert_or_assign(status.name, std::move(status));
        record_event_locked("service", name, "service_event", "service", name, "", "json",
                            "service_health", string_bytes(payload), std::nullopt);
    }

    /**
     * @brief 预注册一个 operation endpoint，使其在尚未收到 goal 时也出现在 status 中。
     */
    void register_operation(std::string name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->operations.try_emplace(name, IntrospectionOperationStatus{
                                                 .name = name,
                                                 .ready = true,
                                             });
    }

    /**
     * @brief 记录 operation 运行态健康状态快照。
     */
    void record_operation_health(IntrospectionOperationStatus status) const {
        const auto payload =
            "{\"ready\":" + std::string(status.ready ? "true" : "false") +
            ",\"running\":" + std::to_string(status.running) +
            ",\"queued\":" + std::to_string(status.queued) + ",\"current_operation_ids\":" +
            detail::json_string_array(status.current_operation_ids) +
            ",\"total_started\":" + std::to_string(status.total_started) +
            ",\"succeeded\":" + std::to_string(status.succeeded_count) +
            ",\"failed\":" + std::to_string(status.failed_count) +
            ",\"canceled\":" + std::to_string(status.canceled_count) +
            ",\"timeout\":" + std::to_string(status.timeout_count) +
            ",\"preempted\":" + std::to_string(status.preempted_count) +
            ",\"last_transition_ms\":" + detail::optional_u64_json(status.last_transition_ms) + "}";
        const auto name = status.name;
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->operations.insert_or_assign(status.name, std::move(status));
        record_event_locked("operation", name, "operation_event", "operation", name, "", "json",
                            "flowrt.operation.status", string_bytes(payload), std::nullopt);
    }

    /**
     * @brief 请求取消指定 operation invocation。
     */
    std::variant<IntrospectionOperationStatus, std::string> cancel_operation(
        std::string_view operation_id) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        for (auto &[name, operation] : inner_->operations) {
            (void)name;
            const auto match =
                std::find(operation.current_operation_ids.begin(),
                          operation.current_operation_ids.end(), std::string{operation_id});
            if (match == operation.current_operation_ids.end()) {
                continue;
            }
            if (operation.running == 0U) {
                return "FlowRT operation `" + std::string{operation_id} + "` is already finished";
            }
            operation.current_operation_ids.erase(match);
            --operation.running;
            if (operation.canceled_count != UINT64_MAX) {
                ++operation.canceled_count;
            }
            operation.last_transition_ms = detail::unix_time_ms();
            record_event_locked(
                "operation", operation.name, "operation_event", "operation", operation.name, "",
                "json", "operation_cancel",
                string_bytes("{\"operation_id\":" + detail::json_string(operation_id) + "}"),
                std::nullopt);
            return operation;
        }
        record_event_locked(
            "operation", operation_id, "operation_event", "operation", operation_id, "", "json",
            "operation_cancel_error",
            string_bytes("{\"operation_id\":" + detail::json_string(operation_id) + "}"),
            std::nullopt);
        return "unknown FlowRT operation `" + std::string{operation_id} + "`";
    }

    /**
     * @brief 记录 task 调度健康快照。
     */
    void record_task_health(IntrospectionTaskHealth health) const {
        const auto payload =
            "{\"lane\":" + detail::json_string(health.lane) +
            ",\"deadline_missed\":" + std::to_string(health.deadline_missed) +
            ",\"stale_input\":" + std::to_string(health.stale_input) +
            ",\"backpressure\":" + std::to_string(health.backpressure) +
            ",\"overflow\":" + std::to_string(health.overflow) +
            ",\"fairness_violations\":" + std::to_string(health.fairness_violations) +
            ",\"run_count\":" + std::to_string(health.run_count) +
            ",\"success_count\":" + std::to_string(health.success_count) +
            ",\"consecutive_failures\":" + std::to_string(health.consecutive_failures) +
            ",\"last_run_ms\":" + detail::optional_u64_json(health.last_run_ms) +
            ",\"last_success_ms\":" + detail::optional_u64_json(health.last_success_ms) + "}";
        const auto name = health.name;
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->tasks.insert_or_assign(health.name, std::move(health));
        record_event_locked("scheduler", name, "scheduler_event", "task", name, "", "json",
                            "flowrt.scheduler.task_health", string_bytes(payload), std::nullopt);
    }

    /**
     * @brief 记录 lane 调度健康快照。
     */
    void record_lane_health(IntrospectionLaneHealth health) const {
        const auto payload =
            "{\"queue_depth\":" + std::to_string(health.queue_depth) +
            ",\"dispatched_count\":" + std::to_string(health.dispatched_count) +
            ",\"fairness_violations\":" + std::to_string(health.fairness_violations) + "}";
        const auto name = health.name;
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->lanes.insert_or_assign(health.name, std::move(health));
        record_event_locked("scheduler", name, "scheduler_event", "lane", name, "", "json",
                            "flowrt.scheduler.lane_health", string_bytes(payload), std::nullopt);
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
        snapshot.io_boundaries.reserve(inner_->io_boundaries.size());
        for (const auto &[name, boundary] : inner_->io_boundaries) {
            snapshot.io_boundaries.push_back(boundary);
        }
        snapshot.services.reserve(inner_->services.size());
        for (const auto &[name, service] : inner_->services) {
            snapshot.services.push_back(service);
        }
        snapshot.operations.reserve(inner_->operations.size());
        for (const auto &[name, operation] : inner_->operations) {
            snapshot.operations.push_back(operation);
        }
        snapshot.tasks.reserve(inner_->tasks.size());
        for (const auto &[name, task] : inner_->tasks) {
            snapshot.tasks.push_back(task);
        }
        snapshot.lanes.reserve(inner_->lanes.size());
        for (const auto &[name, lane] : inner_->lanes) {
            snapshot.lanes.push_back(lane);
        }
        snapshot.recorder = recorder_status_locked();
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
        std::optional<IntrospectionParamStatus> snapshot;
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto param = inner_->params.find(std::string{name});
        if (param == inner_->params.end()) {
            record_event_locked(
                "param", name, "param_event", "param", name, "", "json", "param_get_error",
                string_bytes("{\"name\":" + detail::json_string(name) + "}"), std::nullopt);
            return std::nullopt;
        }
        snapshot = param_status(param->first, param->second);
        record_event_locked("param", name, "param_event", "param", name, "", "json", "param_get",
                            string_bytes("{\"name\":" + detail::json_string(name) + "}"),
                            std::nullopt);
        return snapshot;
    }

    /**
     * @brief 设置参数 pending 值。
     */
    std::variant<IntrospectionParamStatus, std::string> set_param_pending(std::string_view name,
                                                                          std::string value) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto param = inner_->params.find(std::string{name});
        if (param == inner_->params.end()) {
            record_event_locked(
                "param", name, "param_event", "param", name, "", "json", "param_set_error",
                string_bytes("{\"name\":" + detail::json_string(name) + "}"), std::nullopt);
            return "unknown FlowRT parameter `" + std::string{name} + "`";
        }
        if (param->second.update != "on_tick") {
            record_event_locked(
                "param", name, "param_event", "param", name, "", "json", "param_set_error",
                string_bytes("{\"name\":" + detail::json_string(name) + "}"), std::nullopt);
            return "FlowRT parameter `" + std::string{name} + "` is startup-only";
        }
        if (const auto error = validate_param_json_value(param->first, param->second, value)) {
            record_event_locked(
                "param", name, "param_event", "param", name, "", "json", "param_set_error",
                string_bytes("{\"name\":" + detail::json_string(name) + "}"), std::nullopt);
            return *error;
        }
        param->second.pending = std::move(value);
        record_event_locked(
            "param", name, "param_event", "param", name, "", "json", "param_set_pending",
            string_bytes("{\"name\":" + detail::json_string(name) + "}"), std::nullopt);
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
        record_event_locked(
            "param", name, "param_event", "param", name, "", "json", "param_applied",
            string_bytes("{\"name\":" + detail::json_string(name) + "}"), std::nullopt);
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

    struct RecorderState {
        std::optional<std::string> output;
        std::vector<std::string> filters;
        std::size_t queue_depth = 1024;
        std::uint64_t dropped_count = 0;
        std::uint64_t bytes_written = 0;
        std::uint64_t sequence = 0;
        std::vector<IntrospectionRecorderEvent> events;
        std::string package;
        std::string process;
        std::uint32_t runtime_pid = 0;
        std::string self_description_hash;
    };

    struct Inner {
        std::atomic_bool recorder_enabled{false};
        std::mutex mutex;
        std::uint64_t tick_count = 0;
        std::optional<std::string> self_description_json;
        std::map<std::string, ChannelState> channels;
        std::map<std::string, ParamState> params;
        std::map<std::string, BoundaryStatus> io_boundaries;
        std::map<std::string, IntrospectionServiceStatus> services;
        std::map<std::string, IntrospectionOperationStatus> operations;
        std::map<std::string, IntrospectionTaskHealth> tasks;
        std::map<std::string, IntrospectionLaneHealth> lanes;
        RecorderState recorder;
    };

    IntrospectionRecorderStatus recorder_status_locked() const {
        return IntrospectionRecorderStatus{
            .enabled = inner_->recorder_enabled.load(std::memory_order_acquire),
            .output = inner_->recorder.output,
            .dropped_count = inner_->recorder.dropped_count,
            .bytes_written = inner_->recorder.bytes_written,
            .active_filters = inner_->recorder.filters,
            .queued_events = static_cast<std::uint64_t>(inner_->recorder.events.size()),
        };
    }

    static std::vector<std::uint8_t> string_bytes(std::string_view value) {
        return std::vector<std::uint8_t>{value.begin(), value.end()};
    }

    static std::optional<std::string> instance_from_endpoint(std::string_view name) {
        const auto dot = name.find('.');
        if (dot == std::string_view::npos) {
            return std::nullopt;
        }
        return std::string{name.substr(0, dot)};
    }

    static std::string trim_recorder_filter(std::string filter) {
        while (!filter.empty() && detail::json_whitespace(filter.front())) {
            filter.erase(filter.begin());
        }
        while (!filter.empty() && detail::json_whitespace(filter.back())) {
            filter.pop_back();
        }
        return filter;
    }

    static std::vector<std::string> normalize_recorder_filters(std::vector<std::string> filters) {
        std::vector<std::string> normalized;
        normalized.reserve(filters.size());
        for (auto &filter : filters) {
            auto trimmed = trim_recorder_filter(std::move(filter));
            if (!trimmed.empty()) {
                normalized.push_back(std::move(trimmed));
            }
        }
        if (normalized.empty()) {
            normalized.push_back("all");
        }
        std::sort(normalized.begin(), normalized.end());
        normalized.erase(std::unique(normalized.begin(), normalized.end()), normalized.end());
        return normalized;
    }

    static bool recorder_filter_matches(const std::vector<std::string> &filters,
                                        std::string_view kind, std::string_view name) {
        const std::string exact = std::string{kind} + ":" + std::string{name};
        return std::any_of(filters.begin(), filters.end(), [&](const std::string &filter) {
            return filter == "all" || filter == kind || filter == exact;
        });
    }

    static std::string frame_lease_status_name(FrameLeaseStatus status) {
        switch (status) {
            case FrameLeaseStatus::Attached:
                return "attached";
            case FrameLeaseStatus::Acquired:
                return "acquired";
            case FrameLeaseStatus::Released:
                return "released";
            case FrameLeaseStatus::Expired:
                return "expired";
            case FrameLeaseStatus::GenerationMismatch:
                return "generation_mismatch";
            case FrameLeaseStatus::Error:
                return "error";
        }
        return "error";
    }

    static std::string frame_metadata_json(const FrameMetadata &metadata) {
        std::string output;
        output.push_back('{');
        bool first = true;
        for (const auto &[key, value] : metadata) {
            if (!first) {
                output.push_back(',');
            }
            first = false;
            output.append(detail::json_string(key));
            output.push_back(':');
            output.append(detail::json_string(value));
        }
        output.push_back('}');
        return output;
    }

    static std::string frame_descriptor_payload_json(const FrameDescriptor &descriptor,
                                                     FrameLeaseStatus status,
                                                     bool payload_recording) {
        std::string output;
        output.append("{\"resource_id\":");
        output.append(detail::json_string(descriptor.resource().resource_id));
        output.append(",\"slot\":");
        output.append(detail::json_string(descriptor.resource().slot));
        output.append(",\"generation\":");
        output.append(std::to_string(descriptor.resource().generation));
        output.append(",\"size_bytes\":");
        output.append(std::to_string(descriptor.size_bytes()));
        output.append(",\"format\":");
        output.append(detail::json_string(descriptor.format()));
        output.append(",\"encoding\":");
        output.append(detail::json_string(descriptor.encoding()));
        output.append(",\"metadata\":");
        output.append(frame_metadata_json(descriptor.metadata()));
        output.append(",\"status\":");
        output.append(detail::json_string(frame_lease_status_name(status)));
        output.append(",\"payload_recording\":");
        output.append(payload_recording ? "true" : "false");
        output.push_back('}');
        return output;
    }

    IntrospectionProbeRecord record_event_locked(
        std::string_view filter_kind, std::string_view filter_name, std::string_view event_kind,
        std::string_view entity_kind, std::string_view entity_name, std::string_view message_type,
        std::string_view payload_encoding, std::string_view payload_schema,
        std::span<const std::uint8_t> payload, std::optional<std::uint64_t> monotonic_ns) const {
        if (!inner_->recorder_enabled.load(std::memory_order_acquire)) {
            return IntrospectionProbeRecord{};
        }
        if (!recorder_filter_matches(inner_->recorder.filters, filter_kind, filter_name)) {
            return IntrospectionProbeRecord{};
        }
        if (inner_->recorder.events.size() >= inner_->recorder.queue_depth) {
            if (inner_->recorder.dropped_count != UINT64_MAX) {
                ++inner_->recorder.dropped_count;
            }
            return IntrospectionProbeRecord{.recorded = false, .dropped = true};
        }
        const auto sequence = inner_->recorder.sequence;
        if (inner_->recorder.sequence != UINT64_MAX) {
            ++inner_->recorder.sequence;
        }
        IntrospectionRecorderEvent event{
            .schema_version = 1,
            .event_kind = std::string{event_kind},
            .package = inner_->recorder.package,
            .process = inner_->recorder.process,
            .runtime_pid = inner_->recorder.runtime_pid,
            .selfdesc_hash = inner_->recorder.self_description_hash,
            .monotonic_ns = monotonic_ns.value_or(0U),
            .wall_unix_ns = detail::unix_time_ns(),
            .sequence = sequence,
            .entity_kind = std::string{entity_kind},
            .entity_name = std::string{entity_name},
            .entity_instance = instance_from_endpoint(entity_name),
            .entity_task = entity_kind == "task"
                               ? std::optional<std::string>{std::string{entity_name}}
                               : std::nullopt,
            .entity_type_name = message_type.empty()
                                    ? std::nullopt
                                    : std::optional<std::string>{std::string{message_type}},
            .payload_encoding = std::string{payload_encoding},
            .payload_schema = std::string{payload_schema},
            .payload = std::vector<std::uint8_t>{payload.begin(), payload.end()},
        };
        inner_->recorder.bytes_written =
            inner_->recorder.bytes_written >
                    UINT64_MAX - static_cast<std::uint64_t>(event.payload.size())
                ? UINT64_MAX
                : inner_->recorder.bytes_written + static_cast<std::uint64_t>(event.payload.size());
        inner_->recorder.events.push_back(std::move(event));
        return IntrospectionProbeRecord{.recorded = true, .dropped = false};
    }

    IntrospectionProbeRecord record_json_event(
        std::string_view filter_kind, std::string_view entity_kind, std::string_view entity_name,
        std::string_view event_kind, std::string_view payload_schema, std::string_view payload_json,
        std::optional<std::uint64_t> monotonic_ns) const {
        if (!inner_->recorder_enabled.load(std::memory_order_acquire)) {
            return IntrospectionProbeRecord{};
        }
        const auto payload = string_bytes(payload_json);
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return record_event_locked(filter_kind, entity_name, event_kind, entity_kind, entity_name,
                                   "", "json", payload_schema, payload, monotonic_ns);
    }

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
        if (param.min &&
            compare_json_number_by_type(param.type, value, *param.min).value_or(0) < 0) {
            return "FlowRT parameter `" + name + "` is below minimum";
        }
        if (param.max &&
            compare_json_number_by_type(param.type, value, *param.max).value_or(0) > 0) {
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
            const auto number = parse_json_u64(value);
            if (!number.has_value()) {
                return false;
            }
            if (type == "u8") {
                return *number <= std::numeric_limits<std::uint8_t>::max();
            }
            if (type == "u16") {
                return *number <= std::numeric_limits<std::uint16_t>::max();
            }
            if (type == "u32") {
                return *number <= std::numeric_limits<std::uint32_t>::max();
            }
            return true;
        }
        if (type == "i8" || type == "i16" || type == "i32" || type == "i64") {
            const auto number = parse_json_i64(value);
            if (!number.has_value()) {
                return false;
            }
            if (type == "i8") {
                return *number >= std::numeric_limits<std::int8_t>::min() &&
                       *number <= std::numeric_limits<std::int8_t>::max();
            }
            if (type == "i16") {
                return *number >= std::numeric_limits<std::int16_t>::min() &&
                       *number <= std::numeric_limits<std::int16_t>::max();
            }
            if (type == "i32") {
                return *number >= std::numeric_limits<std::int32_t>::min() &&
                       *number <= std::numeric_limits<std::int32_t>::max();
            }
            return true;
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
        if (parsed != parsed || parsed == std::numeric_limits<double>::infinity() ||
            parsed == -std::numeric_limits<double>::infinity()) {
            return std::nullopt;
        }
        return parsed;
    }

    static std::optional<std::uint64_t> parse_json_u64(std::string_view value) {
        if (value.empty() || value.front() == '-') {
            return std::nullopt;
        }
        std::string owned{value};
        char *end = nullptr;
        errno = 0;
        const auto parsed = std::strtoull(owned.c_str(), &end, 10);
        if (errno != 0 || end == owned.c_str() || *end != '\0') {
            return std::nullopt;
        }
        return static_cast<std::uint64_t>(parsed);
    }

    static std::optional<std::int64_t> parse_json_i64(std::string_view value) {
        std::string owned{value};
        char *end = nullptr;
        errno = 0;
        const auto parsed = std::strtoll(owned.c_str(), &end, 10);
        if (errno != 0 || end == owned.c_str() || *end != '\0') {
            return std::nullopt;
        }
        return static_cast<std::int64_t>(parsed);
    }

    static std::optional<int> compare_json_number_by_type(std::string_view type,
                                                          std::string_view left,
                                                          std::string_view right) {
        if (type == "u8" || type == "u16" || type == "u32" || type == "u64") {
            const auto left_number = parse_json_u64(left);
            const auto right_number = parse_json_u64(right);
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
        if (type == "i8" || type == "i16" || type == "i32" || type == "i64") {
            const auto left_number = parse_json_i64(left);
            const auto right_number = parse_json_i64(right);
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

class IntrospectionClientPermit {
   public:
    explicit IntrospectionClientPermit(std::shared_ptr<std::atomic_size_t> active) noexcept
        : active_(std::move(active)) {}
    IntrospectionClientPermit(const IntrospectionClientPermit &) = delete;
    auto operator=(const IntrospectionClientPermit &) -> IntrospectionClientPermit & = delete;

    IntrospectionClientPermit(IntrospectionClientPermit &&other) noexcept
        : active_(std::move(other.active_)) {}

    auto operator=(IntrospectionClientPermit &&other) noexcept -> IntrospectionClientPermit & {
        if (this != std::addressof(other)) {
            release();
            active_ = std::move(other.active_);
        }
        return *this;
    }

    ~IntrospectionClientPermit() { release(); }

   private:
    void release() noexcept {
        if (active_) {
            active_->fetch_sub(1U, std::memory_order_acq_rel);
            active_.reset();
        }
    }

    std::shared_ptr<std::atomic_size_t> active_;
};

inline std::optional<IntrospectionClientPermit> try_acquire_introspection_client_permit(
    const std::shared_ptr<std::atomic_size_t> &active,
    std::size_t limit = MAX_INTROSPECTION_CLIENT_THREADS) {
    limit = std::max<std::size_t>(1U, limit);
    auto current = active->load(std::memory_order_acquire);
    while (true) {
        if (current >= limit) {
            return std::nullopt;
        }
        if (active->compare_exchange_weak(current, current + 1U, std::memory_order_acq_rel,
                                          std::memory_order_acquire)) {
            return IntrospectionClientPermit{active};
        }
    }
}

inline void handle_introspection_connection(
    int client_fd, const IntrospectionHandshake &handshake, const IntrospectionState &state,
    const std::shared_ptr<std::atomic_bool> &stop,
    std::optional<IntrospectionClientPermit> initial_permit,
    const std::shared_ptr<std::atomic_size_t> &active_observers) {
    const auto line = read_line(client_fd);
    std::string response;
    if (!line) {
        response = error_response_json(handshake, "invalid FlowRT introspection request");
    } else if (const auto request = parse_introspection_request(*line)) {
        initial_permit.reset();
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
                auto observer_permit = try_acquire_introspection_client_permit(
                    active_observers, MAX_INTROSPECTION_OBSERVERS);
                if (!observer_permit.has_value()) {
                    response = error_response_json(
                        handshake, "FlowRT introspection observe connection limit reached");
                    break;
                }
                auto guard = state.observe_channel(request->channel);
                const auto channel = state.channel_status(request->channel);
                if (!guard || !channel) {
                    response = error_response_json(handshake, "unknown FlowRT channel");
                    break;
                }
                response = observe_ready_response_json(handshake, *channel);
                response.push_back('\n');
                (void)write_all(client_fd, response);
                while (!stop->load(std::memory_order_relaxed)) {
                    const auto keepalive = read_line_result(client_fd);
                    switch (keepalive.status) {
                        case ReadLineStatus::Line:
                        case ReadLineStatus::Timeout:
                            continue;
                        case ReadLineStatus::Closed:
                        case ReadLineStatus::Error:
                            return;
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
            case IntrospectionRequestKind::OperationCancel: {
                const auto result = state.cancel_operation(request->operation_id);
                if (std::holds_alternative<IntrospectionOperationStatus>(result)) {
                    response = operation_value_response_json(
                        handshake, std::get<IntrospectionOperationStatus>(result));
                } else {
                    response = error_response_json(handshake, std::get<std::string>(result));
                }
                break;
            }
            case IntrospectionRequestKind::RecorderStart: {
                const auto recorder = state.start_recorder(IntrospectionRecorderStart{
                    .output = request->recorder_output,
                    .filters = request->recorder_filters,
                    .queue_depth = request->recorder_queue_depth.value_or(1024U),
                    .package = handshake.package,
                    .process = handshake.process,
                    .runtime_pid = handshake.pid,
                    .self_description_hash = handshake.self_description_hash,
                });
                response = recorder_value_response_json(handshake, recorder);
                break;
            }
            case IntrospectionRequestKind::RecorderStop:
                response = recorder_value_response_json(handshake, state.stop_recorder());
                break;
            case IntrospectionRequestKind::RecorderDrain: {
                const auto events = state.drain_recorder_events();
                response =
                    recorder_events_response_json(handshake, state.status().recorder, events);
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
    auto active_clients = std::make_shared<std::atomic_size_t>(0U);
    auto active_observers = std::make_shared<std::atomic_size_t>(0U);
    auto thread_stop = stop;
    std::thread handle;
    try {
        handle = std::thread([listener_fd, thread_stop, handshake = std::move(handshake),
                              state = std::move(state), active_clients = std::move(active_clients),
                              active_observers = std::move(active_observers)]() mutable {
            while (!thread_stop->load(std::memory_order_relaxed)) {
                const int client_fd = ::accept(listener_fd, nullptr, nullptr);
                if (client_fd >= 0) {
                    detail::set_socket_timeout(client_fd);
                    auto permit = detail::try_acquire_introspection_client_permit(active_clients);
                    if (!permit.has_value()) {
                        auto response = detail::error_response_json(
                            handshake, "FlowRT introspection connection limit reached");
                        response.push_back('\n');
                        (void)detail::write_all(client_fd, response);
                        ::close(client_fd);
                        continue;
                    }
                    try {
                        std::thread([client_fd, handshake, state, thread_stop, active_observers,
                                     permit = std::move(permit)]() mutable {
                            detail::handle_introspection_connection(client_fd, handshake, state,
                                                                    thread_stop, std::move(permit),
                                                                    active_observers);
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
