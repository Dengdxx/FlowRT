use std::mem::{align_of, offset_of, size_of};

use flowrt::{
    BackendHealthSnapshot, BackendHealthState, FrameDescriptor, FrameLeaseStatus, FrameMetadata,
    ReconnectPolicy, ResourceDescriptor, Status,
    abi::{
        FLOWRT_ABI_VERSION_MAJOR, FLOWRT_ABI_VERSION_MINOR, FLOWRT_BACKEND_HEALTH_DEGRADED,
        FLOWRT_BACKEND_HEALTH_FAILED, FLOWRT_BACKEND_HEALTH_READY,
        FLOWRT_BACKEND_HEALTH_RECONNECTING, FLOWRT_BACKEND_HEALTH_UNSUPPORTED,
        FLOWRT_BACKEND_INPROC, FLOWRT_BACKEND_IOX2, FLOWRT_BACKEND_ZENOH,
        FLOWRT_FRAME_LEASE_ACQUIRED, FLOWRT_FRAME_LEASE_ATTACHED, FLOWRT_FRAME_LEASE_ERROR,
        FLOWRT_FRAME_LEASE_EXPIRED, FLOWRT_FRAME_LEASE_GENERATION_MISMATCH,
        FLOWRT_FRAME_LEASE_RELEASED, FLOWRT_STATUS_ERROR, FLOWRT_STATUS_OK, FLOWRT_STATUS_RETRY,
        FlowrtBackendHealthSnapshot, FlowrtBytesView, FlowrtFrameDescriptor, FlowrtI128,
        FlowrtReconnectPolicy, FlowrtResourceDescriptor, FlowrtStringView, FlowrtU128,
        backend_health_snapshot_to_abi, backend_health_state_to_abi, frame_descriptor_to_abi,
        frame_lease_status_to_abi, reconnect_policy_to_abi, status_to_abi,
    },
};

#[test]
fn abi_version_and_status_codes_are_stable() {
    assert_eq!(FLOWRT_ABI_VERSION_MAJOR, 0);
    assert_eq!(FLOWRT_ABI_VERSION_MINOR, 1);

    assert_eq!(FLOWRT_STATUS_OK, 0);
    assert_eq!(FLOWRT_STATUS_RETRY, 1);
    assert_eq!(FLOWRT_STATUS_ERROR, 2);

    assert_eq!(status_to_abi(Status::Ok), FLOWRT_STATUS_OK);
    assert_eq!(status_to_abi(Status::Retry), FLOWRT_STATUS_RETRY);
    assert_eq!(status_to_abi(Status::Error), FLOWRT_STATUS_ERROR);
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
