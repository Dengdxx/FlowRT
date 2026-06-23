use super::prelude::*;

// -- RestartPolicy 测试 --

#[test]
fn restart_policy_never_never_restarts() {
    let policy = RestartPolicy {
        policy: RestartPolicyKind::Never,
        max_restarts: 5,
        initial_delay_ms: 100,
        max_delay_ms: 1000,
    };
    assert!(!policy.can_restart(false, 0));
    assert!(!policy.can_restart(true, 0));
}

#[test]
fn restart_policy_on_failure_restarts_on_failure_only() {
    let policy = DEFAULT_RESTART_POLICY;
    assert!(policy.can_restart(false, 0));
    assert!(policy.can_restart(false, 2));
    assert!(!policy.can_restart(false, 3)); // max_restarts = 3
    assert!(!policy.can_restart(true, 0));
}

#[test]
fn restart_policy_always_restarts_even_on_success() {
    let policy = RestartPolicy {
        policy: RestartPolicyKind::Always,
        max_restarts: 2,
        initial_delay_ms: 100,
        max_delay_ms: 1000,
    };
    assert!(policy.can_restart(true, 0));
    assert!(policy.can_restart(false, 1));
    assert!(!policy.can_restart(true, 2));
}

#[test]
fn restart_delay_exponential_backoff() {
    let policy = RestartPolicy {
        policy: RestartPolicyKind::OnFailure,
        max_restarts: 10,
        initial_delay_ms: 100,
        max_delay_ms: 5000,
    };
    assert_eq!(policy.delay_ms_for(0), 100); // 100 * 1
    assert_eq!(policy.delay_ms_for(1), 200); // 100 * 2
    assert_eq!(policy.delay_ms_for(2), 400); // 100 * 4
    assert_eq!(policy.delay_ms_for(3), 800); // 100 * 8
    assert_eq!(policy.delay_ms_for(4), 1600); // 100 * 16
    assert_eq!(policy.delay_ms_for(5), 3200); // 100 * 32
    assert_eq!(policy.delay_ms_for(6), 5000); // capped at max_delay_ms
}

#[test]
fn restart_delay_saturates_at_max() {
    let policy = RestartPolicy {
        policy: RestartPolicyKind::OnFailure,
        max_restarts: 100,
        initial_delay_ms: 100,
        max_delay_ms: 1000,
    };
    // 100 * 2^63 would overflow, but saturating_mul handles it
    assert_eq!(policy.delay_ms_for(63), 1000);
    assert_eq!(policy.delay_ms_for(100), 1000);
}

#[test]
fn status_snapshot_aggregation_merges_process_files() {
    let dir = temp_test_dir("launch-status-aggregation");
    std::fs::write(
        dir.join("controller.status.json"),
        r#"{"tick_count":6,"graph_health":"healthy","graph_critical_health":"healthy","instances":[],"routes":[],"failovers":[]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("actuator.status.json"),
        r#"{"tick_count":6,"graph_health":"healthy","graph_critical_health":"healthy","instances":[],"routes":[],"failovers":[]}"#,
    )
    .unwrap();

    let json = aggregate_status_snapshots_for_test(&dir, &["controller", "actuator"]).unwrap();

    assert_eq!(json["mode"], "launch");
    assert_eq!(json["processes"].as_array().unwrap().len(), 2);
    assert_eq!(json["processes"][0]["process"], "controller");
    assert_eq!(json["processes"][1]["process"], "actuator");
}

#[cfg(unix)]
#[test]
fn supervised_child_drop_terminates_running_process() {
    let mut command = Command::new("sleep");
    command.arg("30");
    let child = command.spawn().unwrap();
    let pid = child.id();
    let supervised = supervised_child_for_test("drop_cleanup", child);

    drop(supervised);

    assert_process_exits(pid);
}

#[cfg(unix)]
#[test]
fn supervisor_shutdown_token_terminates_active_children() {
    let supervisor_state = IntrospectionState::new();
    let mut command = Command::new("sleep");
    command.arg("30");
    let child = command.spawn().unwrap();
    let pid = child.id();
    let mut children = vec![supervised_child_for_test("shutdown_cleanup", child)];
    let shutdown = ShutdownToken::new_for_test();
    shutdown.request();

    let result = supervise_children(&supervisor_state, &mut children, None, &shutdown, None);

    assert!(result.is_ok());
    assert!(children[0].finished);
    assert_eq!(children[0].state, "shutdown");
    assert_process_exits(pid);
}

#[cfg(unix)]
#[test]
fn restart_dependency_check_requires_ready_dependencies_for_runtime_ready() {
    let mut command = Command::new("sleep");
    command.arg("30");
    let child = command.spawn().unwrap();
    let mut supervised = supervised_child_for_test("worker", child);
    supervised.readiness = ReadinessGate::RuntimeReady;
    supervised.dependencies = vec!["driver".to_string()];
    let mut spawned = BTreeSet::new();
    spawned.insert("driver".to_string());
    let ready = BTreeSet::new();

    assert!(!child_dependencies_satisfied(&supervised, &spawned, &ready));

    supervised.terminate("test_cleanup");
}

#[cfg(unix)]
#[test]
fn restart_dependency_check_allows_spawned_dependency_for_process_started() {
    let mut command = Command::new("sleep");
    command.arg("30");
    let child = command.spawn().unwrap();
    let mut supervised = supervised_child_for_test("worker", child);
    supervised.readiness = ReadinessGate::ProcessStarted;
    supervised.dependencies = vec!["driver".to_string()];
    let mut spawned = BTreeSet::new();
    spawned.insert("driver".to_string());
    let ready = BTreeSet::new();

    assert!(child_dependencies_satisfied(&supervised, &spawned, &ready));

    supervised.terminate("test_cleanup");
}

#[cfg(unix)]
#[test]
fn startup_delay_fails_if_process_exits_after_readiness() {
    let child = Command::new("true").spawn().unwrap();
    let supervisor_state = IntrospectionState::new();
    let mut supervised = supervised_child_for_test("startup_delay_exit", child);
    supervised.startup_delay_ms = 50;

    let result = wait_for_startup_delay(
        &supervisor_state,
        &mut supervised,
        &ShutdownToken::new_for_test(),
    );

    assert!(result.is_err());
    assert!(supervised.finished);
    assert_eq!(supervised.state, "readiness_failed");
}

#[cfg(unix)]
#[test]
fn restart_waits_when_dependency_exits_in_same_monitor_iteration() {
    let driver = Command::new("true").spawn().unwrap();
    let worker = Command::new("true").spawn().unwrap();
    let mut children = vec![
        supervised_child_for_test("driver", driver),
        supervised_child_for_test("worker", worker),
    ];
    children[1].app_exe = PathBuf::from("true");
    children[1].readiness = ReadinessGate::RuntimeReady;
    children[1].dependencies = vec!["driver".to_string()];
    children[1].next_restart_unix_ms = Some(unix_time_ms());
    wait_for_child_exit(&mut children[0].child);
    let supervisor_state = IntrospectionState::new();
    let shutdown = ShutdownToken::new_for_test();
    let shutdown_clone = shutdown.clone();
    let shutdown_thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        shutdown_clone.request();
    });

    let result = supervise_children(&supervisor_state, &mut children, None, &shutdown, None);

    shutdown_thread
        .join()
        .expect("shutdown thread should finish");
    assert!(
        result.is_ok(),
        "worker must not restart against an exited dependency: {result:?}"
    );
    assert_eq!(children[1].restart_count, 0);
    assert_eq!(children[1].state, "shutdown");
}

#[cfg(unix)]
#[test]
fn refresh_child_health_returns_when_socket_stalls() {
    let child = Command::new("sleep").arg("30").spawn().unwrap();
    let mut supervised = supervised_child_for_test("stalled_socket", child);
    let socket = crate::runtime_socket_path_for_pid(supervised.child.id());
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent).expect("socket parent should be created");
    }
    let _ = std::fs::remove_file(&socket);
    let listener =
        std::os::unix::net::UnixListener::bind(&socket).expect("test listener should bind");
    let handle = std::thread::spawn(move || {
        let (_stream, _addr) = listener.accept().expect("test listener should accept");
        std::thread::sleep(Duration::from_millis(250));
    });
    supervised.socket = socket.clone();
    let supervisor_state = IntrospectionState::new();

    let started = Instant::now();
    refresh_child_health(&supervisor_state, &mut supervised);

    assert!(
        started.elapsed() < Duration::from_millis(500),
        "refresh_child_health should not block the supervisor loop indefinitely"
    );
    assert!(!supervised.finished);
    supervised.terminate("test_cleanup");
    handle
        .join()
        .expect("stalled listener thread should finish");
    let _ = std::fs::remove_file(socket);
}

#[cfg(unix)]
#[test]
fn supervisor_terminates_active_children_when_run_ticks_reached() {
    let child = Command::new("sleep").arg("30").spawn().unwrap();
    let mut supervised = supervised_child_for_test("bounded_run", child);
    supervised.last_tick_count = Some(3);
    let supervisor_state = IntrospectionState::new();

    let result = supervise_children(
        &supervisor_state,
        std::slice::from_mut(&mut supervised),
        Some(3),
        &ShutdownToken::new_for_test(),
        None,
    );

    assert!(
        result.is_ok(),
        "supervisor run limit should stop active children: {result:?}"
    );
    assert!(supervised.finished);
    assert_eq!(supervised.state, "completed");
}

#[cfg(unix)]
#[test]
fn supervisor_allows_run_limited_child_to_exit_before_terminate() {
    let child = Command::new("sh")
        .arg("-c")
        .arg("sleep 0.2")
        .spawn()
        .unwrap();
    let mut supervised = supervised_child_for_test("bounded_run", child);
    supervised.last_tick_count = Some(3);
    let supervisor_state = IntrospectionState::new();

    let result = supervise_children(
        &supervisor_state,
        std::slice::from_mut(&mut supervised),
        Some(3),
        &ShutdownToken::new_for_test(),
        None,
    );

    assert!(
        result.is_ok(),
        "supervisor run limit should allow bounded child exit: {result:?}"
    );
    assert!(supervised.finished);
    assert_eq!(supervised.exit_code, Some(0));
}
