#pragma once

#include <chrono>
#include <cstdint>
#include <flowrt/channels.hpp>
#include <optional>
#include <string>
#include <unistd.h>
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

/**
 * @brief island boundary input 注入结果。
 */
struct IntrospectionBoundaryPublishStatus {
    std::string endpoint;
    std::string message_type;
    std::uint64_t revision = 0;
    std::optional<std::uint64_t> published_at_ms;
};

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
    std::optional<std::string> current_state;
    std::optional<std::string> current_owner;
    std::optional<std::uint64_t> current_deadline_ms;
    std::optional<std::string> last_event;
    std::optional<std::string> last_error;
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
    bool inflight = false;
    std::optional<std::uint64_t> scheduled_time_ms;
    std::optional<std::uint64_t> observed_time_ms;
    std::optional<std::uint64_t> lateness_ms;
    std::optional<std::uint64_t> missed_periods;
    std::optional<bool> overrun;
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
 * @brief runtime 时钟模型状态。
 */
struct IntrospectionClockStatus {
    std::string source = "realtime";
    std::optional<std::uint64_t> tick_time_ms;
    std::string unit = "ms";
    std::string field = "tick_time_ms";
};

/**
 * @brief 抽象 resource 的运行态 readiness/health 状态。
 *
 * state 使用 ready / pending / degraded / failed / unknown 字符串。
 */
struct IntrospectionResourceStatus {
    std::string name;
    std::string capability;
    std::optional<std::string> access;
    std::string state = "unknown";
    bool required = false;
    std::optional<std::string> readiness;
    std::optional<std::string> health;
    std::optional<std::string> on_failure;
    std::optional<std::string> contract_status;
    std::optional<bool> satisfied;
    std::optional<std::string> provider;
    std::optional<std::string> provider_scope;
    std::optional<std::string> provider_readiness_source;
    std::optional<std::string> provider_health_source;
    std::optional<std::string> diagnostic;
    std::optional<std::string> suggestion;
    std::optional<std::string> source;
    std::optional<std::string> owner_process;
    std::optional<std::string> last_error;
    std::optional<std::uint64_t> updated_unix_ms;
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
    std::string apply_state = "applied";
    std::optional<std::string> last_reject_reason;
    std::optional<std::uint64_t> updated_unix_ms;
    std::optional<std::string> min;
    std::optional<std::string> max;
    std::vector<std::string> choices;
};

/**
 * @brief 结构化诊断中的单个指标。
 *
 * value 使用合法 JSON 片段，避免 C++ runtime 引入 JSON DOM 依赖。
 */
struct IntrospectionDiagnosticMetric {
    std::string name;
    std::string value;
};

/**
 * @brief runtime live status 的统一诊断项。
 */
struct IntrospectionDiagnostic {
    std::string category;
    std::string entity_kind;
    std::string entity_id;
    std::string state;
    std::string severity;
    std::optional<std::string> reason;
    std::optional<std::string> suggestion;
    std::optional<std::uint64_t> updated_unix_ms;
    std::optional<std::uint64_t> observed_ms;
    std::vector<IntrospectionDiagnosticMetric> metrics;
};

/**
 * @brief 运行态 status 快照。
 */
struct IntrospectionStatus {
    std::uint64_t tick_count = 0;
    IntrospectionClockStatus clock;
    std::vector<IntrospectionChannelStatus> channels;
    std::vector<IntrospectionResourceStatus> resources;
    std::vector<BoundaryStatus> io_boundaries;
    std::vector<IntrospectionParamStatus> params;
    std::vector<IntrospectionServiceStatus> services;
    std::vector<IntrospectionOperationStatus> operations;
    std::vector<IntrospectionTaskHealth> tasks;
    std::vector<IntrospectionLaneHealth> lanes;
    IntrospectionRecorderStatus recorder;
    std::vector<IntrospectionDiagnostic> diagnostics;
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

}  // namespace flowrt
