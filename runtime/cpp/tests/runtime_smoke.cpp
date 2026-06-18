#include <array>
#include <atomic>
#include <cassert>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <flowrt/abi.h>
#include <flowrt/runtime.hpp>
#include <memory>
#include <mutex>
#include <optional>
#include <string_view>
#include <thread>
#include <type_traits>
#include <utility>
#include <vector>

struct Sample {
    std::uint32_t value;
};

struct CompletionGate {
    std::mutex mutex;
    std::condition_variable ready;
    bool entered{false};
    bool released{false};

    void enter_and_wait() {
        std::unique_lock lock(mutex);
        entered = true;
        ready.notify_all();
        ready.wait(lock, [this]() { return released; });
    }

    void wait_entered() {
        std::unique_lock lock(mutex);
        ready.wait(lock, [this]() { return entered; });
    }

    void release() {
        std::lock_guard lock(mutex);
        released = true;
        ready.notify_all();
    }
};

struct TinyWireMessage {
    std::uint16_t value{};

    static constexpr std::size_t wire_size() noexcept { return sizeof(std::uint16_t); }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0, value);
    }

    static TinyWireMessage decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        TinyWireMessage value{};
        value.value = flowrt::read_wire_le<std::uint16_t>(input, 0);
        return value;
    }
};

flowrt::DetachedTask mark_after_schedule(flowrt::ManualExecutor &executor, bool &flag) {
    co_await flowrt::schedule_on(executor);
    flag = true;
}

template <std::size_t N>
void assert_capabilities_equal(flowrt::BackendCapabilities capabilities,
                               const std::array<std::string_view, N> &expected) {
    const auto actual = capabilities.items();
    assert(actual.size() == expected.size());
    for (std::size_t index = 0; index < expected.size(); ++index) {
        assert(actual[index] == expected[index]);
    }
}

int main() {
    static_assert(flowrt::ok() == flowrt::Status::Ok);
    {
        flowrt::ExternalTick grant{.tick_id = 1, .logical_time_ms = 1};
        auto report = flowrt::ExternalTickReport::ok(grant.tick_id);
        assert(report.tick_id == 1);
        assert(report.status == flowrt::Status::Ok);
    }
    static_assert(FLOWRT_ABI_VERSION_MAJOR == 0U);
    static_assert(FLOWRT_ABI_VERSION_MINOR == 2U);
    static_assert(sizeof(flowrt_status_t) == sizeof(std::uint32_t));
    static_assert(FLOWRT_STATUS_OK == 0U);
    static_assert(FLOWRT_STATUS_RETRY == 1U);
    static_assert(FLOWRT_STATUS_ERROR == 2U);
    static_assert(FLOWRT_BACKEND_INPROC == 0U);
    static_assert(FLOWRT_BACKEND_IOX2 == 1U);
    static_assert(FLOWRT_BACKEND_ZENOH == 2U);
    static_assert(FLOWRT_BACKEND_HEALTH_READY == 0U);
    static_assert(FLOWRT_BACKEND_HEALTH_DEGRADED == 1U);
    static_assert(FLOWRT_BACKEND_HEALTH_RECONNECTING == 2U);
    static_assert(FLOWRT_BACKEND_HEALTH_FAILED == 3U);
    static_assert(FLOWRT_BACKEND_HEALTH_UNSUPPORTED == 4U);
    static_assert(FLOWRT_FRAME_LEASE_ATTACHED == 0U);
    static_assert(FLOWRT_FRAME_LEASE_ACQUIRED == 1U);
    static_assert(FLOWRT_FRAME_LEASE_RELEASED == 2U);
    static_assert(FLOWRT_FRAME_LEASE_EXPIRED == 3U);
    static_assert(FLOWRT_FRAME_LEASE_GENERATION_MISMATCH == 4U);
    static_assert(FLOWRT_FRAME_LEASE_ERROR == 5U);
    static_assert(FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR == 0U);
    static_assert(FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR == 2U);
    static_assert(FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 == 1U);
    static_assert(FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1 == 2U);
    static_assert(FLOWRT_C_CLOCK_SOURCE_RUNTIME == 0U);
    static_assert(FLOWRT_C_CLOCK_SOURCE_REPLAY == 1U);
    static_assert(FLOWRT_C_OUTPUT_UNWRITTEN == 0U);
    static_assert(FLOWRT_C_OUTPUT_WRITTEN == 1U);
    static_assert(FLOWRT_C_OUTPUT_TRUNCATED == 2U);
    static_assert(FLOWRT_C_OUTPUT_ERROR == 3U);
    static_assert(FLOWRT_FRAME_ENCODING_FIXED_PLAIN == 0U);
    static_assert(FLOWRT_FRAME_ENCODING_CANONICAL_FRAME_V1 == 1U);
    static_assert(FLOWRT_PARAMS_UPDATE_ACCEPTED == 0U);
    static_assert(FLOWRT_PARAMS_UPDATE_APPLIED == 1U);
    static_assert(FLOWRT_PARAMS_UPDATE_REJECTED == 2U);
    static_assert(FLOWRT_PARAMS_UPDATE_PARTIAL == 3U);
    static_assert(FLOWRT_PARAMS_UPDATE_UNSUPPORTED == 4U);
    static_assert(FLOWRT_PARAMS_UPDATE_ERROR == 5U);
    static_assert(FLOWRT_OPERATION_STATE_IDLE == 0U);
    static_assert(FLOWRT_OPERATION_STATE_STARTING == 1U);
    static_assert(FLOWRT_OPERATION_STATE_RUNNING == 2U);
    static_assert(FLOWRT_OPERATION_STATE_CANCEL_REQUESTED == 3U);
    static_assert(FLOWRT_OPERATION_STATE_SUCCEEDED == 4U);
    static_assert(FLOWRT_OPERATION_STATE_FAILED == 5U);
    static_assert(FLOWRT_OPERATION_STATE_CANCELLED == 6U);
    static_assert(FLOWRT_OPERATION_STATE_TIMED_OUT == 7U);
    static_assert(FLOWRT_DIAGNOSTIC_INFO == 0U);
    static_assert(FLOWRT_DIAGNOSTIC_WARN == 1U);
    static_assert(FLOWRT_DIAGNOSTIC_ERROR == 2U);
    static_assert(FLOWRT_RESOURCE_HEALTH_UNKNOWN == 0U);
    static_assert(FLOWRT_RESOURCE_HEALTH_READY == 1U);
    static_assert(FLOWRT_RESOURCE_HEALTH_DEGRADED == 2U);
    static_assert(FLOWRT_RESOURCE_HEALTH_FAILED == 3U);
    static_assert(FLOWRT_RESOURCE_HEALTH_UNAVAILABLE == 4U);
    static_assert(offsetof(flowrt_string_view_t, data) == 0U);
    static_assert(offsetof(flowrt_string_view_t, len) == sizeof(void *));
    static_assert(sizeof(flowrt_string_view_t) == sizeof(void *) * 2U);
    static_assert(offsetof(flowrt_reconnect_policy_t, initial_delay_ms) == 0U);
    static_assert(offsetof(flowrt_reconnect_policy_t, max_delay_ms) == 8U);
    static_assert(offsetof(flowrt_reconnect_policy_t, max_attempts) == 16U);
    static_assert(offsetof(flowrt_reconnect_policy_t, has_max_attempts) == 20U);
    static_assert(offsetof(flowrt_backend_health_snapshot_t, state) == 0U);
    static_assert(offsetof(flowrt_backend_health_snapshot_t, attempt) == 4U);
    static_assert(offsetof(flowrt_backend_health_snapshot_t, next_retry_unix_ms) == 8U);
    static_assert(offsetof(flowrt_backend_health_snapshot_t, last_error) == 16U);
    static_assert(sizeof(flowrt_u128_t) == 16U);
    static_assert(sizeof(flowrt_i128_t) == 16U);
    static_assert(offsetof(flowrt_u128_t, lo) == 0U);
    static_assert(offsetof(flowrt_u128_t, hi) == 8U);
    static_assert(offsetof(flowrt_i128_t, lo) == 0U);
    static_assert(offsetof(flowrt_i128_t, hi) == 8U);
    static_assert(offsetof(flowrt_resource_descriptor_t, resource_id) == 0U);
    static_assert(offsetof(flowrt_resource_descriptor_t, slot) == sizeof(flowrt_string_view_t));
    static_assert(offsetof(flowrt_frame_descriptor_t, resource) == 0U);
    static_assert(offsetof(flowrt_frame_descriptor_t, size_bytes) ==
                  sizeof(flowrt_resource_descriptor_t));
    static_assert(sizeof(flowrt_frame_view_t) == 128U);
    static_assert(alignof(flowrt_frame_view_t) == alignof(void *));
    static_assert(offsetof(flowrt_frame_view_t, channel_name) == 0U);
    static_assert(offsetof(flowrt_frame_view_t, message_type) == sizeof(flowrt_string_view_t));
    static_assert(offsetof(flowrt_frame_view_t, schema_hash) == 32U);
    static_assert(offsetof(flowrt_frame_view_t, encoding) == 40U);
    static_assert(offsetof(flowrt_frame_view_t, flags) == 44U);
    static_assert(offsetof(flowrt_frame_view_t, frame) == 48U);
    static_assert(offsetof(flowrt_frame_view_t, header) == 64U);
    static_assert(offsetof(flowrt_frame_view_t, tail) == 80U);
    static_assert(offsetof(flowrt_frame_view_t, source_time_ms) == 96U);
    static_assert(offsetof(flowrt_frame_view_t, published_at_ms) == 104U);
    static_assert(offsetof(flowrt_frame_view_t, revision) == 112U);
    static_assert(offsetof(flowrt_frame_view_t, has_source_time_ms) == 120U);
    static_assert(offsetof(flowrt_frame_view_t, has_published_at_ms) == 121U);
    static_assert(offsetof(flowrt_frame_view_t, has_revision) == 122U);
    static_assert(sizeof(flowrt_param_view_t) == 168U);
    static_assert(alignof(flowrt_param_view_t) == alignof(void *));
    static_assert(offsetof(flowrt_param_view_t, instance_name) == 0U);
    static_assert(offsetof(flowrt_param_view_t, param_name) == 16U);
    static_assert(offsetof(flowrt_param_view_t, type_name) == 32U);
    static_assert(offsetof(flowrt_param_view_t, update_policy) == 48U);
    static_assert(offsetof(flowrt_param_view_t, current_json) == 64U);
    static_assert(offsetof(flowrt_param_view_t, pending_json) == 80U);
    static_assert(offsetof(flowrt_param_view_t, min_json) == 96U);
    static_assert(offsetof(flowrt_param_view_t, max_json) == 112U);
    static_assert(offsetof(flowrt_param_view_t, choices_json) == 128U);
    static_assert(offsetof(flowrt_param_view_t, schema_hash) == 144U);
    static_assert(offsetof(flowrt_param_view_t, revision) == 152U);
    static_assert(offsetof(flowrt_param_view_t, mutable_at_runtime) == 160U);
    static_assert(offsetof(flowrt_param_view_t, has_pending) == 161U);
    static_assert(offsetof(flowrt_param_view_t, has_min) == 162U);
    static_assert(offsetof(flowrt_param_view_t, has_max) == 163U);
    static_assert(sizeof(flowrt_params_view_t) == 40U);
    static_assert(alignof(flowrt_params_view_t) == alignof(void *));
    static_assert(offsetof(flowrt_params_view_t, data) == 0U);
    static_assert(offsetof(flowrt_params_view_t, len) == sizeof(void *));
    static_assert(offsetof(flowrt_params_view_t, revision) == 16U);
    static_assert(offsetof(flowrt_params_view_t, applied_unix_ms) == 24U);
    static_assert(offsetof(flowrt_params_view_t, has_applied_unix_ms) == 32U);
    static_assert(sizeof(flowrt_params_update_result_t) == 56U);
    static_assert(alignof(flowrt_params_update_result_t) == alignof(void *));
    static_assert(offsetof(flowrt_params_update_result_t, status) == 0U);
    static_assert(offsetof(flowrt_params_update_result_t, applied_count) == 4U);
    static_assert(offsetof(flowrt_params_update_result_t, rejected_count) == 8U);
    static_assert(offsetof(flowrt_params_update_result_t, revision) == 16U);
    static_assert(offsetof(flowrt_params_update_result_t, error_index) == 24U);
    static_assert(offsetof(flowrt_params_update_result_t, has_error_index) == 32U);
    static_assert(offsetof(flowrt_params_update_result_t, message) == 40U);
    static_assert(sizeof(flowrt_operation_id_t) == 24U);
    static_assert(alignof(flowrt_operation_id_t) == alignof(std::uint64_t));
    static_assert(offsetof(flowrt_operation_id_t, operation_key) == 0U);
    static_assert(offsetof(flowrt_operation_id_t, client_id) == 8U);
    static_assert(offsetof(flowrt_operation_id_t, sequence) == 16U);
    static_assert(sizeof(flowrt_operation_id_array_view_t) == sizeof(void *) * 2U);
    static_assert(alignof(flowrt_operation_id_array_view_t) == alignof(void *));
    static_assert(offsetof(flowrt_operation_id_array_view_t, data) == 0U);
    static_assert(offsetof(flowrt_operation_id_array_view_t, len) == sizeof(void *));
    static_assert(sizeof(flowrt_operation_status_view_t) == 112U);
    static_assert(alignof(flowrt_operation_status_view_t) == alignof(void *));
    static_assert(offsetof(flowrt_operation_status_view_t, operation_name) == 0U);
    static_assert(offsetof(flowrt_operation_status_view_t, current_operation_ids) == 16U);
    static_assert(offsetof(flowrt_operation_status_view_t, running) == 32U);
    static_assert(offsetof(flowrt_operation_status_view_t, queued) == 40U);
    static_assert(offsetof(flowrt_operation_status_view_t, total_started) == 48U);
    static_assert(offsetof(flowrt_operation_status_view_t, succeeded_count) == 56U);
    static_assert(offsetof(flowrt_operation_status_view_t, failed_count) == 64U);
    static_assert(offsetof(flowrt_operation_status_view_t, canceled_count) == 72U);
    static_assert(offsetof(flowrt_operation_status_view_t, timeout_count) == 80U);
    static_assert(offsetof(flowrt_operation_status_view_t, preempted_count) == 88U);
    static_assert(offsetof(flowrt_operation_status_view_t, last_transition_ms) == 96U);
    static_assert(offsetof(flowrt_operation_status_view_t, ready) == 104U);
    static_assert(offsetof(flowrt_operation_status_view_t, has_last_transition_ms) == 105U);
    static_assert(sizeof(flowrt_operation_progress_view_t) == 192U);
    static_assert(alignof(flowrt_operation_progress_view_t) == alignof(void *));
    static_assert(offsetof(flowrt_operation_progress_view_t, operation_name) == 0U);
    static_assert(offsetof(flowrt_operation_progress_view_t, id) == 16U);
    static_assert(offsetof(flowrt_operation_progress_view_t, sequence) == 40U);
    static_assert(offsetof(flowrt_operation_progress_view_t, progress) == 48U);
    static_assert(offsetof(flowrt_operation_progress_view_t, published_at_ms) == 176U);
    static_assert(offsetof(flowrt_operation_progress_view_t, has_published_at_ms) == 184U);
    static_assert(sizeof(flowrt_operation_result_summary_view_t) == 200U);
    static_assert(alignof(flowrt_operation_result_summary_view_t) == alignof(void *));
    static_assert(offsetof(flowrt_operation_result_summary_view_t, operation_name) == 0U);
    static_assert(offsetof(flowrt_operation_result_summary_view_t, id) == 16U);
    static_assert(offsetof(flowrt_operation_result_summary_view_t, state) == 40U);
    static_assert(offsetof(flowrt_operation_result_summary_view_t, has_result) == 44U);
    static_assert(offsetof(flowrt_operation_result_summary_view_t, has_error_message) == 45U);
    static_assert(offsetof(flowrt_operation_result_summary_view_t, has_completed_unix_ms) == 46U);
    static_assert(offsetof(flowrt_operation_result_summary_view_t, completed_unix_ms) == 48U);
    static_assert(offsetof(flowrt_operation_result_summary_view_t, result) == 56U);
    static_assert(offsetof(flowrt_operation_result_summary_view_t, error_message) == 184U);
    static_assert(sizeof(flowrt_diagnostic_view_t) == 72U);
    static_assert(alignof(flowrt_diagnostic_view_t) == alignof(void *));
    static_assert(offsetof(flowrt_diagnostic_view_t, source) == 0U);
    static_assert(offsetof(flowrt_diagnostic_view_t, code) == 16U);
    static_assert(offsetof(flowrt_diagnostic_view_t, message) == 32U);
    static_assert(offsetof(flowrt_diagnostic_view_t, severity) == 48U);
    static_assert(offsetof(flowrt_diagnostic_view_t, timestamp_unix_ms) == 56U);
    static_assert(offsetof(flowrt_diagnostic_view_t, has_timestamp_unix_ms) == 64U);
    static_assert(sizeof(flowrt_resource_health_snapshot_t) == 88U);
    static_assert(alignof(flowrt_resource_health_snapshot_t) == alignof(void *));
    static_assert(offsetof(flowrt_resource_health_snapshot_t, name) == 0U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, capability) == 16U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, state) == 32U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, ready) == 36U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, required) == 37U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, has_updated_unix_ms) == 38U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, has_generation) == 39U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, updated_unix_ms) == 40U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, generation) == 48U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, message) == 56U);
    static_assert(offsetof(flowrt_resource_health_snapshot_t, last_error) == 72U);
    static_assert(sizeof(flowrt_diagnostic_array_view_t) == sizeof(void *) * 2U);
    static_assert(alignof(flowrt_diagnostic_array_view_t) == alignof(void *));
    static_assert(sizeof(flowrt_resource_health_array_view_t) == sizeof(void *) * 2U);
    static_assert(alignof(flowrt_resource_health_array_view_t) == alignof(void *));
    static_assert(sizeof(flowrt_diagnostics_snapshot_t) == 80U);
    static_assert(alignof(flowrt_diagnostics_snapshot_t) == alignof(void *));
    static_assert(offsetof(flowrt_diagnostics_snapshot_t, package_name) == 0U);
    static_assert(offsetof(flowrt_diagnostics_snapshot_t, process_name) == 16U);
    static_assert(offsetof(flowrt_diagnostics_snapshot_t, diagnostics) == 32U);
    static_assert(offsetof(flowrt_diagnostics_snapshot_t, resources) == 48U);
    static_assert(offsetof(flowrt_diagnostics_snapshot_t, generated_unix_ms) == 64U);
    static_assert(offsetof(flowrt_diagnostics_snapshot_t, healthy) == 72U);
    static_assert(offsetof(flowrt_diagnostics_snapshot_t, has_generated_unix_ms) == 73U);
    static_assert(sizeof(flowrt_c_task_timing_t) == 120U);
    static_assert(alignof(flowrt_c_task_timing_t) == alignof(void *));
    static_assert(offsetof(flowrt_c_task_timing_t, step) == 0U);
    static_assert(offsetof(flowrt_c_task_timing_t, task_name) == 8U);
    static_assert(offsetof(flowrt_c_task_timing_t, trigger) == 24U);
    static_assert(offsetof(flowrt_c_task_timing_t, clock_source) == 40U);
    static_assert(offsetof(flowrt_c_task_timing_t, scheduled_time_ms) == 48U);
    static_assert(offsetof(flowrt_c_task_timing_t, observed_time_ms) == 56U);
    static_assert(offsetof(flowrt_c_task_timing_t, scheduled_delta_ms) == 64U);
    static_assert(offsetof(flowrt_c_task_timing_t, observed_delta_ms) == 72U);
    static_assert(offsetof(flowrt_c_task_timing_t, period_ms) == 80U);
    static_assert(offsetof(flowrt_c_task_timing_t, deadline_ms) == 88U);
    static_assert(offsetof(flowrt_c_task_timing_t, lateness_ms) == 96U);
    static_assert(offsetof(flowrt_c_task_timing_t, missed_periods) == 104U);
    static_assert(offsetof(flowrt_c_task_timing_t, has_period_ms) == 112U);
    static_assert(offsetof(flowrt_c_task_timing_t, has_deadline_ms) == 113U);
    static_assert(offsetof(flowrt_c_task_timing_t, deadline_missed) == 114U);
    static_assert(offsetof(flowrt_c_task_timing_t, overrun) == 115U);
    static_assert(sizeof(flowrt_c_component_context_t) == 216U);
    static_assert(offsetof(flowrt_c_component_context_t, component_name) == 0U);
    static_assert(offsetof(flowrt_c_component_context_t, instance_name) ==
                  sizeof(flowrt_string_view_t));
    static_assert(offsetof(flowrt_c_component_context_t, task_name) ==
                  sizeof(flowrt_string_view_t) * 2U);
    static_assert(offsetof(flowrt_c_component_context_t, lane_name) ==
                  sizeof(flowrt_string_view_t) * 3U);
    static_assert(offsetof(flowrt_c_component_context_t, step) == 64U);
    static_assert(offsetof(flowrt_c_component_context_t, tick_time_ms) == 72U);
    static_assert(offsetof(flowrt_c_component_context_t, deadline_ms) == 80U);
    static_assert(offsetof(flowrt_c_component_context_t, has_deadline_ms) == 88U);
    static_assert(offsetof(flowrt_c_component_context_t, has_timing) == 89U);
    static_assert(offsetof(flowrt_c_component_context_t, timing) == 96U);
    static_assert(sizeof(flowrt_c_input_view_t) == 88U);
    static_assert(offsetof(flowrt_c_input_view_t, name) == 0U);
    static_assert(offsetof(flowrt_c_input_view_t, type_name) == sizeof(flowrt_string_view_t));
    static_assert(offsetof(flowrt_c_input_view_t, schema_hash) == 32U);
    static_assert(offsetof(flowrt_c_input_view_t, size_bytes) == 40U);
    static_assert(offsetof(flowrt_c_input_view_t, payload) == 48U);
    static_assert(offsetof(flowrt_c_input_view_t, source_time_ms) == 64U);
    static_assert(offsetof(flowrt_c_input_view_t, revision) == 72U);
    static_assert(offsetof(flowrt_c_input_view_t, present) == 80U);
    static_assert(offsetof(flowrt_c_input_view_t, stale) == 81U);
    static_assert(sizeof(flowrt_c_output_slot_t) == 80U);
    static_assert(offsetof(flowrt_c_output_slot_t, name) == 0U);
    static_assert(offsetof(flowrt_c_output_slot_t, type_name) == sizeof(flowrt_string_view_t));
    static_assert(offsetof(flowrt_c_output_slot_t, schema_hash) == 32U);
    static_assert(offsetof(flowrt_c_output_slot_t, size_bytes) == 40U);
    static_assert(offsetof(flowrt_c_output_slot_t, data) == 48U);
    static_assert(offsetof(flowrt_c_output_slot_t, capacity) == 56U);
    static_assert(offsetof(flowrt_c_output_slot_t, written_len) == 64U);
    static_assert(offsetof(flowrt_c_output_slot_t, status) == 72U);
    static_assert(sizeof(flowrt_c_input_array_view_t) == sizeof(void *) * 2U);
    static_assert(offsetof(flowrt_c_input_array_view_t, data) == 0U);
    static_assert(offsetof(flowrt_c_input_array_view_t, len) == sizeof(void *));
    static_assert(sizeof(flowrt_c_output_array_view_t) == sizeof(void *) * 2U);
    static_assert(offsetof(flowrt_c_output_array_view_t, data) == 0U);
    static_assert(offsetof(flowrt_c_output_array_view_t, len) == sizeof(void *));
    static_assert(sizeof(flowrt_c_component_callback_table_t) == 160U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, size) == 0U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, version_major) == 4U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, version_minor) == 8U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, feature_flags) == 16U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, user_data) == 24U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, on_init) == 32U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, on_start) == 40U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, on_stop) == 48U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, on_shutdown) == 56U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, run_periodic) == 64U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, run_on_message) == 72U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, run_startup) == 80U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, run_shutdown) == 88U);
    static_assert(offsetof(flowrt_c_component_callback_table_t, reserved) == 96U);
    static_assert(std::is_same_v<flowrt::UInt128, flowrt_u128_t>);
    static_assert(std::is_same_v<flowrt::Int128, flowrt_i128_t>);

    flowrt::Context context;
    (void)context;
    assert(!context.is_io_boundary());
    assert(context.timing() == nullptr);
    assert(context.now_ms() == std::nullopt);
    assert(context.dt_ms() == std::nullopt);
    assert(context.now_secs() == std::nullopt);
    assert(context.dt_secs() == std::nullopt);
    const flowrt::TaskTiming timing{
        .step = 3U,
        .task_name = "planner.update",
        .trigger = "on_message",
        .clock_source = flowrt::ClockSource::Runtime,
        .scheduled_time_ms = 120U,
        .observed_time_ms = 125U,
        .scheduled_delta_ms = 40U,
        .observed_delta_ms = 45U,
        .deadline_ms = 10U,
        .lateness_ms = 5U,
        .missed_periods = 2U,
    };
    auto task_context = flowrt::Context::with_timing(timing);
    assert(!task_context.is_io_boundary());
    assert(task_context.boundary() == nullptr);
    assert(task_context.timing() != nullptr);
    assert(task_context.timing()->task_name == "planner.update");
    assert(task_context.timing()->trigger == "on_message");
    assert(task_context.timing()->clock_source == flowrt::ClockSource::Runtime);
    assert(task_context.timing()->period_ms == std::nullopt);
    assert(task_context.timing()->deadline_ms == 10U);
    assert(task_context.now_ms() == 125U);
    assert(task_context.dt_ms() == 45U);
    assert(task_context.now_secs().has_value() && *task_context.now_secs() == 125.0 / 1000.0);
    assert(task_context.dt_secs().has_value() && *task_context.dt_secs() == 45.0 / 1000.0);
    task_context.set_timing(flowrt::TaskTiming{
        .step = 4U,
        .task_name = "planner.update",
        .trigger = "periodic",
        .clock_source = flowrt::ClockSource::Replay,
        .scheduled_time_ms = 200U,
        .observed_time_ms = 200U,
        .scheduled_delta_ms = 40U,
        .observed_delta_ms = 40U,
        .period_ms = 40U,
    });
    assert(task_context.timing()->clock_source == flowrt::ClockSource::Replay);
    assert(task_context.timing()->period_ms == 40U);
    bool boundary_reported = false;
    auto boundary_context = flowrt::Context::for_boundary(flowrt::BoundaryContext{
        "camera", "CameraDriver",
        std::vector<flowrt::BoundaryResourceStatus>{
            flowrt::BoundaryResourceStatus{.name = "camera_shm", .kind = "shm"}},
        [&boundary_reported](flowrt::BoundaryStatus status) {
            boundary_reported = true;
            assert(status.name == "camera");
            assert(status.component == "CameraDriver");
            assert(status.ready);
        }});
    assert(boundary_context.is_io_boundary());
    assert(boundary_context.boundary() != nullptr);
    assert(boundary_context.timing() == nullptr);
    boundary_context.boundary()->mark_ready();
    assert(boundary_reported);

    const flowrt_string_view_t label_view{
        .data = "imu",
        .len = 3U,
    };
    assert(label_view.data[0] == 'i');
    assert(label_view.len == 3U);

    const std::array<std::uint8_t, 3> bytes{1U, 2U, 3U};
    const flowrt_bytes_view_t bytes_view{
        .data = bytes.data(),
        .len = bytes.size(),
    };
    assert(bytes_view.data[2] == 3U);
    assert(bytes_view.len == 3U);

    const flowrt::UInt128 wide_unsigned{0x0123456789ABCDEFULL, 0xFEDCBA9876543210ULL};
    std::array<std::uint8_t, 16> wide_wire{};
    flowrt::write_wire_le(wide_wire, 0, wide_unsigned);
    assert((wide_wire == std::array<std::uint8_t, 16>{0xEFU, 0xCDU, 0xABU, 0x89U, 0x67U, 0x45U,
                                                      0x23U, 0x01U, 0x10U, 0x32U, 0x54U, 0x76U,
                                                      0x98U, 0xBAU, 0xDCU, 0xFEU}));
    const auto wide_unsigned_roundtrip = flowrt::read_wire_le<flowrt::UInt128>(wide_wire, 0);
    assert(wide_unsigned_roundtrip.lo == wide_unsigned.lo);
    assert(wide_unsigned_roundtrip.hi == wide_unsigned.hi);
    const flowrt::Int128 wide_signed{0xFFFFFFFFFFFFFFFFULL, 0xFFFFFFFFFFFFFFFFULL};
    flowrt::write_wire_le(wide_wire, 0, wide_signed);
    const auto wide_signed_roundtrip = flowrt::read_wire_le<flowrt::Int128>(wide_wire, 0);
    assert(wide_signed_roundtrip.lo == wide_signed.lo);
    assert(wide_signed_roundtrip.hi == wide_signed.hi);

    const flowrt_reconnect_policy_t abi_policy{
        .initial_delay_ms = 100U,
        .max_delay_ms = 1000U,
        .max_attempts = 3U,
        .has_max_attempts = 1U,
        .reserved = {0U, 0U, 0U},
    };
    assert(abi_policy.initial_delay_ms == 100U);
    assert(abi_policy.max_attempts == 3U);
    assert(abi_policy.has_max_attempts == 1U);

    const flowrt_backend_health_snapshot_t abi_snapshot{
        .state = FLOWRT_BACKEND_HEALTH_RECONNECTING,
        .attempt = 2U,
        .next_retry_unix_ms = 123456U,
        .last_error = label_view,
        .has_next_retry_unix_ms = 1U,
        .recoverable = 1U,
        .reserved = {0U, 0U, 0U, 0U, 0U, 0U},
    };
    assert(abi_snapshot.state == FLOWRT_BACKEND_HEALTH_RECONNECTING);
    assert(abi_snapshot.last_error.len == 3U);
    assert(abi_snapshot.recoverable == 1U);

    std::array<std::uint8_t, TinyWireMessage::wire_size()> tiny_wire{};
    TinyWireMessage{0x1234U}.encode_wire(tiny_wire);
    assert((tiny_wire == std::array<std::uint8_t, 2>{0x34U, 0x12U}));
    assert(TinyWireMessage::decode_wire(tiny_wire).value == 0x1234U);
    bool saw_wire_size_error = false;
    try {
        TinyWireMessage{7U}.encode_wire(std::span<std::uint8_t>{tiny_wire.data(), 1});
    } catch (const flowrt::WireCodecError &error) {
        saw_wire_size_error = true;
        assert(error.expected() == 2U);
        assert(error.actual() == 1U);
    }
    assert(saw_wire_size_error);
    const flowrt::WireCodecError content_error("invalid frame");
    assert(content_error.expected() == 0U);
    assert(content_error.actual() == 0U);

    std::vector<std::uint8_t> variable_tail;
    const auto payload_span =
        flowrt::append_tail_block(variable_tail, std::span<const std::uint8_t>{bytes});
    const std::array<std::uint8_t, 2> label_bytes{static_cast<std::uint8_t>('o'),
                                                  static_cast<std::uint8_t>('k')};
    const auto label_span =
        flowrt::append_tail_block(variable_tail, std::span<const std::uint8_t>{label_bytes});
    const auto empty_span =
        flowrt::append_tail_block(variable_tail, std::span<const std::uint8_t>{});
    std::array<std::uint8_t, flowrt::VAR_SPAN_WIRE_SIZE * 3U> variable_header{};
    flowrt::write_var_span(std::span<std::uint8_t>{variable_header}.subspan(0, 8), payload_span);
    flowrt::write_var_span(std::span<std::uint8_t>{variable_header}.subspan(8, 8), label_span);
    flowrt::write_var_span(std::span<std::uint8_t>{variable_header}.subspan(16, 8), empty_span);
    assert((variable_header == std::array<std::uint8_t, 24>{0U, 0U, 0U, 0U, 3U, 0U, 0U, 0U,
                                                            3U, 0U, 0U, 0U, 2U, 0U, 0U, 0U,
                                                            0U, 0U, 0U, 0U, 0U, 0U, 0U, 0U}));
    assert((variable_tail == std::vector<std::uint8_t>{1U, 2U, 3U, 'o', 'k'}));
    flowrt::FrameDecoder frame_decoder{std::span<const std::uint8_t>{variable_tail}};
    const auto payload_block = frame_decoder.read_block(
        flowrt::read_var_span(std::span<const std::uint8_t>{variable_header}.subspan(0, 8)));
    assert(payload_block.size() == 3U);
    assert(payload_block[0] == 1U);
    const auto label_block = frame_decoder.read_block(
        flowrt::read_var_span(std::span<const std::uint8_t>{variable_header}.subspan(8, 8)));
    assert(label_block.size() == 2U);
    assert(label_block[0] == static_cast<std::uint8_t>('o'));
    const auto empty_block = frame_decoder.read_block(
        flowrt::read_var_span(std::span<const std::uint8_t>{variable_header}.subspan(16, 8)));
    assert(empty_block.empty());
    frame_decoder.finish();

    flowrt::InprocBackend inproc_backend;
    assert(inproc_backend.kind() == flowrt::BackendKind::Inproc);
    assert(inproc_backend.capabilities().contains("channel:latest"));
    assert(inproc_backend.capabilities().contains("graph:static_graph"));
    assert(inproc_backend.capabilities().contains("timing:deadline_aware"));
    assert_capabilities_equal(inproc_backend.capabilities(), std::array<std::string_view, 24>{
                                                                 "abi:fixed_size_plain_data",
                                                                 "abi:variable_payload_frame",
                                                                 "layout:native_layout",
                                                                 "allocation:bounded",
                                                                 "allocation:unbounded_dynamic",
                                                                 "graph:static_graph",
                                                                 "trigger:periodic",
                                                                 "trigger:on_message",
                                                                 "trigger:startup",
                                                                 "trigger:shutdown",
                                                                 "timing:deadline_aware",
                                                                 "channel:latest",
                                                                 "channel:fifo",
                                                                 "overflow:drop_oldest",
                                                                 "overflow:drop_newest",
                                                                 "overflow:error",
                                                                 "overflow:block",
                                                                 "stale:warn",
                                                                 "stale:drop",
                                                                 "stale:hold_last",
                                                                 "stale:error",
                                                                 "topology:single_process",
                                                                 "transfer:copy",
                                                                 "observability:health",
                                                             });

    static_assert(!flowrt::Iox2Backend::compiled_with_transport(),
                  "default build should not have iox2 transport");
    static_assert(!flowrt::ZenohBackend::compiled_with_transport(),
                  "default build should not have zenoh transport");

    flowrt::Iox2Backend iox2_backend;
    assert(iox2_backend.kind() == flowrt::BackendKind::Iox2);
    assert(iox2_backend.capabilities().contains("topology:multi_process"));
    assert(!iox2_backend.capabilities().contains("overflow:drop_newest"));
    assert(!iox2_backend.capabilities().contains("overflow:error"));
    assert_capabilities_equal(iox2_backend.capabilities(), std::array<std::string_view, 22>{
                                                               "abi:fixed_size_plain_data",
                                                               "layout:native_layout",
                                                               "allocation:bounded",
                                                               "graph:static_graph",
                                                               "trigger:periodic",
                                                               "trigger:on_message",
                                                               "trigger:startup",
                                                               "trigger:shutdown",
                                                               "timing:deadline_aware",
                                                               "channel:latest",
                                                               "channel:fifo",
                                                               "overflow:drop_oldest",
                                                               "overflow:block",
                                                               "stale:warn",
                                                               "stale:drop",
                                                               "stale:hold_last",
                                                               "stale:error",
                                                               "topology:multi_process",
                                                               "topology:single_host",
                                                               "transfer:zero_copy",
                                                               "transfer:loaned",
                                                               "observability:health",
                                                           });

    flowrt::ZenohBackend zenoh_backend;
    assert(zenoh_backend.kind() == flowrt::BackendKind::Zenoh);
    assert(zenoh_backend.capabilities().contains("topology:multi_process"));
    assert(zenoh_backend.capabilities().contains("topology:multi_host"));
    assert(zenoh_backend.capabilities().contains("transfer:copy"));
    assert_capabilities_equal(zenoh_backend.capabilities(), std::array<std::string_view, 22>{
                                                                "abi:fixed_size_plain_data",
                                                                "abi:variable_payload_frame",
                                                                "layout:native_layout",
                                                                "allocation:bounded",
                                                                "allocation:unbounded_dynamic",
                                                                "graph:static_graph",
                                                                "trigger:periodic",
                                                                "trigger:on_message",
                                                                "trigger:startup",
                                                                "trigger:shutdown",
                                                                "timing:deadline_aware",
                                                                "channel:latest",
                                                                "channel:fifo",
                                                                "overflow:drop_oldest",
                                                                "stale:warn",
                                                                "stale:drop",
                                                                "stale:hold_last",
                                                                "stale:error",
                                                                "topology:multi_process",
                                                                "topology:multi_host",
                                                                "transfer:copy",
                                                                "observability:health",
                                                            });
    flowrt::ReconnectPolicy policy{100U, 1000U, std::optional<std::uint32_t>{3U}};
    assert(policy.initial_delay_ms == 100U);
    assert(policy.max_delay_ms == 1000U);
    assert(policy.max_attempts == std::optional<std::uint32_t>{3U});
    assert(policy.delay_for_attempt(0U) == 100U);
    assert(policy.delay_for_attempt(1U) == 200U);
    assert(policy.delay_for_attempt(4U) == 1000U);
    assert(policy.can_retry(2U));
    assert(!policy.can_retry(3U));

    flowrt::BackendHealthTracker tracker{policy};
    assert(tracker.snapshot().state == flowrt::BackendHealthState::Ready);
    assert(tracker.snapshot().attempt == 0U);
    assert(!tracker.snapshot().recoverable);
    tracker.mark_degraded("receive failed");
    assert(tracker.snapshot().state == flowrt::BackendHealthState::Degraded);
    assert(tracker.snapshot().last_error == std::optional<std::string>{"receive failed"});
    assert(tracker.snapshot().recoverable);
    tracker.mark_reconnecting(1U, 500U);
    assert(tracker.snapshot().state == flowrt::BackendHealthState::Reconnecting);
    assert(tracker.snapshot().attempt == 1U);
    assert(tracker.snapshot().next_retry_unix_ms == std::optional<std::uint64_t>{500U});
    tracker.mark_ready();
    assert(tracker.snapshot() == flowrt::BackendHealthSnapshot::ready());
    tracker.mark_failed("retry budget exhausted", 3U);
    assert(tracker.snapshot().state == flowrt::BackendHealthState::Failed);
    assert(tracker.snapshot().attempt == 3U);
    assert(tracker.snapshot().last_error == std::optional<std::string>{"retry budget exhausted"});
    assert(!tracker.snapshot().recoverable);

    assert(zenoh_backend.health().state == flowrt::BackendHealthState::Ready);
    assert(zenoh_backend.reconnect_policy().initial_delay_ms == 100U);
    assert(zenoh_backend.reconnect_policy().max_delay_ms == 5000U);
    assert(!zenoh_backend.reconnect_policy().max_attempts.has_value());

    std::size_t seen = 0;
    const auto scheduler_status = inproc_backend.scheduler().run_ticks(
        5, [&seen](std::size_t tick, flowrt::Context &) -> flowrt::Status {
            ++seen;
            if (tick == 2) {
                return flowrt::Status::Error;
            }
            return flowrt::Status::Ok;
        });
    assert(seen == 3);
    assert(scheduler_status == flowrt::Status::Error);

    std::size_t completed_ticks = 0;
    const auto completed_status = inproc_backend.scheduler().run_ticks(
        4, [&completed_ticks](std::size_t tick, flowrt::Context &) -> flowrt::Status {
            assert(tick == completed_ticks);
            ++completed_ticks;
            return flowrt::Status::Ok;
        });
    assert(completed_ticks == 4);
    assert(completed_status == flowrt::Status::Ok);

    std::size_t shutdown_ticks = 0;
    auto shutdown = flowrt::ShutdownToken::new_for_test();
    const auto shutdown_status = inproc_backend.scheduler().run_ticks_until_shutdown(
        10, shutdown,
        [&shutdown_ticks, &shutdown](std::size_t, flowrt::Context &) -> flowrt::Status {
            ++shutdown_ticks;
            shutdown.request();
            return flowrt::Status::Ok;
        });
    assert(shutdown_ticks == 1);
    assert(shutdown_status == flowrt::Status::Ok);

    flowrt::ScheduleWaiter data_waiter;
    auto data_shutdown = flowrt::ShutdownToken::new_for_test();
    std::thread data_notifier([&data_waiter]() {
        std::this_thread::sleep_for(std::chrono::milliseconds{5});
        data_waiter.notify_data();
    });
    assert(data_waiter.wait_until(std::chrono::steady_clock::now() + std::chrono::seconds{1},
                                  data_shutdown) == flowrt::ScheduleEvent::Data);
    data_notifier.join();

    const flowrt::ScheduleWaiter const_notifier = data_waiter;
    const auto before_const_notify = data_waiter.data_generation();
    const_notifier.notify_data();
    assert(data_waiter.data_generation() == before_const_notify + 1);

    flowrt::ScheduleWaiter barrier_waiter;
    auto barrier_shutdown = flowrt::ShutdownToken::new_for_test();
    barrier_waiter.notify_data();
    const auto seen_generation = barrier_waiter.data_generation();
    assert(barrier_waiter.wait_until_after(
               seen_generation, std::chrono::steady_clock::now() + std::chrono::milliseconds{1},
               barrier_shutdown) == flowrt::ScheduleEvent::Timer);

    flowrt::ScheduleWaiter timer_waiter;
    auto timer_shutdown = flowrt::ShutdownToken::new_for_test();
    assert(timer_waiter.wait_until(std::chrono::steady_clock::now(), timer_shutdown) ==
           flowrt::ScheduleEvent::Timer);

    flowrt::ScheduleWaiter shutdown_waiter;
    auto shutdown_for_waiter = flowrt::ShutdownToken::new_for_test();
    shutdown_for_waiter.request();
    assert(shutdown_waiter.wait_until(std::nullopt, shutdown_for_waiter) ==
           flowrt::ScheduleEvent::Shutdown);

    flowrt::DeterministicExecutor executor{1};
    executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 10});
    executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{2}, .lane = flowrt::LaneId{1}, .priority = 1});
    executor.wake(flowrt::TaskId{1});
    executor.wake(flowrt::TaskId{2});
    auto first_executor_batch = executor.take_ready_batch();
    assert((first_executor_batch.tasks() == std::vector<flowrt::TaskId>{flowrt::TaskId{2}}));
    assert(!executor.complete_task(flowrt::TaskId{99}));
    assert((executor.take_ready_batch().tasks() == std::vector<flowrt::TaskId>{}));
    assert(executor.complete_task(flowrt::TaskId{2}));
    auto second_executor_batch = executor.take_ready_batch();
    assert((second_executor_batch.tasks() == std::vector<flowrt::TaskId>{flowrt::TaskId{1}}));
    assert(executor.complete_task(flowrt::TaskId{1}));
    assert(!executor.complete_task(flowrt::TaskId{1}));
    assert(executor.is_drained());

    flowrt::DeterministicExecutor fair_executor{1};
    fair_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    fair_executor.add_lane(flowrt::LaneId{2}, flowrt::LaneKind::Serial);
    fair_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    fair_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{2}, .lane = flowrt::LaneId{1}, .priority = 1});
    fair_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{3}, .lane = flowrt::LaneId{2}, .priority = 99});
    fair_executor.wake(flowrt::TaskId{1});
    fair_executor.wake(flowrt::TaskId{2});
    fair_executor.wake(flowrt::TaskId{3});
    auto fair_first_batch = fair_executor.take_ready_batch();
    assert((fair_first_batch.tasks() ==
            std::vector<flowrt::TaskId>{flowrt::TaskId{1}, flowrt::TaskId{3}}));
    assert(fair_executor.complete_task(flowrt::TaskId{1}));
    assert(fair_executor.complete_task(flowrt::TaskId{3}));
    auto fair_second_batch = fair_executor.take_ready_batch();
    assert((fair_second_batch.tasks() == std::vector<flowrt::TaskId>{flowrt::TaskId{2}}));
    assert(fair_executor.complete_task(flowrt::TaskId{2}));

    flowrt::DeterministicExecutor timer_executor{1};
    timer_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    timer_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    timer_executor.add_periodic(
        flowrt::PeriodicSpec{.task = flowrt::TaskId{1}, .period = std::chrono::milliseconds{10}});
    timer_executor.set_task_deadline_ms(flowrt::TaskId{1}, 7U);
    timer_executor.advance_to(std::chrono::milliseconds{35});
    auto timer_batch = timer_executor.take_ready_batch();
    assert((timer_batch.tasks() == std::vector<flowrt::TaskId>{flowrt::TaskId{1}}));
    assert(timer_batch.admissions().size() == 1U);
    assert(timer_batch.admissions()[0] == (flowrt::TaskAdmission{
                                              .task = flowrt::TaskId{1},
                                              .lane = flowrt::LaneId{1},
                                              .scheduled_time_ms = 30U,
                                              .observed_time_ms = 35U,
                                              .period_ms = 10U,
                                              .deadline_ms = 7U,
                                              .missed_periods = 2U,
                                              .lateness_ms = 5U,
                                          }));
    assert(timer_executor.next_deadline(flowrt::TaskId{1}) == std::chrono::milliseconds{40});
    assert(timer_executor.missed_periods(flowrt::TaskId{1}) == 2U);
    assert(timer_executor.complete_task(flowrt::TaskId{1}));

    // 故障隔离：suspend 后 task 不再进入 admission，resume 后恢复。
    flowrt::DeterministicExecutor suspend_executor{1};
    suspend_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    suspend_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    suspend_executor.suspend_task(flowrt::TaskId{1});
    suspend_executor.wake(flowrt::TaskId{1});
    assert(suspend_executor.take_ready_batch().empty());
    suspend_executor.resume_task(flowrt::TaskId{1});
    suspend_executor.wake(flowrt::TaskId{1});
    assert((suspend_executor.take_ready_batch().tasks() ==
            std::vector<flowrt::TaskId>{flowrt::TaskId{1}}));
    assert(suspend_executor.complete_task(flowrt::TaskId{1}));

    // suspend 后 periodic due 也不准入；resume 后再 advance 才恢复。
    flowrt::DeterministicExecutor suspend_periodic_executor{1};
    suspend_periodic_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    suspend_periodic_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    suspend_periodic_executor.add_periodic(
        flowrt::PeriodicSpec{.task = flowrt::TaskId{1}, .period = std::chrono::milliseconds{10}});
    suspend_periodic_executor.suspend_task(flowrt::TaskId{1});
    suspend_periodic_executor.advance_to(std::chrono::milliseconds{10});
    assert(suspend_periodic_executor.take_ready_batch().empty());
    suspend_periodic_executor.resume_task(flowrt::TaskId{1});
    suspend_periodic_executor.advance_to(std::chrono::milliseconds{20});
    assert((suspend_periodic_executor.take_ready_batch().tasks() ==
            std::vector<flowrt::TaskId>{flowrt::TaskId{1}}));
    assert(suspend_periodic_executor.complete_task(flowrt::TaskId{1}));

    flowrt::ManualExecutor coroutine_executor;
    bool coroutine_resumed = false;
    auto task = mark_after_schedule(coroutine_executor, coroutine_resumed);
    (void)task;
    assert(!coroutine_resumed);
    assert(coroutine_executor.run_ready() == 1U);
    assert(coroutine_resumed);

    flowrt::WorkerPool worker_pool{2};
    std::atomic<std::size_t> completed_jobs{0};
    for (std::size_t index = 0; index < 8; ++index) {
        assert(worker_pool.spawn([&completed_jobs]() {
            ++completed_jobs;
            return flowrt::Status::Ok;
        }));
    }
    assert(worker_pool.worker_threads() == 2U);
    assert(worker_pool.shutdown() == flowrt::Status::Ok);
    assert(completed_jobs.load() == 8U);
    assert(!worker_pool.spawn([]() { return flowrt::Status::Ok; }));

    flowrt::WorkerPool failing_pool{2};
    assert(failing_pool.spawn([]() { return flowrt::Status::Ok; }));
    assert(failing_pool.spawn([]() { return flowrt::Status::Error; }));
    assert(failing_pool.spawn([]() { return flowrt::Status::Ok; }));
    assert(failing_pool.shutdown() == flowrt::Status::Error);

    flowrt::WorkerPool completion_pool{1};
    flowrt::WorkerCompletionQueue<std::vector<std::uint64_t>> completion_queue;
    auto completion_gate = std::make_shared<CompletionGate>();
    const auto completion_submission =
        completion_pool.submit_collect(flowrt::TaskId{7}, completion_queue, [completion_gate]() {
            completion_gate->enter_and_wait();
            return flowrt::TaskRunOutcome<std::vector<std::uint64_t>>::ok(
                std::vector<std::uint64_t>{7U});
        });
    assert(completion_submission.submitted());
    completion_gate->wait_entered();
    assert(completion_queue.try_drain_completed().empty());
    completion_gate->release();
    assert(completion_pool.drain() == flowrt::Status::Ok);
    auto completion_results = completion_queue.drain_completed();
    assert(completion_results.size() == 1U);
    assert(completion_results[0].task == flowrt::TaskId{7});
    assert(completion_results[0].status == flowrt::Status::Ok);
    assert(completion_results[0].outputs.has_value());
    assert(completion_results[0].outputs->front() == 7U);
    assert(completion_queue.drain_completed().empty());
    assert(completion_pool.shutdown() == flowrt::Status::Ok);

    flowrt::WorkerPool exception_submit_pool{1};
    flowrt::WorkerCompletionQueue<std::vector<std::uint64_t>> exception_submit_queue;
    const auto exception_submission = exception_submit_pool.submit_collect(
        flowrt::TaskId{9}, exception_submit_queue,
        []() -> flowrt::TaskRunOutcome<std::vector<std::uint64_t>> {
            throw std::runtime_error("completion job failed");
        });
    assert(exception_submission.submitted());
    assert(exception_submit_pool.drain() == flowrt::Status::Error);
    auto exception_submit_results = exception_submit_queue.drain_completed();
    assert(exception_submit_results.size() == 1U);
    assert(exception_submit_results[0].task == flowrt::TaskId{9});
    assert(exception_submit_results[0].status == flowrt::Status::Error);
    assert(!exception_submit_results[0].outputs.has_value());
    assert(exception_submit_pool.shutdown() == flowrt::Status::Error);

    flowrt::WorkerPool shutdown_completion_pool{1};
    flowrt::WorkerCompletionQueue<std::vector<std::uint64_t>> shutdown_completion_queue;
    const auto shutdown_submission = shutdown_completion_pool.submit_collect(
        flowrt::TaskId{10}, shutdown_completion_queue, []() {
            return flowrt::TaskRunOutcome<std::vector<std::uint64_t>>::ok(
                std::vector<std::uint64_t>{10U});
        });
    assert(shutdown_submission.submitted());
    assert(shutdown_completion_pool.shutdown() == flowrt::Status::Ok);
    auto shutdown_completion_results = shutdown_completion_queue.drain_completed();
    assert(shutdown_completion_results.size() == 1U);
    assert(shutdown_completion_results[0].task == flowrt::TaskId{10});
    assert(shutdown_completion_results[0].status == flowrt::Status::Ok);
    assert(shutdown_completion_results[0].outputs.has_value());
    assert(shutdown_completion_results[0].outputs->front() == 10U);
    const auto rejected_submission = shutdown_completion_pool.submit_collect(
        flowrt::TaskId{11}, shutdown_completion_queue, []() {
            return flowrt::TaskRunOutcome<std::vector<std::uint64_t>>::ok(
                std::vector<std::uint64_t>{11U});
        });
    assert(!rejected_submission.submitted());
    assert(rejected_submission.task == flowrt::TaskId{11});

    flowrt::DeterministicExecutor parallel_lane_executor{2};
    parallel_lane_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Parallel);
    parallel_lane_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    parallel_lane_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{2}, .lane = flowrt::LaneId{1}, .priority = 1});
    parallel_lane_executor.wake(flowrt::TaskId{1});
    parallel_lane_executor.wake(flowrt::TaskId{2});
    auto parallel_lane_batch = parallel_lane_executor.take_ready_batch();
    assert((parallel_lane_batch.tasks() ==
            std::vector<flowrt::TaskId>{flowrt::TaskId{1}, flowrt::TaskId{2}}));
    assert(parallel_lane_executor.complete_task(flowrt::TaskId{1}));
    assert(parallel_lane_executor.complete_task(flowrt::TaskId{2}));
    assert(parallel_lane_executor.is_drained());

    flowrt::DeterministicExecutor coalesced_periodic_executor{1};
    coalesced_periodic_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    coalesced_periodic_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    coalesced_periodic_executor.add_periodic(
        flowrt::PeriodicSpec{.task = flowrt::TaskId{1}, .period = std::chrono::milliseconds{10}});
    coalesced_periodic_executor.advance_to(std::chrono::milliseconds{10});
    auto coalesced_first = coalesced_periodic_executor.take_ready_batch();
    assert((coalesced_first.tasks() == std::vector<flowrt::TaskId>{flowrt::TaskId{1}}));
    coalesced_periodic_executor.advance_to(std::chrono::milliseconds{35});
    assert(coalesced_periodic_executor.take_ready_batch().empty());
    assert(coalesced_periodic_executor.missed_periods(flowrt::TaskId{1}) == 2U);
    assert(coalesced_periodic_executor.complete_task(flowrt::TaskId{1}));
    auto coalesced_second = coalesced_periodic_executor.take_ready_batch();
    assert((coalesced_second.tasks() == std::vector<flowrt::TaskId>{flowrt::TaskId{1}}));
    assert(coalesced_second.admissions()[0].scheduled_time_ms == 30U);
    assert(coalesced_second.admissions()[0].missed_periods == 2U);
    assert(coalesced_periodic_executor.complete_task(flowrt::TaskId{1}));

    flowrt::DeterministicExecutor collect_executor{2};
    collect_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    collect_executor.add_lane(flowrt::LaneId{2}, flowrt::LaneKind::Serial);
    collect_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    collect_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{2}, .lane = flowrt::LaneId{2}, .priority = 0});
    collect_executor.wake(flowrt::TaskId{1});
    collect_executor.wake(flowrt::TaskId{2});
    auto collect_batch = collect_executor.take_ready_batch();
    flowrt::WorkerPool collect_pool{2};
    flowrt::WorkerCompletionQueue<std::vector<flowrt::OutputCommitRecord<Sample>>> collect_queue;
    for (const auto &admission : collect_batch.admissions()) {
        const auto submission =
            collect_pool.submit_collect(admission.task, collect_queue, [task = admission.task]() {
                flowrt::Output<Sample> task_output;
                task_output.write(Sample{static_cast<std::uint32_t>(task.value)});
                auto record = task_output.take_commit_record(task, "task", "value", 120U, 100U);
                assert(record.has_value());
                return flowrt::TaskRunOutcome<std::vector<flowrt::OutputCommitRecord<Sample>>>::ok(
                    std::vector<flowrt::OutputCommitRecord<Sample>>{std::move(*record)});
            });
        assert(submission.submitted());
    }
    assert(collect_pool.drain() == flowrt::Status::Ok);
    auto collect_results = collect_queue.drain_completed();
    assert(collect_results.size() == 2U);
    for (auto &result : collect_results) {
        assert(result.status == flowrt::Status::Ok);
        assert(result.outputs.has_value());
        assert(result.outputs->front().payload.value == result.task.value);
        assert(collect_executor.complete_task(result.task));
    }
    assert(collect_executor.is_drained());
    assert(collect_pool.shutdown() == flowrt::Status::Ok);

    flowrt::DeterministicExecutor exception_collect_executor{1};
    exception_collect_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    exception_collect_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    exception_collect_executor.wake(flowrt::TaskId{1});
    auto exception_batch = exception_collect_executor.take_ready_batch();
    flowrt::WorkerPool exception_collect_pool{1};
    flowrt::WorkerCompletionQueue<std::vector<flowrt::OutputCommitRecord<Sample>>>
        exception_collect_queue;
    assert(exception_collect_pool
               .submit_collect(
                   exception_batch.admissions()[0].task, exception_collect_queue,
                   []() -> flowrt::TaskRunOutcome<std::vector<flowrt::OutputCommitRecord<Sample>>> {
                       flowrt::Output<Sample> task_output;
                       task_output.write(Sample{1U});
                       throw std::runtime_error("task failed after writing output");
                   })
               .submitted());
    assert(exception_collect_pool.drain() == flowrt::Status::Error);
    auto exception_collect_results = exception_collect_queue.drain_completed();
    assert(exception_collect_results.size() == 1U);
    assert(exception_collect_results[0].task == flowrt::TaskId{1});
    assert(exception_collect_results[0].status == flowrt::Status::Error);
    assert(!exception_collect_results[0].outputs.has_value());
    assert(exception_collect_executor.complete_task(flowrt::TaskId{1}));
    assert(exception_collect_executor.is_drained());
    assert(exception_collect_pool.shutdown() == flowrt::Status::Error);

    flowrt::WorkerPool closed_pool{2};
    std::atomic<std::size_t> close_entered{0};
    std::atomic<std::size_t> close_release{0};
    assert(closed_pool.spawn([&]() {
        close_entered.fetch_add(1U);
        while (close_release.load() == 0U) {
            std::this_thread::yield();
        }
        return flowrt::Status::Ok;
    }));
    while (close_entered.load() == 0U) {
        std::this_thread::yield();
    }
    closed_pool.close_admission();
    assert(!closed_pool.spawn([]() { return flowrt::Status::Ok; }));
    close_release.store(1U);
    assert(closed_pool.drain() == flowrt::Status::Ok);
    assert(closed_pool.shutdown() == flowrt::Status::Ok);

    flowrt::WorkerPool empty_pool{4};
    assert(empty_pool.shutdown() == flowrt::Status::Ok);
    assert(!empty_pool.spawn([]() { return flowrt::Status::Ok; }));

    Sample sample{42U};
    flowrt::Latest<Sample> latest(&sample, true);
    assert(latest.present());
    assert(latest.stale());
    assert(latest.get()->value == 42U);
    assert(latest.as_ref()->value == 42U);

    flowrt::Output<Sample> output;
    assert(!output.present());
    output.write(Sample{7U});
    assert(output.present());
    assert(output.as_ref()->value == 7U);
    assert(output.take()->value == 7U);
    assert(!output.present());
    output.write(Sample{9U});
    auto commit_record =
        output.take_commit_record(flowrt::TaskId{42}, "camera.step", "pose", 120U, 100U);
    assert(commit_record.has_value());
    assert(commit_record->task == flowrt::TaskId{42});
    assert(commit_record->task_name == "camera.step");
    assert(commit_record->port == "pose");
    assert(commit_record->payload.value == 9U);
    assert(commit_record->published_at_ms == 120U);
    assert(commit_record->tick_time_ms == 100U);
    assert(!output.present());

    flowrt::LatestChannel<Sample> latest_channel;
    assert(latest_channel.revision() == 0U);
    latest_channel.publish(Sample{11U});
    assert(latest_channel.revision() == 1U);
    assert(latest_channel.view().present());
    assert(latest_channel.view().get()->value == 11U);
    assert(latest_channel.revision() == 1U);
    latest_channel.publish_at(Sample{12U}, 10U);
    assert(latest_channel.revision() == 2U);
    assert(latest_channel.take()->value == 12U);
    assert(latest_channel.revision() == 2U);

    auto warn_channel = flowrt::LatestChannel<Sample>::with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Warn});
    warn_channel.publish_at(Sample{13U}, 100);
    assert(warn_channel.view_at(109).present());
    assert(!warn_channel.view_at(109).stale());
    assert(warn_channel.view_at(111).present());
    assert(warn_channel.view_at(111).stale());
    assert(warn_channel.view_at(111).get()->value == 13U);

    auto drop_channel = flowrt::LatestChannel<Sample>::with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Drop});
    drop_channel.publish_at(Sample{17U}, 100);
    assert(!drop_channel.view_at(111).present());
    assert(drop_channel.view_at(111).stale());

    auto hold_last_channel = flowrt::LatestChannel<Sample>::with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::HoldLast});
    hold_last_channel.publish_at(Sample{19U}, 100);
    assert(hold_last_channel.view_at(111).present());
    assert(hold_last_channel.view_at(111).stale());
    assert(hold_last_channel.view_at(111).get()->value == 19U);

    auto error_channel = flowrt::LatestChannel<Sample>::with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Error});
    error_channel.publish_at(Sample{23U}, 100);
    assert(error_channel.view_at(111).present());
    assert(error_channel.view_at(111).stale());
    assert(error_channel.view_at(111).get()->value == 23U);

    auto fifo_warn_channel = flowrt::FifoChannel<Sample>::with_stale_config(
        2, flowrt::OverflowPolicy::DropOldest,
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Warn});
    const auto fifo_warn_first = fifo_warn_channel.push_at(Sample{29U}, 100);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(fifo_warn_first));
    assert(std::get<flowrt::ChannelWriteOutcome>(fifo_warn_first) ==
           flowrt::ChannelWriteOutcome::Accepted);
    const auto fifo_warn_second = fifo_warn_channel.push_at(Sample{31U}, 100);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(fifo_warn_second));
    assert(std::get<flowrt::ChannelWriteOutcome>(fifo_warn_second) ==
           flowrt::ChannelWriteOutcome::Accepted);
    const auto fifo_fresh_read = fifo_warn_channel.pop_at(109);
    const auto fifo_fresh = fifo_fresh_read.view();
    assert(fifo_fresh.present());
    assert(!fifo_fresh.stale());
    assert(fifo_fresh.get()->value == 29U);
    const auto fifo_stale_read = fifo_warn_channel.pop_at(111);
    const auto fifo_stale = fifo_stale_read.view();
    assert(fifo_stale.present());
    assert(fifo_stale.stale());
    assert(fifo_stale.get()->value == 31U);

    auto fifo_drop_channel = flowrt::FifoChannel<Sample>::with_stale_config(
        1, flowrt::OverflowPolicy::DropOldest,
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Drop});
    const auto fifo_drop_write = fifo_drop_channel.push_at(Sample{37U}, 100);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(fifo_drop_write));
    const auto fifo_drop_read = fifo_drop_channel.pop_at(111);
    const auto fifo_drop = fifo_drop_read.view();
    assert(!fifo_drop.present());
    assert(fifo_drop.stale());
    assert(fifo_drop_channel.empty());

    auto fifo_error_channel = flowrt::FifoChannel<Sample>::with_stale_config(
        1, flowrt::OverflowPolicy::DropOldest,
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Error});
    const auto fifo_error_write = fifo_error_channel.push_at(Sample{41U}, 100);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(fifo_error_write));
    const auto fifo_error_read = fifo_error_channel.pop_at(111);
    const auto fifo_error = fifo_error_read.view();
    assert(fifo_error.present());
    assert(fifo_error.stale());
    assert(fifo_error.get()->value == 41U);

    flowrt::FifoChannel<Sample> fifo_channel(1, flowrt::OverflowPolicy::DropOldest);
    const auto first = fifo_channel.push(Sample{1U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(first));
    assert(std::get<flowrt::ChannelWriteOutcome>(first) == flowrt::ChannelWriteOutcome::Accepted);
    const auto second = fifo_channel.push(Sample{2U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(second));
    assert(std::get<flowrt::ChannelWriteOutcome>(second) ==
           flowrt::ChannelWriteOutcome::DroppedOldest);
    assert(fifo_channel.pop()->value == 2U);

    flowrt::FifoChannel<Sample> block_channel(1, flowrt::OverflowPolicy::Block);
    assert(block_channel.revision() == 0U);
    const auto block_first = block_channel.push(Sample{3U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(block_first));
    assert(std::get<flowrt::ChannelWriteOutcome>(block_first) ==
           flowrt::ChannelWriteOutcome::Accepted);
    assert(block_channel.revision() == 1U);
    const auto block_second = block_channel.push(Sample{4U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(block_second));
    assert(std::get<flowrt::ChannelWriteOutcome>(block_second) ==
           flowrt::ChannelWriteOutcome::Backpressured);
    assert(block_channel.revision() == 1U);
    assert(block_channel.pop()->value == 3U);
    assert(block_channel.revision() == 1U);

    flowrt::BoundaryInput<Sample> boundary_input;
    flowrt::ScheduleWaiter boundary_waiter;
    boundary_input.set_schedule_waiter(boundary_waiter);
    const auto boundary_generation = boundary_waiter.data_generation();
    const auto boundary_revision = boundary_input.inject_at(Sample{51U}, 100U);
    const auto boundary_read = boundary_input.read_at(109U);
    const auto boundary_view = boundary_read.view();
    assert(boundary_revision == 1U);
    assert(boundary_read.revision() == 1U);
    assert(boundary_view.present());
    assert(!boundary_view.stale());
    assert(boundary_view.get()->value == 51U);
    assert(boundary_waiter.data_generation() > boundary_generation);

    auto stale_boundary_input = flowrt::BoundaryInput<Sample>::with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Drop});
    stale_boundary_input.inject_at(Sample{53U}, 100U);
    const auto stale_boundary_read = stale_boundary_input.read_at(111U);
    const auto stale_boundary_view = stale_boundary_read.view();
    assert(stale_boundary_read.revision() == 1U);
    assert(!stale_boundary_view.present());
    assert(stale_boundary_view.stale());

    flowrt::BoundaryOutput<Sample> boundary_output;
    std::vector<std::pair<std::uint32_t, std::optional<std::uint64_t>>> boundary_seen;
    {
        auto guard = boundary_output.register_sink(
            [&boundary_seen](const Sample &value, std::optional<std::uint64_t> published_at_ms) {
                boundary_seen.emplace_back(value.value, published_at_ms);
            });
        boundary_output.publish_at(Sample{59U}, 100U);
        assert(boundary_output.sink_count() == 1U);
    }
    boundary_output.publish_at(Sample{61U}, 110U);
    assert(boundary_output.sink_count() == 0U);
    assert(boundary_seen.size() == 1U);
    assert(boundary_seen[0].first == 59U);
    assert(boundary_seen[0].second == std::optional<std::uint64_t>{100U});

    auto iox2_config = flowrt::iox2::Iox2ChannelConfig::fifo(0, flowrt::OverflowPolicy::DropOldest)
                           .with_stale_config(flowrt::StaleConfig{std::chrono::milliseconds{5},
                                                                  flowrt::StalePolicy::Error});
    auto iox2_hold_last_config = flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::HoldLast});
    auto iox2_block_config =
        flowrt::iox2::Iox2ChannelConfig::fifo(0, flowrt::OverflowPolicy::Block);
    static_assert(std::string_view{flowrt::iox2::FlowrtIox2Header::IOX2_TYPE_NAME} ==
                  "FlowRTIox2Header");
    static_assert(sizeof(flowrt::iox2::FlowrtIox2Header) == sizeof(std::uint64_t));
    flowrt::iox2::FlowrtIox2Header iox2_header{10U};
    assert(iox2_header.published_at_ms == 10U);
    assert(iox2_config.depth() == 1U);
    assert(iox2_config.overflow() == flowrt::OverflowPolicy::DropOldest);
    assert(iox2_config.stale().policy() == flowrt::StalePolicy::Error);
    assert(iox2_hold_last_config.stale().policy() == flowrt::StalePolicy::HoldLast);
    assert(iox2_hold_last_config.stale().max_age() ==
           std::optional<flowrt::StaleConfig::Duration>{std::chrono::milliseconds{10}});
    assert(iox2_block_config.depth() == 1U);
    assert(iox2_block_config.overflow() == flowrt::OverflowPolicy::Block);

    auto iox2_endpoint =
        flowrt::iox2::Iox2PubSub<Sample>::open_with_config("FlowRT/Cpp/Smoke", iox2_config);
    assert(iox2_endpoint.service_name() == "FlowRT/Cpp/Smoke");
    assert(iox2_endpoint.config().depth() == 1U);
    assert(iox2_endpoint.config().overflow() == flowrt::OverflowPolicy::DropOldest);
    assert(!iox2_endpoint.ready());
    assert(iox2_endpoint.health().state == flowrt::BackendHealthState::Unsupported);
    assert(!iox2_endpoint.health().recoverable);
    const auto transport_write = iox2_endpoint.publish_at(Sample{23U}, 10U);
    assert(std::holds_alternative<flowrt::ChannelError>(transport_write));
    assert(std::get<flowrt::ChannelError>(transport_write) == flowrt::ChannelError::Unsupported);
    assert(iox2_endpoint.health().state == flowrt::BackendHealthState::Unsupported);
    assert(!iox2_endpoint.health().recoverable);
    const auto transport_read = iox2_endpoint.receive_latest_at(10U);
    assert(std::holds_alternative<flowrt::ChannelError>(transport_read));
    assert(std::get<flowrt::ChannelError>(transport_read) == flowrt::ChannelError::Unsupported);

    auto zenoh_config =
        flowrt::zenoh::ZenohChannelConfig::fifo(0, flowrt::OverflowPolicy::DropNewest)
            .with_stale_config(
                flowrt::StaleConfig{std::chrono::milliseconds{5}, flowrt::StalePolicy::Drop});
    auto zenoh_latest_config = flowrt::zenoh::ZenohChannelConfig::latest();
    assert(zenoh_config.depth() == 1U);
    assert(zenoh_config.overflow() == flowrt::OverflowPolicy::DropNewest);
    assert(!zenoh_config.is_latest());
    assert(zenoh_latest_config.is_latest());
    assert(zenoh_config.stale().policy() == flowrt::StalePolicy::Drop);
    assert(zenoh_config.stale().max_age() ==
           std::optional<flowrt::StaleConfig::Duration>{std::chrono::milliseconds{5}});
    assert(zenoh_latest_config.depth() == 1U);
    assert(zenoh_latest_config.overflow() == flowrt::OverflowPolicy::DropOldest);

    auto zenoh_endpoint = flowrt::zenoh::ZenohPubSub<TinyWireMessage>::open_with_config(
        "flowrt/cpp/smoke", zenoh_config);
    assert(zenoh_endpoint.key_expr() == "flowrt/cpp/smoke");
    assert(zenoh_endpoint.config().depth() == 1U);
    assert(zenoh_endpoint.config().overflow() == flowrt::OverflowPolicy::DropNewest);
    assert(!zenoh_endpoint.ready());
    assert(zenoh_endpoint.health().state == flowrt::BackendHealthState::Unsupported);
    assert(!zenoh_endpoint.health().recoverable);
    const auto zenoh_transport_write = zenoh_endpoint.publish_at(TinyWireMessage{23U}, 10U);
    assert(std::holds_alternative<flowrt::ChannelError>(zenoh_transport_write));
    assert(std::get<flowrt::ChannelError>(zenoh_transport_write) ==
           flowrt::ChannelError::Unsupported);
    assert(zenoh_endpoint.health().state == flowrt::BackendHealthState::Unsupported);
    assert(!zenoh_endpoint.health().recoverable);
    const auto zenoh_transport_read = zenoh_endpoint.receive_latest_at(10U);
    assert(std::holds_alternative<flowrt::ChannelError>(zenoh_transport_read));
    assert(std::get<flowrt::ChannelError>(zenoh_transport_read) ==
           flowrt::ChannelError::Unsupported);

    return 0;
}
