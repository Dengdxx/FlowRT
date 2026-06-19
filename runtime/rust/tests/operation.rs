use std::time::Duration;

use flowrt::{
    FlowrtSpan, FlowrtSpanSink, FrameCodec, IntrospectionClockStatus, IntrospectionFailoverEvent,
    IntrospectionOperationStatus, IntrospectionProcessStatus, IntrospectionRouteStatus,
    IntrospectionServiceStatus, IntrospectionStatus, IntrospectionTaskHealth, OperationCancelToken,
    OperationConcurrencyPolicy, OperationControl, OperationControlError, OperationError,
    OperationHealthCounters, OperationHealthSnapshot, OperationId, OperationLifecycle,
    OperationOwner, OperationPolicy, OperationPreemptPolicy, OperationProgress, OperationStartAck,
    OperationStartRequest, OperationState, OperationStatusSnapshot, TracingExporter,
    TracingExporterConfig,
};

fn frame_roundtrip<T>(value: &T)
where
    T: FrameCodec + PartialEq + std::fmt::Debug,
{
    let frame = value.to_frame_vec().unwrap();
    assert_eq!(T::decode_frame(&frame).unwrap(), *value);
}

#[test]
fn tracing_exporter_derives_spans_without_driving_runtime() {
    let status = tracing_status_fixture();
    let sink = CollectingSpanSink::default();
    let exporter = TracingExporter::new(
        TracingExporterConfig {
            enabled: true,
            endpoint: Some("http://127.0.0.1:4318/v1/traces".to_string()),
        },
        &sink,
    );

    let report = exporter.export_status(&status);

    assert_eq!(report.emitted, 7);
    assert!(report.diagnostics.is_empty());
    let names = sink
        .spans()
        .into_iter()
        .map(|span| span.name)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        names,
        std::collections::BTreeSet::from([
            "flowrt.failover.transition".to_string(),
            "flowrt.global_tick".to_string(),
            "flowrt.operation.invocation".to_string(),
            "flowrt.process.lifecycle".to_string(),
            "flowrt.route.publish".to_string(),
            "flowrt.service.request".to_string(),
            "flowrt.task.execution".to_string(),
        ])
    );

    let disabled = TracingExporter::new(TracingExporterConfig::default(), &sink);
    let disabled_report = disabled.export_status(&status);
    assert_eq!(disabled_report.emitted, 0);
    assert!(disabled_report.diagnostics.is_empty());
}

#[test]
fn tracing_exporter_failure_records_diagnostic() {
    let status = tracing_status_fixture();
    let original_diagnostics = status.diagnostics.clone();
    let sink = FailingSpanSink;
    let exporter = TracingExporter::new(
        TracingExporterConfig {
            enabled: true,
            endpoint: Some("otlp://collector".to_string()),
        },
        &sink,
    );

    let report = exporter.export_status(&status);

    assert_eq!(status.diagnostics, original_diagnostics);
    assert_eq!(report.emitted, 0);
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.category, "tracing");
    assert_eq!(diagnostic.entity_kind, "tracing_exporter");
    assert_eq!(diagnostic.entity_id, "otlp://collector");
    assert_eq!(diagnostic.state, "export_failed");
    assert_eq!(diagnostic.severity, "warn");
    assert_eq!(diagnostic.reason.as_deref(), Some("sink unavailable"));
}

#[derive(Default)]
struct CollectingSpanSink {
    spans: std::sync::Mutex<Vec<FlowrtSpan>>,
}

impl CollectingSpanSink {
    fn spans(&self) -> Vec<FlowrtSpan> {
        self.spans.lock().unwrap().clone()
    }
}

impl FlowrtSpanSink for &CollectingSpanSink {
    fn emit(&self, span: FlowrtSpan) -> Result<(), String> {
        self.spans.lock().unwrap().push(span);
        Ok(())
    }
}

struct FailingSpanSink;

impl FlowrtSpanSink for &FailingSpanSink {
    fn emit(&self, _span: FlowrtSpan) -> Result<(), String> {
        Err("sink unavailable".to_string())
    }
}

fn tracing_status_fixture() -> IntrospectionStatus {
    IntrospectionStatus {
        tick_count: 42,
        clock: IntrospectionClockStatus {
            source: "global_tick".to_string(),
            tick_time_ms: Some(420),
            unit: "ms".to_string(),
            field: "tick_time_ms".to_string(),
        },
        processes: vec![IntrospectionProcessStatus {
            name: "control".to_string(),
            state: "running".to_string(),
            pid: Some(1001),
            restart_count: 1,
            tick_count: Some(42),
            last_seen_unix_ms: Some(123_456),
            tick_stale: false,
            exit_code: None,
            readiness_wait: None,
            resource_placement: None,
        }],
        services: vec![IntrospectionServiceStatus {
            name: "planner.plan".to_string(),
            ready: true,
            in_flight: 1,
            queued: 0,
            total_requests: 3,
            timeout_count: 0,
            busy_count: 0,
            unavailable_count: 0,
            late_drop_count: 0,
        }],
        operations: vec![IntrospectionOperationStatus {
            name: "planner.execute".to_string(),
            ready: true,
            running: 1,
            queued: 0,
            current_operation_ids: vec!["op-1".to_string()],
            total_started: 1,
            succeeded_count: 0,
            failed_count: 0,
            canceled_count: 0,
            timeout_count: 0,
            preempted_count: 0,
            current_state: Some("running".to_string()),
            current_owner: Some("planner".to_string()),
            current_deadline_ms: Some(450),
            last_event: Some("started".to_string()),
            last_error: None,
            last_transition_ms: Some(123_400),
        }],
        tasks: vec![IntrospectionTaskHealth {
            name: "planner.tick".to_string(),
            lane: "planner".to_string(),
            inflight: false,
            scheduled_time_ms: Some(410),
            observed_time_ms: Some(411),
            lateness_ms: Some(1),
            run_count: 2,
            success_count: 2,
            last_run_ms: Some(123_410),
            last_success_ms: Some(123_410),
            ..IntrospectionTaskHealth::default()
        }],
        routes: vec![IntrospectionRouteStatus {
            name: "planner.command".to_string(),
            from: "planner.command".to_string(),
            to: "driver.command".to_string(),
            message_type: "Command".to_string(),
            backend: "zenoh".to_string(),
            selected_reason: "explicit".to_string(),
            published_count: 5,
            last_publish_ms: Some(123_420),
            ..IntrospectionRouteStatus::default()
        }],
        failovers: vec![IntrospectionFailoverEvent {
            event: "switch_active".to_string(),
            group: "drive_redundancy".to_string(),
            old_active: "drive_primary".to_string(),
            new_active: "drive_standby".to_string(),
            tick_id: 42,
            reason: "critical_fault".to_string(),
        }],
        graph_health: "healthy".to_string(),
        graph_critical_health: "healthy".to_string(),
        ..IntrospectionStatus::default()
    }
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
fn operation_control_queue_promotes_next_invocation_after_completion() {
    let policy = OperationPolicy::new(
        Duration::from_millis(50),
        OperationConcurrencyPolicy::Queue,
        OperationPreemptPolicy::Reject,
        2,
        1,
    )
    .unwrap();
    let owner = OperationOwner::new(10, 20);
    let mut control = OperationControl::new(99, policy);

    let first = control.start(owner, 100).unwrap();
    control.mark_running(first.id).unwrap();
    let second = control.start(owner, 101).unwrap();

    assert_eq!(control.queued_len(), 1);
    assert_eq!(
        control.status(second.id).unwrap().state,
        OperationState::Starting
    );
    assert_eq!(
        control.mark_running(second.id),
        Err(OperationControlError::StaleInvocation {
            requested: second.id,
            current: Some(first.id),
        })
    );

    control
        .complete_at(first.id, OperationState::Succeeded, 110)
        .unwrap();
    assert_eq!(control.queued_len(), 0);
    assert_eq!(control.snapshot().id, second.id);
    assert_eq!(control.snapshot().state, OperationState::Starting);
    control.mark_running(second.id).unwrap();
}

#[test]
fn operation_control_times_out_queued_invocation_before_promotion() {
    let policy = OperationPolicy::new(
        Duration::from_millis(50),
        OperationConcurrencyPolicy::Queue,
        OperationPreemptPolicy::Reject,
        2,
        1,
    )
    .and_then(|policy| policy.with_result_retention(Duration::from_millis(100)))
    .unwrap();
    let owner = OperationOwner::new(10, 20);
    let mut control = OperationControl::new(99, policy);

    let first = control
        .start_with_timeout(owner, 100, Duration::from_millis(200))
        .unwrap();
    control.mark_running(first.id).unwrap();
    let second = control
        .start_with_timeout(owner, 101, Duration::from_millis(10))
        .unwrap();

    assert_eq!(control.queued_len(), 1);
    assert!(control.check_deadline(111));
    assert_eq!(control.queued_len(), 0);
    assert_eq!(control.snapshot().id, first.id);
    assert_eq!(control.snapshot().state, OperationState::Running);
    assert_eq!(
        control.status(second.id).unwrap().state,
        OperationState::TimedOut
    );
    assert!(!control.ready_to_run(second.id));
}

#[test]
fn operation_control_cancel_running_preempts_active_invocation() {
    let policy = OperationPolicy::new(
        Duration::from_millis(50),
        OperationConcurrencyPolicy::Reject,
        OperationPreemptPolicy::CancelRunning,
        8,
        1,
    )
    .unwrap();
    let owner = OperationOwner::new(10, 20);
    let mut control = OperationControl::new(99, policy);

    let first = control.start(owner, 100).unwrap();
    control.mark_running(first.id).unwrap();
    let first_cancel = control.cancel_token().unwrap();

    let second = control.start(owner, 101).unwrap();

    assert!(first_cancel.is_canceled());
    assert_eq!(
        control.status(first.id).unwrap().state,
        OperationState::CancelRequested
    );
    assert_eq!(control.snapshot().id, second.id);
    assert_eq!(control.snapshot().state, OperationState::Starting);
    assert_eq!(control.snapshot().health.preempted, 1);
    control.mark_running(second.id).unwrap();
}

#[test]
fn operation_control_allows_multiple_in_flight_invocations_until_limit() {
    let policy = OperationPolicy::new(
        Duration::from_millis(50),
        OperationConcurrencyPolicy::Queue,
        OperationPreemptPolicy::Reject,
        2,
        2,
    )
    .unwrap();
    let owner = OperationOwner::new(10, 20);
    let mut control = OperationControl::new(99, policy);

    let first = control.start(owner, 100).unwrap();
    let second = control.start(owner, 101).unwrap();
    let third = control.start(owner, 102).unwrap();

    assert!(control.ready_to_run(first.id));
    assert!(control.ready_to_run(second.id));
    assert!(!control.ready_to_run(third.id));
    assert_eq!(control.queued_len(), 1);
    control.mark_running(first.id).unwrap();
    control.mark_running(second.id).unwrap();
    control
        .complete_at(first.id, OperationState::Succeeded, 110)
        .unwrap();

    assert!(control.ready_to_run(third.id));
    assert_eq!(control.queued_len(), 0);
    control.mark_running(third.id).unwrap();
}

#[test]
fn operation_control_retains_terminal_status_until_retention_deadline() {
    let policy = OperationPolicy {
        result_retention: Duration::from_millis(20),
        ..OperationPolicy::default()
    };
    let owner = OperationOwner::new(10, 20);
    let mut control = OperationControl::new(99, policy);

    let ack = control.start(owner, 100).unwrap();
    control.mark_running(ack.id).unwrap();
    control
        .complete_at(ack.id, OperationState::Succeeded, 110)
        .unwrap();

    assert_eq!(
        control.status(ack.id).unwrap().state,
        OperationState::Succeeded
    );
    control.evict_retained_results(129);
    assert_eq!(
        control.status(ack.id).unwrap().state,
        OperationState::Succeeded
    );
    control.evict_retained_results(131);
    assert_eq!(
        control.status(ack.id),
        Err(OperationControlError::StaleInvocation {
            requested: ack.id,
            current: None,
        })
    );
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
