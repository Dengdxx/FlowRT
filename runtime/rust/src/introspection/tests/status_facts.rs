use super::*;

#[test]
fn recorder_captures_param_operation_and_scheduler_events() {
    let state = IntrospectionState::new();
    state.start_recorder(IntrospectionRecorderStart {
        output: None,
        filters: vec!["all".to_string()],
        queue_depth: Some(16),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime_pid: 42,
        selfdesc_hash: "abc123".to_string(),
    });
    state.register_param(IntrospectionParamSchema {
        name: "controller.kp".to_string(),
        ty: "f64".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: None,
        max: None,
        choices: vec![],
    });
    state
        .set_param_pending("controller.kp", serde_json::json!(2.0))
        .expect("param set should be accepted");
    state.record_param_applied("controller.kp", serde_json::json!(2.0));
    state.record_service_health(IntrospectionServiceStatus {
        name: "planner.plan_to_executor.execute".to_string(),
        ready: true,
        in_flight: 1,
        queued: 0,
        total_requests: 1,
        timeout_count: 0,
        busy_count: 0,
        unavailable_count: 0,
        late_drop_count: 0,
    });
    state.record_operation_health(IntrospectionOperationStatus {
        name: "controller.plan".to_string(),
        ready: true,
        running: 1,
        queued: 0,
        current_operation_ids: vec!["1:2:3".to_string()],
        total_started: 1,
        succeeded_count: 0,
        failed_count: 0,
        canceled_count: 0,
        timeout_count: 0,
        preempted_count: 0,
        current_state: Some("running".to_string()),
        current_owner: Some("controller.plan".to_string()),
        current_deadline_ms: Some(1500),
        last_event: Some("flowrt.operation.state_changed".to_string()),
        last_error: None,
        last_transition_ms: Some(12),
    });
    state.record_operation_transition(
        "controller.plan",
        "1:2:3",
        "running",
        Some("controller.plan"),
        Some(1500),
    );
    state.record_operation_progress("controller.plan", "1:2:3", 0);
    state.record_operation_result("controller.plan", "1:2:3", "succeeded", None);
    state.record_operation_result("controller.plan", "1:2:4", "failed", Some("handler error"));
    state.record_task_health(IntrospectionTaskHealth {
        name: "control_loop".to_string(),
        lane: "control".to_string(),
        run_count: 1,
        success_count: 1,
        ..Default::default()
    });
    state.record_lane_health(IntrospectionLaneHealth {
        name: "control".to_string(),
        queue_depth: 0,
        dispatched_count: 1,
        fairness_violations: 0,
    });

    let events = state.drain_recorder_events();
    assert!(events.iter().any(|event| {
        event.event_kind == flowrt_record::RecordEventKind::ParamEvent
            && event.entity.name == "controller.kp"
    }));
    assert!(events.iter().any(|event| {
        event.event_kind == flowrt_record::RecordEventKind::ServiceEvent
            && event.entity.name == "planner.plan_to_executor.execute"
    }));
    assert!(events.iter().any(|event| {
        event.event_kind == flowrt_record::RecordEventKind::OperationEvent
            && event.entity.name == "controller.plan"
            && event.payload_schema == "flowrt.operation.state_changed"
    }));
    assert!(events.iter().any(|event| {
        event.event_kind == flowrt_record::RecordEventKind::OperationEvent
            && event.entity.name == "controller.plan"
            && event.payload_schema == "flowrt.operation.progress"
    }));
    assert!(events.iter().any(|event| {
        event.event_kind == flowrt_record::RecordEventKind::OperationEvent
            && event.entity.name == "controller.plan"
            && event.payload_schema == "flowrt.operation.result"
    }));
    assert!(events.iter().any(|event| {
        event.event_kind == flowrt_record::RecordEventKind::OperationEvent
            && event.entity.name == "controller.plan"
            && event.payload_schema == "flowrt.operation.error"
    }));
    assert!(
        events
            .iter()
            .any(|event| event.event_kind == flowrt_record::RecordEventKind::SchedulerEvent)
    );
}

#[test]
fn clock_record_event_uses_scheduler_time_model() {
    let state = IntrospectionState::new();
    state.start_recorder(IntrospectionRecorderStart {
        output: None,
        filters: vec!["all".to_string()],
        queue_depth: None,
        package: String::new(),
        process: String::new(),
        runtime_pid: 0,
        selfdesc_hash: String::new(),
    });

    state.record_tick_at(25, "simulated_replay");

    let events = state.drain_recorder_events();
    let clock = events
        .iter()
        .find(|event| event.event_kind == flowrt_record::RecordEventKind::ClockEvent)
        .expect("clock event should be recorded");
    assert_eq!(clock.monotonic_ns, 25_000_000);
    let payload: serde_json::Value = serde_json::from_slice(&clock.payload).unwrap();
    assert_eq!(payload["tick_time_ms"], 25);
    assert_eq!(payload["time_source"], "simulated_replay");
}

#[test]
fn input_read_records_presence_and_route_counters() {
    let state = IntrospectionState::new();
    state.register_route(IntrospectionRouteStatus {
        name: "source.packet_to_sink.packet".to_string(),
        from: "source.packet".to_string(),
        to: "sink.packet".to_string(),
        message_type: "Packet".to_string(),
        backend: "zenoh".to_string(),
        selected_reason: "explicit".to_string(),
        dropped_samples: 1,
        backpressure_count: 2,
        overflow_count: 3,
        ..Default::default()
    });

    state.record_input_read(
        "sink.main.packet",
        "sink.main",
        "packet",
        "source.packet_to_sink.packet",
        "Packet",
        true,
        false,
        Some(7),
        Some(42),
    );

    let status = state.status();
    assert_eq!(status.inputs.len(), 1);
    let input = &status.inputs[0];
    assert_eq!(input.task, "sink.main");
    assert_eq!(input.input, "packet");
    assert_eq!(input.channel, "source.packet_to_sink.packet");
    assert_eq!(input.message_type, "Packet");
    assert!(input.present);
    assert!(!input.stale);
    assert_eq!(input.last_revision, Some(7));
    assert_eq!(input.last_read_ms, Some(42));
    assert_eq!(input.dropped_samples, 1);
    assert_eq!(input.backpressure_count, 2);
    assert_eq!(input.overflow_count, 3);
}

#[test]
fn status_derives_structured_diagnostics_from_live_state() {
    let state = IntrospectionState::new();
    state.record_tick_at(250, "simulated_replay");
    state.record_lifecycle_state("sink", crate::LifecycleState::Running);
    state.register_route(IntrospectionRouteStatus {
        name: "source.packet_to_sink.packet".to_string(),
        from: "source.packet".to_string(),
        to: "sink.packet".to_string(),
        message_type: "Packet".to_string(),
        backend: "zenoh".to_string(),
        selected_reason: "variable_frame_auto_fallback".to_string(),
        published_count: 4,
        dropped_samples: 1,
        backpressure_count: 2,
        overflow_count: 3,
        last_publish_ms: Some(120),
        last_error: Some("queue overflow".to_string()),
        ..Default::default()
    });
    state.record_input_status(IntrospectionInputStatus {
        task: "sink.main".to_string(),
        input: "packet".to_string(),
        channel: "source.packet_to_sink.packet".to_string(),
        message_type: "Packet".to_string(),
        present: false,
        stale: true,
        last_revision: Some(9),
        last_read_ms: Some(125),
        updated_unix_ms: Some(2000),
        dropped_samples: 1,
        backpressure_count: 2,
        overflow_count: 3,
    });
    state.record_process_health(IntrospectionProcessStatus {
        name: "sensors".to_string(),
        state: "stale".to_string(),
        pid: Some(77),
        restart_count: 2,
        tick_count: Some(10),
        last_seen_unix_ms: Some(2000),
        tick_stale: true,
        exit_code: Some(1),
        readiness_wait: Some("resource_ready".to_string()),
        resource_placement: None,
    });
    state.register_resource(IntrospectionResourceStatus {
        name: "sensor.lidar_uart".to_string(),
        capability: "perception.lidar.samples".to_string(),
        state: "failed".to_string(),
        required: true,
        readiness: Some("before_start".to_string()),
        health: Some("required".to_string()),
        on_failure: Some("stop_process".to_string()),
        diagnostic: Some("provider failed".to_string()),
        suggestion: Some("start driver package".to_string()),
        updated_unix_ms: Some(4000),
        ..Default::default()
    });
    state.register_io_boundary(
        "camera",
        "CameraDriver",
        vec![IntrospectionIoBoundaryResourceStatus {
            name: "camera_shm".to_string(),
            kind: "shm".to_string(),
            ready: false,
            message: Some("waiting for frame".to_string()),
            last_error: Some("timeout".to_string()),
            updated_unix_ms: Some(5000),
        }],
    );
    state.record_io_boundary_error("camera", "frame timeout");
    state.register_param(IntrospectionParamSchema {
        name: "controller.kp".to_string(),
        ty: "f32".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: Some(serde_json::json!(0.0)),
        max: Some(serde_json::json!(10.0)),
        choices: vec![],
    });
    state
        .set_param_pending("controller.kp", serde_json::json!(2.0))
        .unwrap();
    state.record_param_rejected("controller.kp", serde_json::json!(2.0), "callback_rejected");
    state.record_operation_result("controller.plan", "1:2:3", "failed", Some("handler error"));
    state.record_task_health(IntrospectionTaskHealth {
        name: "control_loop".to_string(),
        lane: "control".to_string(),
        scheduled_time_ms: Some(1_000),
        observed_time_ms: Some(1_025),
        lateness_ms: Some(25),
        missed_periods: Some(2),
        overrun: Some(true),
        deadline_missed: 1,
        stale_input: 1,
        backpressure: 1,
        overflow: 1,
        run_count: 8,
        success_count: 8,
        consecutive_failures: 0,
        ..Default::default()
    });

    let status = state.status();
    assert!(
        status
            .params
            .iter()
            .any(|param| param.name == "controller.kp")
    );
    let categories = status
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.category.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for category in [
        "task",
        "input",
        "route",
        "resource",
        "io_boundary",
        "process",
        "param",
        "operation",
        "clock",
        "graph_health",
    ] {
        assert!(
            categories.contains(category),
            "missing diagnostics category `{category}` in {:?}",
            status.diagnostics
        );
    }
    let route = status
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.category == "route")
        .expect("route diagnostic should exist");
    assert_eq!(route.entity_kind, "route");
    assert_eq!(route.entity_id, "source.packet_to_sink.packet");
    assert_eq!(route.reason.as_deref(), Some("queue overflow"));
    assert!(route.metrics.iter().any(|metric| {
        metric.name == "latest_age_ms" && metric.value == serde_json::json!(130)
    }));
    let task = status
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.category == "task")
        .expect("task diagnostic should exist");
    assert_eq!(task.state, "degraded");
    assert_eq!(
        task.reason.as_deref(),
        Some("runtime observed task timing issue")
    );
    assert!(task.metrics.iter().any(|metric| {
        metric.name == "scheduled_time_ms" && metric.value == serde_json::json!(1_000)
    }));
    assert!(task.metrics.iter().any(|metric| {
        metric.name == "observed_time_ms" && metric.value == serde_json::json!(1_025)
    }));
    assert!(
        task.metrics.iter().any(|metric| {
            metric.name == "lateness_ms" && metric.value == serde_json::json!(25)
        })
    );
    assert!(
        task.metrics.iter().any(|metric| {
            metric.name == "missed_periods" && metric.value == serde_json::json!(2)
        })
    );
    assert!(
        task.metrics
            .iter()
            .any(|metric| metric.name == "overrun" && metric.value == serde_json::json!(true))
    );
}

#[test]
fn route_backend_health_snapshot_updates_status_and_diagnostics() {
    let state = IntrospectionState::new();
    state.register_route(IntrospectionRouteStatus {
        name: "source.packet_to_sink.packet".to_string(),
        from: "source.packet".to_string(),
        to: "sink.packet".to_string(),
        message_type: "Packet".to_string(),
        backend: "zenoh".to_string(),
        selected_reason: "profile_default".to_string(),
        ..Default::default()
    });
    state.record_route_backend_health(
        "source.packet_to_sink.packet",
        BackendHealthSnapshot {
            state: BackendHealthState::Reconnecting,
            last_error: Some("publish Zenoh sample: session closed".to_string()),
            attempt: 2,
            next_retry_unix_ms: Some(4_200),
            recoverable: true,
        },
    );

    let status = state.status();
    let route = status
        .routes
        .iter()
        .find(|route| route.name == "source.packet_to_sink.packet")
        .expect("route should be present");
    assert_eq!(route.backend_health_state, "reconnecting");
    assert_eq!(
        route.backend_health_error.as_deref(),
        Some("publish Zenoh sample: session closed")
    );
    assert_eq!(route.backend_reconnect_attempt, 2);
    assert_eq!(route.backend_next_retry_unix_ms, Some(4_200));
    assert!(route.backend_recoverable);
    assert_eq!(
        route.last_error.as_deref(),
        Some("publish Zenoh sample: session closed")
    );

    let route_diagnostic = status
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.category == "route")
        .expect("route diagnostic should be derived");
    assert_eq!(route_diagnostic.state, "reconnecting");
    assert_eq!(route_diagnostic.severity, "warn");
    assert_eq!(
        route_diagnostic.reason.as_deref(),
        Some("publish Zenoh sample: session closed")
    );
    assert!(route_diagnostic.metrics.iter().any(|metric| {
        metric.name == "backend_health_state" && metric.value == serde_json::json!("reconnecting")
    }));
    assert!(route_diagnostic.metrics.iter().any(|metric| {
        metric.name == "backend_recoverable" && metric.value == serde_json::json!(true)
    }));
}

#[test]
fn route_backend_health_ready_clears_active_error() {
    let state = IntrospectionState::new();
    state.record_route_error("source.packet_to_sink.packet", "queue overflow");
    state.record_route_backend_health(
        "source.packet_to_sink.packet",
        BackendHealthSnapshot::ready(),
    );

    let status = state.status();
    let route = status.routes.first().expect("route should be present");
    assert_eq!(route.backend_health_state, "ready");
    assert!(route.backend_health_error.is_none());
    assert!(route.last_error.is_none());
}

#[test]
fn route_transport_error_updates_policy_counter_and_backend_error() {
    let state = IntrospectionState::new();

    state.record_route_transport_error(
        "source.packet_to_sink.packet",
        crate::OverflowPolicy::Block,
        "publish transport route: queue full",
    );
    state.record_route_transport_error(
        "source.packet_to_sink.packet",
        crate::OverflowPolicy::DropNewest,
        "publish transport route: queue full",
    );
    state.record_route_transport_error(
        "source.packet_to_sink.packet",
        crate::OverflowPolicy::Error,
        "publish transport route: queue full",
    );

    let status = state.status();
    let route = status.routes.first().expect("route should be present");
    assert_eq!(route.backpressure_count, 1);
    assert_eq!(route.dropped_samples, 1);
    assert_eq!(route.overflow_count, 1);
    assert_eq!(
        route.last_error.as_deref(),
        Some("publish transport route: queue full")
    );
    assert_eq!(route.backend_health_state, "degraded");
    assert_eq!(
        route.backend_health_error.as_deref(),
        Some("publish transport route: queue full")
    );
}

#[test]
fn recorder_captures_diagnostics_events_from_status() {
    let state = IntrospectionState::new();
    state.start_recorder(IntrospectionRecorderStart {
        output: None,
        filters: vec!["all".to_string()],
        queue_depth: None,
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime_pid: 42,
        selfdesc_hash: "abc123".to_string(),
    });
    state.record_route_error("source.packet_to_sink.packet", "queue overflow");
    state.record_task_health(IntrospectionTaskHealth {
        name: "controller.loop".to_string(),
        lane: "control".to_string(),
        scheduled_time_ms: Some(2_000),
        observed_time_ms: Some(2_018),
        lateness_ms: Some(18),
        missed_periods: Some(1),
        overrun: Some(true),
        ..Default::default()
    });

    state.record_current_diagnostics();

    let events = state.drain_recorder_events();
    let diagnostic = events
        .iter()
        .find(|event| {
            event.event_kind == flowrt_record::RecordEventKind::DiagnosticsEvent
                && event.entity.name == "source.packet_to_sink.packet"
        })
        .expect("current diagnostics should record diagnostics events");
    assert_eq!(diagnostic.selfdesc_hash, "abc123");
    assert_eq!(
        diagnostic.entity.kind,
        flowrt_record::RecordEntityKind::Diagnostic
    );
    assert_eq!(diagnostic.entity.name, "source.packet_to_sink.packet");
    assert_eq!(diagnostic.payload_schema, "flowrt.diagnostics.status");
    let payload: serde_json::Value = serde_json::from_slice(&diagnostic.payload).unwrap();
    assert_eq!(payload["category"], "route");
    assert_eq!(payload["reason"], "queue overflow");

    let timing_diagnostic = events
        .iter()
        .find(|event| {
            event.event_kind == flowrt_record::RecordEventKind::DiagnosticsEvent
                && event.entity.name == "controller.loop"
        })
        .expect("timing diagnostics should record diagnostics event");
    let payload: serde_json::Value = serde_json::from_slice(&timing_diagnostic.payload).unwrap();
    assert_eq!(payload["category"], "task");
    assert_eq!(payload["reason"], "runtime observed task timing issue");
    assert!(payload["metrics"].as_array().unwrap().iter().any(|metric| {
        metric["name"] == "lateness_ms" && metric["value"] == serde_json::json!(18)
    }));
}

#[test]
fn observability_facts_feed_status_diagnostics_and_recorder_events() {
    let state = IntrospectionState::new();
    state.record_tick_at(500, "simulated_replay");
    state.record_route_error("source.packet_to_sink.packet", "queue overflow");
    state.record_task_health(IntrospectionTaskHealth {
        name: "controller.loop".to_string(),
        lane: "control".to_string(),
        scheduled_time_ms: Some(2_000),
        observed_time_ms: Some(2_018),
        lateness_ms: Some(18),
        missed_periods: Some(1),
        overrun: Some(true),
        ..Default::default()
    });

    let facts = crate::introspection::facts::RuntimeObservabilityFacts::from_status_snapshot(
        state.status(),
    );
    let status = facts.status_snapshot();
    let diagnostics = facts.diagnostic_snapshot();
    let recorder_events = facts.recorder_diagnostic_events();

    assert_eq!(status.diagnostics, diagnostics);
    let task_diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.entity_id == "controller.loop")
        .expect("task diagnostic fact should exist");
    assert_eq!(
        task_diagnostic.reason.as_deref(),
        Some("runtime observed task timing issue")
    );

    let task_event = recorder_events
        .iter()
        .find(|event| event.entity_id == "controller.loop")
        .expect("task recorder event fact should exist");
    assert_eq!(task_event.payload_schema, "flowrt.diagnostics.status");
    assert_eq!(task_event.monotonic_ns, Some(2_018_000_000));
    assert_eq!(task_event.payload["category"], "task");
    assert_eq!(
        task_event.payload["reason"],
        "runtime observed task timing issue"
    );
}

#[test]
fn health_fields_serialize_roundtrip() {
    let status = IntrospectionStatus {
        tick_count: 42,
        clock: IntrospectionClockStatus::default(),
        channels: vec![],
        processes: vec![],
        resources: vec![IntrospectionResourceStatus {
            name: "sensor.lidar_uart".to_string(),
            capability: "perception.lidar.samples".to_string(),
            state: "pending".to_string(),
            required: true,
            source: Some("contract".to_string()),
            owner_process: Some("main".to_string()),
            last_error: Some("provider not reported ready".to_string()),
            updated_unix_ms: Some(4000),
            ..Default::default()
        }],
        inputs: vec![IntrospectionInputStatus {
            task: "sink.main".to_string(),
            input: "packet".to_string(),
            channel: "source.packet_to_sink.packet".to_string(),
            message_type: "Packet".to_string(),
            present: true,
            stale: false,
            last_revision: Some(7),
            last_read_ms: Some(996),
            updated_unix_ms: Some(997),
            dropped_samples: 1,
            backpressure_count: 2,
            overflow_count: 3,
        }],
        routes: vec![IntrospectionRouteStatus {
            name: "source.packet_to_sink.packet".to_string(),
            from: "source.packet".to_string(),
            to: "sink.packet".to_string(),
            message_type: "Packet".to_string(),
            backend: "zenoh".to_string(),
            selected_reason: "variable_frame_auto_fallback".to_string(),
            published_count: 11,
            dropped_samples: 1,
            backpressure_count: 2,
            overflow_count: 3,
            last_publish_ms: Some(995),
            last_error: Some("queue overflow".to_string()),
            ..Default::default()
        }],
        io_boundaries: vec![IntrospectionIoBoundaryStatus {
            name: "camera".to_string(),
            component: "CameraDriver".to_string(),
            ready: true,
            healthy: true,
            last_error: None,
            resources: vec![IntrospectionIoBoundaryResourceStatus {
                name: "camera_shm".to_string(),
                kind: "shm".to_string(),
                ready: true,
                message: None,
                last_error: None,
                updated_unix_ms: Some(997),
            }],
            updated_unix_ms: Some(998),
        }],
        params: Vec::new(),
        services: vec![],
        operations: vec![IntrospectionOperationStatus {
            name: "controller.plan".to_string(),
            ready: true,
            running: 1,
            queued: 0,
            current_operation_ids: vec!["1:2:3".to_string()],
            total_started: 1,
            succeeded_count: 0,
            failed_count: 0,
            canceled_count: 0,
            timeout_count: 0,
            preempted_count: 0,
            last_transition_ms: Some(998),
            ..Default::default()
        }],
        tasks: vec![IntrospectionTaskHealth {
            name: "t1".to_string(),
            lane: "l1".to_string(),
            inflight: true,
            scheduled_time_ms: Some(1_000),
            observed_time_ms: Some(1_020),
            lateness_ms: Some(20),
            missed_periods: Some(2),
            overrun: Some(true),
            deadline_missed: 5,
            stale_input: 2,
            backpressure: 1,
            overflow: 0,
            fairness_violations: 0,
            run_count: 100,
            success_count: 95,
            consecutive_failures: 0,
            last_run_ms: Some(1000),
            last_success_ms: Some(999),
        }],
        lanes: vec![IntrospectionLaneHealth {
            name: "l1".to_string(),
            queue_depth: 3,
            dispatched_count: 200,
            fairness_violations: 1,
        }],
        recorder: Default::default(),
        instances: Vec::new(),
        failovers: Vec::new(),
        critical_instances: vec!["controller".to_string()],
        graph_health: "degraded".to_string(),
        graph_critical_health: "healthy".to_string(),
        diagnostics: Vec::new(),
    };

    let json = serde_json::to_string(&status).unwrap();
    let parsed: IntrospectionStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.graph_health, "degraded");
    assert_eq!(parsed.graph_critical_health, "healthy");
    assert_eq!(parsed.critical_instances, ["controller"]);
    assert_eq!(parsed.clock.source, "realtime");
    assert_eq!(parsed.clock.unit, "ms");
    assert_eq!(parsed.clock.field, "tick_time_ms");
    assert_eq!(parsed.inputs.len(), 1);
    assert_eq!(parsed.inputs[0].task, "sink.main");
    assert!(parsed.inputs[0].present);
    assert_eq!(parsed.routes.len(), 1);
    assert_eq!(parsed.routes[0].backend, "zenoh");
    assert_eq!(
        parsed.routes[0].selected_reason,
        "variable_frame_auto_fallback"
    );
    assert_eq!(parsed.resources.len(), 1);
    assert_eq!(parsed.resources[0].name, "sensor.lidar_uart");
    assert_eq!(parsed.resources[0].state, "pending");
    assert_eq!(parsed.resources[0].source.as_deref(), Some("contract"));
    assert_eq!(parsed.operations.len(), 1);
    assert_eq!(parsed.operations[0].name, "controller.plan");
    assert_eq!(parsed.io_boundaries.len(), 1);
    assert_eq!(parsed.io_boundaries[0].name, "camera");
    assert_eq!(parsed.tasks.len(), 1);
    assert_eq!(parsed.tasks[0].name, "t1");
    assert!(parsed.tasks[0].inflight);
    assert_eq!(parsed.tasks[0].scheduled_time_ms, Some(1_000));
    assert_eq!(parsed.tasks[0].observed_time_ms, Some(1_020));
    assert_eq!(parsed.tasks[0].lateness_ms, Some(20));
    assert_eq!(parsed.tasks[0].missed_periods, Some(2));
    assert_eq!(parsed.tasks[0].overrun, Some(true));
    assert_eq!(parsed.tasks[0].deadline_missed, 5);
    assert_eq!(parsed.lanes.len(), 1);
    assert_eq!(parsed.lanes[0].queue_depth, 3);
}

#[test]
fn records_instance_lifecycle_state_and_derives_diagnostic() {
    let state = IntrospectionState::new();
    state.record_lifecycle_state("controller", crate::LifecycleState::Running);
    state.record_lifecycle_state("plant", crate::LifecycleState::Faulted);
    state.record_lifecycle_state("monitor", crate::LifecycleState::Degraded);

    let status = state.status();
    let names: Vec<_> = status
        .instances
        .iter()
        .map(|i| i.instance.as_str())
        .collect();
    assert_eq!(names, ["controller", "monitor", "plant"]);
    assert_eq!(status.instances[0].lifecycle_state, "running");
    assert_eq!(status.instances[1].lifecycle_state, "degraded");
    assert_eq!(status.instances[2].lifecycle_state, "faulted");

    let lifecycle: Vec<_> = status
        .diagnostics
        .iter()
        .filter(|d| d.category == "lifecycle")
        .collect();
    assert_eq!(lifecycle.len(), 3);
    let plant = lifecycle.iter().find(|d| d.entity_id == "plant").unwrap();
    assert_eq!(plant.entity_kind, "instance");
    assert_eq!(plant.state, "faulted");
    assert_eq!(plant.severity, "error");

    // degraded 是降级续跑，介于健康与停机之间 → warn，而非误报 error 或淹没为 info。
    let monitor = lifecycle.iter().find(|d| d.entity_id == "monitor").unwrap();
    assert_eq!(monitor.state, "degraded");
    assert_eq!(monitor.severity, "warn");

    // 图级 health = worst-of：存在 faulted 实例 → 图 faulted（error），聚合到单一图级诊断。
    assert_eq!(status.graph_health, "faulted");
    let graph = status
        .diagnostics
        .iter()
        .find(|d| d.category == "graph_health")
        .unwrap();
    assert_eq!(graph.entity_kind, "graph");
    assert_eq!(graph.entity_id, "graph");
    assert_eq!(graph.state, "faulted");
    assert_eq!(graph.severity, "error");
}

#[test]
fn graph_health_tracks_critical_subset_and_instance_fault_metrics() {
    let state = IntrospectionState::new();
    state.register_critical_instances(["controller_a", "controller_b"]);
    state.record_lifecycle_transition(
        "controller_a",
        crate::LifecycleState::Running,
        Some(1),
        None,
    );
    state.record_lifecycle_transition(
        "controller_b",
        crate::LifecycleState::Running,
        Some(1),
        None,
    );
    state.record_lifecycle_transition(
        "monitor",
        crate::LifecycleState::Faulted,
        Some(2),
        Some("sensor_timeout"),
    );

    let status = state.status();
    assert_eq!(status.graph_health, "faulted");
    assert_eq!(status.graph_critical_health, "healthy");
    assert_eq!(status.critical_instances, ["controller_a", "controller_b"]);
    let monitor = status
        .instances
        .iter()
        .find(|instance| instance.instance == "monitor")
        .expect("monitor instance should be present");
    assert_eq!(monitor.restart_count, 0);
    assert_eq!(monitor.last_fault_reason.as_deref(), Some("sensor_timeout"));
    assert_eq!(monitor.last_fault_tick, Some(2));
    assert_eq!(monitor.last_transition_tick, Some(2));

    state.record_instance_restart("controller_a");
    state.record_lifecycle_transition(
        "controller_a",
        crate::LifecycleState::Faulted,
        Some(7),
        Some("critical_fault"),
    );

    let status = state.status();
    assert_eq!(status.graph_health, "faulted");
    assert_eq!(status.graph_critical_health, "faulted");
    let controller_a = status
        .instances
        .iter()
        .find(|instance| instance.instance == "controller_a")
        .expect("controller_a instance should be present");
    assert_eq!(controller_a.restart_count, 1);
    assert_eq!(
        controller_a.last_fault_reason.as_deref(),
        Some("critical_fault")
    );
    assert_eq!(controller_a.last_fault_tick, Some(7));
    assert_eq!(controller_a.last_transition_tick, Some(7));
}

#[test]
fn records_failover_events() {
    let state = IntrospectionState::new();
    state.record_failover(IntrospectionFailoverEvent {
        event: "failover".to_string(),
        group: "controller_ha".to_string(),
        old_active: "controller_a".to_string(),
        new_active: "controller_b".to_string(),
        tick_id: 7,
        reason: "critical_fault".to_string(),
    });

    let status = state.status();
    assert_eq!(status.failovers.len(), 1);
    let event = &status.failovers[0];
    assert_eq!(event.event, "failover");
    assert_eq!(event.group, "controller_ha");
    assert_eq!(event.old_active, "controller_a");
    assert_eq!(event.new_active, "controller_b");
    assert_eq!(event.tick_id, 7);
    assert_eq!(event.reason, "critical_fault");
}
