use std::mem::{align_of, offset_of, size_of};

use flowrt::{
    BackendHealthSnapshot, BackendHealthState, ClockSource, FrameDescriptor, FrameLeaseStatus,
    FrameMetadata, OperationId, OperationState, ReconnectPolicy, ResourceDescriptor, Status,
    abi::{
        FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0, FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1,
        FLOWRT_ABI_VERSION_MAJOR, FLOWRT_ABI_VERSION_MINOR, FLOWRT_BACKEND_HEALTH_DEGRADED,
        FLOWRT_BACKEND_HEALTH_FAILED, FLOWRT_BACKEND_HEALTH_READY,
        FLOWRT_BACKEND_HEALTH_RECONNECTING, FLOWRT_BACKEND_HEALTH_UNSUPPORTED,
        FLOWRT_BACKEND_INPROC, FLOWRT_BACKEND_IOX2, FLOWRT_BACKEND_ZENOH,
        FLOWRT_C_CLOCK_SOURCE_REPLAY, FLOWRT_C_CLOCK_SOURCE_RUNTIME,
        FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR,
        FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR, FLOWRT_C_OUTPUT_ERROR,
        FLOWRT_C_OUTPUT_TRUNCATED, FLOWRT_C_OUTPUT_UNWRITTEN, FLOWRT_C_OUTPUT_WRITTEN,
        FLOWRT_DIAGNOSTIC_ERROR, FLOWRT_DIAGNOSTIC_INFO, FLOWRT_DIAGNOSTIC_WARN,
        FLOWRT_FRAME_ENCODING_CANONICAL_FRAME_V1, FLOWRT_FRAME_ENCODING_FIXED_PLAIN,
        FLOWRT_FRAME_LEASE_ACQUIRED, FLOWRT_FRAME_LEASE_ATTACHED, FLOWRT_FRAME_LEASE_ERROR,
        FLOWRT_FRAME_LEASE_EXPIRED, FLOWRT_FRAME_LEASE_GENERATION_MISMATCH,
        FLOWRT_FRAME_LEASE_RELEASED, FLOWRT_OPERATION_STATE_CANCEL_REQUESTED,
        FLOWRT_OPERATION_STATE_CANCELLED, FLOWRT_OPERATION_STATE_FAILED,
        FLOWRT_OPERATION_STATE_IDLE, FLOWRT_OPERATION_STATE_RUNNING,
        FLOWRT_OPERATION_STATE_STARTING, FLOWRT_OPERATION_STATE_SUCCEEDED,
        FLOWRT_OPERATION_STATE_TIMED_OUT, FLOWRT_PARAMS_UPDATE_ACCEPTED,
        FLOWRT_PARAMS_UPDATE_APPLIED, FLOWRT_PARAMS_UPDATE_ERROR, FLOWRT_PARAMS_UPDATE_PARTIAL,
        FLOWRT_PARAMS_UPDATE_REJECTED, FLOWRT_PARAMS_UPDATE_UNSUPPORTED,
        FLOWRT_RESOURCE_HEALTH_DEGRADED, FLOWRT_RESOURCE_HEALTH_FAILED,
        FLOWRT_RESOURCE_HEALTH_READY, FLOWRT_RESOURCE_HEALTH_UNAVAILABLE,
        FLOWRT_RESOURCE_HEALTH_UNKNOWN, FLOWRT_STATUS_ERROR, FLOWRT_STATUS_OK, FLOWRT_STATUS_RETRY,
        FlowrtBackendHealthSnapshot, FlowrtBytesView, FlowrtCComponentCallbackTable,
        FlowrtCComponentContext, FlowrtCInputArrayView, FlowrtCInputView, FlowrtCLifecycleCallback,
        FlowrtCOutputArrayView, FlowrtCOutputSlot, FlowrtCParamSnapshotV0, FlowrtCTaskCallback,
        FlowrtCTaskTiming, FlowrtDiagnosticArrayView, FlowrtDiagnosticView,
        FlowrtDiagnosticsSnapshot, FlowrtFrameDescriptor, FlowrtFrameView, FlowrtI128,
        FlowrtOperationId, FlowrtOperationIdArrayView, FlowrtOperationProgressView,
        FlowrtOperationResultSummaryView, FlowrtOperationStatusView, FlowrtParamView,
        FlowrtParamsUpdateResult, FlowrtParamsView, FlowrtReconnectPolicy,
        FlowrtResourceDescriptor, FlowrtResourceHealthArrayView, FlowrtResourceHealthSnapshot,
        FlowrtStringView, FlowrtU128, backend_health_snapshot_to_abi, backend_health_state_to_abi,
        clock_source_to_c_abi, frame_descriptor_to_abi, frame_lease_status_to_abi,
        operation_id_to_abi, operation_state_to_abi, reconnect_policy_to_abi, status_to_abi,
    },
};

#[test]
fn abi_version_and_status_codes_are_stable() {
    assert_eq!(FLOWRT_ABI_VERSION_MAJOR, 0);
    assert_eq!(FLOWRT_ABI_VERSION_MINOR, 2);

    assert_eq!(FLOWRT_STATUS_OK, 0);
    assert_eq!(FLOWRT_STATUS_RETRY, 1);
    assert_eq!(FLOWRT_STATUS_ERROR, 2);

    assert_eq!(status_to_abi(Status::Ok), FLOWRT_STATUS_OK);
    assert_eq!(status_to_abi(Status::Retry), FLOWRT_STATUS_RETRY);
    assert_eq!(status_to_abi(Status::Error), FLOWRT_STATUS_ERROR);
}

#[test]
fn c_abi_future_boundary_constants_are_stable() {
    assert_eq!(FLOWRT_FRAME_ENCODING_FIXED_PLAIN, 0);
    assert_eq!(FLOWRT_FRAME_ENCODING_CANONICAL_FRAME_V1, 1);

    assert_eq!(FLOWRT_PARAMS_UPDATE_ACCEPTED, 0);
    assert_eq!(FLOWRT_PARAMS_UPDATE_APPLIED, 1);
    assert_eq!(FLOWRT_PARAMS_UPDATE_REJECTED, 2);
    assert_eq!(FLOWRT_PARAMS_UPDATE_PARTIAL, 3);
    assert_eq!(FLOWRT_PARAMS_UPDATE_UNSUPPORTED, 4);
    assert_eq!(FLOWRT_PARAMS_UPDATE_ERROR, 5);

    assert_eq!(FLOWRT_OPERATION_STATE_IDLE, 0);
    assert_eq!(FLOWRT_OPERATION_STATE_STARTING, 1);
    assert_eq!(FLOWRT_OPERATION_STATE_RUNNING, 2);
    assert_eq!(FLOWRT_OPERATION_STATE_CANCEL_REQUESTED, 3);
    assert_eq!(FLOWRT_OPERATION_STATE_SUCCEEDED, 4);
    assert_eq!(FLOWRT_OPERATION_STATE_FAILED, 5);
    assert_eq!(FLOWRT_OPERATION_STATE_CANCELLED, 6);
    assert_eq!(FLOWRT_OPERATION_STATE_TIMED_OUT, 7);
    assert_eq!(
        operation_state_to_abi(OperationState::CancelRequested),
        FLOWRT_OPERATION_STATE_CANCEL_REQUESTED
    );
    assert_eq!(
        operation_state_to_abi(OperationState::TimedOut),
        FLOWRT_OPERATION_STATE_TIMED_OUT
    );
    let id = operation_id_to_abi(OperationId::new(10, 20, 30));
    assert_eq!(id.operation_key, 10);
    assert_eq!(id.client_id, 20);
    assert_eq!(id.sequence, 30);

    assert_eq!(FLOWRT_DIAGNOSTIC_INFO, 0);
    assert_eq!(FLOWRT_DIAGNOSTIC_WARN, 1);
    assert_eq!(FLOWRT_DIAGNOSTIC_ERROR, 2);

    assert_eq!(FLOWRT_RESOURCE_HEALTH_UNKNOWN, 0);
    assert_eq!(FLOWRT_RESOURCE_HEALTH_READY, 1);
    assert_eq!(FLOWRT_RESOURCE_HEALTH_DEGRADED, 2);
    assert_eq!(FLOWRT_RESOURCE_HEALTH_FAILED, 3);
    assert_eq!(FLOWRT_RESOURCE_HEALTH_UNAVAILABLE, 4);
}

#[test]
fn c_abi_frame_view_layout_is_stable() {
    assert_eq!(size_of::<FlowrtFrameView>(), 128);
    assert_eq!(align_of::<FlowrtFrameView>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtFrameView, channel_name), 0);
    assert_eq!(
        offset_of!(FlowrtFrameView, message_type),
        size_of::<FlowrtStringView>()
    );
    assert_eq!(offset_of!(FlowrtFrameView, schema_hash), 32);
    assert_eq!(offset_of!(FlowrtFrameView, encoding), 40);
    assert_eq!(offset_of!(FlowrtFrameView, flags), 44);
    assert_eq!(offset_of!(FlowrtFrameView, frame), 48);
    assert_eq!(offset_of!(FlowrtFrameView, header), 64);
    assert_eq!(offset_of!(FlowrtFrameView, tail), 80);
    assert_eq!(offset_of!(FlowrtFrameView, source_time_ms), 96);
    assert_eq!(offset_of!(FlowrtFrameView, published_at_ms), 104);
    assert_eq!(offset_of!(FlowrtFrameView, revision), 112);
    assert_eq!(offset_of!(FlowrtFrameView, has_source_time_ms), 120);
    assert_eq!(offset_of!(FlowrtFrameView, has_published_at_ms), 121);
    assert_eq!(offset_of!(FlowrtFrameView, has_revision), 122);
}

#[test]
fn c_abi_params_views_use_borrowed_json_values() {
    assert_eq!(size_of::<FlowrtParamView>(), 168);
    assert_eq!(align_of::<FlowrtParamView>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtParamView, instance_name), 0);
    assert_eq!(offset_of!(FlowrtParamView, param_name), 16);
    assert_eq!(offset_of!(FlowrtParamView, type_name), 32);
    assert_eq!(offset_of!(FlowrtParamView, update_policy), 48);
    assert_eq!(offset_of!(FlowrtParamView, current_json), 64);
    assert_eq!(offset_of!(FlowrtParamView, pending_json), 80);
    assert_eq!(offset_of!(FlowrtParamView, min_json), 96);
    assert_eq!(offset_of!(FlowrtParamView, max_json), 112);
    assert_eq!(offset_of!(FlowrtParamView, choices_json), 128);
    assert_eq!(offset_of!(FlowrtParamView, schema_hash), 144);
    assert_eq!(offset_of!(FlowrtParamView, revision), 152);
    assert_eq!(offset_of!(FlowrtParamView, mutable_at_runtime), 160);
    assert_eq!(offset_of!(FlowrtParamView, has_pending), 161);
    assert_eq!(offset_of!(FlowrtParamView, has_min), 162);
    assert_eq!(offset_of!(FlowrtParamView, has_max), 163);

    assert_eq!(size_of::<FlowrtParamsView>(), 40);
    assert_eq!(align_of::<FlowrtParamsView>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtParamsView, data), 0);
    assert_eq!(offset_of!(FlowrtParamsView, len), size_of::<usize>());
    assert_eq!(offset_of!(FlowrtParamsView, revision), 16);
    assert_eq!(offset_of!(FlowrtParamsView, applied_unix_ms), 24);
    assert_eq!(offset_of!(FlowrtParamsView, has_applied_unix_ms), 32);

    assert_eq!(size_of::<FlowrtParamsUpdateResult>(), 56);
    assert_eq!(align_of::<FlowrtParamsUpdateResult>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtParamsUpdateResult, status), 0);
    assert_eq!(offset_of!(FlowrtParamsUpdateResult, applied_count), 4);
    assert_eq!(offset_of!(FlowrtParamsUpdateResult, rejected_count), 8);
    assert_eq!(offset_of!(FlowrtParamsUpdateResult, revision), 16);
    assert_eq!(offset_of!(FlowrtParamsUpdateResult, error_index), 24);
    assert_eq!(offset_of!(FlowrtParamsUpdateResult, has_error_index), 32);
    assert_eq!(offset_of!(FlowrtParamsUpdateResult, message), 40);
}

#[test]
fn c_component_param_snapshot_v0_abi_layout_is_stable() {
    assert_eq!(size_of::<FlowrtCParamSnapshotV0>(), 32);
    assert_eq!(align_of::<FlowrtCParamSnapshotV0>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtCParamSnapshotV0, abi_version), 0);
    assert_eq!(offset_of!(FlowrtCParamSnapshotV0, param_count), 4);
    assert_eq!(offset_of!(FlowrtCParamSnapshotV0, params), 8);
    assert_eq!(offset_of!(FlowrtCParamSnapshotV0, reserved), 16);
}

#[test]
fn c_abi_operation_status_progress_and_result_layouts_are_stable() {
    assert_eq!(size_of::<FlowrtOperationId>(), 24);
    assert_eq!(align_of::<FlowrtOperationId>(), 8);
    assert_eq!(offset_of!(FlowrtOperationId, operation_key), 0);
    assert_eq!(offset_of!(FlowrtOperationId, client_id), 8);
    assert_eq!(offset_of!(FlowrtOperationId, sequence), 16);

    assert_eq!(
        size_of::<FlowrtOperationIdArrayView>(),
        size_of::<usize>() * 2
    );
    assert_eq!(
        align_of::<FlowrtOperationIdArrayView>(),
        align_of::<usize>()
    );
    assert_eq!(offset_of!(FlowrtOperationIdArrayView, data), 0);
    assert_eq!(
        offset_of!(FlowrtOperationIdArrayView, len),
        size_of::<usize>()
    );

    assert_eq!(size_of::<FlowrtOperationStatusView>(), 112);
    assert_eq!(align_of::<FlowrtOperationStatusView>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtOperationStatusView, operation_name), 0);
    assert_eq!(
        offset_of!(FlowrtOperationStatusView, current_operation_ids),
        16
    );
    assert_eq!(offset_of!(FlowrtOperationStatusView, running), 32);
    assert_eq!(offset_of!(FlowrtOperationStatusView, queued), 40);
    assert_eq!(offset_of!(FlowrtOperationStatusView, total_started), 48);
    assert_eq!(offset_of!(FlowrtOperationStatusView, succeeded_count), 56);
    assert_eq!(offset_of!(FlowrtOperationStatusView, failed_count), 64);
    assert_eq!(offset_of!(FlowrtOperationStatusView, canceled_count), 72);
    assert_eq!(offset_of!(FlowrtOperationStatusView, timeout_count), 80);
    assert_eq!(offset_of!(FlowrtOperationStatusView, preempted_count), 88);
    assert_eq!(
        offset_of!(FlowrtOperationStatusView, last_transition_ms),
        96
    );
    assert_eq!(offset_of!(FlowrtOperationStatusView, ready), 104);
    assert_eq!(
        offset_of!(FlowrtOperationStatusView, has_last_transition_ms),
        105
    );

    assert_eq!(size_of::<FlowrtOperationProgressView>(), 192);
    assert_eq!(
        align_of::<FlowrtOperationProgressView>(),
        align_of::<usize>()
    );
    assert_eq!(offset_of!(FlowrtOperationProgressView, operation_name), 0);
    assert_eq!(offset_of!(FlowrtOperationProgressView, id), 16);
    assert_eq!(offset_of!(FlowrtOperationProgressView, sequence), 40);
    assert_eq!(offset_of!(FlowrtOperationProgressView, progress), 48);
    assert_eq!(
        offset_of!(FlowrtOperationProgressView, published_at_ms),
        176
    );
    assert_eq!(
        offset_of!(FlowrtOperationProgressView, has_published_at_ms),
        184
    );

    assert_eq!(size_of::<FlowrtOperationResultSummaryView>(), 200);
    assert_eq!(
        align_of::<FlowrtOperationResultSummaryView>(),
        align_of::<usize>()
    );
    assert_eq!(
        offset_of!(FlowrtOperationResultSummaryView, operation_name),
        0
    );
    assert_eq!(offset_of!(FlowrtOperationResultSummaryView, id), 16);
    assert_eq!(offset_of!(FlowrtOperationResultSummaryView, state), 40);
    assert_eq!(offset_of!(FlowrtOperationResultSummaryView, has_result), 44);
    assert_eq!(
        offset_of!(FlowrtOperationResultSummaryView, has_error_message),
        45
    );
    assert_eq!(
        offset_of!(FlowrtOperationResultSummaryView, has_completed_unix_ms),
        46
    );
    assert_eq!(
        offset_of!(FlowrtOperationResultSummaryView, completed_unix_ms),
        48
    );
    assert_eq!(offset_of!(FlowrtOperationResultSummaryView, result), 56);
    assert_eq!(
        offset_of!(FlowrtOperationResultSummaryView, error_message),
        184
    );
}

#[test]
fn c_abi_diagnostics_and_resource_health_layouts_are_stable() {
    assert_eq!(size_of::<FlowrtDiagnosticView>(), 72);
    assert_eq!(align_of::<FlowrtDiagnosticView>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtDiagnosticView, source), 0);
    assert_eq!(offset_of!(FlowrtDiagnosticView, code), 16);
    assert_eq!(offset_of!(FlowrtDiagnosticView, message), 32);
    assert_eq!(offset_of!(FlowrtDiagnosticView, severity), 48);
    assert_eq!(offset_of!(FlowrtDiagnosticView, timestamp_unix_ms), 56);
    assert_eq!(offset_of!(FlowrtDiagnosticView, has_timestamp_unix_ms), 64);

    assert_eq!(size_of::<FlowrtResourceHealthSnapshot>(), 88);
    assert_eq!(
        align_of::<FlowrtResourceHealthSnapshot>(),
        align_of::<usize>()
    );
    assert_eq!(offset_of!(FlowrtResourceHealthSnapshot, name), 0);
    assert_eq!(offset_of!(FlowrtResourceHealthSnapshot, capability), 16);
    assert_eq!(offset_of!(FlowrtResourceHealthSnapshot, state), 32);
    assert_eq!(offset_of!(FlowrtResourceHealthSnapshot, ready), 36);
    assert_eq!(offset_of!(FlowrtResourceHealthSnapshot, required), 37);
    assert_eq!(
        offset_of!(FlowrtResourceHealthSnapshot, has_updated_unix_ms),
        38
    );
    assert_eq!(offset_of!(FlowrtResourceHealthSnapshot, has_generation), 39);
    assert_eq!(
        offset_of!(FlowrtResourceHealthSnapshot, updated_unix_ms),
        40
    );
    assert_eq!(offset_of!(FlowrtResourceHealthSnapshot, generation), 48);
    assert_eq!(offset_of!(FlowrtResourceHealthSnapshot, message), 56);
    assert_eq!(offset_of!(FlowrtResourceHealthSnapshot, last_error), 72);

    assert_eq!(
        size_of::<FlowrtDiagnosticArrayView>(),
        size_of::<usize>() * 2
    );
    assert_eq!(align_of::<FlowrtDiagnosticArrayView>(), align_of::<usize>());
    assert_eq!(
        size_of::<FlowrtResourceHealthArrayView>(),
        size_of::<usize>() * 2
    );
    assert_eq!(
        align_of::<FlowrtResourceHealthArrayView>(),
        align_of::<usize>()
    );

    assert_eq!(size_of::<FlowrtDiagnosticsSnapshot>(), 80);
    assert_eq!(align_of::<FlowrtDiagnosticsSnapshot>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtDiagnosticsSnapshot, package_name), 0);
    assert_eq!(offset_of!(FlowrtDiagnosticsSnapshot, process_name), 16);
    assert_eq!(offset_of!(FlowrtDiagnosticsSnapshot, diagnostics), 32);
    assert_eq!(offset_of!(FlowrtDiagnosticsSnapshot, resources), 48);
    assert_eq!(offset_of!(FlowrtDiagnosticsSnapshot, generated_unix_ms), 64);
    assert_eq!(offset_of!(FlowrtDiagnosticsSnapshot, healthy), 72);
    assert_eq!(
        offset_of!(FlowrtDiagnosticsSnapshot, has_generated_unix_ms),
        73
    );
}

#[test]
fn abi_backend_codes_are_stable() {
    assert_eq!(FLOWRT_BACKEND_INPROC, 0);
    assert_eq!(FLOWRT_BACKEND_IOX2, 1);
    assert_eq!(FLOWRT_BACKEND_ZENOH, 2);

    assert_eq!(FLOWRT_BACKEND_HEALTH_READY, 0);
    assert_eq!(FLOWRT_BACKEND_HEALTH_DEGRADED, 1);
    assert_eq!(FLOWRT_BACKEND_HEALTH_RECONNECTING, 2);
    assert_eq!(FLOWRT_BACKEND_HEALTH_FAILED, 3);
    assert_eq!(FLOWRT_BACKEND_HEALTH_UNSUPPORTED, 4);

    assert_eq!(
        backend_health_state_to_abi(BackendHealthState::Unsupported),
        FLOWRT_BACKEND_HEALTH_UNSUPPORTED
    );
}

#[test]
fn abi_views_have_c_pointer_and_size_layout() {
    assert_eq!(size_of::<FlowrtStringView>(), size_of::<usize>() * 2);
    assert_eq!(align_of::<FlowrtStringView>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtStringView, data), 0);
    assert_eq!(offset_of!(FlowrtStringView, len), size_of::<usize>());

    assert_eq!(size_of::<FlowrtBytesView>(), size_of::<usize>() * 2);
    assert_eq!(align_of::<FlowrtBytesView>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtBytesView, data), 0);
    assert_eq!(offset_of!(FlowrtBytesView, len), size_of::<usize>());
}

#[test]
fn abi_128_bit_pods_match_c_layout() {
    assert_eq!(size_of::<FlowrtU128>(), 16);
    assert_eq!(align_of::<FlowrtU128>(), 8);
    assert_eq!(offset_of!(FlowrtU128, lo), 0);
    assert_eq!(offset_of!(FlowrtU128, hi), 8);

    assert_eq!(size_of::<FlowrtI128>(), 16);
    assert_eq!(align_of::<FlowrtI128>(), 8);
    assert_eq!(offset_of!(FlowrtI128, lo), 0);
    assert_eq!(offset_of!(FlowrtI128, hi), 8);
}

#[test]
fn abi_reconnect_policy_uses_explicit_option_flag() {
    assert_eq!(offset_of!(FlowrtReconnectPolicy, initial_delay_ms), 0);
    assert_eq!(offset_of!(FlowrtReconnectPolicy, max_delay_ms), 8);
    assert_eq!(offset_of!(FlowrtReconnectPolicy, max_attempts), 16);
    assert_eq!(offset_of!(FlowrtReconnectPolicy, has_max_attempts), 20);

    let bounded = reconnect_policy_to_abi(ReconnectPolicy::new(100, 1_000, 3));
    assert_eq!(bounded.initial_delay_ms, 100);
    assert_eq!(bounded.max_delay_ms, 1_000);
    assert_eq!(bounded.max_attempts, 3);
    assert_eq!(bounded.has_max_attempts, 1);
    assert_eq!(bounded.reserved, [0; 3]);

    let forever = reconnect_policy_to_abi(ReconnectPolicy::forever(10, 500));
    assert_eq!(forever.initial_delay_ms, 10);
    assert_eq!(forever.max_delay_ms, 500);
    assert_eq!(forever.max_attempts, 0);
    assert_eq!(forever.has_max_attempts, 0);
    assert_eq!(forever.reserved, [0; 3]);
}

#[test]
fn abi_backend_health_snapshot_uses_borrowed_nullable_fields() {
    assert_eq!(offset_of!(FlowrtBackendHealthSnapshot, state), 0);
    assert_eq!(offset_of!(FlowrtBackendHealthSnapshot, attempt), 4);
    assert_eq!(
        offset_of!(FlowrtBackendHealthSnapshot, next_retry_unix_ms),
        8
    );
    assert_eq!(offset_of!(FlowrtBackendHealthSnapshot, last_error), 16);
    assert_eq!(
        offset_of!(FlowrtBackendHealthSnapshot, has_next_retry_unix_ms),
        16 + size_of::<FlowrtStringView>()
    );

    let snapshot = BackendHealthSnapshot {
        state: BackendHealthState::Reconnecting,
        last_error: Some("transport reset".to_string()),
        attempt: 2,
        next_retry_unix_ms: Some(123_456),
        recoverable: true,
    };
    let abi = backend_health_snapshot_to_abi(&snapshot);

    assert_eq!(abi.state, FLOWRT_BACKEND_HEALTH_RECONNECTING);
    assert_eq!(abi.attempt, 2);
    assert_eq!(abi.next_retry_unix_ms, 123_456);
    assert_eq!(abi.has_next_retry_unix_ms, 1);
    assert_eq!(abi.recoverable, 1);
    assert_eq!(abi.reserved, [0; 6]);
    assert_eq!(abi.last_error.len, "transport reset".len());
    assert!(!abi.last_error.data.is_null());

    let ready = backend_health_snapshot_to_abi(&BackendHealthSnapshot::ready());
    assert_eq!(ready.state, FLOWRT_BACKEND_HEALTH_READY);
    assert_eq!(ready.last_error.len, 0);
    assert!(ready.last_error.data.is_null());
    assert_eq!(ready.has_next_retry_unix_ms, 0);
    assert_eq!(ready.recoverable, 0);
}

#[test]
fn abi_frame_descriptor_and_lease_status_are_plain_borrowed_views() {
    assert_eq!(FLOWRT_FRAME_LEASE_ATTACHED, 0);
    assert_eq!(FLOWRT_FRAME_LEASE_ACQUIRED, 1);
    assert_eq!(FLOWRT_FRAME_LEASE_RELEASED, 2);
    assert_eq!(FLOWRT_FRAME_LEASE_EXPIRED, 3);
    assert_eq!(FLOWRT_FRAME_LEASE_GENERATION_MISMATCH, 4);
    assert_eq!(FLOWRT_FRAME_LEASE_ERROR, 5);
    assert_eq!(
        frame_lease_status_to_abi(FrameLeaseStatus::GenerationMismatch),
        FLOWRT_FRAME_LEASE_GENERATION_MISMATCH
    );

    assert_eq!(offset_of!(FlowrtResourceDescriptor, resource_id), 0);
    assert_eq!(
        offset_of!(FlowrtResourceDescriptor, slot),
        size_of::<FlowrtStringView>()
    );
    assert_eq!(
        offset_of!(FlowrtResourceDescriptor, generation),
        size_of::<FlowrtStringView>() * 2
    );
    assert_eq!(offset_of!(FlowrtFrameDescriptor, resource), 0);
    assert_eq!(
        offset_of!(FlowrtFrameDescriptor, size_bytes),
        size_of::<FlowrtResourceDescriptor>()
    );

    let descriptor = FrameDescriptor::new(
        ResourceDescriptor::new("camera_frames", "slot-7", 42),
        921_600,
        "rgb8",
        "row_major",
        FrameMetadata::new(),
    )
    .unwrap();
    let abi = frame_descriptor_to_abi(&descriptor, r#"{"width":"640"}"#);

    assert_eq!(abi.resource.generation, 42);
    assert_eq!(abi.size_bytes, 921_600);
    assert_eq!(abi.format.len, 4);
    assert_eq!(abi.encoding.len, "row_major".len());
    assert_eq!(abi.metadata_json.len, r#"{"width":"640"}"#.len());
}

#[test]
fn c_component_callback_abi_constants_are_stable() {
    assert_eq!(FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR, 0);
    assert_eq!(FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR, 3);
    assert_eq!(FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0, 1);
    assert_eq!(FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1, 1 << 1);
    assert_eq!(FLOWRT_C_CLOCK_SOURCE_RUNTIME, 0);
    assert_eq!(FLOWRT_C_CLOCK_SOURCE_REPLAY, 1);
    assert_eq!(
        clock_source_to_c_abi(ClockSource::Runtime),
        FLOWRT_C_CLOCK_SOURCE_RUNTIME
    );
    assert_eq!(
        clock_source_to_c_abi(ClockSource::Replay),
        FLOWRT_C_CLOCK_SOURCE_REPLAY
    );

    assert_eq!(FLOWRT_C_OUTPUT_UNWRITTEN, 0);
    assert_eq!(FLOWRT_C_OUTPUT_WRITTEN, 1);
    assert_eq!(FLOWRT_C_OUTPUT_TRUNCATED, 2);
    assert_eq!(FLOWRT_C_OUTPUT_ERROR, 3);
}

#[test]
fn c_component_task_timing_abi_layout_is_stable() {
    assert_eq!(size_of::<FlowrtCTaskTiming>(), 120);
    assert_eq!(align_of::<FlowrtCTaskTiming>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtCTaskTiming, step), 0);
    assert_eq!(offset_of!(FlowrtCTaskTiming, task_name), 8);
    assert_eq!(offset_of!(FlowrtCTaskTiming, trigger), 24);
    assert_eq!(offset_of!(FlowrtCTaskTiming, clock_source), 40);
    assert_eq!(offset_of!(FlowrtCTaskTiming, scheduled_time_ms), 48);
    assert_eq!(offset_of!(FlowrtCTaskTiming, observed_time_ms), 56);
    assert_eq!(offset_of!(FlowrtCTaskTiming, scheduled_delta_ms), 64);
    assert_eq!(offset_of!(FlowrtCTaskTiming, observed_delta_ms), 72);
    assert_eq!(offset_of!(FlowrtCTaskTiming, period_ms), 80);
    assert_eq!(offset_of!(FlowrtCTaskTiming, deadline_ms), 88);
    assert_eq!(offset_of!(FlowrtCTaskTiming, lateness_ms), 96);
    assert_eq!(offset_of!(FlowrtCTaskTiming, missed_periods), 104);
    assert_eq!(offset_of!(FlowrtCTaskTiming, has_period_ms), 112);
    assert_eq!(offset_of!(FlowrtCTaskTiming, has_deadline_ms), 113);
    assert_eq!(offset_of!(FlowrtCTaskTiming, deadline_missed), 114);
    assert_eq!(offset_of!(FlowrtCTaskTiming, overrun), 115);
}

#[test]
fn c_component_context_abi_layout_is_stable() {
    assert_eq!(size_of::<FlowrtCComponentContext>(), 248);
    assert_eq!(align_of::<FlowrtCComponentContext>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtCComponentContext, component_name), 0);
    assert_eq!(
        offset_of!(FlowrtCComponentContext, instance_name),
        size_of::<FlowrtStringView>()
    );
    assert_eq!(
        offset_of!(FlowrtCComponentContext, task_name),
        size_of::<FlowrtStringView>() * 2
    );
    assert_eq!(
        offset_of!(FlowrtCComponentContext, lane_name),
        size_of::<FlowrtStringView>() * 3
    );
    assert_eq!(offset_of!(FlowrtCComponentContext, step), 64);
    assert_eq!(offset_of!(FlowrtCComponentContext, tick_time_ms), 72);
    assert_eq!(offset_of!(FlowrtCComponentContext, deadline_ms), 80);
    assert_eq!(offset_of!(FlowrtCComponentContext, has_deadline_ms), 88);
    assert_eq!(offset_of!(FlowrtCComponentContext, has_timing), 89);
    assert_eq!(offset_of!(FlowrtCComponentContext, timing), 96);
    assert_eq!(offset_of!(FlowrtCComponentContext, params), 216);
}

#[test]
fn c_component_input_and_output_abi_views_use_borrowed_buffers() {
    assert_eq!(size_of::<FlowrtCInputView>(), 88);
    assert_eq!(align_of::<FlowrtCInputView>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtCInputView, name), 0);
    assert_eq!(
        offset_of!(FlowrtCInputView, type_name),
        size_of::<FlowrtStringView>()
    );
    assert_eq!(offset_of!(FlowrtCInputView, schema_hash), 32);
    assert_eq!(offset_of!(FlowrtCInputView, size_bytes), 40);
    assert_eq!(offset_of!(FlowrtCInputView, payload), 48);
    assert_eq!(offset_of!(FlowrtCInputView, source_time_ms), 64);
    assert_eq!(offset_of!(FlowrtCInputView, revision), 72);
    assert_eq!(offset_of!(FlowrtCInputView, present), 80);
    assert_eq!(offset_of!(FlowrtCInputView, stale), 81);

    assert_eq!(size_of::<FlowrtCOutputSlot>(), 80);
    assert_eq!(align_of::<FlowrtCOutputSlot>(), align_of::<usize>());
    assert_eq!(offset_of!(FlowrtCOutputSlot, name), 0);
    assert_eq!(
        offset_of!(FlowrtCOutputSlot, type_name),
        size_of::<FlowrtStringView>()
    );
    assert_eq!(offset_of!(FlowrtCOutputSlot, schema_hash), 32);
    assert_eq!(offset_of!(FlowrtCOutputSlot, size_bytes), 40);
    assert_eq!(offset_of!(FlowrtCOutputSlot, data), 48);
    assert_eq!(offset_of!(FlowrtCOutputSlot, capacity), 56);
    assert_eq!(offset_of!(FlowrtCOutputSlot, written_len), 64);
    assert_eq!(offset_of!(FlowrtCOutputSlot, status), 72);
}

#[test]
fn c_component_callback_table_abi_layout_is_stable() {
    assert_eq!(size_of::<FlowrtCLifecycleCallback>(), size_of::<usize>());
    assert_eq!(size_of::<FlowrtCTaskCallback>(), size_of::<usize>());

    assert_eq!(size_of::<FlowrtCInputArrayView>(), size_of::<usize>() * 2);
    assert_eq!(offset_of!(FlowrtCInputArrayView, data), 0);
    assert_eq!(offset_of!(FlowrtCInputArrayView, len), size_of::<usize>());

    assert_eq!(size_of::<FlowrtCOutputArrayView>(), size_of::<usize>() * 2);
    assert_eq!(offset_of!(FlowrtCOutputArrayView, data), 0);
    assert_eq!(offset_of!(FlowrtCOutputArrayView, len), size_of::<usize>());

    assert_eq!(size_of::<FlowrtCComponentCallbackTable>(), 160);
    assert_eq!(
        align_of::<FlowrtCComponentCallbackTable>(),
        align_of::<usize>()
    );
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, size), 0);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, version_major), 4);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, version_minor), 8);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, feature_flags), 16);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, user_data), 24);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, on_init), 32);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, on_start), 40);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, on_stop), 48);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, on_shutdown), 56);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, run_periodic), 64);
    assert_eq!(
        offset_of!(FlowrtCComponentCallbackTable, run_on_message),
        72
    );
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, run_startup), 80);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, run_shutdown), 88);
    assert_eq!(offset_of!(FlowrtCComponentCallbackTable, reserved), 96);
}
