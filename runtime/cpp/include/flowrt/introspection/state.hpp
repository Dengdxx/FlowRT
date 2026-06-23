#pragma once

#include <algorithm>
#include <atomic>
#include <cerrno>
#include <cstdint>
#include <cstring>
#include <flowrt/backend_health.hpp>
#include <flowrt/channels.hpp>
#include <flowrt/core.hpp>
#include <flowrt/introspection/json.hpp>
#include <flowrt/introspection/probe.hpp>
#include <flowrt/lifecycle.hpp>
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

    /// 从 canonical frame payload 提取 sensor sample-time（纳秒）；由声明 timestamp 源的 boundary
    /// 在注册时提供（typed decode 后读字段）。返回 nullopt 表示该样本无 sample-time。
    using SampleTimeFn = std::function<std::optional<std::uint64_t>(std::span<const std::uint8_t>)>;

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
     * @brief 预注册一条 route，使 backend 选择原因和传输健康进入 live status。
     */
    void register_route(IntrospectionRouteStatus status) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto key = status.name;
        inner_->routes.insert_or_assign(key, std::move(status));
    }

    /**
     * @brief 记录 route 成功发布或进入传输路径。
     */
    void record_route_publish(std::string_view name,
                              std::optional<std::uint64_t> published_at_ms) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &route = route_entry_locked(std::string{name});
        route.published_count += 1U;
        route.last_publish_ms = published_at_ms;
    }

    /**
     * @brief 记录 route drop。
     */
    void record_route_drop(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &route = route_entry_locked(std::string{name});
        route.dropped_samples += 1U;
    }

    /**
     * @brief 记录 route backpressure。
     */
    void record_route_backpressure(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &route = route_entry_locked(std::string{name});
        route.backpressure_count += 1U;
    }

    /**
     * @brief 记录 route overflow。
     */
    void record_route_overflow(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &route = route_entry_locked(std::string{name});
        route.overflow_count += 1U;
    }

    /**
     * @brief 记录 transport publish 失败，并按 route overflow policy 投影到统一 counters。
     */
    void record_route_transport_error(std::string_view name, OverflowPolicy overflow,
                                      std::string error) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &route = route_entry_locked(std::string{name});
        switch (overflow) {
            case OverflowPolicy::DropOldest:
            case OverflowPolicy::DropNewest:
                route.dropped_samples += 1U;
                break;
            case OverflowPolicy::Block:
                route.backpressure_count += 1U;
                break;
            case OverflowPolicy::Error:
                route.overflow_count += 1U;
                break;
        }
        route.last_error = error;
        if (route.backend_health_state.empty() || route.backend_health_state == "ready") {
            route.backend_health_state = "degraded";
            route.backend_health_error = std::move(error);
            route.backend_reconnect_attempt = 0;
            route.backend_next_retry_unix_ms = std::nullopt;
            route.backend_recoverable = true;
        } else if (!route.backend_health_error) {
            route.backend_health_error = std::move(error);
        }
    }

    /**
     * @brief 记录 transport channel error；unsupported 只记 backend error，不记队列计数。
     */
    void record_route_transport_error(std::string_view name, OverflowPolicy overflow,
                                      ChannelError error, std::string_view context) const {
        std::string reason{context};
        reason += ": ";
        switch (error) {
            case ChannelError::Overflow:
                reason += "overflow";
                record_route_transport_error(name, overflow, std::move(reason));
                return;
            case ChannelError::Transport:
                reason += "transport";
                record_route_transport_error(name, overflow, std::move(reason));
                return;
            case ChannelError::Unsupported:
                reason += "unsupported";
                record_route_error(name, std::move(reason));
                return;
        }
        record_route_error(name, std::move(reason));
    }

    /**
     * @brief 记录 route/backend 最近错误。
     */
    void record_route_error(std::string_view name, std::string error) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &route = route_entry_locked(std::string{name});
        route.last_error = error;
        if (route.backend_health_state.empty() || route.backend_health_state == "ready") {
            route.backend_health_state = "degraded";
            route.backend_health_error = std::move(error);
            route.backend_reconnect_attempt = 0;
            route.backend_next_retry_unix_ms = std::nullopt;
            route.backend_recoverable = true;
        } else if (!route.backend_health_error) {
            route.backend_health_error = std::move(error);
        }
    }

    /**
     * @brief 记录 route 当前 backend health 快照。
     */
    void record_route_backend_health(std::string_view name, BackendHealthSnapshot health) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto &route = route_entry_locked(std::string{name});
        route.backend_health_state = std::string{backend_health_state_str(health.state)};
        route.backend_health_error = health.last_error;
        route.backend_reconnect_attempt = health.attempt;
        route.backend_next_retry_unix_ms = health.next_retry_unix_ms;
        route.backend_recoverable = health.recoverable;
        if (health.state == BackendHealthState::Ready) {
            route.last_error = std::nullopt;
        } else if (health.last_error) {
            route.last_error = std::move(health.last_error);
        }
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
                                         BoundaryInputHandler handler,
                                         SampleTimeFn sample_time_ns_fn = {}) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->boundary_inputs.insert_or_assign(
            std::move(endpoint),
            BoundaryInputState{.message_type = std::move(message_type),
                               .handler = std::move(handler),
                               .sample_time_ns_fn = std::move(sample_time_ns_fn)});
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
     * @brief 注册带 sensor sample-time 提取器的 typed `BoundaryInput<T>`。
     *
     * `sample_time_ns_fn` 从 canonical frame payload 读出 sample-time（纳秒）——由生成 shell
     * 对声明了 timestamp 源的 boundary 提供。录制该 boundary 激励时填入 envelope.sample_time_ns，供
     * event-time 回放按 sensor 采集时刻步进。与 Rust register_boundary_input_with_sample_time
     * 对齐。
     */
    template <CanonicalTransportMessage T>
    void register_boundary_input_with_sample_time(std::string endpoint, std::string message_type,
                                                  BoundaryInput<T> &input,
                                                  SampleTimeFn sample_time_ns_fn) const {
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
            },
            std::move(sample_time_ns_fn));
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
            const auto sample_time_ns =
                boundary.sample_time_ns_fn ? boundary.sample_time_ns_fn(payload) : std::nullopt;
            (void)try_record_channel_sample_frame_bytes_with_sample_time(
                endpoint, boundary.message_type, payload, published_at_ms, sample_time_ns);
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
        const auto payload =
            frame_descriptor_payload_json(descriptor, status, payload_recording, std::nullopt);
        const auto payload_bytes = string_bytes(payload);
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return record_event_locked("descriptor", descriptor.resource().resource_id,
                                   "descriptor_event", "resource",
                                   descriptor.resource().resource_id, "FrameDescriptor", "json",
                                   "flowrt.descriptor.frame.v1", payload_bytes, std::nullopt);
    }

    /**
     * @brief 记录 frame descriptor 事件及 payload artifact 元数据。
     */
    IntrospectionProbeRecord record_frame_descriptor_payload_event(
        std::string_view name, const FrameDescriptor &descriptor, FrameLeaseStatus status,
        FramePayloadArtifact artifact) const {
        (void)name;
        if (!inner_->recorder_enabled.load(std::memory_order_acquire)) {
            return IntrospectionProbeRecord{};
        }
        const auto payload =
            frame_descriptor_payload_json(descriptor, status, true, std::move(artifact));
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
     * @brief 以 canonical frame 编码记录带 sensor sample-time 的 channel 样本。
     *
     * `sample_time_ns` 由声明了 timestamp 源的 boundary 提取器提供，写入 envelope.sample_time_ns，
     * 供 event-time 回放按 sensor 采集时刻步进。与 Rust
     * try_record_channel_sample_frame_bytes_with_sample_time 对齐。
     */
    IntrospectionProbeRecord try_record_channel_sample_frame_bytes_with_sample_time(
        std::string_view name, std::string_view message_type, std::span<const std::uint8_t> payload,
        std::optional<std::uint64_t> published_at_ms,
        std::optional<std::uint64_t> sample_time_ns) const {
        if (!inner_->recorder_enabled.load(std::memory_order_acquire)) {
            return IntrospectionProbeRecord{};
        }
        std::lock_guard<std::mutex> lock(inner_->mutex);
        return record_event_locked("channel", name, "channel_sample", "channel", name, message_type,
                                   "canonical_frame", message_type, payload,
                                   published_at_ms
                                       ? std::optional<std::uint64_t>{*published_at_ms * 1000000U}
                                       : std::nullopt,
                                   sample_time_ns);
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
     * @brief 注册 graph critical health 参与聚合的 instance 名集合；空集合表示所有已观测实例。
     */
    void register_critical_instances(std::vector<std::string> instances) const {
        std::sort(instances.begin(), instances.end());
        instances.erase(std::unique(instances.begin(), instances.end()), instances.end());
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->critical_instances = std::move(instances);
    }

    /**
     * @brief 记录某 instance 的最新生命周期状态。generated shell 在各生命周期阶段调用。
     */
    void record_lifecycle_state(std::string instance, LifecycleState state) const {
        record_lifecycle_transition(std::move(instance), state, std::nullopt, std::nullopt);
    }

    /**
     * @brief 记录某 instance 生命周期边沿及可选故障上下文。
     */
    void record_lifecycle_transition(std::string instance, LifecycleState state,
                                     std::optional<std::uint64_t> tick_id,
                                     std::optional<std::string> reason) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto transition_tick =
            tick_id ? tick_id : std::optional<std::uint64_t>{inner_->tick_count};
        auto &entry = inner_->instances[std::move(instance)];
        entry.lifecycle = state;
        entry.last_transition_tick = transition_tick;
        if (state == LifecycleState::Faulted) {
            entry.last_fault_tick = transition_tick;
            entry.last_fault_reason = std::move(reason);
        }
    }

    void record_lifecycle_transition(std::string instance, LifecycleState state,
                                     std::uint64_t tick_id, std::string reason) const {
        record_lifecycle_transition(std::move(instance), state,
                                    std::optional<std::uint64_t>{tick_id},
                                    std::optional<std::string>{std::move(reason)});
    }

    /**
     * @brief 记录某 instance 的一次重启尝试。
     */
    void record_instance_restart(std::string instance) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        ++inner_->instances[std::move(instance)].restart_count;
    }

    /**
     * @brief 记录一次 standby redundancy failover。
     */
    void record_failover(IntrospectionFailoverEvent event) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->failovers.push_back(std::move(event));
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
     * @brief 注册 operation status control hook。
     */
    void register_operation_status_handler(
        std::string name,
        std::function<std::variant<IntrospectionOperationStatus, std::string>(std::string_view)>
            handler) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->operation_status_handlers.insert_or_assign(std::move(name), std::move(handler));
    }

    /**
     * @brief 注册 operation start control hook。
     */
    void register_operation_start_handler(
        std::string name,
        std::function<std::variant<IntrospectionOperationStartStatus, std::string>(
            std::vector<std::uint8_t>, std::optional<std::uint64_t>, std::optional<std::string>)>
            handler) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->operation_start_handlers.insert_or_assign(std::move(name), std::move(handler));
    }

    /**
     * @brief 请求启动指定 operation endpoint。
     */
    std::variant<IntrospectionOperationStartStatus, std::string> start_operation(
        std::string_view operation, std::vector<std::uint8_t> payload,
        std::optional<std::uint64_t> timeout_ms, std::optional<std::string> owner) const {
        std::function<std::variant<IntrospectionOperationStartStatus, std::string>(
            std::vector<std::uint8_t>, std::optional<std::uint64_t>, std::optional<std::string>)>
            handler;
        {
            std::lock_guard<std::mutex> lock(inner_->mutex);
            if (auto it = inner_->operation_start_handlers.find(std::string{operation});
                it != inner_->operation_start_handlers.end()) {
                handler = it->second;
            }
        }
        if (handler) {
            return handler(std::move(payload), timeout_ms, std::move(owner));
        }
        return "FlowRT operation `" + std::string{operation} +
               "` does not accept introspection start";
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
        push_operation_event_locked(inner_, operation_name, id, "state", state_name, std::nullopt,
                                    std::nullopt, std::nullopt, now);
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
        record_operation_progress_payload(operation, operation_id, sequence, std::nullopt);
    }

    /**
     * @brief 记录 operation progress 事件，并保留 payload 长度供观测事件说明。
     */
    void record_operation_progress_payload(
        std::string_view operation, std::string_view operation_id, std::uint64_t sequence,
        const std::optional<std::vector<std::uint8_t>> &payload_bytes) const {
        const auto operation_name = std::string{operation};
        const auto id = std::string{operation_id};
        const auto now = detail::unix_time_ms();
        std::lock_guard<std::mutex> lock(inner_->mutex);
        push_operation_event_locked(inner_, operation_name, id, "progress", std::nullopt, sequence,
                                    payload_bytes, std::nullopt, now);
        auto &entry = inner_->operations[operation_name];
        entry.name = operation_name;
        entry.last_event = "flowrt.operation.progress";
        const auto payload_json =
            "{\"operation_id\":" + detail::json_string(operation_id) +
            ",\"sequence\":" + std::to_string(sequence) + ",\"payload_len\":" +
            (payload_bytes ? std::to_string(payload_bytes->size()) : std::string{"null"}) + "}";
        record_event_locked("operation", operation_name, "operation_event", "operation",
                            operation_name, "", "json", "flowrt.operation.progress",
                            string_bytes(payload_json), std::nullopt);
    }

    /**
     * @brief 记录 operation result/error 事件。
     */
    void record_operation_result(std::string_view operation, std::string_view operation_id,
                                 std::string_view result,
                                 std::optional<std::string_view> error) const {
        record_operation_result_payload(operation, operation_id, result, error, std::nullopt);
    }

    /**
     * @brief 记录 operation result/error 事件，并按 retention 清理 result 与 event log。
     */
    void record_operation_result_with_retention(std::string_view operation,
                                                std::string_view operation_id,
                                                std::string_view result,
                                                std::optional<std::string_view> error,
                                                std::optional<std::uint64_t> retention_ms) const {
        record_operation_result_payload_with_retention(operation, operation_id, result, error,
                                                       std::nullopt, retention_ms);
    }

    /**
     * @brief 记录 operation result/error 事件，并保留 payload 长度供观测事件说明。
     */
    void record_operation_result_payload(
        std::string_view operation, std::string_view operation_id, std::string_view result,
        std::optional<std::string_view> error,
        const std::optional<std::vector<std::uint8_t>> &payload_bytes) const {
        record_operation_result_payload_with_retention(operation, operation_id, result, error,
                                                       payload_bytes, std::nullopt);
    }

    /**
     * @brief 记录 operation result/error 事件，并按 retention 保留 payload 与 observation log。
     */
    void record_operation_result_payload_with_retention(
        std::string_view operation, std::string_view operation_id, std::string_view result,
        std::optional<std::string_view> error,
        const std::optional<std::vector<std::uint8_t>> &payload_bytes,
        std::optional<std::uint64_t> retention_ms) const {
        const auto operation_name = std::string{operation};
        const auto id = std::string{operation_id};
        const auto event = (error.has_value() || result == "failed")
                               ? std::string{"flowrt.operation.error"}
                               : std::string{"flowrt.operation.result"};
        const auto kind = (error.has_value() || result == "failed") ? std::string{"error"}
                                                                    : std::string{"result"};
        const auto now = detail::unix_time_ms();
        const auto expires_unix_ms =
            retention_ms
                ? std::optional<std::uint64_t>{*retention_ms >
                                                       std::numeric_limits<std::uint64_t>::max() -
                                                           now
                                                   ? std::numeric_limits<std::uint64_t>::max()
                                                   : now + *retention_ms}
                : std::nullopt;
        std::lock_guard<std::mutex> lock(inner_->mutex);
        push_operation_event_locked(
            inner_, operation_name, id, kind, std::string{result}, std::nullopt, payload_bytes,
            error ? std::optional<std::string>{std::string{*error}} : std::nullopt, now);
        auto &entry = inner_->operations[operation_name];
        entry.name = operation_name;
        entry.last_event = event;
        entry.last_error = error ? std::optional<std::string>{std::string{*error}} : std::nullopt;
        entry.current_state = std::string{result};
        entry.current_operation_ids.erase(
            std::remove(entry.current_operation_ids.begin(), entry.current_operation_ids.end(), id),
            entry.current_operation_ids.end());
        entry.running = 0;
        entry.last_transition_ms = now;
        inner_->operation_results.insert_or_assign(
            id, IntrospectionOperationResult{
                    .operation_id = id,
                    .operation = operation_name,
                    .state = std::string{result},
                    .result = error.has_value() ? std::nullopt
                                                : std::optional<std::string>{std::string{result}},
                    .error = error ? std::optional<std::string>{std::string{*error}} : std::nullopt,
                    .payload = payload_bytes,
                    .completed_unix_ms = now,
                    .expires_unix_ms = expires_unix_ms,
                });
        if (retention_ms == std::optional<std::uint64_t>{0U}) {
            inner_->operation_results.erase(id);
            inner_->operation_events.erase(id);
        }
        const auto payload_json =
            "{\"operation_id\":" + detail::json_string(operation_id) +
            ",\"result\":" + detail::json_string(result) +
            ",\"error\":" + (error ? detail::json_string(*error) : std::string{"null"}) +
            ",\"payload_len\":" +
            (payload_bytes ? std::to_string(payload_bytes->size()) : std::string{"null"}) + "}";
        record_event_locked("operation", operation_name, "operation_event", "operation",
                            operation_name, "", "json", event, string_bytes(payload_json),
                            std::nullopt);
    }

    /**
     * @brief 清理超过 result retention 的 operation result 与 observation event log。
     */
    void evict_retained_operation_observations(std::uint64_t now_unix_ms) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        std::vector<std::string> expired_ids;
        for (const auto &[operation_id, result] : inner_->operation_results) {
            if (result.expires_unix_ms.has_value() && now_unix_ms > *result.expires_unix_ms) {
                expired_ids.push_back(operation_id);
            }
        }
        for (const auto &operation_id : expired_ids) {
            inner_->operation_results.erase(operation_id);
            inner_->operation_events.erase(operation_id);
        }
    }

    /**
     * @brief 按当前 wall-clock 清理超过 result retention 的 operation observation。
     */
    void evict_expired_operation_observations() const {
        evict_retained_operation_observations(detail::unix_time_ms());
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
     * @brief 请求查询指定 operation invocation。
     */
    std::variant<IntrospectionOperationStatus, std::string> status_operation(
        std::string_view operation_id) const {
        std::function<std::variant<IntrospectionOperationStatus, std::string>(std::string_view)>
            handler;
        std::optional<IntrospectionOperationStatus> cached;
        {
            std::lock_guard<std::mutex> lock(inner_->mutex);
            for (auto &[name, operation] : inner_->operations) {
                const auto match =
                    std::find(operation.current_operation_ids.begin(),
                              operation.current_operation_ids.end(), std::string{operation_id});
                if (match == operation.current_operation_ids.end()) {
                    continue;
                }
                cached = operation;
                if (auto handler_it = inner_->operation_status_handlers.find(name);
                    handler_it != inner_->operation_status_handlers.end()) {
                    handler = handler_it->second;
                }
                break;
            }
        }
        if (!cached.has_value()) {
            return "unknown FlowRT operation `" + std::string{operation_id} + "`";
        }
        if (handler) {
            return handler(operation_id);
        }
        return *cached;
    }

    /**
     * @brief 查询保留的 operation result。
     */
    std::variant<IntrospectionOperationResult, std::string> result_operation(
        std::string_view operation_id) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        if (auto it = inner_->operation_results.find(std::string{operation_id});
            it != inner_->operation_results.end()) {
            return it->second;
        }
        return "unknown FlowRT operation result `" + std::string{operation_id} + "`";
    }

    struct OperationObservePage {
        std::vector<IntrospectionOperationEvent> events;
        std::uint64_t next_sequence = 0;
        bool terminal = false;
    };

    /**
     * @brief 查询 Operation observation event page。
     */
    std::variant<OperationObservePage, std::string> observe_operation(
        std::string_view operation_id, std::uint64_t after_sequence,
        std::optional<std::size_t> limit) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto it = inner_->operation_events.find(std::string{operation_id});
        if (it == inner_->operation_events.end()) {
            return "unknown FlowRT operation `" + std::string{operation_id} + "`";
        }
        const auto clamped_limit =
            std::max<std::size_t>(1U, std::min<std::size_t>(limit.value_or(64U), 1024U));
        OperationObservePage page;
        for (const auto &event : it->second) {
            if (event.sequence < after_sequence) {
                continue;
            }
            if (page.events.size() >= clamped_limit) {
                break;
            }
            page.events.push_back(event);
        }
        page.next_sequence =
            page.events.empty() ? after_sequence : page.events.back().sequence + 1U;
        page.terminal = !it->second.empty() &&
                        (it->second.back().kind == "result" || it->second.back().kind == "error");
        return page;
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
        snapshot.routes.reserve(inner_->routes.size());
        for (const auto &[name, route] : inner_->routes) {
            snapshot.routes.push_back(route);
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
        snapshot.instances.reserve(inner_->instances.size());
        for (const auto &[name, state] : inner_->instances) {
            snapshot.instances.push_back(IntrospectionInstanceStatus{
                .instance = name,
                .lifecycle_state = std::string{lifecycle_state_str(state.lifecycle)},
                .restart_count = state.restart_count,
                .last_fault_reason = state.last_fault_reason,
                .last_fault_tick = state.last_fault_tick,
                .last_transition_tick = state.last_transition_tick,
            });
        }
        snapshot.failovers = inner_->failovers;
        if (inner_->critical_instances.empty()) {
            snapshot.critical_instances.reserve(snapshot.instances.size());
            for (const auto &instance : snapshot.instances) {
                snapshot.critical_instances.push_back(instance.instance);
            }
        } else {
            snapshot.critical_instances = inner_->critical_instances;
        }
        std::vector<IntrospectionInstanceStatus> critical_statuses;
        critical_statuses.reserve(snapshot.instances.size());
        for (const auto &instance : snapshot.instances) {
            if (std::find(snapshot.critical_instances.begin(), snapshot.critical_instances.end(),
                          instance.instance) != snapshot.critical_instances.end()) {
                critical_statuses.push_back(instance);
            }
        }
        snapshot.graph_health = std::string{graph_health_label(snapshot.instances)};
        snapshot.graph_critical_health = std::string{graph_health_label(critical_statuses)};
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
        SampleTimeFn sample_time_ns_fn;
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

    struct InstanceRuntimeState {
        LifecycleState lifecycle = LifecycleState::Uninitialized;
        std::uint64_t restart_count = 0;
        std::optional<std::string> last_fault_reason;
        std::optional<std::uint64_t> last_fault_tick;
        std::optional<std::uint64_t> last_transition_tick;
    };

    struct Inner {
        std::atomic_bool recorder_enabled{false};
        std::mutex mutex;
        std::uint64_t tick_count = 0;
        IntrospectionClockStatus clock;
        std::optional<std::string> self_description_json;
        std::map<std::string, ChannelState> channels;
        std::map<std::string, IntrospectionRouteStatus> routes;
        std::map<std::string, ParamState> params;
        std::map<std::string, BoundaryInputState> boundary_inputs;
        std::map<std::string, IntrospectionResourceStatus> resources;
        std::map<std::string, BoundaryStatus> io_boundaries;
        std::map<std::string, IntrospectionServiceStatus> services;
        std::map<std::string, IntrospectionOperationStatus> operations;
        std::map<std::string, IntrospectionOperationResult> operation_results;
        std::map<std::string, std::vector<IntrospectionOperationEvent>> operation_events;
        std::map<std::string,
                 std::function<std::variant<IntrospectionOperationStartStatus, std::string>(
                     std::vector<std::uint8_t>, std::optional<std::uint64_t>,
                     std::optional<std::string>)>>
            operation_start_handlers;
        std::map<std::string, std::function<std::variant<IntrospectionOperationStatus, std::string>(
                                  std::string_view)>>
            operation_status_handlers;
        std::map<std::string, std::function<std::variant<IntrospectionOperationStatus, std::string>(
                                  std::string_view)>>
            operation_cancel_handlers;
        std::map<std::string, IntrospectionTaskHealth> tasks;
        std::map<std::string, IntrospectionLaneHealth> lanes;
        std::map<std::string, InstanceRuntimeState> instances;
        std::vector<std::string> critical_instances;
        std::vector<IntrospectionFailoverEvent> failovers;
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

    static void push_operation_event_locked(
        const std::shared_ptr<Inner> &inner, const std::string &operation,
        const std::string &operation_id, const std::string &kind, std::optional<std::string> state,
        std::optional<std::uint64_t> progress_sequence,
        const std::optional<std::vector<std::uint8_t>> &payload, std::optional<std::string> message,
        std::optional<std::uint64_t> unix_ms) {
        auto &events = inner->operation_events[operation_id];
        events.push_back(IntrospectionOperationEvent{
            .sequence = static_cast<std::uint64_t>(events.size()),
            .kind = kind,
            .operation_id = operation_id,
            .operation = operation,
            .state = std::move(state),
            .progress_sequence = progress_sequence,
            .payload = payload,
            .message = std::move(message),
            .unix_ms = unix_ms,
        });
    }

    IntrospectionRouteStatus &route_entry_locked(std::string name) const {
        auto [it, inserted] =
            inner_->routes.try_emplace(name, IntrospectionRouteStatus{.name = name});
        (void)inserted;
        return it->second;
    }

    static std::string_view backend_health_state_str(BackendHealthState state) {
        switch (state) {
            case BackendHealthState::Ready:
                return "ready";
            case BackendHealthState::Degraded:
                return "degraded";
            case BackendHealthState::Reconnecting:
                return "reconnecting";
            case BackendHealthState::Failed:
                return "failed";
            case BackendHealthState::Unsupported:
                return "unsupported";
        }
        return "unsupported";
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
                                                     bool payload_recording,
                                                     std::optional<FramePayloadArtifact> artifact) {
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
        output.append(payload_recording && artifact.has_value() ? "true" : "false");
        if (artifact.has_value()) {
            output.append(",\"payload_artifact\":{\"artifact_ref\":");
            output.append(detail::json_string(artifact->artifact_ref));
            output.append(",\"content_hash\":");
            output.append(detail::json_string(artifact->content_hash));
            output.append(",\"size_bytes\":");
            output.append(std::to_string(artifact->size_bytes));
            output.push_back('}');
        }
        output.push_back('}');
        return output;
    }

    IntrospectionProbeRecord record_event_locked(
        std::string_view filter_kind, std::string_view filter_name, std::string_view event_kind,
        std::string_view entity_kind, std::string_view entity_name, std::string_view message_type,
        std::string_view payload_encoding, std::string_view payload_schema,
        std::span<const std::uint8_t> payload, std::optional<std::uint64_t> monotonic_ns,
        std::optional<std::uint64_t> sample_time_ns = std::nullopt) const {
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
            .sample_time_ns = sample_time_ns,
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

        for (const auto &route : status.routes) {
            const auto backend_health_state = route.backend_health_state.empty()
                                                  ? std::string{"ready"}
                                                  : route.backend_health_state;
            const bool has_error = route.last_error.has_value();
            const bool has_loss = route.dropped_samples != 0U || route.backpressure_count != 0U ||
                                  route.overflow_count != 0U;
            std::string route_state = "selected";
            std::string severity = "info";
            if (backend_health_state == "failed" || backend_health_state == "unsupported") {
                route_state = backend_health_state;
                severity = "error";
            } else if (backend_health_state == "degraded" ||
                       backend_health_state == "reconnecting") {
                route_state = backend_health_state;
                severity = "warn";
            } else if (has_error) {
                route_state = "error";
                severity = "error";
            } else if (has_loss) {
                route_state = "degraded";
                severity = "warn";
            }
            auto reason =
                route.backend_health_error ? route.backend_health_error : route.last_error;
            if (!reason) {
                reason = "backend selected: " + route.selected_reason;
            }
            auto metrics = std::vector<IntrospectionDiagnosticMetric>{
                metric("published_count", std::to_string(route.published_count)),
                metric("dropped_samples", std::to_string(route.dropped_samples)),
                metric("backpressure_count", std::to_string(route.backpressure_count)),
                metric("overflow_count", std::to_string(route.overflow_count)),
                metric("last_publish_ms", optional_u64_metric(route.last_publish_ms)),
                metric("backend_health_state", detail::json_string(backend_health_state)),
                metric("backend_health_error", optional_string_metric(route.backend_health_error)),
                metric("backend_reconnect_attempt",
                       std::to_string(route.backend_reconnect_attempt)),
                metric("backend_next_retry_unix_ms",
                       optional_u64_metric(route.backend_next_retry_unix_ms)),
                metric("backend_recoverable", bool_metric(route.backend_recoverable)),
                metric("backend", detail::json_string(route.backend)),
                metric("selected_reason", detail::json_string(route.selected_reason)),
            };
            if (status.clock.tick_time_ms && route.last_publish_ms &&
                *status.clock.tick_time_ms >= *route.last_publish_ms) {
                metrics.push_back(
                    metric("latest_age_ms",
                           std::to_string(*status.clock.tick_time_ms - *route.last_publish_ms)));
            }
            diagnostics.push_back(diagnostic(
                "route", "route", route.name, route_state, severity, reason, std::nullopt,
                std::nullopt,
                route.last_publish_ms ? route.last_publish_ms : status.clock.tick_time_ms,
                std::move(metrics)));
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

        for (const auto &instance : status.instances) {
            const auto severity = instance.lifecycle_state == "faulted"    ? "error"
                                  : instance.lifecycle_state == "degraded" ? "warn"
                                                                           : "info";
            diagnostics.push_back(diagnostic(
                "lifecycle", "instance", instance.instance, instance.lifecycle_state, severity,
                std::nullopt, std::nullopt, std::nullopt, status.clock.tick_time_ms,
                {metric("restart_count", std::to_string(instance.restart_count)),
                 metric("last_fault_reason", optional_string_metric(instance.last_fault_reason)),
                 metric("last_fault_tick", optional_u64_metric(instance.last_fault_tick)),
                 metric("last_transition_tick",
                        optional_u64_metric(instance.last_transition_tick))}));
        }

        // graph_health 诊断仅在有实例可聚合时派生（与 per-instance lifecycle 诊断一致）；
        // graph_health 字段本身始终存在（默认 healthy）。
        if (!status.instances.empty()) {
            const auto graph_health = graph_health_label(status.instances);
            const auto graph_severity = graph_health == "faulted"    ? "error"
                                        : graph_health == "degraded" ? "warn"
                                                                     : "info";
            diagnostics.push_back(diagnostic(
                "graph_health", "graph", "graph", std::string{graph_health}, graph_severity,
                std::nullopt, std::nullopt, std::nullopt, status.clock.tick_time_ms,
                {metric("critical_instances", detail::json_string_array(status.critical_instances)),
                 metric("graph_critical_health",
                        detail::json_string(status.graph_critical_health))}));
        }
        return diagnostics;
    }

    /// 图级 health 聚合：每实例 lifecycle 的 worst-of（`faulted` > `degraded` > `healthy`），
    /// 与 Rust `graph_health_label` 镜像。正常态不抬升；无实例即 `healthy`。
    static std::string_view graph_health_label(
        const std::vector<IntrospectionInstanceStatus> &instances) {
        bool degraded = false;
        for (const auto &instance : instances) {
            if (instance.lifecycle_state == "faulted") {
                return "faulted";
            }
            if (instance.lifecycle_state == "degraded") {
                degraded = true;
            }
        }
        return degraded ? "degraded" : "healthy";
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
