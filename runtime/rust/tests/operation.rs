use std::time::Duration;

use flowrt::{
    FrameCodec, OperationCancelToken, OperationConcurrencyPolicy, OperationError,
    OperationHealthCounters, OperationHealthSnapshot, OperationId, OperationLifecycle,
    OperationOwner, OperationPolicy, OperationPreemptPolicy, OperationProgress, OperationStartAck,
    OperationStartRequest, OperationState, OperationStatusSnapshot,
};

fn frame_roundtrip<T>(value: &T)
where
    T: FrameCodec + PartialEq + std::fmt::Debug,
{
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(T::decode_frame(&frame).unwrap(), *value);
}

#[test]
fn operation_state_machine_tracks_success_and_counters() {
    let id = OperationId::new(0xAA, 1, 0xBB);
    let mut lifecycle = OperationLifecycle::new(id, OperationPolicy::default()).unwrap();

    assert_eq!(lifecycle.state(), OperationState::Starting);
    lifecycle.transition(OperationState::Running).unwrap();
    lifecycle.transition(OperationState::Succeeded).unwrap();

    let snapshot = lifecycle.snapshot();
    assert_eq!(snapshot.id, id);
    assert_eq!(snapshot.state, OperationState::Succeeded);
    assert!(!snapshot.cancel_requested);
    assert_eq!(snapshot.health.started, 1);
    assert_eq!(snapshot.health.succeeded, 1);
    assert_eq!(snapshot.health.failed, 0);
}

#[test]
fn operation_rejects_illegal_transition_after_terminal_state() {
    let id = OperationId::new(1, 1, 7);
    let mut lifecycle = OperationLifecycle::new(id, OperationPolicy::default()).unwrap();

    lifecycle.transition(OperationState::Running).unwrap();
    lifecycle.transition(OperationState::Succeeded).unwrap();

    let error = lifecycle
        .transition(OperationState::Failed)
        .expect_err("terminal state must reject later transition");
    assert_eq!(
        error,
        OperationError::InvalidTransition {
            from: OperationState::Succeeded,
            to: OperationState::Failed,
        }
    );
}

#[test]
fn operation_cancel_is_cooperative_and_counts_canceled() {
    let id = OperationId::new(1, 2, 7);
    let mut lifecycle = OperationLifecycle::new(id, OperationPolicy::default()).unwrap();
    let token = lifecycle.cancel_token();

    assert!(!token.is_canceled());
    lifecycle.transition(OperationState::Running).unwrap();
    lifecycle.request_cancel().unwrap();

    assert!(token.is_canceled());
    assert_eq!(lifecycle.state(), OperationState::CancelRequested);
    lifecycle.transition(OperationState::Cancelled).unwrap();

    let snapshot = lifecycle.snapshot();
    assert!(snapshot.cancel_requested);
    assert_eq!(snapshot.health.canceled, 1);
}

#[test]
fn operation_policy_rejects_zero_limits() {
    assert_eq!(
        OperationPolicy::new(
            Duration::ZERO,
            OperationConcurrencyPolicy::Reject,
            OperationPreemptPolicy::Reject,
            8,
            1,
        ),
        Err(OperationError::InvalidPolicy("timeout_ms"))
    );
    assert_eq!(
        OperationPolicy::new(
            Duration::from_millis(100),
            OperationConcurrencyPolicy::Reject,
            OperationPreemptPolicy::Reject,
            0,
            1,
        ),
        Err(OperationError::InvalidPolicy("queue_depth"))
    );
    assert_eq!(
        OperationPolicy::new(
            Duration::from_millis(100),
            OperationConcurrencyPolicy::Queue,
            OperationPreemptPolicy::CancelRunning,
            8,
            0,
        ),
        Err(OperationError::InvalidPolicy("max_in_flight"))
    );
}

#[test]
fn operation_lifecycle_rejects_manually_invalid_policy() {
    let policy = OperationPolicy {
        timeout: Duration::ZERO,
        ..OperationPolicy::default()
    };

    let error = OperationLifecycle::new(OperationId::new(1, 1, 1), policy)
        .expect_err("invalid policy should fail");
    assert_eq!(error, OperationError::InvalidPolicy("timeout_ms"));
}

#[test]
fn operation_timeout_updates_counters() {
    let timeout_id = OperationId::new(1, 3, 7);
    let mut timed = OperationLifecycle::new(timeout_id, OperationPolicy::default()).unwrap();
    timed.transition(OperationState::Running).unwrap();
    timed.transition(OperationState::TimedOut).unwrap();
    assert_eq!(timed.snapshot().health.timeout, 1);
}

#[test]
fn operation_progress_carries_id_sequence_and_value() {
    let progress = OperationProgress::new(OperationId::new(9, 10, 11), 3, 42u32);

    assert_eq!(progress.id.operation_key, 9);
    assert_eq!(progress.sequence, 3);
    assert_eq!(progress.value, 42);
}

#[test]
fn operation_health_counters_can_record_terminal_states() {
    let counters = OperationHealthCounters::default();
    counters.record_state(OperationState::Running);
    counters.record_state(OperationState::Failed);
    counters.record_state(OperationState::Cancelled);
    counters.record_state(OperationState::TimedOut);

    let snapshot = counters.snapshot();
    assert_eq!(snapshot.started, 1);
    assert_eq!(snapshot.failed, 1);
    assert_eq!(snapshot.canceled, 1);
    assert_eq!(snapshot.timeout, 1);
}

#[test]
fn operation_cancel_token_can_be_shared_without_blocking() {
    let token = OperationCancelToken::new();
    let clone = token.clone();

    token.request_cancel();

    assert!(token.is_canceled());
    assert!(clone.is_canceled());
}

#[test]
fn operation_transport_envelopes_use_canonical_frame_codec() {
    let id = OperationId::new(
        0x1122_3344_5566_7788,
        0x0102_0304_0506_0708,
        0xA0B0_C0D0_E0F0_1020,
    );
    let owner = OperationOwner::new(0x0A0B_0C0D_0E0F_1011, 0x2122_2324_2526_2728);
    let ack = OperationStartAck::accepted_with_authority(id, owner, 123_456);
    let status = OperationStatusSnapshot {
        id,
        owner,
        state: OperationState::CancelRequested,
        cancel_requested: true,
        deadline_ms: 123_456,
        health: OperationHealthSnapshot {
            started: 1,
            succeeded: 2,
            failed: 3,
            canceled: 4,
            timeout: 5,
            preempted: 6,
        },
    };
    let start = OperationStartRequest::new(42u32, owner, Duration::from_millis(250));

    frame_roundtrip(&id);
    frame_roundtrip(&owner);
    frame_roundtrip(&ack);
    frame_roundtrip(&status);
    frame_roundtrip(&start);

    assert_eq!(id.to_frame_vec().unwrap().len(), 24);
    assert_eq!(owner.to_frame_vec().unwrap().len(), 16);
    assert_eq!(ack.to_frame_vec().unwrap().len(), 49);
    assert_eq!(status.to_frame_vec().unwrap().len(), 98);
    assert_eq!(start.to_frame_vec().unwrap().len(), 28);
}
