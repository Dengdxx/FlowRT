// FlowRT 管理产物。不要手工修改。
#include "flowrt_app/runtime_shell.hpp"

#include "flowrt_app/c_components.h"

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
#include <iostream>
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

using namespace flowrt_app;

flowrt_string_view_t c_string_view(std::string_view value) {
    return flowrt_string_view_t{.data = value.data(), .len = value.size()};
}

flowrt_bytes_view_t c_bytes_view(const void* data, std::size_t size) {
    return flowrt_bytes_view_t{.data = reinterpret_cast<const std::uint8_t*>(data), .len = size};
}

std::string c_param_json(bool value) {
    return value ? "true" : "false";
}

template <typename T>
std::string c_param_json(T value) {
    return std::to_string(value);
}

std::string c_param_json_string(std::string_view value) {
    std::string output;
    output.reserve(value.size() + 2U);
    output.push_back('"');
    for (unsigned char ch : value) {
        switch (ch) {
            case '"':
                output += "\\\"";
                break;
            case '\\':
                output += "\\\\";
                break;
            case '\b':
                output += "\\b";
                break;
            case '\f':
                output += "\\f";
                break;
            case '\n':
                output += "\\n";
                break;
            case '\r':
                output += "\\r";
                break;
            case '\t':
                output += "\\t";
                break;
            default:
                if (ch < 0x20U) {
                    constexpr char kHex[] = "0123456789abcdef";
                    output += "\\u00";
                    output.push_back(kHex[(ch >> 4U) & 0x0FU]);
                    output.push_back(kHex[ch & 0x0FU]);
                } else {
                    output.push_back(static_cast<char>(ch));
                }
                break;
        }
    }
    output.push_back('"');
    return output;
}

std::string c_param_json(const std::string& value) {
    return c_param_json_string(value);
}

flowrt_c_param_snapshot_v0_t make_c_param_snapshot(const flowrt_param_view_t* params,
                                                   std::size_t param_count) {
    return flowrt_c_param_snapshot_v0_t{
        .abi_version = 0U,
        .param_count = static_cast<std::uint32_t>(param_count),
        .params = params,
        .reserved = {},
    };
}

flowrt_c_param_snapshot_v1_t make_c_param_snapshot_v1(const flowrt_param_view_v1_t* params,
                                                      std::size_t param_count) {
    return flowrt_c_param_snapshot_v1_t{
        .abi_version = 1U,
        .param_count = static_cast<std::uint32_t>(param_count),
        .params = params,
        .reserved = {},
    };
}

flowrt_c_clock_source_t c_clock_source(flowrt::ClockSource source) {
    switch (source) {
        case flowrt::ClockSource::Runtime:
            return FLOWRT_C_CLOCK_SOURCE_RUNTIME;
        case flowrt::ClockSource::Replay:
            return FLOWRT_C_CLOCK_SOURCE_REPLAY;
    }
    return FLOWRT_C_CLOCK_SOURCE_RUNTIME;
}

flowrt_c_task_timing_t make_c_task_timing(const flowrt::TaskTiming& timing) {
    return flowrt_c_task_timing_t{
        .step = timing.step,
        .task_name = c_string_view(timing.task_name),
        .trigger = c_string_view(timing.trigger),
        .clock_source = c_clock_source(timing.clock_source),
        .reserved0 = 0U,
        .scheduled_time_ms = timing.scheduled_time_ms,
        .observed_time_ms = timing.observed_time_ms,
        .scheduled_delta_ms = timing.scheduled_delta_ms,
        .observed_delta_ms = timing.observed_delta_ms,
        .period_ms = timing.period_ms.value_or(0U),
        .deadline_ms = timing.deadline_ms.value_or(0U),
        .lateness_ms = timing.lateness_ms,
        .missed_periods = timing.missed_periods,
        .has_period_ms = timing.period_ms.has_value() ? std::uint8_t{1} : std::uint8_t{0},
        .has_deadline_ms = timing.deadline_ms.has_value() ? std::uint8_t{1} : std::uint8_t{0},
        .deadline_missed = timing.deadline_missed ? std::uint8_t{1} : std::uint8_t{0},
        .overrun = timing.overrun ? std::uint8_t{1} : std::uint8_t{0},
        .reserved = {},
    };
}

flowrt::Status status_from_c(flowrt_status_t status) {
    switch (status) {
        case FLOWRT_STATUS_OK:
            return flowrt::Status::Ok;
        case FLOWRT_STATUS_RETRY:
            return flowrt::Status::Retry;
        case FLOWRT_STATUS_ERROR:
            return flowrt::Status::Error;
        default:
            return flowrt::Status::Error;
    }
}

const char* callback_table_validation_error(const flowrt_c_component_callback_table_t* callbacks,
                                            bool needs_periodic,
                                            bool needs_on_message,
                                            bool needs_startup,
                                            bool needs_shutdown) {
    if (callbacks == nullptr) {
        return "FlowRT C component callback table is null";
    }
    if (callbacks->size < sizeof(flowrt_c_component_callback_table_t)) {
        return "FlowRT C component callback table size is too small";
    }
    if (callbacks->version_major != FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR) {
        return "FlowRT C component callback table major version mismatch";
    }
    if (callbacks->version_minor != FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR) {
        return "FlowRT C component callback table minor version mismatch";
    }
    constexpr std::uint64_t kRequiredFeatures =
        FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 |
        FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1;
    constexpr std::uint64_t kKnownFeatures = kRequiredFeatures;
    if ((callbacks->feature_flags & FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0) !=
        FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0) {
        return "FlowRT C component callback table missing v0 feature bit";
    }
    if ((callbacks->feature_flags & FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1) !=
        FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1) {
        return "FlowRT C component callback table missing task timing feature bit";
    }
    if ((callbacks->feature_flags & ~kKnownFeatures) != 0U) {
        return "FlowRT C component callback table has unknown feature bit";
    }
    if (needs_periodic && callbacks->run_periodic == nullptr) {
        return "FlowRT C component callback table missing run_periodic";
    }
    if (needs_on_message && callbacks->run_on_message == nullptr) {
        return "FlowRT C component callback table missing run_on_message";
    }
    if (needs_startup && callbacks->run_startup == nullptr) {
        return "FlowRT C component callback table missing run_startup";
    }
    if (needs_shutdown && callbacks->run_shutdown == nullptr) {
        return "FlowRT C component callback table missing run_shutdown";
    }
    return nullptr;
}

flowrt_c_component_context_t make_c_component_context(std::string_view component_name,
                                                      std::string_view instance_name,
                                                      std::string_view task_name,
                                                      std::string_view lane_name,
                                                      const flowrt::Context& runtime_context,
                                                      std::uint64_t step,
                                                      std::uint64_t tick_time_ms,
                                                      std::uint64_t deadline_ms,
                                                      bool has_deadline_ms,
                                                      flowrt_c_param_snapshot_v0_t param_snapshot,
                                                      flowrt_c_param_snapshot_v1_t param_snapshot_v1) {
    const auto* timing = runtime_context.timing();
    return flowrt_c_component_context_t{
        .component_name = c_string_view(component_name),
        .instance_name = c_string_view(instance_name),
        .task_name = c_string_view(task_name),
        .lane_name = c_string_view(lane_name),
        .step = timing != nullptr ? timing->step : step,
        .tick_time_ms = timing != nullptr ? timing->observed_time_ms : tick_time_ms,
        .deadline_ms =
            timing != nullptr ? timing->deadline_ms.value_or(0U) : deadline_ms,
        .has_deadline_ms =
            timing != nullptr ? (timing->deadline_ms.has_value() ? std::uint8_t{1} : std::uint8_t{0})
                              : (has_deadline_ms ? std::uint8_t{1} : std::uint8_t{0}),
        .has_timing = timing != nullptr ? std::uint8_t{1} : std::uint8_t{0},
        .reserved = {},
        .timing = timing != nullptr ? make_c_task_timing(*timing) : flowrt_c_task_timing_t{},
        .params = param_snapshot,
        .params_v1 = param_snapshot_v1,
    };
}

class CControllerAdapter final : public flowrt_app::ControllerInterface {
public:
    explicit CControllerAdapter(const flowrt_c_component_callback_table_t* callbacks) : callbacks_(callbacks) {}

    flowrt::Status on_init(flowrt::Context& context) override {
        return call_lifecycle(callbacks_ != nullptr ? callbacks_->on_init : nullptr, "on_init", "controller", "controller", context);
    }

    flowrt::Status on_start(flowrt::Context& context) override {
        return call_lifecycle(callbacks_ != nullptr ? callbacks_->on_start : nullptr, "on_start", "controller", "controller", context);
    }

    flowrt::Status on_stop(flowrt::Context& context) override {
        return call_lifecycle(callbacks_ != nullptr ? callbacks_->on_stop : nullptr, "on_stop", "controller", "controller", context);
    }

    flowrt::Status on_shutdown(flowrt::Context& context) override {
        return call_lifecycle(callbacks_ != nullptr ? callbacks_->on_shutdown : nullptr, "on_shutdown", "controller", "controller", context);
    }

    flowrt::Status on_tick(
        const ControllerParams& params,
        flowrt::Output<Cmd>& cmd) override {
        return flowrt::Status::Error;
    }

    flowrt::Status run_controller_main(
        std::uint64_t step,
        std::uint64_t tick_time_ms,
        flowrt::Context& tick_context,
        const ControllerParams& params,
        flowrt::Output<Cmd>& cmd) {
        if (!callbacks_valid("controller") || callbacks_->run_periodic == nullptr) {
            return flowrt::Status::Error;
        }
        std::array<std::string, 4> controller_param_json{{
            c_param_json(params.enabled),
            c_param_json(params.gain),
            params.limits,
            c_param_json(params.mode),
        }};
        std::array<flowrt_param_view_t, 4> controller_param_views{{
            flowrt_param_view_t{
                .instance_name = c_string_view("controller"),
                .param_name = c_string_view("enabled"),
                .type_name = c_string_view("bool"),
                .update_policy = c_string_view("startup"),
                .current_json = c_string_view(controller_param_json[0]),
                .pending_json = flowrt_string_view_t{},
                .min_json = flowrt_string_view_t{},
                .max_json = flowrt_string_view_t{},
                .choices_json = flowrt_string_view_t{},
                .schema_hash = flowrt::fnv1a64("controller.enabled"),
                .revision = 0U,
                .mutable_at_runtime = 0U,
                .has_pending = 0U,
                .has_min = 0U,
                .has_max = 0U,
                .reserved = {},
            },
            flowrt_param_view_t{
                .instance_name = c_string_view("controller"),
                .param_name = c_string_view("gain"),
                .type_name = c_string_view("f32"),
                .update_policy = c_string_view("on_tick"),
                .current_json = c_string_view(controller_param_json[1]),
                .pending_json = flowrt_string_view_t{},
                .min_json = c_string_view("0.0"),
                .max_json = c_string_view("10.0"),
                .choices_json = flowrt_string_view_t{},
                .schema_hash = flowrt::fnv1a64("controller.gain"),
                .revision = 0U,
                .mutable_at_runtime = 1U,
                .has_pending = 0U,
                .has_min = 1U,
                .has_max = 1U,
                .reserved = {},
            },
            flowrt_param_view_t{
                .instance_name = c_string_view("controller"),
                .param_name = c_string_view("limits"),
                .type_name = c_string_view("array"),
                .update_policy = c_string_view("startup"),
                .current_json = c_string_view(controller_param_json[2]),
                .pending_json = flowrt_string_view_t{},
                .min_json = flowrt_string_view_t{},
                .max_json = flowrt_string_view_t{},
                .choices_json = flowrt_string_view_t{},
                .schema_hash = flowrt::fnv1a64("controller.limits"),
                .revision = 0U,
                .mutable_at_runtime = 0U,
                .has_pending = 0U,
                .has_min = 0U,
                .has_max = 0U,
                .reserved = {},
            },
            flowrt_param_view_t{
                .instance_name = c_string_view("controller"),
                .param_name = c_string_view("mode"),
                .type_name = c_string_view("string"),
                .update_policy = c_string_view("startup"),
                .current_json = c_string_view(controller_param_json[3]),
                .pending_json = flowrt_string_view_t{},
                .min_json = flowrt_string_view_t{},
                .max_json = flowrt_string_view_t{},
                .choices_json = flowrt_string_view_t{},
                .schema_hash = flowrt::fnv1a64("controller.mode"),
                .revision = 0U,
                .mutable_at_runtime = 0U,
                .has_pending = 0U,
                .has_min = 0U,
                .has_max = 0U,
                .reserved = {},
            },
        }};
        std::array<flowrt_param_value_view_t, 4> controller_param_values{{
            flowrt_param_value_view_t{
                .kind = FLOWRT_PARAM_VALUE_BOOL,
                .reserved0 = 0U,
                .value = flowrt_param_value_data_t{.bool_value = params.enabled ? std::uint8_t{1} : std::uint8_t{0}},
                .reserved = {},
            },
            flowrt_param_value_view_t{
                .kind = FLOWRT_PARAM_VALUE_F64,
                .reserved0 = 0U,
                .value = flowrt_param_value_data_t{.f64_value = static_cast<double>(params.gain)},
                .reserved = {},
            },
            flowrt_param_value_view_t{
                .kind = FLOWRT_PARAM_VALUE_JSON,
                .reserved0 = 0U,
                .value = flowrt_param_value_data_t{.json_value = c_string_view(params.limits)},
                .reserved = {},
            },
            flowrt_param_value_view_t{
                .kind = FLOWRT_PARAM_VALUE_STRING,
                .reserved0 = 0U,
                .value = flowrt_param_value_data_t{.string_value = c_string_view(params.mode)},
                .reserved = {},
            },
        }};
        std::array<flowrt_param_view_v1_t, 4> controller_param_views_v1{{
            flowrt_param_view_v1_t{
                .instance_name = c_string_view("controller"),
                .param_name = c_string_view("enabled"),
                .type_name = c_string_view("bool"),
                .update_policy = c_string_view("startup"),
                .current_json = c_string_view(controller_param_json[0]),
                .pending_json = flowrt_string_view_t{},
                .min_json = flowrt_string_view_t{},
                .max_json = flowrt_string_view_t{},
                .choices_json = flowrt_string_view_t{},
                .schema_hash = flowrt::fnv1a64("controller.enabled"),
                .revision = 0U,
                .mutable_at_runtime = 0U,
                .has_pending = 0U,
                .has_min = 0U,
                .has_max = 0U,
                .reserved = {},
                .current_value = controller_param_values[0],
            },
            flowrt_param_view_v1_t{
                .instance_name = c_string_view("controller"),
                .param_name = c_string_view("gain"),
                .type_name = c_string_view("f32"),
                .update_policy = c_string_view("on_tick"),
                .current_json = c_string_view(controller_param_json[1]),
                .pending_json = flowrt_string_view_t{},
                .min_json = c_string_view("0.0"),
                .max_json = c_string_view("10.0"),
                .choices_json = flowrt_string_view_t{},
                .schema_hash = flowrt::fnv1a64("controller.gain"),
                .revision = 0U,
                .mutable_at_runtime = 1U,
                .has_pending = 0U,
                .has_min = 1U,
                .has_max = 1U,
                .reserved = {},
                .current_value = controller_param_values[1],
            },
            flowrt_param_view_v1_t{
                .instance_name = c_string_view("controller"),
                .param_name = c_string_view("limits"),
                .type_name = c_string_view("array"),
                .update_policy = c_string_view("startup"),
                .current_json = c_string_view(controller_param_json[2]),
                .pending_json = flowrt_string_view_t{},
                .min_json = flowrt_string_view_t{},
                .max_json = flowrt_string_view_t{},
                .choices_json = flowrt_string_view_t{},
                .schema_hash = flowrt::fnv1a64("controller.limits"),
                .revision = 0U,
                .mutable_at_runtime = 0U,
                .has_pending = 0U,
                .has_min = 0U,
                .has_max = 0U,
                .reserved = {},
                .current_value = controller_param_values[2],
            },
            flowrt_param_view_v1_t{
                .instance_name = c_string_view("controller"),
                .param_name = c_string_view("mode"),
                .type_name = c_string_view("string"),
                .update_policy = c_string_view("startup"),
                .current_json = c_string_view(controller_param_json[3]),
                .pending_json = flowrt_string_view_t{},
                .min_json = flowrt_string_view_t{},
                .max_json = flowrt_string_view_t{},
                .choices_json = flowrt_string_view_t{},
                .schema_hash = flowrt::fnv1a64("controller.mode"),
                .revision = 0U,
                .mutable_at_runtime = 0U,
                .has_pending = 0U,
                .has_min = 0U,
                .has_max = 0U,
                .reserved = {},
                .current_value = controller_param_values[3],
            },
        }};
        const auto param_snapshot = make_c_param_snapshot(controller_param_views.data(), controller_param_views.size());
        const auto param_snapshot_v1 = make_c_param_snapshot_v1(controller_param_views_v1.data(), controller_param_views_v1.size());
        const auto context = make_c_component_context("controller", "controller", "main", "controller_serial", tick_context, step, tick_time_ms, 0, false, param_snapshot, param_snapshot_v1);
        std::array<std::uint8_t, sizeof(Cmd)> controller_cmd_storage{};
        flowrt_c_output_slot_t controller_cmd_output{
            .name = c_string_view("cmd"),
            .type_name = c_string_view("Cmd"),
            .schema_hash = flowrt::fnv1a64("Cmd"),
            .size_bytes = sizeof(Cmd),
            .data = controller_cmd_storage.data(),
            .capacity = controller_cmd_storage.size(),
            .written_len = 0U,
            .status = FLOWRT_C_OUTPUT_UNWRITTEN,
            .reserved = {},
        };
        std::array<flowrt_c_input_view_t, 0> input_views{};
        flowrt_c_input_array_view_t inputs{.data = nullptr, .len = 0U};
        std::array<flowrt_c_output_slot_t, 1> output_slots{{
            controller_cmd_output,
        }};
        flowrt_c_output_array_view_t outputs{.data = output_slots.data(), .len = output_slots.size()};
        const auto callback_status = status_from_c(callbacks_->run_periodic(callbacks_->user_data, &context, &inputs, &outputs));
        if (callback_status != flowrt::Status::Ok) {
            return callback_status;
        }
        controller_cmd_output = output_slots[0];
        if (controller_cmd_output.status == FLOWRT_C_OUTPUT_WRITTEN && controller_cmd_output.written_len == sizeof(Cmd)) {
            Cmd controller_cmd_value{};
            std::memcpy(&controller_cmd_value, controller_cmd_storage.data(), sizeof(Cmd));
            cmd.write(controller_cmd_value);
        } else if (controller_cmd_output.status == FLOWRT_C_OUTPUT_UNWRITTEN) {
        } else {
            std::cerr << "FlowRT C component output invalid for instance `controller`, port `cmd`: status="
                      << controller_cmd_output.status << " written_len="
                      << controller_cmd_output.written_len << " expected=" << sizeof(Cmd) << '\n';
            return flowrt::Status::Error;
        }
        return flowrt::Status::Ok;
    }

private:
    bool callbacks_valid(std::string_view instance_name) const {
        const char* error = callback_table_validation_error(callbacks_, true, false, false, false);
        if (error == nullptr) {
            return true;
        }
        std::cerr << "FlowRT C component callback table invalid for instance `" << instance_name << "`: " << error << '\n';
        return false;
    }

    flowrt::Status call_lifecycle(flowrt_c_lifecycle_callback_t callback, std::string_view hook_name, std::string_view component_name, std::string_view instance_name, const flowrt::Context& runtime_context) {
        if (!callbacks_valid(instance_name)) {
            return flowrt::Status::Error;
        }
        if (callback == nullptr) {
            return flowrt::Status::Ok;
        }
        const auto context = make_c_component_context(component_name, instance_name, hook_name, std::string_view{}, runtime_context, 0U, 0U, 0U, false, flowrt_c_param_snapshot_v0_t{}, flowrt_c_param_snapshot_v1_t{});
        return status_from_c(callback(callbacks_->user_data, &context));
    }

    const flowrt_c_component_callback_table_t* callbacks_ = nullptr;
};

std::unique_ptr<flowrt_app::ControllerInterface> make_c_controller_adapter() {
    return std::make_unique<CControllerAdapter>(flowrt_app_controller_callbacks());
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

bool flowrt_validate_pending_param_controller_gain(const float& value) {
    return value >= 0.0F && value <= 10.0F;
}

}  // namespace

namespace flowrt_app {

App::App()
    : controller_(make_c_controller_adapter()),
      controller_params_(ControllerParams{
        .enabled = true,
        .gain = 2.0F,
        .limits = "[1,2,3]",
        .mode = "normal",
    }) {
}

flowrt::Status App::step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    if (const auto controller_gain_pending = introspection_state.peek_pending_param("controller.gain"); controller_gain_pending.has_value()) {
        auto controller_gain_next_params = controller_params_;
        if (!decode_flowrt_param_value(*controller_gain_pending, controller_gain_next_params.gain)) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "decode_failed");
        } else if (!flowrt_validate_pending_param_controller_gain(controller_gain_next_params.gain)) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "constraint_failed");
        } else if (controller_ && controller_->on_params_update(controller_params_, controller_gain_next_params, tick_context) != flowrt::Status::Ok) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "callback_rejected");
        } else {
            controller_params_ = std::move(controller_gain_next_params);
            introspection_state.record_param_applied("controller.gain", *controller_gain_pending);
        }
    }
    {
        health_map["controller.main"].name = "controller.main";
        health_map["controller.main"].lane = "controller_serial";
        flowrt::Output<Cmd> controller_cmd;
        if (controller_) {
            switch (static_cast<CControllerAdapter*>(controller_.get())->run_controller_main(static_cast<std::uint64_t>(tick), tick_time_ms, tick_context, controller_params_, controller_cmd)) {
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

FlowrtTaskOutcome App::step_task_controller_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    std::vector<FlowrtOutputCommit> flowrt_output_commits;
    {
        health_map["controller.main"].name = "controller.main";
        health_map["controller.main"].lane = "controller_serial";
        flowrt::Output<Cmd> controller_cmd;
        if (controller_) {
            switch (static_cast<CControllerAdapter*>(controller_.get())->run_controller_main(static_cast<std::uint64_t>(tick), tick_time_ms, tick_context, controller_params_, controller_cmd)) {
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

flowrt::Status App::step_process_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    if (const auto controller_gain_pending = introspection_state.peek_pending_param("controller.gain"); controller_gain_pending.has_value()) {
        auto controller_gain_next_params = controller_params_;
        if (!decode_flowrt_param_value(*controller_gain_pending, controller_gain_next_params.gain)) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "decode_failed");
        } else if (!flowrt_validate_pending_param_controller_gain(controller_gain_next_params.gain)) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "constraint_failed");
        } else if (controller_ && controller_->on_params_update(controller_params_, controller_gain_next_params, tick_context) != flowrt::Status::Ok) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "callback_rejected");
        } else {
            controller_params_ = std::move(controller_gain_next_params);
            introspection_state.record_param_applied("controller.gain", *controller_gain_pending);
        }
    }
    {
        health_map["controller.main"].name = "controller.main";
        health_map["controller.main"].lane = "controller_serial";
        flowrt::Output<Cmd> controller_cmd;
        if (controller_) {
            switch (static_cast<CControllerAdapter*>(controller_.get())->run_controller_main(static_cast<std::uint64_t>(tick), tick_time_ms, tick_context, controller_params_, controller_cmd)) {
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

FlowrtTaskOutcome App::step_process_main_task_controller_main(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {
    const auto tick_time_ms = static_cast<std::uint64_t>(tick);
    (void)tick_time_ms;
    (void)tick_context;
    (void)introspection_state;
    (void)scheduler_events;
    (void)health_map;
    std::vector<FlowrtOutputCommit> flowrt_output_commits;
    {
        health_map["controller.main"].name = "controller.main";
        health_map["controller.main"].lane = "controller_serial";
        flowrt::Output<Cmd> controller_cmd;
        if (controller_) {
            switch (static_cast<CControllerAdapter*>(controller_.get())->run_controller_main(static_cast<std::uint64_t>(tick), tick_time_ms, tick_context, controller_params_, controller_cmd)) {
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

flowrt::Status App::run(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {
    flowrt::Context lifecycle_context;
    auto status = flowrt::Status::Ok;
    (void)backend;
    auto shutdown = flowrt::install_signal_shutdown_token();
    flowrt::IntrospectionState introspection_state;
    flowrt::ScheduleWaiter scheduler_events;
    introspection_state.set_self_description_json(std::string{flowrt_app::self_description_json()});
    introspection_state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.enabled",
        .ty = "bool",
        .update = "startup",
        .current = "true",
        .min = std::nullopt,
        .max = std::nullopt,
        .choices = {},
    });
    introspection_state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.gain",
        .ty = "f32",
        .update = "on_tick",
        .current = "2.0",
        .min = std::optional<std::string>{"0.0"},
        .max = std::optional<std::string>{"10.0"},
        .choices = {},
    });
    introspection_state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.limits",
        .ty = "array",
        .update = "startup",
        .current = "[1,2,3]",
        .min = std::nullopt,
        .max = std::nullopt,
        .choices = {},
    });
    introspection_state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.mode",
        .ty = "string",
        .update = "startup",
        .current = "\"normal\"",
        .min = std::nullopt,
        .max = std::nullopt,
        .choices = {},
    });
    auto introspection_server = flowrt::spawn_status_server(
        flowrt::IntrospectionIdentity{
            .self_description_hash = std::string{flowrt_app::self_description_hash()},
            .package = "c_params_demo",
            .process = "main",
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
        status = step_startup(0, lifecycle_context, introspection_state, scheduler_events, startup_health_map);
    }
    flowrt::DeterministicExecutor scheduler{1};
    flowrt::WorkerPool worker_pool{1};
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("controller_serial")}, flowrt::LaneKind::Serial);
    (void)"controller_serial";
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{flowrt::fnv1a64("controller_serial")}, .priority = 0});
    scheduler.add_periodic(flowrt::PeriodicSpec{.task = flowrt::TaskId{1}, .period = std::chrono::milliseconds{5}});
    scheduler.wake(flowrt::TaskId{1});
    const auto scheduler_base_period_ms = std::uint64_t{5};
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
    if (const auto controller_gain_pending = introspection_state.peek_pending_param("controller.gain"); controller_gain_pending.has_value()) {
        auto controller_gain_next_params = controller_params_;
        if (!decode_flowrt_param_value(*controller_gain_pending, controller_gain_next_params.gain)) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "decode_failed");
        } else if (!flowrt_validate_pending_param_controller_gain(controller_gain_next_params.gain)) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "constraint_failed");
        } else if (controller_ && controller_->on_params_update(controller_params_, controller_gain_next_params, lifecycle_context) != flowrt::Status::Ok) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "callback_rejected");
        } else {
            controller_params_ = std::move(controller_gain_next_params);
            introspection_state.record_param_applied("controller.gain", *controller_gain_pending);
        }
    }
        introspection_state.record_tick(tick_time_ms, clock_source);
        while (true) {
            observed_data_generation = scheduler_events.data_generation();
            const bool woke_on_message = false;
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
auto task_outcome = step_task_controller_main(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
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
        status = step_shutdown(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);
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
    introspection_state.set_self_description_json(std::string{flowrt_app::self_description_json()});
    introspection_state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.enabled",
        .ty = "bool",
        .update = "startup",
        .current = "true",
        .min = std::nullopt,
        .max = std::nullopt,
        .choices = {},
    });
    introspection_state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.gain",
        .ty = "f32",
        .update = "on_tick",
        .current = "2.0",
        .min = std::optional<std::string>{"0.0"},
        .max = std::optional<std::string>{"10.0"},
        .choices = {},
    });
    introspection_state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.limits",
        .ty = "array",
        .update = "startup",
        .current = "[1,2,3]",
        .min = std::nullopt,
        .max = std::nullopt,
        .choices = {},
    });
    introspection_state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.mode",
        .ty = "string",
        .update = "startup",
        .current = "\"normal\"",
        .min = std::nullopt,
        .max = std::nullopt,
        .choices = {},
    });
    auto introspection_server = flowrt::spawn_status_server(
        flowrt::IntrospectionIdentity{
            .self_description_hash = std::string{flowrt_app::self_description_hash()},
            .package = "c_params_demo",
            .process = "main",
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
        status = step_process_main_startup(0, lifecycle_context, introspection_state, scheduler_events, startup_health_map);
    }
    flowrt::DeterministicExecutor scheduler{1};
    flowrt::WorkerPool worker_pool{1};
    scheduler.add_lane(flowrt::LaneId{flowrt::fnv1a64("controller_serial")}, flowrt::LaneKind::Serial);
    (void)"controller_serial";
    scheduler.add_task(flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{flowrt::fnv1a64("controller_serial")}, .priority = 0});
    scheduler.add_periodic(flowrt::PeriodicSpec{.task = flowrt::TaskId{1}, .period = std::chrono::milliseconds{5}});
    scheduler.wake(flowrt::TaskId{1});
    const auto scheduler_base_period_ms = std::uint64_t{5};
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
    if (const auto controller_gain_pending = introspection_state.peek_pending_param("controller.gain"); controller_gain_pending.has_value()) {
        auto controller_gain_next_params = controller_params_;
        if (!decode_flowrt_param_value(*controller_gain_pending, controller_gain_next_params.gain)) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "decode_failed");
        } else if (!flowrt_validate_pending_param_controller_gain(controller_gain_next_params.gain)) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "constraint_failed");
        } else if (controller_ && controller_->on_params_update(controller_params_, controller_gain_next_params, lifecycle_context) != flowrt::Status::Ok) {
            introspection_state.record_param_rejected("controller.gain", *controller_gain_pending, "callback_rejected");
        } else {
            controller_params_ = std::move(controller_gain_next_params);
            introspection_state.record_param_applied("controller.gain", *controller_gain_pending);
        }
    }
        introspection_state.record_tick(tick_time_ms, clock_source);
        while (true) {
            observed_data_generation = scheduler_events.data_generation();
            const bool woke_on_message = false;
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
auto task_outcome = step_process_main_task_controller_main(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);
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
        status = step_process_main_shutdown(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);
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

flowrt::Status run(std::optional<std::size_t> run_ticks) {
    auto backend = flowrt::inproc_backend();
    return flowrt_app::App().run(backend, run_ticks);
}

flowrt::Status run_process(std::string_view process, std::optional<std::size_t> run_ticks) {
    auto backend = flowrt::inproc_backend();
    return flowrt_app::App().run_process(backend, process, run_ticks);
}

}  // namespace flowrt_app
