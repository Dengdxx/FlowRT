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
#include <fstream>
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
    std::unique_ptr<ControllerInterface> controller,
    std::unique_ptr<NavigatorInterface> navigator
)
    : controller_(std::move(controller)),
      navigator_(std::move(navigator)) {
    {
        const auto operation_policy_0 = flowrt::OperationPolicy::make(
            std::chrono::milliseconds{5000},
            flowrt::OperationConcurrencyPolicy::Reject,
            flowrt::OperationPreemptPolicy::Reject,
            4U,
            1U,
            std::chrono::milliseconds{60000});
        this->operation_control_0_ = std::make_shared<flowrt::OperationControl>(
            flowrt::fnv1a64("controller.plan"),
            operation_policy_0.value());
    }
}

flowrt::Status App::step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    {
        health_map["controller.main"].name = "controller.main";
        health_map["controller.main"].lane = "controller_serial";
        if (controller_) {
            switch (controller_->on_tick(operation_client_controller_plan_)) {
                case flowrt::Status::Ok:
                    break;
                case flowrt::Status::Retry:
                    return flowrt::Status::Retry;
                case flowrt::Status::Error:
                    return flowrt::Status::Error;
            }
        }
    }
    {
        health_map["navigator.main"].name = "navigator.main";
        health_map["navigator.main"].lane = "navigator_serial";
        if (navigator_) {
            switch (navigator_->on_tick()) {
                case flowrt::Status::Ok:
                    break;
                case flowrt::Status::Retry:
                    return flowrt::Status::Retry;
                case flowrt::Status::Error:
                    return flowrt::Status::Error;
            }
        }
    }
    return flowrt::Status::Ok;
}

flowrt::Status App::step_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

flowrt::Status App::step_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

FlowrtTaskOutcome App::step_task_controller_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    std::vector<FlowrtOutputCommit> flowrt_output_commits;
    {
        health_map["controller.main"].name = "controller.main";
        health_map["controller.main"].lane = "controller_serial";
        if (controller_) {
            switch (controller_->on_tick(operation_client_controller_plan_)) {
                case flowrt::Status::Ok:
                    break;
                case flowrt::Status::Retry:
                    return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{});
                case flowrt::Status::Error:
                    return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
            }
        }
    }
    return FlowrtTaskOutcome::ok(std::move(flowrt_output_commits));
}

FlowrtTaskOutcome App::step_task_navigator_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    std::vector<FlowrtOutputCommit> flowrt_output_commits;
    {
        health_map["navigator.main"].name = "navigator.main";
        health_map["navigator.main"].lane = "navigator_serial";
        if (navigator_) {
            switch (navigator_->on_tick()) {
                case flowrt::Status::Ok:
                    break;
                case flowrt::Status::Retry:
                    return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{});
                case flowrt::Status::Error:
                    return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
            }
        }
    }
    return FlowrtTaskOutcome::ok(std::move(flowrt_output_commits));
}

flowrt::Status App::step_process_client_proc(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    {
        health_map["controller.main"].name = "controller.main";
        health_map["controller.main"].lane = "controller_serial";
        if (controller_) {
            switch (controller_->on_tick(operation_client_controller_plan_)) {
                case flowrt::Status::Ok:
                    break;
                case flowrt::Status::Retry:
                    return flowrt::Status::Retry;
                case flowrt::Status::Error:
                    return flowrt::Status::Error;
            }
        }
    }
    return flowrt::Status::Ok;
}

flowrt::Status App::step_process_client_proc_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

flowrt::Status App::step_process_client_proc_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

FlowrtTaskOutcome App::step_process_client_proc_task_controller_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    std::vector<FlowrtOutputCommit> flowrt_output_commits;
    {
        health_map["controller.main"].name = "controller.main";
        health_map["controller.main"].lane = "controller_serial";
        if (controller_) {
            switch (controller_->on_tick(operation_client_controller_plan_)) {
                case flowrt::Status::Ok:
                    break;
                case flowrt::Status::Retry:
                    return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{});
                case flowrt::Status::Error:
                    return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
            }
        }
    }
    return FlowrtTaskOutcome::ok(std::move(flowrt_output_commits));
}

flowrt::Status App::step_process_server_proc(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    {
        health_map["navigator.main"].name = "navigator.main";
        health_map["navigator.main"].lane = "navigator_serial";
        if (navigator_) {
            switch (navigator_->on_tick()) {
                case flowrt::Status::Ok:
                    break;
                case flowrt::Status::Retry:
                    return flowrt::Status::Retry;
                case flowrt::Status::Error:
                    return flowrt::Status::Error;
            }
        }
    }
    return flowrt::Status::Ok;
}

flowrt::Status App::step_process_server_proc_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

flowrt::Status App::step_process_server_proc_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    return flowrt::Status::Ok;
}

FlowrtTaskOutcome App::step_process_server_proc_task_navigator_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    std::vector<FlowrtOutputCommit> flowrt_output_commits;
    {
        health_map["navigator.main"].name = "navigator.main";
        health_map["navigator.main"].lane = "navigator_serial";
        if (navigator_) {
            switch (navigator_->on_tick()) {
                case flowrt::Status::Ok:
                    break;
                case flowrt::Status::Retry:
                    return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{});
                case flowrt::Status::Error:
                    return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
            }
        }
    }
    return FlowrtTaskOutcome::ok(std::move(flowrt_output_commits));
}

flowrt::Status App::step_operation_navigator_plan(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    (void)tick;
    (void)tick_context;
    (void)scheduler_events;
    (void)health_map;
    introspection_state.register_operation_cancel_handler("controller.plan", [this](std::string_view operation_id) -> std::variant<flowrt::IntrospectionOperationStatus, std::string> {
        const auto snapshot = this->operation_control_0_->snapshot();
        if (flowrt_operation_id_string(snapshot.id) != operation_id) {
            return std::string{"stale operation invocation `"} + std::string{operation_id} + "`; current is `" + flowrt_operation_id_string(snapshot.id) + "`";
        }
        if (const auto error = this->operation_control_0_->request_cancel(snapshot.id, snapshot.owner); error != flowrt::OperationControlError::Ok) {
            return std::string{flowrt::to_string(error)};
        }
        return flowrt_operation_status_from_snapshot("controller.plan", "controller.plan", this->operation_control_0_->snapshot());
    });
    if (operation_start_server_navigator_plan_.has_value()) {
        const auto handled = operation_start_server_navigator_plan_->poll_requests([this](const flowrt::OperationStartRequest<PlanGoal>& request) -> flowrt::ServiceResult<flowrt::OperationStartAck> {
            auto operation_worker_server = this->navigator_;
            if (!operation_worker_server) {
                return flowrt::ServiceResult<flowrt::OperationStartAck>::err(flowrt::ServiceError::Unavailable);
            }
            auto operation_control = this->operation_control_0_;
            const auto started = operation_control->start_with_timeout(request.owner, flowrt::monotonic_time_ms(), request.timeout);
            if (!started.has_value()) {
                return flowrt_operation_control_error<flowrt::OperationStartAck>(started.error());
            }
            const auto ack = started.value();
            const auto id = ack.id;
            auto goal_for_worker = request.goal;
            try {
                std::thread([operation_worker_server, operation_control, id, goal_for_worker = std::move(goal_for_worker)]() mutable {
                    while (true) {
                        const auto status = operation_control->status(id);
                        if (!status.has_value() || flowrt::is_terminal(status->state)) {
                            return;
                        }
                        if (operation_control->ready_to_run(id)) {
                            break;
                        }
                        std::this_thread::sleep_for(std::chrono::milliseconds{1});
                    }
                    const auto cancel = operation_control->cancel_token_for(id);
                    if (!cancel.has_value()) {
                        return;
                    }
                    if (const auto error = operation_control->mark_running(id); error != flowrt::OperationControlError::Ok) {
                        return;
                    }
                    auto progress = flowrt::OperationProgressPublisher<PlanFeedback>{id, [operation_control](flowrt::OperationId progress_id, std::uint64_t sequence) {
                        operation_control->publish_progress(progress_id, sequence);
                    }};
                    flowrt::OperationState terminal_state = flowrt::OperationState::Failed;
                    try {
                        const auto result = operation_worker_server->on_plan_operation(goal_for_worker, *cancel, progress);
                        switch (result.kind()) {
                            case flowrt::OperationHandlerResult<PlanResult>::Kind::Succeeded:
                                terminal_state = flowrt::OperationState::Succeeded;
                                break;
                            case flowrt::OperationHandlerResult<PlanResult>::Kind::Failed:
                                terminal_state = flowrt::OperationState::Failed;
                                break;
                            case flowrt::OperationHandlerResult<PlanResult>::Kind::Canceled:
                                terminal_state = flowrt::OperationState::Cancelled;
                                break;
                        }
                    } catch (...) {
                        terminal_state = flowrt::OperationState::Failed;
                    }
                    (void)operation_control->complete(id, terminal_state);
                }).detach();
            } catch (...) {
                (void)operation_control->complete(id, flowrt::OperationState::Failed);
                return flowrt::ServiceResult<flowrt::OperationStartAck>::err(flowrt::ServiceError::HandlerError);
            }
            return flowrt::ServiceResult<flowrt::OperationStartAck>::ok(ack);
        });
        if (!handled.has_value()) {
            return flowrt::Status::Error;
        }
    }
    if (operation_cancel_server_navigator_plan_.has_value()) {
        const auto handled = operation_cancel_server_navigator_plan_->poll_requests([this](const flowrt::OperationId& id) -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {
            const auto snapshot = this->operation_control_0_->snapshot();
            if (const auto error = this->operation_control_0_->request_cancel(id, snapshot.owner); error != flowrt::OperationControlError::Ok) {
                return flowrt_operation_control_error<flowrt::OperationStatusSnapshot>(error);
            }
            return flowrt::ServiceResult<flowrt::OperationStatusSnapshot>::ok(this->operation_control_0_->snapshot());
        });
        if (!handled.has_value()) {
            return flowrt::Status::Error;
        }
    }
    if (operation_status_server_navigator_plan_.has_value()) {
        const auto handled = operation_status_server_navigator_plan_->poll_requests([this](const flowrt::OperationId& id) -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {
            const auto status = this->operation_control_0_->status(id);
            if (!status.has_value()) {
                return flowrt_operation_control_error<flowrt::OperationStatusSnapshot>(status.error());
            }
            return flowrt::ServiceResult<flowrt::OperationStatusSnapshot>::ok(status.value());
        });
        if (!handled.has_value()) {
            return flowrt::Status::Error;
        }
    }
    if (this->operation_control_0_) {
        (void)this->operation_control_0_->check_deadline(flowrt::monotonic_time_ms());
        const auto snapshot = this->operation_control_0_->snapshot();
        const auto events = this->operation_control_0_->drain_events();
        for (const auto& event : events) {
            const auto operation_id = flowrt_operation_id_string(event.id);
            switch (event.kind) {
                case flowrt::OperationRuntimeEventKind::StateChanged:
                    if (event.state.has_value()) {
                        introspection_state.record_operation_transition(
                            "controller.plan",
                            operation_id,
                            flowrt::to_string(*event.state),
                            std::optional<std::string_view>{"controller.plan"},
                            flowrt::is_terminal(*event.state) ? std::nullopt : std::optional<std::uint64_t>{snapshot.deadline_ms});
                    }
                    break;
                case flowrt::OperationRuntimeEventKind::Progress:
                    introspection_state.record_operation_progress("controller.plan", operation_id, event.sequence.value_or(0U));
                    break;
                case flowrt::OperationRuntimeEventKind::Result: {
                    const auto result = event.state.has_value() ? flowrt::to_string(*event.state) : std::string_view{"succeeded"};
                    introspection_state.record_operation_result("controller.plan", operation_id, result, std::nullopt);
                    break;
                }
                case flowrt::OperationRuntimeEventKind::Error: {
                    const auto result = event.state.has_value() ? flowrt::to_string(*event.state) : std::string_view{"failed"};
                    introspection_state.record_operation_result("controller.plan", operation_id, result, std::optional<std::string_view>{"handler error"});
                    break;
                }
            }
        }
        introspection_state.record_operation_health(flowrt_operation_status_from_snapshot("controller.plan", "controller.plan", snapshot));
    }
    return flowrt::Status::Ok;
}

flowrt::Status App::run(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {
    flowrt::Context lifecycle_context;
    auto status = flowrt::Status::Ok;
    (void)backend;
    auto shutdown = flowrt::install_signal_shutdown_token();
    flowrt::IntrospectionState introspection_state;
    flowrt::ScheduleWaiter scheduler_events;
    introspection_state.set_self_description_json(std::string{flowrt_app::self_description_json()});
    auto introspection_server = flowrt::spawn_status_server(
        flowrt::IntrospectionIdentity{
            .self_description_hash = std::string{flowrt_app::self_description_hash()},
            .package = "bounded_operation_iox2_cpp",
            .process = "main",
            .runtime = "cpp",
        },
        introspection_state);
    (void)introspection_server;
    bool controller_initialized = false;
    bool controller_started = false;
    introspection_state.record_lifecycle_state("controller", flowrt::LifecycleState::Uninitialized);
    bool navigator_initialized = false;
    bool navigator_started = false;
    introspection_state.record_lifecycle_state("navigator", flowrt::LifecycleState::Uninitialized);
    if (status == flowrt::Status::Ok && controller_) {
        status = controller_->on_init(lifecycle_context);
        controller_initialized = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("controller", controller_initialized ? flowrt::LifecycleState::Initialized : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok && navigator_) {
        status = navigator_->on_init(lifecycle_context);
        navigator_initialized = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("navigator", navigator_initialized ? flowrt::LifecycleState::Initialized : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok && controller_initialized && controller_) {
        status = controller_->on_start(lifecycle_context);
        controller_started = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("controller", controller_started ? flowrt::LifecycleState::Running : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok && navigator_initialized && navigator_) {
        status = navigator_->on_start(lifecycle_context);
        navigator_started = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("navigator", navigator_started ? flowrt::LifecycleState::Running : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> startup_health_map;
        status = step_startup(0, lifecycle_context, introspection_state, scheduler_events, startup_health_map);
    }
    if (status == flowrt::Status::Ok) {
        this->operation_client_controller_plan_.bind(
            flowrt::iox2::Iox2FrameServiceClient<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::open("FlowRT/service/__flowrt_operation_controller_plan_start"),
            flowrt::iox2::Iox2ServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_cancel"),
            flowrt::iox2::Iox2ServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_status"));
        this->operation_start_server_navigator_plan_ = flowrt::iox2::Iox2FrameServiceServer<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::open("FlowRT/service/__flowrt_operation_controller_plan_start", 1U);
        this->operation_start_server_navigator_plan_->set_schedule_waiter(scheduler_events);
        if (this->operation_start_server_navigator_plan_->health().state != flowrt::BackendHealthState::Ready) {
            status = flowrt::Status::Error;
        }
        this->operation_cancel_server_navigator_plan_ = flowrt::iox2::Iox2ServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_cancel", 1U);
        this->operation_cancel_server_navigator_plan_->set_schedule_waiter(scheduler_events);
        if (this->operation_cancel_server_navigator_plan_->health().state != flowrt::BackendHealthState::Ready) {
            status = flowrt::Status::Error;
        }
        this->operation_status_server_navigator_plan_ = flowrt::iox2::Iox2ServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_status", 1U);
        this->operation_status_server_navigator_plan_->set_schedule_waiter(scheduler_events);
        if (this->operation_status_server_navigator_plan_->health().state != flowrt::BackendHealthState::Ready) {
            status = flowrt::Status::Error;
        }
    }
    flowrt::DeterministicExecutor scheduler{1};
    flowrt::WorkerPool worker_pool{1};
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("controller_serial")}, flowrt::LaneKind::Serial);
    (void)"controller_serial";
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")}, flowrt::LaneKind::Serial);
    (void)"navigator_operation_serial";
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_serial")}, flowrt::LaneKind::Serial);
    (void)"navigator_serial";
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{flowrt::fnv1a64("controller_serial")}, .priority = 0});
    scheduler.add_periodic(flowrt::PeriodicSpec{.task = flowrt::TaskId{1}, .period = std::chrono::milliseconds{100}});
    scheduler.wake(flowrt::TaskId{1});
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{2}, .lane = flowrt::LaneId{flowrt::fnv1a64("navigator_serial")}, .priority = 0});
    scheduler.add_periodic(flowrt::PeriodicSpec{.task = flowrt::TaskId{2}, .period = std::chrono::milliseconds{1000}});
    scheduler.wake(flowrt::TaskId{2});
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{3}, .lane = flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")}, .priority = 0});
    const auto scheduler_base_period_ms = std::uint64_t{100};
    std::size_t tick_base = 0;
    std::uint64_t scheduler_now_ms = 0;
    std::map<std::string, flowrt::IntrospectionTaskHealth> health_map;
    constexpr std::uint64_t fairness_starvation_threshold = 10;
    const auto scheduler_started_at = std::chrono::steady_clock::now();
    const auto scheduler_runtime_now_ms = [&scheduler_started_at]() -> std::uint64_t {
        const auto elapsed_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                                    std::chrono::steady_clock::now() - scheduler_started_at)
                                    .count();
        return elapsed_ms <= 0 ? 0U : static_cast<std::uint64_t>(elapsed_ms);
    };
    const auto clock_source = std::string_view{"realtime"};
    const auto task_clock_source = flowrt::ClockSource::Runtime;
    flowrt::WorkerCompletionQueue<std::vector<FlowrtOutputCommit>> task_completion_queue;
    task_completion_queue.set_wake_callback([&scheduler_events]() { scheduler_events.notify_data(); });
    std::deque<flowrt::TaskId> pending_task_order;
    std::map<flowrt::TaskId, flowrt::TaskRunOutput<std::vector<FlowrtOutputCommit>>> pending_task_results;
    std::map<flowrt::TaskId, flowrt::TaskAdmission> pending_task_admissions;
    std::mutex task_health_mutex;
    std::map<std::string, flowrt::IntrospectionTaskHealth> task_health_from_workers;
    std::map<flowrt::TaskId, std::uint64_t> task_last_scheduled_time_ms;
    std::map<flowrt::TaskId, std::uint64_t> task_last_observed_time_ms;
    while (status == flowrt::Status::Ok && !shutdown.is_requested() && ((!run_ticks.has_value() || tick_base < *run_ticks) || !pending_task_order.empty())) {
        std::uint64_t observed_data_generation = scheduler_events.data_generation();
        scheduler_now_ms = std::max(scheduler_now_ms, scheduler_runtime_now_ms());
        (void)scheduler_events.take_data_time_ms();
        const auto tick_time_ms = scheduler_now_ms;
        scheduler.advance_to(std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(tick_time_ms)});
        scheduler.set_current_tick(static_cast<std::uint64_t>(tick_base));
        {
            auto& health = health_map["controller.main"];
            health.name = "controller.main";
            health.lane = "controller_serial";
        }
        {
            auto& health = health_map["navigator.main"];
            health.name = "navigator.main";
            health.lane = "navigator_serial";
        }
        bool flowrt_operation_tick_driven_0 = false;
        introspection_state.record_tick(tick_time_ms, clock_source);
        while (true) {
            observed_data_generation = scheduler_events.data_generation();
            bool woke_on_message = false;
            const auto flowrt_operation_snapshot_0 = this->operation_control_0_ ? this->operation_control_0_->snapshot() : flowrt::OperationStatusSnapshot{};
            const bool flowrt_operation_active_0 = !flowrt::is_terminal(flowrt_operation_snapshot_0.state) && flowrt_operation_snapshot_0.state != flowrt::OperationState::Idle;
            if (operation_start_server_navigator_plan_.has_value() || operation_cancel_server_navigator_plan_.has_value() || operation_status_server_navigator_plan_.has_value() || (flowrt_operation_active_0 && !flowrt_operation_tick_driven_0)) {
                scheduler.wake(flowrt::TaskId{3});
                if (flowrt_operation_active_0) {
                    flowrt_operation_tick_driven_0 = true;
                }
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
                const auto submitted = worker_pool.submit_collect(admission.task, task_completion_queue, [this, &introspection_state, &scheduler_events, &task_health_mutex, &task_health_from_workers, admission, scheduled_delta_ms, observed_delta_ms, task_clock_source, tick_base, tick_time_ms]() {
                    auto local_health_map = std::map<std::string, flowrt::IntrospectionTaskHealth>{};
                    const auto [task_name, task_trigger] = [&]() -> std::pair<std::string_view, std::string_view> {
                        switch (admission.task.value) {
                            case 1: return {"controller.main", "periodic"};
                            case 2: return {"navigator.main", "periodic"};
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
auto flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId{flowrt::fnv1a64("controller_serial")});
(void)flowrt_lane_guard;
auto task_outcome = step_task_controller_main(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
merge_local_health(std::move(local_health_map));
return task_outcome;
}
                    case 2: {
auto flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_serial")});
(void)flowrt_lane_guard;
auto task_outcome = step_task_navigator_main(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
merge_local_health(std::move(local_health_map));
return task_outcome;
}
                    case 3: {
auto flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")});
(void)flowrt_lane_guard;
auto task_status = step_operation_navigator_plan(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
merge_local_health(std::move(local_health_map));
return FlowrtTaskOutcome{.status = task_status, .outputs = std::vector<FlowrtOutputCommit>{}};
}
                    default: return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
                }
                });
                if (submitted.accepted) {
                    pending_task_order.push_back(admission.task);
                    pending_task_admissions.insert_or_assign(admission.task, admission);
                    switch (admission.task.value) {
                        case 1: {
auto& health = health_map["controller.main"];
health.name = "controller.main";
health.lane = "controller_serial";
health.inflight = true;
health.scheduled_time_ms = admission.scheduled_time_ms;
health.observed_time_ms = admission.observed_time_ms;
health.lateness_ms = admission.lateness_ms;
health.missed_periods = admission.missed_periods;
health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);
break;
}
                        case 2: {
auto& health = health_map["navigator.main"];
health.name = "navigator.main";
health.lane = "navigator_serial";
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
auto& health = health_map["controller.main"];
health.name = "controller.main";
health.lane = "controller_serial";
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
                    case 2: {
auto& health = health_map["navigator.main"];
health.name = "navigator.main";
health.lane = "navigator_serial";
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
                    status = flowrt::Status::Error;
                    break;
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
        if (scheduler.lane_starvation_ticks(flowrt::LaneId{flowrt::fnv1a64("controller_serial")}) > fairness_starvation_threshold) {
            for (auto &[name, health] : health_map) {
                if (health.lane == "controller_serial") {
                    health.fairness_violations += 1;
                }
            }
        }
        if (scheduler.lane_starvation_ticks(flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")}) > fairness_starvation_threshold) {
            for (auto &[name, health] : health_map) {
                if (health.lane == "navigator_operation_serial") {
                    health.fairness_violations += 1;
                }
            }
        }
        if (scheduler.lane_starvation_ticks(flowrt::LaneId{flowrt::fnv1a64("navigator_serial")}) > fairness_starvation_threshold) {
            for (auto &[name, health] : health_map) {
                if (health.lane == "navigator_serial") {
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
            if (run_ticks.has_value() && pending_task_order.empty()) {
                scheduler_now_ms += scheduler_base_period_ms;
                continue;
            }
            const auto next_periodic_deadline_ms = std::optional<std::chrono::milliseconds>{std::min({scheduler.next_deadline(flowrt::TaskId{1}), scheduler.next_deadline(flowrt::TaskId{2})})};
            const auto next_wake_deadline = next_periodic_deadline_ms.has_value()
                ? std::optional<std::chrono::steady_clock::time_point>{
                      std::chrono::steady_clock::now() +
                      std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(
                          static_cast<std::uint64_t>(next_periodic_deadline_ms->count()) > scheduler_now_ms
                              ? static_cast<std::uint64_t>(next_periodic_deadline_ms->count()) - scheduler_now_ms
                              : 0U)}}
                : std::nullopt;
            switch (scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, shutdown)) {
                case flowrt::ScheduleEvent::Shutdown:
                    status = flowrt::Status::Ok;
                    break;
                case flowrt::ScheduleEvent::Timer:
                    scheduler_now_ms = next_periodic_deadline_ms.has_value()
                                           ? static_cast<std::uint64_t>(next_periodic_deadline_ms->count())
                                           : scheduler_now_ms + scheduler_base_period_ms;
                    break;
                case flowrt::ScheduleEvent::Data:
                    scheduler_now_ms = std::max(scheduler_now_ms, scheduler_runtime_now_ms());
                    (void)scheduler_events.take_data_time_ms();
                    break;
            }
            if (shutdown.is_requested()) {
                break;
            }
        }
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> shutdown_health_map;
        status = step_shutdown(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);
    }
    if (navigator_started && navigator_) {
        const auto stop_status = navigator_->on_stop(lifecycle_context);
        if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("navigator", stop_status == flowrt::Status::Ok ? flowrt::LifecycleState::Stopped : flowrt::LifecycleState::Faulted);
    }
    if (controller_started && controller_) {
        const auto stop_status = controller_->on_stop(lifecycle_context);
        if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("controller", stop_status == flowrt::Status::Ok ? flowrt::LifecycleState::Stopped : flowrt::LifecycleState::Faulted);
    }
    if (navigator_initialized && navigator_) {
        const auto shutdown_status = navigator_->on_shutdown(lifecycle_context);
        if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("navigator", shutdown_status == flowrt::Status::Ok ? flowrt::LifecycleState::ShutDown : flowrt::LifecycleState::Faulted);
    }
    if (controller_initialized && controller_) {
        const auto shutdown_status = controller_->on_shutdown(lifecycle_context);
        if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("controller", shutdown_status == flowrt::Status::Ok ? flowrt::LifecycleState::ShutDown : flowrt::LifecycleState::Faulted);
    }
    if (const char* flowrt_status_out = std::getenv("FLOWRT_STATUS_OUT");
        flowrt_status_out != nullptr && flowrt_status_out[0] != '\0') {
        std::ofstream flowrt_status_file(flowrt_status_out);
        if (flowrt_status_file.good()) {
            flowrt_status_file << flowrt::introspection_status_json(introspection_state.status()) << '\n';
        } else {
            std::fprintf(stderr, "FlowRT: failed to write FLOWRT_STATUS_OUT `%s`\n", flowrt_status_out);
        }
    }
    return status;
}

flowrt::Status App::run_process(const flowrt::Backend& backend, std::string_view process, std::optional<std::size_t> run_ticks) {
    if (process == "client_proc") {
        return run_process_client_proc(backend, run_ticks);
    }
    if (process == "server_proc") {
        return run_process_server_proc(backend, run_ticks);
    }
    return flowrt::Status::Error;
}

flowrt::Status App::run_process_client_proc(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {
    flowrt::Context lifecycle_context;
    auto status = flowrt::Status::Ok;
    (void)backend;
    auto shutdown = flowrt::install_signal_shutdown_token();
    flowrt::IntrospectionState introspection_state;
    flowrt::ScheduleWaiter scheduler_events;
    introspection_state.set_self_description_json(std::string{flowrt_app::self_description_json()});
    auto introspection_server = flowrt::spawn_status_server(
        flowrt::IntrospectionIdentity{
            .self_description_hash = std::string{flowrt_app::self_description_hash()},
            .package = "bounded_operation_iox2_cpp",
            .process = "client_proc",
            .runtime = "cpp",
        },
        introspection_state);
    (void)introspection_server;
    bool controller_initialized = false;
    bool controller_started = false;
    introspection_state.record_lifecycle_state("controller", flowrt::LifecycleState::Uninitialized);
    if (status == flowrt::Status::Ok && controller_) {
        status = controller_->on_init(lifecycle_context);
        controller_initialized = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("controller", controller_initialized ? flowrt::LifecycleState::Initialized : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok && controller_initialized && controller_) {
        status = controller_->on_start(lifecycle_context);
        controller_started = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("controller", controller_started ? flowrt::LifecycleState::Running : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> startup_health_map;
        status = step_process_client_proc_startup(0, lifecycle_context, introspection_state, scheduler_events, startup_health_map);
    }
    if (status == flowrt::Status::Ok) {
        this->operation_client_controller_plan_.bind(
            flowrt::iox2::Iox2FrameServiceClient<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::open("FlowRT/service/__flowrt_operation_controller_plan_start"),
            flowrt::iox2::Iox2ServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_cancel"),
            flowrt::iox2::Iox2ServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_status"));
    }
    flowrt::DeterministicExecutor scheduler{1};
    flowrt::WorkerPool worker_pool{1};
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("controller_serial")}, flowrt::LaneKind::Serial);
    (void)"controller_serial";
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")}, flowrt::LaneKind::Serial);
    (void)"navigator_operation_serial";
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{flowrt::fnv1a64("controller_serial")}, .priority = 0});
    scheduler.add_periodic(flowrt::PeriodicSpec{.task = flowrt::TaskId{1}, .period = std::chrono::milliseconds{100}});
    scheduler.wake(flowrt::TaskId{1});
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{2}, .lane = flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")}, .priority = 0});
    const auto scheduler_base_period_ms = std::uint64_t{100};
    std::size_t tick_base = 0;
    std::uint64_t scheduler_now_ms = 0;
    std::map<std::string, flowrt::IntrospectionTaskHealth> health_map;
    constexpr std::uint64_t fairness_starvation_threshold = 10;
    const auto scheduler_started_at = std::chrono::steady_clock::now();
    const auto scheduler_runtime_now_ms = [&scheduler_started_at]() -> std::uint64_t {
        const auto elapsed_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                                    std::chrono::steady_clock::now() - scheduler_started_at)
                                    .count();
        return elapsed_ms <= 0 ? 0U : static_cast<std::uint64_t>(elapsed_ms);
    };
    const auto clock_source = std::string_view{"realtime"};
    const auto task_clock_source = flowrt::ClockSource::Runtime;
    flowrt::WorkerCompletionQueue<std::vector<FlowrtOutputCommit>> task_completion_queue;
    task_completion_queue.set_wake_callback([&scheduler_events]() { scheduler_events.notify_data(); });
    std::deque<flowrt::TaskId> pending_task_order;
    std::map<flowrt::TaskId, flowrt::TaskRunOutput<std::vector<FlowrtOutputCommit>>> pending_task_results;
    std::map<flowrt::TaskId, flowrt::TaskAdmission> pending_task_admissions;
    std::mutex task_health_mutex;
    std::map<std::string, flowrt::IntrospectionTaskHealth> task_health_from_workers;
    std::map<flowrt::TaskId, std::uint64_t> task_last_scheduled_time_ms;
    std::map<flowrt::TaskId, std::uint64_t> task_last_observed_time_ms;
    while (status == flowrt::Status::Ok && !shutdown.is_requested() && ((!run_ticks.has_value() || tick_base < *run_ticks) || !pending_task_order.empty())) {
        std::uint64_t observed_data_generation = scheduler_events.data_generation();
        scheduler_now_ms = std::max(scheduler_now_ms, scheduler_runtime_now_ms());
        (void)scheduler_events.take_data_time_ms();
        const auto tick_time_ms = scheduler_now_ms;
        scheduler.advance_to(std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(tick_time_ms)});
        scheduler.set_current_tick(static_cast<std::uint64_t>(tick_base));
        {
            auto& health = health_map["controller.main"];
            health.name = "controller.main";
            health.lane = "controller_serial";
        }
        bool flowrt_operation_tick_driven_0 = false;
        introspection_state.record_tick(tick_time_ms, clock_source);
        while (true) {
            observed_data_generation = scheduler_events.data_generation();
            bool woke_on_message = false;
            const auto flowrt_operation_snapshot_0 = this->operation_control_0_ ? this->operation_control_0_->snapshot() : flowrt::OperationStatusSnapshot{};
            const bool flowrt_operation_active_0 = !flowrt::is_terminal(flowrt_operation_snapshot_0.state) && flowrt_operation_snapshot_0.state != flowrt::OperationState::Idle;
            if (operation_start_server_navigator_plan_.has_value() || operation_cancel_server_navigator_plan_.has_value() || operation_status_server_navigator_plan_.has_value() || (flowrt_operation_active_0 && !flowrt_operation_tick_driven_0)) {
                scheduler.wake(flowrt::TaskId{2});
                if (flowrt_operation_active_0) {
                    flowrt_operation_tick_driven_0 = true;
                }
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
                const auto submitted = worker_pool.submit_collect(admission.task, task_completion_queue, [this, &introspection_state, &scheduler_events, &task_health_mutex, &task_health_from_workers, admission, scheduled_delta_ms, observed_delta_ms, task_clock_source, tick_base, tick_time_ms]() {
                    auto local_health_map = std::map<std::string, flowrt::IntrospectionTaskHealth>{};
                    const auto [task_name, task_trigger] = [&]() -> std::pair<std::string_view, std::string_view> {
                        switch (admission.task.value) {
                            case 1: return {"controller.main", "periodic"};
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
auto flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId{flowrt::fnv1a64("controller_serial")});
(void)flowrt_lane_guard;
auto task_outcome = step_process_client_proc_task_controller_main(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
merge_local_health(std::move(local_health_map));
return task_outcome;
}
                    case 2: {
auto flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")});
(void)flowrt_lane_guard;
auto task_status = step_operation_navigator_plan(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
merge_local_health(std::move(local_health_map));
return FlowrtTaskOutcome{.status = task_status, .outputs = std::vector<FlowrtOutputCommit>{}};
}
                    default: return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
                }
                });
                if (submitted.accepted) {
                    pending_task_order.push_back(admission.task);
                    pending_task_admissions.insert_or_assign(admission.task, admission);
                    switch (admission.task.value) {
                        case 1: {
auto& health = health_map["controller.main"];
health.name = "controller.main";
health.lane = "controller_serial";
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
auto& health = health_map["controller.main"];
health.name = "controller.main";
health.lane = "controller_serial";
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
                    status = flowrt::Status::Error;
                    break;
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
        if (scheduler.lane_starvation_ticks(flowrt::LaneId{flowrt::fnv1a64("controller_serial")}) > fairness_starvation_threshold) {
            for (auto &[name, health] : health_map) {
                if (health.lane == "controller_serial") {
                    health.fairness_violations += 1;
                }
            }
        }
        if (scheduler.lane_starvation_ticks(flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")}) > fairness_starvation_threshold) {
            for (auto &[name, health] : health_map) {
                if (health.lane == "navigator_operation_serial") {
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
            if (run_ticks.has_value() && pending_task_order.empty()) {
                scheduler_now_ms += scheduler_base_period_ms;
                continue;
            }
            const auto next_periodic_deadline_ms = std::optional<std::chrono::milliseconds>{std::min({scheduler.next_deadline(flowrt::TaskId{1})})};
            const auto next_wake_deadline = next_periodic_deadline_ms.has_value()
                ? std::optional<std::chrono::steady_clock::time_point>{
                      std::chrono::steady_clock::now() +
                      std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(
                          static_cast<std::uint64_t>(next_periodic_deadline_ms->count()) > scheduler_now_ms
                              ? static_cast<std::uint64_t>(next_periodic_deadline_ms->count()) - scheduler_now_ms
                              : 0U)}}
                : std::nullopt;
            switch (scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, shutdown)) {
                case flowrt::ScheduleEvent::Shutdown:
                    status = flowrt::Status::Ok;
                    break;
                case flowrt::ScheduleEvent::Timer:
                    scheduler_now_ms = next_periodic_deadline_ms.has_value()
                                           ? static_cast<std::uint64_t>(next_periodic_deadline_ms->count())
                                           : scheduler_now_ms + scheduler_base_period_ms;
                    break;
                case flowrt::ScheduleEvent::Data:
                    scheduler_now_ms = std::max(scheduler_now_ms, scheduler_runtime_now_ms());
                    (void)scheduler_events.take_data_time_ms();
                    break;
            }
            if (shutdown.is_requested()) {
                break;
            }
        }
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> shutdown_health_map;
        status = step_process_client_proc_shutdown(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);
    }
    if (controller_started && controller_) {
        const auto stop_status = controller_->on_stop(lifecycle_context);
        if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("controller", stop_status == flowrt::Status::Ok ? flowrt::LifecycleState::Stopped : flowrt::LifecycleState::Faulted);
    }
    if (controller_initialized && controller_) {
        const auto shutdown_status = controller_->on_shutdown(lifecycle_context);
        if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("controller", shutdown_status == flowrt::Status::Ok ? flowrt::LifecycleState::ShutDown : flowrt::LifecycleState::Faulted);
    }
    if (const char* flowrt_status_out = std::getenv("FLOWRT_STATUS_OUT");
        flowrt_status_out != nullptr && flowrt_status_out[0] != '\0') {
        std::ofstream flowrt_status_file(flowrt_status_out);
        if (flowrt_status_file.good()) {
            flowrt_status_file << flowrt::introspection_status_json(introspection_state.status()) << '\n';
        } else {
            std::fprintf(stderr, "FlowRT: failed to write FLOWRT_STATUS_OUT `%s`\n", flowrt_status_out);
        }
    }
    return status;
}

flowrt::Status App::run_process_server_proc(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {
    flowrt::Context lifecycle_context;
    auto status = flowrt::Status::Ok;
    (void)backend;
    auto shutdown = flowrt::install_signal_shutdown_token();
    flowrt::IntrospectionState introspection_state;
    flowrt::ScheduleWaiter scheduler_events;
    introspection_state.set_self_description_json(std::string{flowrt_app::self_description_json()});
    auto introspection_server = flowrt::spawn_status_server(
        flowrt::IntrospectionIdentity{
            .self_description_hash = std::string{flowrt_app::self_description_hash()},
            .package = "bounded_operation_iox2_cpp",
            .process = "server_proc",
            .runtime = "cpp",
        },
        introspection_state);
    (void)introspection_server;
    bool navigator_initialized = false;
    bool navigator_started = false;
    introspection_state.record_lifecycle_state("navigator", flowrt::LifecycleState::Uninitialized);
    if (status == flowrt::Status::Ok && navigator_) {
        status = navigator_->on_init(lifecycle_context);
        navigator_initialized = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("navigator", navigator_initialized ? flowrt::LifecycleState::Initialized : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok && navigator_initialized && navigator_) {
        status = navigator_->on_start(lifecycle_context);
        navigator_started = status == flowrt::Status::Ok;
        introspection_state.record_lifecycle_state("navigator", navigator_started ? flowrt::LifecycleState::Running : flowrt::LifecycleState::Faulted);
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> startup_health_map;
        status = step_process_server_proc_startup(0, lifecycle_context, introspection_state, scheduler_events, startup_health_map);
    }
    if (status == flowrt::Status::Ok) {
        this->operation_start_server_navigator_plan_ = flowrt::iox2::Iox2FrameServiceServer<flowrt::OperationStartRequest<PlanGoal>, flowrt::OperationStartAck, 40, 49>::open("FlowRT/service/__flowrt_operation_controller_plan_start", 1U);
        this->operation_start_server_navigator_plan_->set_schedule_waiter(scheduler_events);
        if (this->operation_start_server_navigator_plan_->health().state != flowrt::BackendHealthState::Ready) {
            status = flowrt::Status::Error;
        }
        this->operation_cancel_server_navigator_plan_ = flowrt::iox2::Iox2ServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_cancel", 1U);
        this->operation_cancel_server_navigator_plan_->set_schedule_waiter(scheduler_events);
        if (this->operation_cancel_server_navigator_plan_->health().state != flowrt::BackendHealthState::Ready) {
            status = flowrt::Status::Error;
        }
        this->operation_status_server_navigator_plan_ = flowrt::iox2::Iox2ServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>::open("FlowRT/service/__flowrt_operation_controller_plan_status", 1U);
        this->operation_status_server_navigator_plan_->set_schedule_waiter(scheduler_events);
        if (this->operation_status_server_navigator_plan_->health().state != flowrt::BackendHealthState::Ready) {
            status = flowrt::Status::Error;
        }
    }
    flowrt::DeterministicExecutor scheduler{1};
    flowrt::WorkerPool worker_pool{1};
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")}, flowrt::LaneKind::Serial);
    (void)"navigator_operation_serial";
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_serial")}, flowrt::LaneKind::Serial);
    (void)"navigator_serial";
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{flowrt::fnv1a64("navigator_serial")}, .priority = 0});
    scheduler.add_periodic(flowrt::PeriodicSpec{.task = flowrt::TaskId{1}, .period = std::chrono::milliseconds{1000}});
    scheduler.wake(flowrt::TaskId{1});
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{2}, .lane = flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")}, .priority = 0});
    const auto scheduler_base_period_ms = std::uint64_t{1000};
    std::size_t tick_base = 0;
    std::uint64_t scheduler_now_ms = 0;
    std::map<std::string, flowrt::IntrospectionTaskHealth> health_map;
    constexpr std::uint64_t fairness_starvation_threshold = 10;
    const auto scheduler_started_at = std::chrono::steady_clock::now();
    const auto scheduler_runtime_now_ms = [&scheduler_started_at]() -> std::uint64_t {
        const auto elapsed_ms = std::chrono::duration_cast<std::chrono::milliseconds>(
                                    std::chrono::steady_clock::now() - scheduler_started_at)
                                    .count();
        return elapsed_ms <= 0 ? 0U : static_cast<std::uint64_t>(elapsed_ms);
    };
    const auto clock_source = std::string_view{"realtime"};
    const auto task_clock_source = flowrt::ClockSource::Runtime;
    flowrt::WorkerCompletionQueue<std::vector<FlowrtOutputCommit>> task_completion_queue;
    task_completion_queue.set_wake_callback([&scheduler_events]() { scheduler_events.notify_data(); });
    std::deque<flowrt::TaskId> pending_task_order;
    std::map<flowrt::TaskId, flowrt::TaskRunOutput<std::vector<FlowrtOutputCommit>>> pending_task_results;
    std::map<flowrt::TaskId, flowrt::TaskAdmission> pending_task_admissions;
    std::mutex task_health_mutex;
    std::map<std::string, flowrt::IntrospectionTaskHealth> task_health_from_workers;
    std::map<flowrt::TaskId, std::uint64_t> task_last_scheduled_time_ms;
    std::map<flowrt::TaskId, std::uint64_t> task_last_observed_time_ms;
    while (status == flowrt::Status::Ok && !shutdown.is_requested() && ((!run_ticks.has_value() || tick_base < *run_ticks) || !pending_task_order.empty())) {
        std::uint64_t observed_data_generation = scheduler_events.data_generation();
        scheduler_now_ms = std::max(scheduler_now_ms, scheduler_runtime_now_ms());
        (void)scheduler_events.take_data_time_ms();
        const auto tick_time_ms = scheduler_now_ms;
        scheduler.advance_to(std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(tick_time_ms)});
        scheduler.set_current_tick(static_cast<std::uint64_t>(tick_base));
        {
            auto& health = health_map["navigator.main"];
            health.name = "navigator.main";
            health.lane = "navigator_serial";
        }
        bool flowrt_operation_tick_driven_0 = false;
        introspection_state.record_tick(tick_time_ms, clock_source);
        while (true) {
            observed_data_generation = scheduler_events.data_generation();
            bool woke_on_message = false;
            const auto flowrt_operation_snapshot_0 = this->operation_control_0_ ? this->operation_control_0_->snapshot() : flowrt::OperationStatusSnapshot{};
            const bool flowrt_operation_active_0 = !flowrt::is_terminal(flowrt_operation_snapshot_0.state) && flowrt_operation_snapshot_0.state != flowrt::OperationState::Idle;
            if (operation_start_server_navigator_plan_.has_value() || operation_cancel_server_navigator_plan_.has_value() || operation_status_server_navigator_plan_.has_value() || (flowrt_operation_active_0 && !flowrt_operation_tick_driven_0)) {
                scheduler.wake(flowrt::TaskId{2});
                if (flowrt_operation_active_0) {
                    flowrt_operation_tick_driven_0 = true;
                }
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
                const auto submitted = worker_pool.submit_collect(admission.task, task_completion_queue, [this, &introspection_state, &scheduler_events, &task_health_mutex, &task_health_from_workers, admission, scheduled_delta_ms, observed_delta_ms, task_clock_source, tick_base, tick_time_ms]() {
                    auto local_health_map = std::map<std::string, flowrt::IntrospectionTaskHealth>{};
                    const auto [task_name, task_trigger] = [&]() -> std::pair<std::string_view, std::string_view> {
                        switch (admission.task.value) {
                            case 1: return {"navigator.main", "periodic"};
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
auto flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_serial")});
(void)flowrt_lane_guard;
auto task_outcome = step_process_server_proc_task_navigator_main(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
merge_local_health(std::move(local_health_map));
return task_outcome;
}
                    case 2: {
auto flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")});
(void)flowrt_lane_guard;
auto task_status = step_operation_navigator_plan(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
merge_local_health(std::move(local_health_map));
return FlowrtTaskOutcome{.status = task_status, .outputs = std::vector<FlowrtOutputCommit>{}};
}
                    default: return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});
                }
                });
                if (submitted.accepted) {
                    pending_task_order.push_back(admission.task);
                    pending_task_admissions.insert_or_assign(admission.task, admission);
                    switch (admission.task.value) {
                        case 1: {
auto& health = health_map["navigator.main"];
health.name = "navigator.main";
health.lane = "navigator_serial";
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
auto& health = health_map["navigator.main"];
health.name = "navigator.main";
health.lane = "navigator_serial";
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
                    status = flowrt::Status::Error;
                    break;
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
        if (scheduler.lane_starvation_ticks(flowrt::LaneId{flowrt::fnv1a64("navigator_operation_serial")}) > fairness_starvation_threshold) {
            for (auto &[name, health] : health_map) {
                if (health.lane == "navigator_operation_serial") {
                    health.fairness_violations += 1;
                }
            }
        }
        if (scheduler.lane_starvation_ticks(flowrt::LaneId{flowrt::fnv1a64("navigator_serial")}) > fairness_starvation_threshold) {
            for (auto &[name, health] : health_map) {
                if (health.lane == "navigator_serial") {
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
            if (run_ticks.has_value() && pending_task_order.empty()) {
                scheduler_now_ms += scheduler_base_period_ms;
                continue;
            }
            const auto next_periodic_deadline_ms = std::optional<std::chrono::milliseconds>{std::min({scheduler.next_deadline(flowrt::TaskId{1})})};
            const auto next_wake_deadline = next_periodic_deadline_ms.has_value()
                ? std::optional<std::chrono::steady_clock::time_point>{
                      std::chrono::steady_clock::now() +
                      std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(
                          static_cast<std::uint64_t>(next_periodic_deadline_ms->count()) > scheduler_now_ms
                              ? static_cast<std::uint64_t>(next_periodic_deadline_ms->count()) - scheduler_now_ms
                              : 0U)}}
                : std::nullopt;
            switch (scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, shutdown)) {
                case flowrt::ScheduleEvent::Shutdown:
                    status = flowrt::Status::Ok;
                    break;
                case flowrt::ScheduleEvent::Timer:
                    scheduler_now_ms = next_periodic_deadline_ms.has_value()
                                           ? static_cast<std::uint64_t>(next_periodic_deadline_ms->count())
                                           : scheduler_now_ms + scheduler_base_period_ms;
                    break;
                case flowrt::ScheduleEvent::Data:
                    scheduler_now_ms = std::max(scheduler_now_ms, scheduler_runtime_now_ms());
                    (void)scheduler_events.take_data_time_ms();
                    break;
            }
            if (shutdown.is_requested()) {
                break;
            }
        }
    }
    if (status == flowrt::Status::Ok) {
        std::map<std::string, flowrt::IntrospectionTaskHealth> shutdown_health_map;
        status = step_process_server_proc_shutdown(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);
    }
    if (navigator_started && navigator_) {
        const auto stop_status = navigator_->on_stop(lifecycle_context);
        if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("navigator", stop_status == flowrt::Status::Ok ? flowrt::LifecycleState::Stopped : flowrt::LifecycleState::Faulted);
    }
    if (navigator_initialized && navigator_) {
        const auto shutdown_status = navigator_->on_shutdown(lifecycle_context);
        if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok) {
            status = flowrt::Status::Error;
        }
        introspection_state.record_lifecycle_state("navigator", shutdown_status == flowrt::Status::Ok ? flowrt::LifecycleState::ShutDown : flowrt::LifecycleState::Faulted);
    }
    if (const char* flowrt_status_out = std::getenv("FLOWRT_STATUS_OUT");
        flowrt_status_out != nullptr && flowrt_status_out[0] != '\0') {
        std::ofstream flowrt_status_file(flowrt_status_out);
        if (flowrt_status_file.good()) {
            flowrt_status_file << flowrt::introspection_status_json(introspection_state.status()) << '\n';
        } else {
            std::fprintf(stderr, "FlowRT: failed to write FLOWRT_STATUS_OUT `%s`\n", flowrt_status_out);
        }
    }
    return status;
}

flowrt::Status run(std::optional<std::size_t> run_ticks) {
    auto backend = flowrt::iox2_backend();
    return flowrt_user::build_app().run(backend, run_ticks);
}

flowrt::Status run_process(std::string_view process, std::optional<std::size_t> run_ticks) {
    auto backend = flowrt::iox2_backend();
    return flowrt_user::build_app().run_process(backend, process, run_ticks);
}

}  // namespace flowrt_app
