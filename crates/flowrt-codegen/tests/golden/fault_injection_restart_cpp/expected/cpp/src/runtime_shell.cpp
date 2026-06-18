// FlowRT 管理产物。不要手工修改。
#include "flowrt_app/runtime_shell.hpp"

#include "flowrt_app/selfdesc.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <limits>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <span>
#include <string>
#include <string_view>
#include <thread>
#include <type_traits>
#include <utility>
#include <variant>
#include <vector>

namespace {

flowrt::Status status_from_push_result(const flowrt::ChannelPushResult& result) {
    if (std::holds_alternative<flowrt::ChannelError>(result)) {
        return flowrt::Status::Error;
    }

    switch (std::get<flowrt::ChannelWriteOutcome>(result)) {
        case flowrt::ChannelWriteOutcome::Accepted:
        case flowrt::ChannelWriteOutcome::DroppedOldest:
        case flowrt::ChannelWriteOutcome::DroppedNewest:
            return flowrt::Status::Ok;
        case flowrt::ChannelWriteOutcome::Backpressured:
            return flowrt::Status::Retry;
    }

    return flowrt::Status::Error;
}

std::string flowrt_operation_id_string(flowrt::OperationId id) {
    return std::to_string(id.operation_key) + ":" + std::to_string(id.client_id) + ":" + std::to_string(id.sequence);
}

flowrt::IntrospectionOperationStatus flowrt_operation_status_from_snapshot(std::string_view name, std::string_view owner, const flowrt::OperationStatusSnapshot& snapshot) {
    const bool active = !flowrt::is_terminal(snapshot.state) && snapshot.state != flowrt::OperationState::Idle;
    flowrt::IntrospectionOperationStatus status;
    status.name = std::string{name};
    status.ready = true;
    status.running = active ? 1U : 0U;
    status.queued = 0U;
    if (active) {
        status.current_operation_ids.push_back(flowrt_operation_id_string(snapshot.id));
    }
    status.total_started = snapshot.health.started;
    status.succeeded_count = snapshot.health.succeeded;
    status.failed_count = snapshot.health.failed;
    status.canceled_count = snapshot.health.canceled;
    status.timeout_count = snapshot.health.timeout;
    status.preempted_count = snapshot.health.preempted;
    status.current_state = std::string{flowrt::to_string(snapshot.state)};
    status.current_owner = snapshot.owner.owner_key == 0U ? std::nullopt : std::optional<std::string>{std::string{owner}};
    status.current_deadline_ms = active ? std::optional<std::uint64_t>{snapshot.deadline_ms} : std::nullopt;
    status.last_event = "flowrt.operation.state_changed";
    status.last_error = std::nullopt;
    status.last_transition_ms = flowrt::monotonic_time_ms();
    return status;
}

template <typename T>
flowrt::ServiceResult<T> flowrt_operation_control_error(flowrt::OperationControlError error) {
    switch (error) {
        case flowrt::OperationControlError::Busy:
        case flowrt::OperationControlError::OwnerConflict:
            return flowrt::ServiceResult<T>::err_with_message(flowrt::ServiceError::Busy, std::string{flowrt::to_string(error)});
        case flowrt::OperationControlError::StaleInvocation:
        case flowrt::OperationControlError::AlreadyTerminal:
            return flowrt::ServiceResult<T>::err_with_message(flowrt::ServiceError::Rejected, std::string{flowrt::to_string(error)});
        case flowrt::OperationControlError::InvalidTransition:
        case flowrt::OperationControlError::InvalidPolicy:
        case flowrt::OperationControlError::Ok:
            return flowrt::ServiceResult<T>::err_with_message(flowrt::ServiceError::HandlerError, std::string{flowrt::to_string(error)});
    }
    return flowrt::ServiceResult<T>::err(flowrt::ServiceError::HandlerError);
}

flowrt::IntrospectionChannelProbe register_introspection_channel(
    flowrt::IntrospectionState& state,
    std::string_view name,
    std::string_view message_type,
    std::optional<std::size_t> max_payload_len
) {
    try {
        state.register_channel_with_probe_capacity(
            std::string{name},
            std::string{message_type},
            max_payload_len);
        if (const auto probe = state.channel_probe(name); probe.has_value()) {
            return *probe;
        }
    } catch (...) {
    }
    return flowrt::IntrospectionChannelProbe{};
}

template <typename T>
void record_introspection_publish_copy(
    flowrt::IntrospectionState& state,
    std::string_view name,
    std::string_view message_type,
    const flowrt::IntrospectionChannelProbe& probe,
    const T& value,
    std::uint64_t published_at_ms
) {
    probe.record_publish_event();
    if (!probe.enabled() && !state.recorder_enabled_for_channel(name)) {
        return;
    }
    try {
        const auto payload = std::span<const std::uint8_t>{
            reinterpret_cast<const std::uint8_t*>(&value), sizeof(T)};
        state.try_record_channel_sample_bytes(
            name,
            message_type,
            payload,
            std::optional<std::uint64_t>{published_at_ms});
        if (probe.enabled()) {
            probe.try_record_bytes(payload, std::optional<std::uint64_t>{published_at_ms});
        }
    } catch (...) {
    }
}

template <typename T>
void record_introspection_publish_frame(
    flowrt::IntrospectionState& state,
    std::string_view name,
    std::string_view message_type,
    const flowrt::IntrospectionChannelProbe& probe,
    const T& value,
    std::uint64_t published_at_ms
) {
    probe.record_publish_event();
    if (!probe.enabled() && !state.recorder_enabled_for_channel(name)) {
        return;
    }
    try {
        std::vector<std::uint8_t> payload(flowrt::detail::encoded_frame_size(value));
        flowrt::detail::encode_frame(value, payload);
        state.try_record_channel_sample_bytes(
            name,
            message_type,
            payload,
            std::optional<std::uint64_t>{published_at_ms});
        if (probe.enabled()) {
            probe.try_record_bytes(payload, std::optional<std::uint64_t>{published_at_ms});
        }
    } catch (...) {
    }
}

inline bool decode_json_string_fragment(std::string_view value, std::string& output) {
    if (value.size() < 2 || value.front() != '"' || value.back() != '"') {
        return false;
    }
    output.clear();
    for (std::size_t index = 1; index + 1 < value.size(); ++index) {
        const char byte = value[index];
        if (byte != '\\') {
            output.push_back(byte);
            continue;
        }
        if (index + 1 >= value.size() - 1) {
            return false;
        }
        const char escape = value[++index];
        switch (escape) {
            case '"':
            case '\\':
            case '/':
                output.push_back(escape);
                break;
            case 'b':
                output.push_back('\b');
                break;
            case 'f':
                output.push_back('\f');
                break;
            case 'n':
                output.push_back('\n');
                break;
            case 'r':
                output.push_back('\r');
                break;
            case 't':
                output.push_back('\t');
                break;
            default:
                return false;
        }
    }
    return true;
}

inline bool decode_flowrt_param_value(std::string_view value, bool& output) {
    if (value == "true") {
        output = true;
        return true;
    }
    if (value == "false") {
        output = false;
        return true;
    }
    return false;
}

template <typename T>
bool decode_flowrt_param_value(std::string_view value, T& output)
    requires(std::is_integral_v<T> && !std::is_same_v<T, bool>)
{
    std::string owned{value};
    char* end = nullptr;
    errno = 0;
    if constexpr (std::is_signed_v<T>) {
        const long long parsed = std::strtoll(owned.c_str(), &end, 10);
        if (errno != 0 || end == owned.c_str() || *end != '\0') {
            return false;
        }
        if (parsed < static_cast<long long>(std::numeric_limits<T>::min()) ||
            parsed > static_cast<long long>(std::numeric_limits<T>::max())) {
            return false;
        }
        output = static_cast<T>(parsed);
    } else {
        if (!owned.empty() && owned.front() == '-') {
            return false;
        }
        const unsigned long long parsed = std::strtoull(owned.c_str(), &end, 10);
        if (errno != 0 || end == owned.c_str() || *end != '\0') {
            return false;
        }
        if (parsed > static_cast<unsigned long long>(std::numeric_limits<T>::max())) {
            return false;
        }
        output = static_cast<T>(parsed);
    }
    return true;
}

inline bool decode_flowrt_param_value(std::string_view value, float& output) {
    std::string owned{value};
    char* end = nullptr;
    errno = 0;
    const float parsed = std::strtof(owned.c_str(), &end);
    if (errno != 0 || end == owned.c_str() || *end != '\0' || !std::isfinite(parsed)) {
        return false;
    }
    output = parsed;
    return true;
}

inline bool decode_flowrt_param_value(std::string_view value, double& output) {
    std::string owned{value};
    char* end = nullptr;
    errno = 0;
    const double parsed = std::strtod(owned.c_str(), &end);
    if (errno != 0 || end == owned.c_str() || *end != '\0' || !std::isfinite(parsed)) {
        return false;
    }
    output = parsed;
    return true;
}

inline bool decode_flowrt_param_value(std::string_view value, std::string& output) {
    return decode_json_string_fragment(value, output);
}

}  // namespace

namespace flowrt_app {

App::App(
    std::unique_ptr<FlakyInterface> flaky
)
    : flaky_(std::move(flaky)) {
}

flowrt::Status App::step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    {
        const auto flaky_sample_read = boundary_input_feed_.read_at(tick_time_ms);
        const auto flaky_sample = flaky_sample_read.view();
        health_map["flaky.main"].name = "flaky.main";
        health_map["flaky.main"].lane = "flaky_serial";
        if (flaky_sample.present()) {
            flowrt::Output<Sample> flaky_echo;
            if (flaky_) {
                switch (flaky_->on_tick(flaky_sample, flaky_echo)) {
                    case flowrt::Status::Ok:
                        break;
                    case flowrt::Status::Retry:
                        return flowrt::Status::Retry;
                    case flowrt::Status::Error:
                        return flowrt::Status::Error;
                }
            }
            if (const auto* value = flaky_echo.as_ref()) {
                boundary_output_emit_.publish_at(*value, tick_time_ms);
            }
        }
    }
    return flowrt::Status::Ok;
}

flowrt::Status App::step_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

flowrt::Status App::step_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

FlowrtTaskOutcome App::step_task_flaky_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    std::vector<FlowrtOutputCommit> flowrt_output_commits;
    {
        const auto flaky_sample_read = boundary_input_feed_.read_at(tick_time_ms);
        const auto flaky_sample = flaky_sample_read.view();
        health_map["flaky.main"].name = "flaky.main";
        health_map["flaky.main"].lane = "flaky_serial";
        if (flaky_sample.present()) {
            flowrt::Output<Sample> flaky_echo;
            if (flaky_) {
                switch (flaky_->on_tick(flaky_sample, flaky_echo)) {
                    case flowrt::Status::Ok:
                        break;
                    case flowrt::Status::Retry:
                        return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{});
                    case flowrt::Status::Error:
                        return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
                }
            }
            if (const auto* value = flaky_echo.as_ref()) {
                auto flowrt_payload_0 = *value;
                flowrt_output_commits.emplace_back([flowrt_payload_0 = std::move(flowrt_payload_0), tick_time_ms](App& app, flowrt::IntrospectionState& /*introspection_state*/, flowrt::ScheduleWaiter& /*scheduler_events*/, std::map<std::string, flowrt::IntrospectionTaskHealth>& /*health_map*/) mutable {
                    const auto* value = &flowrt_payload_0;
                    app.boundary_output_emit_.publish_at(*value, tick_time_ms);
                    return flowrt::Status::Ok;
                });
            }
        }
    }
    return FlowrtTaskOutcome::ok(std::move(flowrt_output_commits));
}

flowrt::Status App::step_process_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    {
        const auto flaky_sample_read = boundary_input_feed_.read_at(tick_time_ms);
        const auto flaky_sample = flaky_sample_read.view();
        health_map["flaky.main"].name = "flaky.main";
        health_map["flaky.main"].lane = "flaky_serial";
        if (flaky_sample.present()) {
            flowrt::Output<Sample> flaky_echo;
            if (flaky_) {
                switch (flaky_->on_tick(flaky_sample, flaky_echo)) {
                    case flowrt::Status::Ok:
                        break;
                    case flowrt::Status::Retry:
                        return flowrt::Status::Retry;
                    case flowrt::Status::Error:
                        return flowrt::Status::Error;
                }
            }
            if (const auto* value = flaky_echo.as_ref()) {
                boundary_output_emit_.publish_at(*value, tick_time_ms);
            }
        }
    }
    return flowrt::Status::Ok;
}

flowrt::Status App::step_process_main_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

flowrt::Status App::step_process_main_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

FlowrtTaskOutcome App::step_process_main_task_flaky_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    std::vector<FlowrtOutputCommit> flowrt_output_commits;
    {
        const auto flaky_sample_read = boundary_input_feed_.read_at(tick_time_ms);
        const auto flaky_sample = flaky_sample_read.view();
        health_map["flaky.main"].name = "flaky.main";
        health_map["flaky.main"].lane = "flaky_serial";
        if (flaky_sample.present()) {
            flowrt::Output<Sample> flaky_echo;
            if (flaky_) {
                switch (flaky_->on_tick(flaky_sample, flaky_echo)) {
                    case flowrt::Status::Ok:
                        break;
                    case flowrt::Status::Retry:
                        return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{});
                    case flowrt::Status::Error:
                        return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
                }
            }
            if (const auto* value = flaky_echo.as_ref()) {
                auto flowrt_payload_0 = *value;
                flowrt_output_commits.emplace_back([flowrt_payload_0 = std::move(flowrt_payload_0), tick_time_ms](App& app, flowrt::IntrospectionState& /*introspection_state*/, flowrt::ScheduleWaiter& /*scheduler_events*/, std::map<std::string, flowrt::IntrospectionTaskHealth>& /*health_map*/) mutable {
                    const auto* value = &flowrt_payload_0;
                    app.boundary_output_emit_.publish_at(*value, tick_time_ms);
                    return flowrt::Status::Ok;
                });
            }
        }
    }
    return FlowrtTaskOutcome::ok(std::move(flowrt_output_commits));
}

flowrt::Status App::run(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {
    flowrt::Context lifecycle_context;
    auto status = flowrt::Status::Ok;
    (void)backend;
    auto shutdown = flowrt::install_signal_shutdown_token();
    flowrt::IntrospectionState introspection_state;
    flowrt::ScheduleWaiter scheduler_events;
    boundary_input_feed_.set_schedule_waiter(scheduler_events);
    introspection_state.set_self_description_json(std::string{flowrt_app::self_description_json()});
    introspection_state.register_boundary_input("feed", "Sample", boundary_input_feed_);
    introspection_state.register_channel("emit", "Sample");
    auto boundary_output_emit_probe = boundary_output_emit_.register_sink(
        [&introspection_state](const Sample& value, std::optional<std::uint64_t> published_at_ms) {
            try {
                std::vector<std::uint8_t> payload(flowrt::detail::encoded_frame_size(value));
                flowrt::detail::encode_frame(value, std::span<std::uint8_t>{payload});
                introspection_state.record_channel_publish_bytes("emit", "Sample", std::move(payload), published_at_ms);
            } catch (...) {
            }
        });
    (void)boundary_output_emit_probe;
    auto introspection_server = flowrt::spawn_status_server(
        flowrt::IntrospectionIdentity{
            .self_description_hash = std::string{flowrt_app::self_description_hash()},
            .package = "fault_injection_restart_demo",
            .process = "main",
            .runtime = "cpp",
        },
        introspection_state);
    (void)introspection_server;
    bool flaky_initialized = false;
    bool flaky_started = false;
    introspection_state.record_lifecycle_state("flaky", flowrt::LifecycleState::Uninitialized);
    if (status == flowrt::Status::Ok && flaky_) {
        status = flaky_->on_init(lifecycle_context);
        flaky_initialized = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("flaky", flaky_initialized ? flowrt::LifecycleState::Initialized : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok && flaky_initialized && flaky_) {
        status = flaky_->on_start(lifecycle_context);
        flaky_started = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("flaky", flaky_started ? flowrt::LifecycleState::Running : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> startup_health_map;
        status = step_startup(0, lifecycle_context, introspection_state, scheduler_events, startup_health_map);
    }
    flowrt::DeterministicExecutor scheduler{1};
    flowrt::WorkerPool worker_pool{1};
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("flaky_serial")}, flowrt::LaneKind::Serial);
    (void)"flaky_serial";
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{flowrt::fnv1a64("flaky_serial")}, .priority = 0});
    std::uint64_t boundary_input_feed_seen_revision_for_flaky_main = 0;
    const auto scheduler_base_period_ms = std::uint64_t{1};
    std::size_t tick_base = 0;
    std::uint64_t scheduler_now_ms = 0;
    std::map<std::string, flowrt::IntrospectionTaskHealth> health_map;
    constexpr std::uint64_t fairness_starvation_threshold = 10;
    const std::set<std::string> replay_boundary_inputs = {"feed"};
    std::optional<flowrt::ReplayDriver> replay_time_driver;
    {
        const char* replay_source = std::getenv("FLOWRT_REPLAY_SOURCE");
        if (replay_source != nullptr && replay_source[0] != '\0') {
            auto replay_loaded = flowrt::replay_driver_from_timeline_file(replay_source, replay_boundary_inputs);
            if (std::holds_alternative<flowrt::ReplayDriver>(replay_loaded)) {
                replay_time_driver = std::move(std::get<flowrt::ReplayDriver>(replay_loaded));
            } else {
                std::fprintf(stderr, "FlowRT: 无法加载 FLOWRT_REPLAY_SOURCE `%s`: %s\n", replay_source, std::get<std::string>(replay_loaded).c_str());
                status = flowrt::Status::Error;
            }
        }
    }
    const auto clock_source = std::string_view{"simulated_replay"};
    const auto task_clock_source = flowrt::ClockSource::Replay;
    flowrt::WorkerCompletionQueue<std::vector<FlowrtOutputCommit>> task_completion_queue;
    task_completion_queue.set_wake_callback([&scheduler_events]() { scheduler_events.notify_data(); });
    std::deque<flowrt::TaskId> pending_task_order;
    std::map<flowrt::TaskId, flowrt::TaskRunOutput<std::vector<FlowrtOutputCommit>>> pending_task_results;
    std::map<flowrt::TaskId, flowrt::TaskAdmission> pending_task_admissions;
    std::mutex task_health_mutex;
    std::map<std::string, flowrt::IntrospectionTaskHealth> task_health_from_workers;
    std::map<flowrt::TaskId, std::uint64_t> task_last_scheduled_time_ms;
    std::map<flowrt::TaskId, std::uint64_t> task_last_observed_time_ms;
    std::optional<std::uint64_t> flaky_next_restart_ms;
    std::uint32_t flaky_fault_consecutive = 0;
    bool flaky_terminal_faulted = false;
    std::uint64_t __inject_count_1 = 0;
    while (status == flowrt::Status::Ok && !shutdown.is_requested() && ((!run_ticks.has_value() || tick_base < *run_ticks) || !pending_task_order.empty())) {
        std::uint64_t observed_data_generation = scheduler_events.data_generation();
        const auto tick_time_ms = scheduler_now_ms;
        scheduler.advance_to(std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(tick_time_ms)});
        scheduler.set_current_tick(static_cast<std::uint64_t>(tick_base));
        if (flaky_next_restart_ms.has_value() && scheduler_now_ms >= *flaky_next_restart_ms) {
            flaky_next_restart_ms.reset();
            auto flaky_restart_status = flaky_ ? flaky_->on_init(lifecycle_context) : flowrt::Status::Error;
            if (flaky_restart_status == flowrt::Status::Ok) {
                flaky_restart_status = flaky_->on_start(lifecycle_context);
            }
            if (flaky_restart_status == flowrt::Status::Ok) {
                flaky_fault_consecutive = 0;
                introspection_state.record_lifecycle_state("flaky", flowrt::LifecycleState::Running);
                    scheduler.resume_task(flowrt::TaskId{1});
            } else {
                flaky_fault_consecutive += 1;
                introspection_state.record_lifecycle_state("flaky", flowrt::LifecycleState::Faulted);
                if (flaky_fault_consecutive >= 2U) {
                    flaky_terminal_faulted = true;
                } else {
                    flaky_next_restart_ms = scheduler_now_ms + std::min<std::uint64_t>(10ULL << std::min<std::uint32_t>(flaky_fault_consecutive, 31U), 40ULL);
                }
            }
        }
        {
            auto& health = health_map["flaky.main"];
            health.name = "flaky.main";
            health.lane = "flaky_serial";
        }
        introspection_state.record_tick(tick_time_ms, clock_source);
        while (true) {
            observed_data_generation = scheduler_events.data_generation();
            bool woke_on_message = false;
            if (boundary_input_feed_.revision() != boundary_input_feed_seen_revision_for_flaky_main) {
                boundary_input_feed_seen_revision_for_flaky_main = boundary_input_feed_.revision();
                scheduler.wake(flowrt::TaskId{1});
                woke_on_message = true;
            }
            for (auto task_result : task_completion_queue.drain_completed()) {
                pending_task_results.insert_or_assign(task_result.task, std::move(task_result));
            }
            {
                std::lock_guard<std::mutex> lock(task_health_mutex);
                for (auto &[name, health] : task_health_from_workers) {
                    health_map.insert_or_assign(name, std::move(health));
                }
                task_health_from_workers.clear();
            }
            auto ready_batch = scheduler.take_ready_batch();
            const auto submitted_task_count = ready_batch.size();
            for (const auto admission : ready_batch.admissions()) {
                const auto scheduled_delta_ms = [&]() -> std::uint64_t {
                    const auto [it, inserted] = task_last_scheduled_time_ms.insert_or_assign(admission.task, admission.scheduled_time_ms);
                    return inserted || admission.scheduled_time_ms < it->second ? 0U : admission.scheduled_time_ms - it->second;
                }();
                const auto observed_delta_ms = [&]() -> std::uint64_t {
                    const auto [it, inserted] = task_last_observed_time_ms.insert_or_assign(admission.task, admission.observed_time_ms);
                    return inserted || admission.observed_time_ms < it->second ? 0U : admission.observed_time_ms - it->second;
                }();
                bool flowrt_inject_fault = false;
                switch (admission.task.value) {
                    case 1: { ++__inject_count_1; flowrt_inject_fault = __inject_count_1 >= 1ULL; break; }
                    default: break;
                }
                const auto submitted = worker_pool.submit_collect(admission.task, task_completion_queue, [this, &introspection_state, &scheduler_events, &task_health_mutex, &task_health_from_workers, admission, scheduled_delta_ms, observed_delta_ms, task_clock_source, tick_base, tick_time_ms, flowrt_inject_fault]() {
                    auto local_health_map = std::map<std::string, flowrt::IntrospectionTaskHealth>{};
                    const auto [task_name, task_trigger] = [&]() -> std::pair<std::string_view, std::string_view> {
                        switch (admission.task.value) {
                            case 1: return {"flaky.main", "on_message"};
                            default: return {"__flowrt_hidden", "on_message"};
                        }
                    }();
                    auto local_context = flowrt::Context::with_timing(flowrt::TaskTiming{
                        .step = static_cast<std::uint64_t>(tick_base),
                        .task_name = std::string{task_name},
                        .trigger = std::string{task_trigger},
                        .clock_source = task_clock_source,
                        .scheduled_time_ms = admission.scheduled_time_ms,
                        .observed_time_ms = admission.observed_time_ms,
                        .scheduled_delta_ms = scheduled_delta_ms,
                        .observed_delta_ms = observed_delta_ms,
                        .period_ms = admission.period_ms,
                        .deadline_ms = admission.deadline_ms,
                        .lateness_ms = admission.lateness_ms,
                        .missed_periods = admission.missed_periods,
                        .deadline_missed = admission.deadline_ms.has_value() && admission.lateness_ms > *admission.deadline_ms,
                        .overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms),
                    });
                    auto merge_local_health = [&task_health_mutex, &task_health_from_workers, admission, task_name](std::map<std::string, flowrt::IntrospectionTaskHealth>&& local_health_map) {
                        auto health_it = local_health_map.find(std::string{task_name});
                        if (health_it != local_health_map.end()) {
                            auto& health = health_it->second;
                            health.inflight = false;
                            health.scheduled_time_ms = admission.scheduled_time_ms;
                            health.observed_time_ms = admission.observed_time_ms;
                            health.lateness_ms = admission.lateness_ms;
                            health.missed_periods = admission.missed_periods;
                            health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);
                        }
                        std::lock_guard<std::mutex> lock(task_health_mutex);
                        for (auto &[name, health] : local_health_map) {
                            task_health_from_workers.insert_or_assign(name, std::move(health));
                        }
                    };
                    switch (admission.task.value) {
                    case 1: {
auto flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId{flowrt::fnv1a64("flaky_serial")});
(void)flowrt_lane_guard;
auto task_outcome = flowrt_inject_fault ? FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{}) : step_task_flaky_main(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
merge_local_health(std::move(local_health_map));
return task_outcome;
}
                    default: return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
                }
                });
                if (submitted.accepted) {
                    pending_task_order.push_back(admission.task);
                    pending_task_admissions.insert_or_assign(admission.task, admission);
                    switch (admission.task.value) {
                        case 1: {
auto& health = health_map["flaky.main"];
health.name = "flaky.main";
health.lane = "flaky_serial";
health.inflight = true;
health.scheduled_time_ms = admission.scheduled_time_ms;
health.observed_time_ms = admission.observed_time_ms;
health.lateness_ms = admission.lateness_ms;
health.missed_periods = admission.missed_periods;
health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);
break;
}
                        default:
                            break;
                    }
                } else {
                    (void)scheduler.complete_task(admission.task);
                    status = flowrt::Status::Error;
                    break;
                }
            }
            if (status != flowrt::Status::Ok) {
                break;
            }
            std::size_t committed_task_count = 0;
            while (!pending_task_order.empty()) {
                const auto task = pending_task_order.front();
                const auto result_it = pending_task_results.find(task);
                if (result_it == pending_task_results.end()) {
                    break;
                }
                auto task_result = std::move(result_it->second);
                pending_task_results.erase(result_it);
                pending_task_order.pop_front();
                (void)scheduler.complete_task(task_result.task);
                ++committed_task_count;
                switch (task_result.task.value) {
                    case 1: {
auto& health = health_map["flaky.main"];
health.name = "flaky.main";
health.lane = "flaky_serial";
health.inflight = false;
if (const auto admission_it = pending_task_admissions.find(task_result.task); admission_it != pending_task_admissions.end()) {
const auto& admission = admission_it->second;
health.scheduled_time_ms = admission.scheduled_time_ms;
health.observed_time_ms = admission.observed_time_ms;
health.lateness_ms = admission.lateness_ms;
health.missed_periods = admission.missed_periods;
health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);
pending_task_admissions.erase(admission_it);
}
health.run_count += 1;
health.last_run_ms = tick_time_ms;
if (task_result.status == flowrt::Status::Ok) {
health.success_count += 1;
health.consecutive_failures = 0;
health.last_success_ms = tick_time_ms;
} else if (task_result.status == flowrt::Status::Error) {
health.consecutive_failures += 1;
}
break;
}
                    default:
                        break;
                }
                if (task_result.status == flowrt::Status::Error) {
                    switch (task_result.task.value) {
                        case 1:
                        {
                            introspection_state.record_lifecycle_state("flaky", flowrt::LifecycleState::Faulted);
                            scheduler.suspend_task(flowrt::TaskId{1});
                            if (!flaky_terminal_faulted) {
                                flaky_next_restart_ms = scheduler_now_ms + std::min<std::uint64_t>(10ULL << std::min<std::uint32_t>(flaky_fault_consecutive, 31U), 40ULL);
                            }
                            break;
                        }
                        default:
                            status = flowrt::Status::Error;
                            break;
                    }
                }
                if (task_result.outputs.has_value()) {
                    for (auto& commit : *task_result.outputs) {
                        const auto commit_status = commit(*this, introspection_state, scheduler_events, health_map);
                        if (commit_status == flowrt::Status::Error) {
                            status = flowrt::Status::Error;
                            break;
                        }
                        if (commit_status == flowrt::Status::Retry) {
                            status = flowrt::Status::Retry;
                            break;
                        }
                    }
                }
                if (status != flowrt::Status::Ok) {
                    break;
                }
            }
            if (status != flowrt::Status::Ok) {
                break;
            }
            if (committed_task_count == 0U || (!woke_on_message && submitted_task_count == 0U)) {
                break;
            }
        }
        // 公平性检测：检查 lane 饥饿。
        if (scheduler.lane_starvation_ticks(flowrt::LaneId{flowrt::fnv1a64("flaky_serial")}) > fairness_starvation_threshold) {
            for (auto &[name, health] : health_map) {
                if (health.lane == "flaky_serial") {
                    health.fairness_violations += 1;
                }
            }
        }
        // 将本轮健康快照写入 introspection。
        for (auto &[name, health] : health_map) {
            introspection_state.record_task_health(std::move(health));
        }
        health_map.clear();
        if (status == flowrt::Status::Ok) {
            ++tick_base;
            if (replay_time_driver.has_value()) {
                auto& replay_driver = *replay_time_driver;
                const auto next_periodic_deadline_chrono = std::optional<std::chrono::milliseconds>{};
                const auto replay_next_periodic_deadline_ms = next_periodic_deadline_chrono.has_value()
                    ? std::optional<std::uint64_t>{static_cast<std::uint64_t>(next_periodic_deadline_chrono->count())}
                    : std::nullopt;
                const auto replay_step = replay_driver.next_step(replay_next_periodic_deadline_ms, shutdown);
                if (replay_step == flowrt::Step::Shutdown) {
                    break;
                }
                scheduler_now_ms = replay_driver.now_ms();
                if (replay_step == flowrt::Step::Data) {
                    for (const auto& replay_event : replay_driver.take_pending_events()) {
                        (void)introspection_state.publish_boundary_input(replay_event.target, std::span<const std::uint8_t>{replay_event.payload.data(), replay_event.payload.size()}, std::optional<std::uint64_t>{replay_event.time_ms});
                    }
                }
            } else {
                switch (scheduler_events.wait_until_after(observed_data_generation, std::nullopt, shutdown)) {
                    case flowrt::ScheduleEvent::Shutdown:
                        status = flowrt::Status::Ok;
                        break;
                    case flowrt::ScheduleEvent::Timer:
                        break;
                    case flowrt::ScheduleEvent::Data:
                        if (const auto data_time_ms = scheduler_events.take_data_time_ms()) {
                            scheduler_now_ms = std::max(scheduler_now_ms, *data_time_ms);
                        }
                        break;
                }
                if (shutdown.is_requested()) {
                    break;
                }
            }
        }
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> shutdown_health_map;
        status = step_shutdown(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);
    }
    if (flaky_started && flaky_) {
        const auto stop_status = flaky_->on_stop(lifecycle_context);
        if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("flaky", stop_status == flowrt::Status::Ok ? flowrt::LifecycleState::Stopped : flowrt::LifecycleState::Faulted);
    }
    if (flaky_initialized && flaky_) {
        const auto shutdown_status = flaky_->on_shutdown(lifecycle_context);
        if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("flaky", shutdown_status == flowrt::Status::Ok ? flowrt::LifecycleState::ShutDown : flowrt::LifecycleState::Faulted);
    }
    return status;
}

flowrt::Status App::run_process(const flowrt::Backend& backend, std::string_view process, std::optional<std::size_t> run_ticks) {
    if (process == "main") {
        return run_process_main(backend, run_ticks);
    }
    return flowrt::Status::Error;
}

flowrt::Status App::run_process_main(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {
    flowrt::Context lifecycle_context;
    auto status = flowrt::Status::Ok;
    (void)backend;
    auto shutdown = flowrt::install_signal_shutdown_token();
    flowrt::IntrospectionState introspection_state;
    flowrt::ScheduleWaiter scheduler_events;
    boundary_input_feed_.set_schedule_waiter(scheduler_events);
    introspection_state.set_self_description_json(std::string{flowrt_app::self_description_json()});
    introspection_state.register_boundary_input("feed", "Sample", boundary_input_feed_);
    introspection_state.register_channel("emit", "Sample");
    auto boundary_output_emit_probe = boundary_output_emit_.register_sink(
        [&introspection_state](const Sample& value, std::optional<std::uint64_t> published_at_ms) {
            try {
                std::vector<std::uint8_t> payload(flowrt::detail::encoded_frame_size(value));
                flowrt::detail::encode_frame(value, std::span<std::uint8_t>{payload});
                introspection_state.record_channel_publish_bytes("emit", "Sample", std::move(payload), published_at_ms);
            } catch (...) {
            }
        });
    (void)boundary_output_emit_probe;
    auto introspection_server = flowrt::spawn_status_server(
        flowrt::IntrospectionIdentity{
            .self_description_hash = std::string{flowrt_app::self_description_hash()},
            .package = "fault_injection_restart_demo",
            .process = "main",
            .runtime = "cpp",
        },
        introspection_state);
    (void)introspection_server;
    bool flaky_initialized = false;
    bool flaky_started = false;
    introspection_state.record_lifecycle_state("flaky", flowrt::LifecycleState::Uninitialized);
    if (status == flowrt::Status::Ok && flaky_) {
        status = flaky_->on_init(lifecycle_context);
        flaky_initialized = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("flaky", flaky_initialized ? flowrt::LifecycleState::Initialized : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok && flaky_initialized && flaky_) {
        status = flaky_->on_start(lifecycle_context);
        flaky_started = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("flaky", flaky_started ? flowrt::LifecycleState::Running : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> startup_health_map;
        status = step_process_main_startup(0, lifecycle_context, introspection_state, scheduler_events, startup_health_map);
    }
    flowrt::DeterministicExecutor scheduler{1};
    flowrt::WorkerPool worker_pool{1};
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("flaky_serial")}, flowrt::LaneKind::Serial);
    (void)"flaky_serial";
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{flowrt::fnv1a64("flaky_serial")}, .priority = 0});
    std::uint64_t boundary_input_feed_seen_revision_for_flaky_main = 0;
    const auto scheduler_base_period_ms = std::uint64_t{1};
    std::size_t tick_base = 0;
    std::uint64_t scheduler_now_ms = 0;
    std::map<std::string, flowrt::IntrospectionTaskHealth> health_map;
    constexpr std::uint64_t fairness_starvation_threshold = 10;
    const std::set<std::string> replay_boundary_inputs = {"feed"};
    std::optional<flowrt::ReplayDriver> replay_time_driver;
    {
        const char* replay_source = std::getenv("FLOWRT_REPLAY_SOURCE");
        if (replay_source != nullptr && replay_source[0] != '\0') {
            auto replay_loaded = flowrt::replay_driver_from_timeline_file(replay_source, replay_boundary_inputs);
            if (std::holds_alternative<flowrt::ReplayDriver>(replay_loaded)) {
                replay_time_driver = std::move(std::get<flowrt::ReplayDriver>(replay_loaded));
            } else {
                std::fprintf(stderr, "FlowRT: 无法加载 FLOWRT_REPLAY_SOURCE `%s`: %s\n", replay_source, std::get<std::string>(replay_loaded).c_str());
                status = flowrt::Status::Error;
            }
        }
    }
    const auto clock_source = std::string_view{"simulated_replay"};
    const auto task_clock_source = flowrt::ClockSource::Replay;
    flowrt::WorkerCompletionQueue<std::vector<FlowrtOutputCommit>> task_completion_queue;
    task_completion_queue.set_wake_callback([&scheduler_events]() { scheduler_events.notify_data(); });
    std::deque<flowrt::TaskId> pending_task_order;
    std::map<flowrt::TaskId, flowrt::TaskRunOutput<std::vector<FlowrtOutputCommit>>> pending_task_results;
    std::map<flowrt::TaskId, flowrt::TaskAdmission> pending_task_admissions;
    std::mutex task_health_mutex;
    std::map<std::string, flowrt::IntrospectionTaskHealth> task_health_from_workers;
    std::map<flowrt::TaskId, std::uint64_t> task_last_scheduled_time_ms;
    std::map<flowrt::TaskId, std::uint64_t> task_last_observed_time_ms;
    std::optional<std::uint64_t> flaky_next_restart_ms;
    std::uint32_t flaky_fault_consecutive = 0;
    bool flaky_terminal_faulted = false;
    std::uint64_t __inject_count_1 = 0;
    while (status == flowrt::Status::Ok && !shutdown.is_requested() && ((!run_ticks.has_value() || tick_base < *run_ticks) || !pending_task_order.empty())) {
        std::uint64_t observed_data_generation = scheduler_events.data_generation();
        const auto tick_time_ms = scheduler_now_ms;
        scheduler.advance_to(std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(tick_time_ms)});
        scheduler.set_current_tick(static_cast<std::uint64_t>(tick_base));
        if (flaky_next_restart_ms.has_value() && scheduler_now_ms >= *flaky_next_restart_ms) {
            flaky_next_restart_ms.reset();
            auto flaky_restart_status = flaky_ ? flaky_->on_init(lifecycle_context) : flowrt::Status::Error;
            if (flaky_restart_status == flowrt::Status::Ok) {
                flaky_restart_status = flaky_->on_start(lifecycle_context);
            }
            if (flaky_restart_status == flowrt::Status::Ok) {
                flaky_fault_consecutive = 0;
                introspection_state.record_lifecycle_state("flaky", flowrt::LifecycleState::Running);
                    scheduler.resume_task(flowrt::TaskId{1});
            } else {
                flaky_fault_consecutive += 1;
                introspection_state.record_lifecycle_state("flaky", flowrt::LifecycleState::Faulted);
                if (flaky_fault_consecutive >= 2U) {
                    flaky_terminal_faulted = true;
                } else {
                    flaky_next_restart_ms = scheduler_now_ms + std::min<std::uint64_t>(10ULL << std::min<std::uint32_t>(flaky_fault_consecutive, 31U), 40ULL);
                }
            }
        }
        {
            auto& health = health_map["flaky.main"];
            health.name = "flaky.main";
            health.lane = "flaky_serial";
        }
        introspection_state.record_tick(tick_time_ms, clock_source);
        while (true) {
            observed_data_generation = scheduler_events.data_generation();
            bool woke_on_message = false;
            if (boundary_input_feed_.revision() != boundary_input_feed_seen_revision_for_flaky_main) {
                boundary_input_feed_seen_revision_for_flaky_main = boundary_input_feed_.revision();
                scheduler.wake(flowrt::TaskId{1});
                woke_on_message = true;
            }
            for (auto task_result : task_completion_queue.drain_completed()) {
                pending_task_results.insert_or_assign(task_result.task, std::move(task_result));
            }
            {
                std::lock_guard<std::mutex> lock(task_health_mutex);
                for (auto &[name, health] : task_health_from_workers) {
                    health_map.insert_or_assign(name, std::move(health));
                }
                task_health_from_workers.clear();
            }
            auto ready_batch = scheduler.take_ready_batch();
            const auto submitted_task_count = ready_batch.size();
            for (const auto admission : ready_batch.admissions()) {
                const auto scheduled_delta_ms = [&]() -> std::uint64_t {
                    const auto [it, inserted] = task_last_scheduled_time_ms.insert_or_assign(admission.task, admission.scheduled_time_ms);
                    return inserted || admission.scheduled_time_ms < it->second ? 0U : admission.scheduled_time_ms - it->second;
                }();
                const auto observed_delta_ms = [&]() -> std::uint64_t {
                    const auto [it, inserted] = task_last_observed_time_ms.insert_or_assign(admission.task, admission.observed_time_ms);
                    return inserted || admission.observed_time_ms < it->second ? 0U : admission.observed_time_ms - it->second;
                }();
                bool flowrt_inject_fault = false;
                switch (admission.task.value) {
                    case 1: { ++__inject_count_1; flowrt_inject_fault = __inject_count_1 >= 1ULL; break; }
                    default: break;
                }
                const auto submitted = worker_pool.submit_collect(admission.task, task_completion_queue, [this, &introspection_state, &scheduler_events, &task_health_mutex, &task_health_from_workers, admission, scheduled_delta_ms, observed_delta_ms, task_clock_source, tick_base, tick_time_ms, flowrt_inject_fault]() {
                    auto local_health_map = std::map<std::string, flowrt::IntrospectionTaskHealth>{};
                    const auto [task_name, task_trigger] = [&]() -> std::pair<std::string_view, std::string_view> {
                        switch (admission.task.value) {
                            case 1: return {"flaky.main", "on_message"};
                            default: return {"__flowrt_hidden", "on_message"};
                        }
                    }();
                    auto local_context = flowrt::Context::with_timing(flowrt::TaskTiming{
                        .step = static_cast<std::uint64_t>(tick_base),
                        .task_name = std::string{task_name},
                        .trigger = std::string{task_trigger},
                        .clock_source = task_clock_source,
                        .scheduled_time_ms = admission.scheduled_time_ms,
                        .observed_time_ms = admission.observed_time_ms,
                        .scheduled_delta_ms = scheduled_delta_ms,
                        .observed_delta_ms = observed_delta_ms,
                        .period_ms = admission.period_ms,
                        .deadline_ms = admission.deadline_ms,
                        .lateness_ms = admission.lateness_ms,
                        .missed_periods = admission.missed_periods,
                        .deadline_missed = admission.deadline_ms.has_value() && admission.lateness_ms > *admission.deadline_ms,
                        .overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms),
                    });
                    auto merge_local_health = [&task_health_mutex, &task_health_from_workers, admission, task_name](std::map<std::string, flowrt::IntrospectionTaskHealth>&& local_health_map) {
                        auto health_it = local_health_map.find(std::string{task_name});
                        if (health_it != local_health_map.end()) {
                            auto& health = health_it->second;
                            health.inflight = false;
                            health.scheduled_time_ms = admission.scheduled_time_ms;
                            health.observed_time_ms = admission.observed_time_ms;
                            health.lateness_ms = admission.lateness_ms;
                            health.missed_periods = admission.missed_periods;
                            health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);
                        }
                        std::lock_guard<std::mutex> lock(task_health_mutex);
                        for (auto &[name, health] : local_health_map) {
                            task_health_from_workers.insert_or_assign(name, std::move(health));
                        }
                    };
                    switch (admission.task.value) {
                    case 1: {
auto flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId{flowrt::fnv1a64("flaky_serial")});
(void)flowrt_lane_guard;
auto task_outcome = flowrt_inject_fault ? FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{}) : step_process_main_task_flaky_main(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
merge_local_health(std::move(local_health_map));
return task_outcome;
}
                    default: return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
                }
                });
                if (submitted.accepted) {
                    pending_task_order.push_back(admission.task);
                    pending_task_admissions.insert_or_assign(admission.task, admission);
                    switch (admission.task.value) {
                        case 1: {
auto& health = health_map["flaky.main"];
health.name = "flaky.main";
health.lane = "flaky_serial";
health.inflight = true;
health.scheduled_time_ms = admission.scheduled_time_ms;
health.observed_time_ms = admission.observed_time_ms;
health.lateness_ms = admission.lateness_ms;
health.missed_periods = admission.missed_periods;
health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);
break;
}
                        default:
                            break;
                    }
                } else {
                    (void)scheduler.complete_task(admission.task);
                    status = flowrt::Status::Error;
                    break;
                }
            }
            if (status != flowrt::Status::Ok) {
                break;
            }
            std::size_t committed_task_count = 0;
            while (!pending_task_order.empty()) {
                const auto task = pending_task_order.front();
                const auto result_it = pending_task_results.find(task);
                if (result_it == pending_task_results.end()) {
                    break;
                }
                auto task_result = std::move(result_it->second);
                pending_task_results.erase(result_it);
                pending_task_order.pop_front();
                (void)scheduler.complete_task(task_result.task);
                ++committed_task_count;
                switch (task_result.task.value) {
                    case 1: {
auto& health = health_map["flaky.main"];
health.name = "flaky.main";
health.lane = "flaky_serial";
health.inflight = false;
if (const auto admission_it = pending_task_admissions.find(task_result.task); admission_it != pending_task_admissions.end()) {
const auto& admission = admission_it->second;
health.scheduled_time_ms = admission.scheduled_time_ms;
health.observed_time_ms = admission.observed_time_ms;
health.lateness_ms = admission.lateness_ms;
health.missed_periods = admission.missed_periods;
health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);
pending_task_admissions.erase(admission_it);
}
health.run_count += 1;
health.last_run_ms = tick_time_ms;
if (task_result.status == flowrt::Status::Ok) {
health.success_count += 1;
health.consecutive_failures = 0;
health.last_success_ms = tick_time_ms;
} else if (task_result.status == flowrt::Status::Error) {
health.consecutive_failures += 1;
}
break;
}
                    default:
                        break;
                }
                if (task_result.status == flowrt::Status::Error) {
                    switch (task_result.task.value) {
                        case 1:
                        {
                            introspection_state.record_lifecycle_state("flaky", flowrt::LifecycleState::Faulted);
                            scheduler.suspend_task(flowrt::TaskId{1});
                            if (!flaky_terminal_faulted) {
                                flaky_next_restart_ms = scheduler_now_ms + std::min<std::uint64_t>(10ULL << std::min<std::uint32_t>(flaky_fault_consecutive, 31U), 40ULL);
                            }
                            break;
                        }
                        default:
                            status = flowrt::Status::Error;
                            break;
                    }
                }
                if (task_result.outputs.has_value()) {
                    for (auto& commit : *task_result.outputs) {
                        const auto commit_status = commit(*this, introspection_state, scheduler_events, health_map);
                        if (commit_status == flowrt::Status::Error) {
                            status = flowrt::Status::Error;
                            break;
                        }
                        if (commit_status == flowrt::Status::Retry) {
                            status = flowrt::Status::Retry;
                            break;
                        }
                    }
                }
                if (status != flowrt::Status::Ok) {
                    break;
                }
            }
            if (status != flowrt::Status::Ok) {
                break;
            }
            if (committed_task_count == 0U || (!woke_on_message && submitted_task_count == 0U)) {
                break;
            }
        }
        // 公平性检测：检查 lane 饥饿。
        if (scheduler.lane_starvation_ticks(flowrt::LaneId{flowrt::fnv1a64("flaky_serial")}) > fairness_starvation_threshold) {
            for (auto &[name, health] : health_map) {
                if (health.lane == "flaky_serial") {
                    health.fairness_violations += 1;
                }
            }
        }
        // 将本轮健康快照写入 introspection。
        for (auto &[name, health] : health_map) {
            introspection_state.record_task_health(std::move(health));
        }
        health_map.clear();
        if (status == flowrt::Status::Ok) {
            ++tick_base;
            if (replay_time_driver.has_value()) {
                auto& replay_driver = *replay_time_driver;
                const auto next_periodic_deadline_chrono = std::optional<std::chrono::milliseconds>{};
                const auto replay_next_periodic_deadline_ms = next_periodic_deadline_chrono.has_value()
                    ? std::optional<std::uint64_t>{static_cast<std::uint64_t>(next_periodic_deadline_chrono->count())}
                    : std::nullopt;
                const auto replay_step = replay_driver.next_step(replay_next_periodic_deadline_ms, shutdown);
                if (replay_step == flowrt::Step::Shutdown) {
                    break;
                }
                scheduler_now_ms = replay_driver.now_ms();
                if (replay_step == flowrt::Step::Data) {
                    for (const auto& replay_event : replay_driver.take_pending_events()) {
                        (void)introspection_state.publish_boundary_input(replay_event.target, std::span<const std::uint8_t>{replay_event.payload.data(), replay_event.payload.size()}, std::optional<std::uint64_t>{replay_event.time_ms});
                    }
                }
            } else {
                switch (scheduler_events.wait_until_after(observed_data_generation, std::nullopt, shutdown)) {
                    case flowrt::ScheduleEvent::Shutdown:
                        status = flowrt::Status::Ok;
                        break;
                    case flowrt::ScheduleEvent::Timer:
                        break;
                    case flowrt::ScheduleEvent::Data:
                        if (const auto data_time_ms = scheduler_events.take_data_time_ms()) {
                            scheduler_now_ms = std::max(scheduler_now_ms, *data_time_ms);
                        }
                        break;
                }
                if (shutdown.is_requested()) {
                    break;
                }
            }
        }
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> shutdown_health_map;
        status = step_process_main_shutdown(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);
    }
    if (flaky_started && flaky_) {
        const auto stop_status = flaky_->on_stop(lifecycle_context);
        if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("flaky", stop_status == flowrt::Status::Ok ? flowrt::LifecycleState::Stopped : flowrt::LifecycleState::Faulted);
    }
    if (flaky_initialized && flaky_) {
        const auto shutdown_status = flaky_->on_shutdown(lifecycle_context);
        if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("flaky", shutdown_status == flowrt::Status::Ok ? flowrt::LifecycleState::ShutDown : flowrt::LifecycleState::Faulted);
    }
    return status;
}

flowrt::Status run(std::optional<std::size_t> run_ticks) {
    auto backend = flowrt::inproc_backend();
    return flowrt_user::build_app().run(backend, run_ticks);
}

flowrt::Status run_process(std::string_view process, std::optional<std::size_t> run_ticks) {
    auto backend = flowrt::inproc_backend();
    return flowrt_user::build_app().run_process(backend, process, run_ticks);
}

}  // namespace flowrt_app
