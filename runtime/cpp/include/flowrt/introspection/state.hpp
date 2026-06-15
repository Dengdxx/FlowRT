#pragma once

#include <algorithm>
#include <atomic>
#include <cerrno>
#include <cstdint>
#include <cstring>
#include <flowrt/core.hpp>
#include <flowrt/introspection/json.hpp>
#include <flowrt/introspection/probe.hpp>
#include <flowrt/wire.hpp>
#include <functional>
#include <limits>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <variant>
#include <vector>

namespace flowrt {

/**
 * @brief runtime shell 可共享更新的 introspection live 状态。
 */
class IntrospectionState {
   public:
    using BoundaryInputHandler = std::function<std::variant<std::uint64_t, std::string>(
        std::span<const std::uint8_t>, std::optional<std::uint64_t>)>;

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
     * @brief 注册 island boundary input 的底层注入 handler。
     */
    void register_boundary_input_handler(std::string endpoint, std::string message_type,
                                         BoundaryInputHandler handler) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->boundary_inputs.insert_or_assign(
            std::move(endpoint), BoundaryInputState{.message_type = std::move(message_type),
                                                    .handler = std::move(handler)});
    }

    /**
     * @brief 注册 typed `BoundaryInput<T>`，供 `flowrt pub` 注入 canonical payload。
     */
    template <CanonicalTransportMessage T>
    void register_boundary_input(std::string endpoint, std::string message_type,
                                 BoundaryInput<T> &input) const {
        const auto endpoint_for_error = endpoint;
        register_boundary_input_handler(
            std::move(endpoint), std::move(message_type),
            [&input, endpoint_for_error](std::span<const std::uint8_t> payload,
                                         std::optional<std::uint64_t> timestamp) mutable
            -> std::variant<std::uint64_t, std::string> {
                try {
                    auto value = detail::decode_frame<T>(payload);
                    return timestamp ? input.inject_at(std::move(value), *timestamp)
                                     : input.inject(std::move(value));
                } catch (const WireCodecError &error) {
                    return "decode FlowRT boundary input `" + endpoint_for_error +
                           "`: " + error.what();
                }
            });
    }

    /**
     * @brief 向已注册的 island boundary input 注入 canonical Message ABI payload。
     */
    std::variant<IntrospectionBoundaryPublishStatus, std::string> publish_boundary_input(
        std::string_view endpoint, std::span<const std::uint8_t> payload,
        std::optional<std::uint64_t> published_at_ms) const {
        BoundaryInputState boundary;
        {
            std::lock_guard<std::mutex> lock(inner_->mutex);
            const auto found = inner_->boundary_inputs.find(std::string{endpoint});
            if (found == inner_->boundary_inputs.end()) {
                return "unknown FlowRT boundary input `" + std::string{endpoint} + "`";
            }
            boundary = found->second;
        }
        const auto revision = boundary.handler(payload, published_at_ms);
        if (std::holds_alternative<std::string>(revision)) {
            return std::get<std::string>(revision);
        }
        // recorder 开启且覆盖该 endpoint 时，把注入作为 canonical frame channel sample 记录，
        // 作为确定性回放的边界激励来源——回放只重放这些外部激励，由 runtime 重新推导下游 channel。
        if (recorder_enabled_for_channel(endpoint)) {
            (void)try_record_channel_sample_frame_bytes(endpoint, boundary.message_type, payload,
                                                        published_at_ms);
        }
        return IntrospectionBoundaryPublishStatus{
            .endpoint = std::string{endpoint},
            .message_type = boundary.message_type,
            .revision = std::get<std::uint64_t>(revision),
            .published_at_ms = published_at_ms,
        };
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
    void record_tick() const { record_tick(0U, std::string_view{"realtime"}); }

    /**
     * @brief 按统一 runtime 毫秒时间模型记录 scheduler tick。
     */
    void record_tick(std::uint64_t tick_time_ms, std::string_view time_source) const {
        std::uint64_t tick_count = 0;
        {
            std::lock_guard<std::mutex> lock(inner_->mutex);
            if (inner_->tick_count != UINT64_MAX) {
                ++inner_->tick_count;
            }
            tick_count = inner_->tick_count;
            inner_->clock = IntrospectionClockStatus{
                .source = std::string{time_source},
                .tick_time_ms = tick_time_ms,
                .unit = "ms",
                .field = "tick_time_ms",
            };
        }
        record_json_event("runtime", "clock", "flowrt.clock", "clock_event", "clock_tick",
                          "{\"tick_count\":" + std::to_string(tick_count) +
                              ",\"tick_time_ms\":" + std::to_string(tick_time_ms) +
                              ",\"time_source\":" + detail::json_string(time_source) +
                              ",\"time_unit\":\"ms\"}",
                          tick_time_ms * 1000000U);
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
     * @brief 按需以 canonical frame 编码记录 channel 样本。
     *
     * boundary input 注入的是 canonical Message ABI frame，作为确定性回放的边界激励来源，
     * 编码标记为 `canonical_frame`，与 Rust try_record_channel_sample_frame_bytes 对齐。
     */
    IntrospectionProbeRecord try_record_channel_sample_frame_bytes(
        std::string_view name, std::string_view message_type, std::span<const std::uint8_t> payload,
        std::optional<std::uint64_t> published_at_ms) const {
        if (!inner_->recorder_enabled.load(std::memory_order_acquire)) {
            return IntrospectionProbeRecord{};
        }
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return record_event_locked("channel", name, "channel_sample", "channel", name, message_type,
                                   "canonical_frame", message_type, payload,
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
     * @brief 预注册一个抽象 resource，使未知运行态也可被 status 发现。
     */
    void register_resource(IntrospectionResourceStatus status) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto key = status.name;
        inner_->resources.try_emplace(key, std::move(status));
    }

    /**
     * @brief 记录抽象 resource 的最新运行态。
     */
    void record_resource_status(IntrospectionResourceStatus status) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto key = status.name;
        inner_->resources.insert_or_assign(key, std::move(status));
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
     * @brief 注册 operation cancel control hook。
     */
    void register_operation_cancel_handler(
        std::string name,
        std::function<std::variant<IntrospectionOperationStatus, std::string>(std::string_view)>
            handler) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->operation_cancel_handlers.insert_or_assign(std::move(name), std::move(handler));
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
            ",\"preempted\":" + std::to_string(status.preempted_count) + ",\"state\":" +
            (status.current_state ? detail::json_string(*status.current_state) : "null") +
            ",\"owner\":" +
            (status.current_owner ? detail::json_string(*status.current_owner) : "null") +
            ",\"deadline_ms\":" + detail::optional_u64_json(status.current_deadline_ms) +
            ",\"last_event\":" +
            (status.last_event ? detail::json_string(*status.last_event) : "null") +
            ",\"last_error\":" +
            (status.last_error ? detail::json_string(*status.last_error) : "null") +
            ",\"last_transition_ms\":" + detail::optional_u64_json(status.last_transition_ms) + "}";
        const auto name = status.name;
        const auto monotonic_ns =
            status.last_transition_ms.has_value()
                ? std::optional<std::uint64_t>{*status.last_transition_ms * 1000000U}
                : std::nullopt;
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->operations.insert_or_assign(status.name, std::move(status));
        record_event_locked("operation", name, "operation_event", "operation", name, "", "json",
                            "flowrt.operation.status", string_bytes(payload), monotonic_ns);
    }

    /**
     * @brief 记录 operation 状态转换事件。
     */
    void record_operation_transition(std::string_view operation, std::string_view operation_id,
                                     std::string_view state, std::optional<std::string_view> owner,
                                     std::optional<std::uint64_t> deadline_ms) const {
        const auto now = detail::unix_time_ms();
        const auto operation_name = std::string{operation};
        const auto id = std::string{operation_id};
        const auto state_name = std::string{state};
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &entry = inner_->operations[operation_name];
        entry.name = operation_name;
        entry.ready = true;
        entry.current_state = state_name;
        entry.current_owner =
            owner ? std::optional<std::string>{std::string{*owner}} : std::nullopt;
        entry.current_deadline_ms = deadline_ms;
        entry.last_event = "flowrt.operation.state_changed";
        entry.last_error = std::nullopt;
        entry.last_transition_ms = now;
        if (state != "idle" && state != "succeeded" && state != "failed" && state != "cancelled" &&
            state != "timed_out") {
            if (std::find(entry.current_operation_ids.begin(), entry.current_operation_ids.end(),
                          id) == entry.current_operation_ids.end()) {
                entry.current_operation_ids.push_back(id);
            }
            entry.running = 1;
        } else {
            entry.current_operation_ids.erase(std::remove(entry.current_operation_ids.begin(),
                                                          entry.current_operation_ids.end(), id),
                                              entry.current_operation_ids.end());
            entry.running = 0;
        }
        const auto payload = "{\"operation_id\":" + detail::json_string(id) +
                             ",\"state\":" + detail::json_string(state_name) + ",\"owner\":" +
                             (owner ? detail::json_string(*owner) : std::string{"null"}) +
                             ",\"deadline_ms\":" + detail::optional_u64_json(deadline_ms) +
                             ",\"transition_ms\":" + std::to_string(now) + "}";
        record_event_locked("operation", operation_name, "operation_event", "operation",
                            operation_name, "", "json", "flowrt.operation.state_changed",
                            string_bytes(payload), std::nullopt);
    }

    /**
     * @brief 记录 operation progress 事件。
     */
    void record_operation_progress(std::string_view operation, std::string_view operation_id,
                                   std::uint64_t sequence) const {
        const auto operation_name = std::string{operation};
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &entry = inner_->operations[operation_name];
        entry.name = operation_name;
        entry.last_event = "flowrt.operation.progress";
        const auto payload = "{\"operation_id\":" + detail::json_string(operation_id) +
                             ",\"sequence\":" + std::to_string(sequence) + "}";
        record_event_locked("operation", operation_name, "operation_event", "operation",
                            operation_name, "", "json", "flowrt.operation.progress",
                            string_bytes(payload), std::nullopt);
    }

    /**
     * @brief 记录 operation result/error 事件。
     */
    void record_operation_result(std::string_view operation, std::string_view operation_id,
                                 std::string_view result,
                                 std::optional<std::string_view> error) const {
        const auto operation_name = std::string{operation};
        const auto event = (error.has_value() || result == "failed")
                               ? std::string{"flowrt.operation.error"}
                               : std::string{"flowrt.operation.result"};
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &entry = inner_->operations[operation_name];
        entry.name = operation_name;
        entry.last_event = event;
        entry.last_error = error ? std::optional<std::string>{std::string{*error}} : std::nullopt;
        const auto payload = "{\"operation_id\":" + detail::json_string(operation_id) +
                             ",\"result\":" + detail::json_string(result) + ",\"error\":" +
                             (error ? detail::json_string(*error) : std::string{"null"}) + "}";
        record_event_locked("operation", operation_name, "operation_event", "operation",
                            operation_name, "", "json", event, string_bytes(payload), std::nullopt);
    }

    /**
     * @brief 请求取消指定 operation invocation。
     */
    std::variant<IntrospectionOperationStatus, std::string> cancel_operation(
        std::string_view operation_id) const {
        std::function<std::variant<IntrospectionOperationStatus, std::string>(std::string_view)>
            handler;
        {
            std::lock_guard<std::mutex> lock(inner_->mutex);
            for (auto &[name, operation] : inner_->operations) {
                const auto match =
                    std::find(operation.current_operation_ids.begin(),
                              operation.current_operation_ids.end(), std::string{operation_id});
                if (match == operation.current_operation_ids.end()) {
                    continue;
                }
                if (auto handler_it = inner_->operation_cancel_handlers.find(name);
                    handler_it != inner_->operation_cancel_handlers.end()) {
                    handler = handler_it->second;
                    break;
                }
            }
        }
        if (handler) {
            return handler(operation_id);
        }

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
            ",\"inflight\":" + std::string(health.inflight ? "true" : "false") +
            ",\"scheduled_time_ms\":" + detail::optional_u64_json(health.scheduled_time_ms) +
            ",\"observed_time_ms\":" + detail::optional_u64_json(health.observed_time_ms) +
            ",\"lateness_ms\":" + detail::optional_u64_json(health.lateness_ms) +
            ",\"missed_periods\":" + detail::optional_u64_json(health.missed_periods) +
            ",\"overrun\":" + detail::optional_bool_json(health.overrun) +
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
        snapshot.clock = inner_->clock;
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
        snapshot.resources.reserve(inner_->resources.size());
        for (const auto &[name, resource] : inner_->resources) {
            snapshot.resources.push_back(resource);
        }
        snapshot.params.reserve(inner_->params.size());
        for (const auto &[name, param] : inner_->params) {
            snapshot.params.push_back(param_status(name, param));
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
        snapshot.diagnostics = derive_diagnostics(snapshot);
        return snapshot;
    }

    /**
     * @brief 将当前结构化诊断快照写入 recorder。
     *
     * `status()` 保持无副作用；控制面在需要把诊断纳入 record 时显式调用该方法。
     */
    void record_current_diagnostics() const {
        const auto snapshot = status();
        std::lock_guard<std::mutex> lock(inner_->mutex);
        record_diagnostics_locked(snapshot.diagnostics);
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
        param->second.apply_state = "pending";
        param->second.last_reject_reason = std::nullopt;
        param->second.updated_unix_ms = detail::unix_time_ms();
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
     * @brief 读取参数 pending 值但不清空，供 generated shell 在 apply 边界先校验再提交。
     */
    std::optional<std::string> peek_pending_param(std::string_view name) const {
        return pending_param(name);
    }

    /**
     * @brief 取出并清空参数 pending 值。新 generated shell 使用 peek/apply 状态转换。
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
            const auto clear_pending =
                param->second.pending.has_value() && *param->second.pending == value;
            param->second.current = std::move(value);
            if (clear_pending) {
                param->second.pending = std::nullopt;
            }
            param->second.apply_state = "applied";
            param->second.last_reject_reason = std::nullopt;
            param->second.updated_unix_ms = detail::unix_time_ms();
        }
        record_event_locked(
            "param", name, "param_event", "param", name, "", "json", "param_applied",
            string_bytes("{\"name\":" + detail::json_string(name) + "}"), std::nullopt);
    }

    /**
     * @brief 记录参数 pending 值被 apply 边界拒绝，保留旧 current。
     */
    void record_param_rejected(std::string_view name, std::string value,
                               std::string_view reason) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto param = inner_->params.find(std::string{name});
        if (param != inner_->params.end() && param->second.pending.has_value() &&
            *param->second.pending == value) {
            param->second.pending = std::nullopt;
        }
        if (param != inner_->params.end()) {
            param->second.apply_state = "rejected";
            param->second.last_reject_reason = std::string{reason};
            param->second.updated_unix_ms = detail::unix_time_ms();
        }
        record_event_locked("param", name, "param_event", "param", name, "", "json",
                            "param_rejected",
                            string_bytes("{\"name\":" + detail::json_string(name) +
                                         ",\"reason\":" + detail::json_string(reason) + "}"),
                            std::nullopt);
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
        std::string apply_state = "applied";
        std::optional<std::string> last_reject_reason;
        std::optional<std::uint64_t> updated_unix_ms;
        std::optional<std::string> min;
        std::optional<std::string> max;
        std::vector<std::string> choices;
    };

    struct BoundaryInputState {
        std::string message_type;
        BoundaryInputHandler handler;
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
        IntrospectionClockStatus clock;
        std::optional<std::string> self_description_json;
        std::map<std::string, ChannelState> channels;
        std::map<std::string, ParamState> params;
        std::map<std::string, BoundaryInputState> boundary_inputs;
        std::map<std::string, IntrospectionResourceStatus> resources;
        std::map<std::string, BoundaryStatus> io_boundaries;
        std::map<std::string, IntrospectionServiceStatus> services;
        std::map<std::string, IntrospectionOperationStatus> operations;
        std::map<std::string, std::function<std::variant<IntrospectionOperationStatus, std::string>(
                                  std::string_view)>>
            operation_cancel_handlers;
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

    void record_diagnostics_locked(const std::vector<IntrospectionDiagnostic> &diagnostics) const {
        for (const auto &diagnostic : diagnostics) {
            const auto payload = detail::diagnostic_status_json(diagnostic);
            const auto monotonic_ns =
                diagnostic.observed_ms
                    ? std::optional<std::uint64_t>{diagnostic.observed_ms.value() * 1000000U}
                    : std::nullopt;
            record_event_locked("diagnostics", diagnostic.entity_id, "diagnostics_event",
                                "diagnostic", diagnostic.entity_id, "", "json",
                                "flowrt.diagnostics.status", string_bytes(payload), monotonic_ns);
        }
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
            .apply_state = param.apply_state,
            .last_reject_reason = param.last_reject_reason,
            .updated_unix_ms = param.updated_unix_ms,
            .min = param.min,
            .max = param.max,
            .choices = param.choices,
        };
    }

    static IntrospectionDiagnosticMetric metric(std::string name, std::string value_json) {
        return IntrospectionDiagnosticMetric{
            .name = std::move(name),
            .value = std::move(value_json),
        };
    }

    static std::string optional_string_metric(const std::optional<std::string> &value) {
        return value ? detail::json_string(*value) : "null";
    }

    static std::string optional_u64_metric(const std::optional<std::uint64_t> &value) {
        return detail::optional_u64_json(value);
    }

    static std::string bool_metric(bool value) { return value ? "true" : "false"; }

    static IntrospectionDiagnostic diagnostic(
        std::string category, std::string entity_kind, std::string entity_id, std::string state,
        std::string severity, std::optional<std::string> reason = std::nullopt,
        std::optional<std::string> suggestion = std::nullopt,
        std::optional<std::uint64_t> updated_unix_ms = std::nullopt,
        std::optional<std::uint64_t> observed_ms = std::nullopt,
        std::vector<IntrospectionDiagnosticMetric> metrics = {}) {
        return IntrospectionDiagnostic{
            .category = std::move(category),
            .entity_kind = std::move(entity_kind),
            .entity_id = std::move(entity_id),
            .state = std::move(state),
            .severity = std::move(severity),
            .reason = std::move(reason),
            .suggestion = std::move(suggestion),
            .updated_unix_ms = updated_unix_ms,
            .observed_ms = observed_ms,
            .metrics = std::move(metrics),
        };
    }

    static std::vector<IntrospectionDiagnostic> derive_diagnostics(
        const IntrospectionStatus &status) {
        std::vector<IntrospectionDiagnostic> diagnostics;
        diagnostics.push_back(
            diagnostic("clock", "clock", status.clock.source, status.clock.source,
                       status.clock.source == "realtime" ? "info" : "warn",
                       status.clock.source + " time source", std::nullopt, std::nullopt,
                       status.clock.tick_time_ms,
                       {metric("tick_time_ms", optional_u64_metric(status.clock.tick_time_ms)),
                        metric("unit", detail::json_string(status.clock.unit)),
                        metric("field", detail::json_string(status.clock.field))}));

        for (const auto &channel : status.channels) {
            const bool dropping = channel.dropped_samples != 0U;
            diagnostics.push_back(diagnostic(
                "channel", "channel", channel.name, dropping ? "dropping" : "ok",
                dropping ? "warn" : "info",
                dropping ? std::optional<std::string>{"channel probe dropped samples"}
                         : std::nullopt,
                std::nullopt, std::nullopt, status.clock.tick_time_ms,
                {metric("published_count", std::to_string(channel.published_count)),
                 metric("last_payload_len", channel.last_payload_len
                                                ? std::to_string(*channel.last_payload_len)
                                                : "null"),
                 metric("active_observers", std::to_string(channel.active_observers)),
                 metric("dropped_samples", std::to_string(channel.dropped_samples))}));
        }

        for (const auto &resource : status.resources) {
            const bool satisfied = resource.satisfied.value_or(resource.state == "ready");
            const auto severity =
                resource.state == "failed" || (resource.required && !satisfied) ? "error"
                : (resource.state == "pending" || resource.state == "degraded") ? "warn"
                                                                                : "info";
            diagnostics.push_back(diagnostic(
                "resource", "resource", resource.name, resource.state, severity,
                resource.last_error
                    ? resource.last_error
                    : (resource.diagnostic ? resource.diagnostic : resource.contract_status),
                resource.suggestion, resource.updated_unix_ms, status.clock.tick_time_ms,
                {metric("capability", detail::json_string(resource.capability)),
                 metric("required", bool_metric(resource.required)),
                 metric("readiness", optional_string_metric(resource.readiness)),
                 metric("health", optional_string_metric(resource.health)),
                 metric("on_failure", optional_string_metric(resource.on_failure)),
                 metric("satisfied",
                        resource.satisfied ? bool_metric(*resource.satisfied) : "null"),
                 metric("provider", optional_string_metric(resource.provider))}));
        }

        for (const auto &boundary : status.io_boundaries) {
            const auto state = boundary.ready && boundary.healthy
                                   ? "ready"
                                   : (!boundary.healthy ? "unhealthy" : "not_ready");
            const auto severity = !boundary.healthy ? "error" : (!boundary.ready ? "warn" : "info");
            diagnostics.push_back(diagnostic(
                "io_boundary", "io_boundary", boundary.name, state, severity, boundary.last_error,
                std::nullopt, boundary.updated_unix_ms, status.clock.tick_time_ms,
                {metric("ready", bool_metric(boundary.ready)),
                 metric("healthy", bool_metric(boundary.healthy)),
                 metric("resource_count", std::to_string(boundary.resources.size()))}));
            for (const auto &resource : boundary.resources) {
                diagnostics.push_back(diagnostic(
                    "io_boundary", "io_boundary_resource", boundary.name + "." + resource.name,
                    resource.ready ? "ready" : "not_ready", resource.ready ? "info" : "warn",
                    resource.last_error ? resource.last_error : resource.message, std::nullopt,
                    resource.updated_unix_ms, status.clock.tick_time_ms,
                    {metric("kind", detail::json_string(resource.kind)),
                     metric("ready", bool_metric(resource.ready))}));
            }
        }

        for (const auto &param : status.params) {
            diagnostics.push_back(diagnostic(
                "param", "param", param.name, param.apply_state,
                param.apply_state == "rejected" ? "error" : (param.pending ? "warn" : "info"),
                param.last_reject_reason, std::nullopt, param.updated_unix_ms,
                status.clock.tick_time_ms,
                {metric("update", detail::json_string(param.update)),
                 metric("pending", detail::optional_json_fragment(param.pending)),
                 metric("current", param.current)}));
        }

        for (const auto &service : status.services) {
            const auto failures = service.timeout_count + service.busy_count +
                                  service.unavailable_count + service.late_drop_count;
            diagnostics.push_back(diagnostic(
                "service", "service", service.name, service.ready ? "ready" : "unavailable",
                (!service.ready || failures != 0U) ? "warn" : "info",
                !service.ready
                    ? std::optional<std::string>{"service endpoint is not ready"}
                    : (failures != 0U ? std::optional<std::string>{"service has failed requests"}
                                      : std::nullopt),
                std::nullopt, std::nullopt, status.clock.tick_time_ms,
                {metric("in_flight", std::to_string(service.in_flight)),
                 metric("queued", std::to_string(service.queued)),
                 metric("total_requests", std::to_string(service.total_requests)),
                 metric("timeout_count", std::to_string(service.timeout_count)),
                 metric("busy_count", std::to_string(service.busy_count)),
                 metric("unavailable_count", std::to_string(service.unavailable_count)),
                 metric("late_drop_count", std::to_string(service.late_drop_count))}));
        }

        for (const auto &operation : status.operations) {
            const auto failed =
                operation.failed_count + operation.timeout_count + operation.preempted_count;
            const auto state =
                operation.current_state.value_or(operation.ready ? "ready" : "unavailable");
            diagnostics.push_back(diagnostic(
                "operation", "operation", operation.name, state,
                (operation.last_error || failed != 0U)
                    ? "error"
                    : ((operation.running != 0U || operation.queued != 0U) ? "warn" : "info"),
                operation.last_error ? operation.last_error : operation.last_event, std::nullopt,
                operation.last_transition_ms,
                operation.current_deadline_ms ? operation.current_deadline_ms
                                              : status.clock.tick_time_ms,
                {metric("ready", bool_metric(operation.ready)),
                 metric("running", std::to_string(operation.running)),
                 metric("queued", std::to_string(operation.queued)),
                 metric("total_started", std::to_string(operation.total_started)),
                 metric("succeeded_count", std::to_string(operation.succeeded_count)),
                 metric("failed_count", std::to_string(operation.failed_count)),
                 metric("timeout_count", std::to_string(operation.timeout_count)),
                 metric("current_deadline_ms",
                        optional_u64_metric(operation.current_deadline_ms))}));
        }

        for (const auto &task : status.tasks) {
            const bool failing = task.consecutive_failures != 0U;
            const bool runtime_timing_issue = task.lateness_ms.value_or(0U) != 0U ||
                                              task.missed_periods.value_or(0U) != 0U ||
                                              task.overrun.value_or(false);
            const bool counter_issue = task.deadline_missed != 0U || task.stale_input != 0U ||
                                       task.backpressure != 0U || task.overflow != 0U;
            const bool timing_issue = runtime_timing_issue || counter_issue;
            diagnostics.push_back(diagnostic(
                "task", "task", task.name, failing ? "failing" : (timing_issue ? "degraded" : "ok"),
                failing ? "error" : (timing_issue ? "warn" : "info"),
                failing ? std::optional<std::string>{"task has consecutive failures"}
                        : (runtime_timing_issue
                               ? std::optional<std::string>{"runtime observed task timing issue"}
                               : (counter_issue
                                      ? std::optional<
                                            std::string>{"task timing/input counters are non-zero"}
                                      : std::nullopt)),
                runtime_timing_issue
                    ? std::optional<std::string>{"timing is runtime-observed scheduler time, not a "
                                                 "hard realtime guarantee"}
                    : std::nullopt,
                task.last_run_ms ? task.last_run_ms : task.observed_time_ms,
                task.observed_time_ms
                    ? task.observed_time_ms
                    : (task.last_run_ms ? task.last_run_ms : status.clock.tick_time_ms),
                {metric("lane", detail::json_string(task.lane)),
                 metric("inflight", bool_metric(task.inflight)),
                 metric("scheduled_time_ms", optional_u64_metric(task.scheduled_time_ms)),
                 metric("observed_time_ms", optional_u64_metric(task.observed_time_ms)),
                 metric("lateness_ms", optional_u64_metric(task.lateness_ms)),
                 metric("missed_periods", optional_u64_metric(task.missed_periods)),
                 metric("overrun", task.overrun ? bool_metric(*task.overrun) : std::string{"null"}),
                 metric("deadline_missed", std::to_string(task.deadline_missed)),
                 metric("stale_input", std::to_string(task.stale_input)),
                 metric("backpressure", std::to_string(task.backpressure)),
                 metric("overflow", std::to_string(task.overflow)),
                 metric("run_count", std::to_string(task.run_count)),
                 metric("success_count", std::to_string(task.success_count)),
                 metric("consecutive_failures", std::to_string(task.consecutive_failures))}));
        }
        return diagnostics;
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

}  // namespace flowrt
