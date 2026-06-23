use super::prelude::*;

#[test]
fn expected_services_for_process_uses_server_instances() {
    let graph = LaunchGraph {
        name: "main".to_string(),
        mode: "strict".to_string(),
        scheduler: serde_json::json!({}),
        resource_contract: LaunchResourceContract::default(),
        channels: vec![],
        boundary_endpoints: vec![],
        services: vec![
            LaunchService {
                name: "client.plan".to_string(),
                client: "client.plan".to_string(),
                client_instance: "client".to_string(),
                client_port: "plan".to_string(),
                server: "server.plan".to_string(),
                server_instance: "server".to_string(),
                server_port: "plan".to_string(),
                request: "PlanRequest".to_string(),
                response: "PlanResponse".to_string(),
                backend: "inproc".to_string(),
                backend_source: None,
                request_frame: None,
                response_frame: None,
                service: None,
                key_expr: None,
                timeout_ms: 100,
                queue_depth: 1,
                overflow: "error".to_string(),
                lane: None,
                max_in_flight: 64,
            },
            LaunchService {
                name: "client.inspect".to_string(),
                client: "client.inspect".to_string(),
                client_instance: "client".to_string(),
                client_port: "inspect".to_string(),
                server: "inspector.inspect".to_string(),
                server_instance: "inspector".to_string(),
                server_port: "inspect".to_string(),
                request: "InspectRequest".to_string(),
                response: "InspectResponse".to_string(),
                backend: "inproc".to_string(),
                backend_source: None,
                request_frame: None,
                response_frame: None,
                service: None,
                key_expr: None,
                timeout_ms: 100,
                queue_depth: 1,
                overflow: "error".to_string(),
                lane: None,
                max_in_flight: 64,
            },
        ],
        operations: vec![],
        ros2_bridges: vec![],
        instances: vec![],
        tasks: vec![],
        processes: vec![],
    };
    let mut process = test_process("server_proc", vec![]);
    process.instances = vec!["server".to_string()];

    assert_eq!(
        expected_services_for_process(&graph, &process),
        vec!["client.plan".to_string()]
    );
}

#[test]
fn expected_services_ready_requires_each_expected_ready_endpoint() {
    let expected = vec!["client.plan".to_string(), "client.inspect".to_string()];
    let live = vec![
        service_status("client.plan", true),
        service_status("client.inspect", true),
    ];
    assert!(expected_services_ready(&expected, &live));

    let missing = vec![service_status("client.plan", true)];
    assert!(!expected_services_ready(&expected, &missing));

    let not_ready = vec![
        service_status("client.plan", true),
        service_status("client.inspect", false),
    ];
    assert!(!expected_services_ready(&expected, &not_ready));
}

#[test]
fn expected_services_ready_falls_back_to_runtime_ready_when_no_expected_services() {
    assert!(expected_services_ready(&[], &[]));
}

// -- readiness timeout 和交互测试 --

fn short_readiness_config() -> ReadinessConfig {
    ReadinessConfig {
        timeout: Duration::from_millis(200),
        poll_interval: Duration::from_millis(10),
    }
}

#[test]
fn readiness_wait_aborts_when_shutdown_requested() {
    let supervisor_state = IntrospectionState::new();
    let shutdown = ShutdownToken::new_for_test();
    shutdown.request();
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    let real_child = cmd.spawn().unwrap();
    let socket = crate::runtime_socket_path_for_pid(real_child.id());

    let mut child = SupervisedChild {
        name: "test_shutdown".into(),
        backend: "inproc".into(),
        runtime_kind: "rust".into(),
        external: None,
        external_resolution: None,
        app_exe: PathBuf::from("/tmp/fake"),
        zenoh_env: None,
        dependencies: vec![],
        env: BTreeMap::new(),
        restart_policy: DEFAULT_RESTART_POLICY,
        failure: "propagate".into(),
        readiness: ReadinessGate::RuntimeReady,
        expected_services: vec![],
        startup_delay_ms: 0,
        resource_gate: ProcessResourceGate::default(),
        resource_degraded: false,
        resource_wait: None,
        resource_placement: ResourcePlacement::default(),
        resource_placement_status: ResourcePlacementStatus::default(),
        child: real_child,
        socket,
        finished: false,
        restart_count: 0,
        next_restart_unix_ms: None,
        last_seen_unix_ms: None,
        last_tick_count: None,
        last_tick_changed_unix_ms: unix_time_ms(),
        state: "starting".into(),
        exit_code: None,
    };

    let result = wait_for_readiness(
        &supervisor_state,
        &mut child,
        &short_readiness_config(),
        &shutdown,
    );

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("shutdown requested"));
    assert!(child.finished);
    assert_eq!(child.state, "shutdown");
}

#[test]
fn readiness_timeout_marks_state_and_returns_error() {
    // 启动一个不会建立 introspection socket 的子进程（sleep）。
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    let real_child = cmd.spawn().unwrap();
    let socket = crate::runtime_socket_path_for_pid(real_child.id());
    let supervisor_state = IntrospectionState::new();

    let mut child = SupervisedChild {
        name: "test_timeout".into(),
        backend: "inproc".into(),
        runtime_kind: "rust".into(),
        external: None,
        external_resolution: None,
        app_exe: PathBuf::from("/tmp/fake"),
        zenoh_env: None,
        dependencies: vec![],
        env: BTreeMap::new(),
        restart_policy: DEFAULT_RESTART_POLICY,
        failure: "propagate".into(),
        readiness: ReadinessGate::RuntimeReady,
        expected_services: vec![],
        startup_delay_ms: 0,
        resource_gate: ProcessResourceGate::default(),
        resource_degraded: false,
        resource_wait: None,
        resource_placement: ResourcePlacement::default(),
        resource_placement_status: ResourcePlacementStatus::default(),
        child: real_child,
        socket,
        finished: false,
        restart_count: 0,
        next_restart_unix_ms: None,
        last_seen_unix_ms: None,
        last_tick_count: None,
        last_tick_changed_unix_ms: unix_time_ms(),
        state: "starting".into(),
        exit_code: None,
    };

    let config = short_readiness_config();
    let shutdown = ShutdownToken::new_for_test();
    let result = wait_for_runtime_ready(&supervisor_state, &mut child, &config, &shutdown);

    assert!(result.is_err(), "should timeout");
    let err = result.unwrap_err();
    assert!(
        err.contains("timed out"),
        "error should mention timeout: {err}"
    );
    assert!(
        err.contains("test_timeout"),
        "error should name the process"
    );
    assert!(
        child.finished,
        "child should be marked finished after timeout"
    );
    assert_eq!(child.state, "readiness_timeout");
}

#[test]
fn readiness_service_ready_timeout_marks_state() {
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    let real_child = cmd.spawn().unwrap();
    let socket = crate::runtime_socket_path_for_pid(real_child.id());
    let supervisor_state = IntrospectionState::new();

    let mut child = SupervisedChild {
        name: "test_svc_timeout".into(),
        backend: "inproc".into(),
        runtime_kind: "rust".into(),
        external: None,
        external_resolution: None,
        app_exe: PathBuf::from("/tmp/fake"),
        zenoh_env: None,
        dependencies: vec![],
        env: BTreeMap::new(),
        restart_policy: DEFAULT_RESTART_POLICY,
        failure: "propagate".into(),
        readiness: ReadinessGate::ServiceReady,
        expected_services: vec!["planner.plan".to_string()],
        startup_delay_ms: 0,
        resource_gate: ProcessResourceGate::default(),
        resource_degraded: false,
        resource_wait: None,
        resource_placement: ResourcePlacement::default(),
        resource_placement_status: ResourcePlacementStatus::default(),
        child: real_child,
        socket,
        finished: false,
        restart_count: 0,
        next_restart_unix_ms: None,
        last_seen_unix_ms: None,
        last_tick_count: None,
        last_tick_changed_unix_ms: unix_time_ms(),
        state: "starting".into(),
        exit_code: None,
    };

    let config = short_readiness_config();
    let shutdown = ShutdownToken::new_for_test();
    let result = wait_for_service_ready(&supervisor_state, &mut child, &config, &shutdown);

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("timed out"));
    assert_eq!(child.state, "readiness_timeout");
    assert!(child.finished);
}

#[test]
fn readiness_process_exit_during_wait_marks_failed() {
    // 启动一个立即退出的进程。
    let mut cmd = Command::new("true");
    let real_child = cmd.spawn().unwrap();
    let socket = crate::runtime_socket_path_for_pid(real_child.id());
    let supervisor_state = IntrospectionState::new();

    let mut child = SupervisedChild {
        name: "test_exit".into(),
        backend: "inproc".into(),
        runtime_kind: "rust".into(),
        external: None,
        external_resolution: None,
        app_exe: PathBuf::from("/tmp/fake"),
        zenoh_env: None,
        dependencies: vec![],
        env: BTreeMap::new(),
        restart_policy: DEFAULT_RESTART_POLICY,
        failure: "propagate".into(),
        readiness: ReadinessGate::RuntimeReady,
        expected_services: vec![],
        startup_delay_ms: 0,
        resource_gate: ProcessResourceGate::default(),
        resource_degraded: false,
        resource_wait: None,
        resource_placement: ResourcePlacement::default(),
        resource_placement_status: ResourcePlacementStatus::default(),
        child: real_child,
        socket,
        finished: false,
        restart_count: 0,
        next_restart_unix_ms: None,
        last_seen_unix_ms: None,
        last_tick_count: None,
        last_tick_changed_unix_ms: unix_time_ms(),
        state: "starting".into(),
        exit_code: None,
    };

    let config = short_readiness_config();
    let shutdown = ShutdownToken::new_for_test();
    let result = wait_for_runtime_ready(&supervisor_state, &mut child, &config, &shutdown);

    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("exited during readiness wait"),
        "error should mention process exit"
    );
    assert_eq!(child.state, "readiness_failed");
    assert!(child.finished);
    assert_eq!(child.exit_code, Some(0));
}

#[test]
fn readiness_config_defaults_use_production_values() {
    let config = ReadinessConfig::default();
    assert_eq!(config.timeout, READINESS_TIMEOUT);
    assert_eq!(config.poll_interval, READINESS_POLL_INTERVAL);
}

#[test]
fn readiness_wait_process_started_returns_immediately() {
    let supervisor_state = IntrospectionState::new();
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    let real_child = cmd.spawn().unwrap();
    let socket = crate::runtime_socket_path_for_pid(real_child.id());

    let mut child = SupervisedChild {
        name: "test_started".into(),
        backend: "inproc".into(),
        runtime_kind: "rust".into(),
        external: None,
        external_resolution: None,
        app_exe: PathBuf::from("/tmp/fake"),
        zenoh_env: None,
        dependencies: vec![],
        env: BTreeMap::new(),
        restart_policy: DEFAULT_RESTART_POLICY,
        failure: "propagate".into(),
        readiness: ReadinessGate::ProcessStarted,
        expected_services: vec![],
        startup_delay_ms: 0,
        resource_gate: ProcessResourceGate::default(),
        resource_degraded: false,
        resource_wait: None,
        resource_placement: ResourcePlacement::default(),
        resource_placement_status: ResourcePlacementStatus::default(),
        child: real_child,
        socket,
        finished: false,
        restart_count: 0,
        next_restart_unix_ms: None,
        last_seen_unix_ms: None,
        last_tick_count: None,
        last_tick_changed_unix_ms: unix_time_ms(),
        state: "starting".into(),
        exit_code: None,
    };

    let config = ReadinessConfig::default();
    let shutdown = ShutdownToken::new_for_test();
    let result = wait_for_readiness(&supervisor_state, &mut child, &config, &shutdown);
    assert!(result.is_ok(), "ProcessStarted should return immediately");
    // 子进程不应被 kill，仍在运行。
    assert!(!child.finished);
    // 清理
    let _ = child.child.kill();
    let _ = child.child.wait();
}

#[test]
fn readiness_gate_label_returns_correct_string() {
    assert_eq!(
        readiness_gate_label(ReadinessGate::ProcessStarted),
        "process_started"
    );
    assert_eq!(
        readiness_gate_label(ReadinessGate::RuntimeReady),
        "runtime_ready"
    );
    assert_eq!(
        readiness_gate_label(ReadinessGate::ServiceReady),
        "service_ready"
    );
}

#[test]
fn readiness_record_health_sets_wait_field() {
    let supervisor_state = IntrospectionState::new();
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    let real_child = cmd.spawn().unwrap();
    let socket = crate::runtime_socket_path_for_pid(real_child.id());

    let mut child = SupervisedChild {
        name: "test_health".into(),
        backend: "inproc".into(),
        runtime_kind: "rust".into(),
        external: None,
        external_resolution: None,
        app_exe: PathBuf::from("/tmp/fake"),
        zenoh_env: None,
        dependencies: vec![],
        env: BTreeMap::new(),
        restart_policy: DEFAULT_RESTART_POLICY,
        failure: "propagate".into(),
        readiness: ReadinessGate::ServiceReady,
        expected_services: vec!["planner.plan".to_string()],
        startup_delay_ms: 0,
        resource_gate: ProcessResourceGate::default(),
        resource_degraded: false,
        resource_wait: None,
        resource_placement: ResourcePlacement::default(),
        resource_placement_status: ResourcePlacementStatus::default(),
        child: real_child,
        socket,
        finished: false,
        restart_count: 0,
        next_restart_unix_ms: None,
        last_seen_unix_ms: None,
        last_tick_count: None,
        last_tick_changed_unix_ms: unix_time_ms(),
        state: "waiting_readiness".into(),
        exit_code: None,
    };
    record_child_health(&supervisor_state, &child, false);

    let status = supervisor_state.status();
    let proc_status = status
        .processes
        .iter()
        .find(|p| p.name == "test_health")
        .unwrap();
    assert_eq!(
        proc_status.readiness_wait,
        Some("service_ready".to_string())
    );
    assert_eq!(proc_status.state, "waiting_readiness");

    // 清理
    let _ = child.child.kill();
    let _ = child.child.wait();
}

fn service_status(name: &str, ready: bool) -> crate::IntrospectionServiceStatus {
    crate::IntrospectionServiceStatus {
        name: name.to_string(),
        ready,
        in_flight: 0,
        queued: 0,
        total_requests: 0,
        timeout_count: 0,
        busy_count: 0,
        unavailable_count: 0,
        late_drop_count: 0,
    }
}
