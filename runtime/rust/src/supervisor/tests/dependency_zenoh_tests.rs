use super::prelude::*;

// -- 依赖排序测试 --

#[test]
fn resolve_dependency_order_no_deps() {
    let processes = vec![test_process("a", vec![]), test_process("b", vec![])];
    let order = resolve_dependency_order(&processes).unwrap();
    assert_eq!(order.len(), 2);
    assert!(order.contains(&"a".to_string()));
    assert!(order.contains(&"b".to_string()));
}

#[test]
fn resolve_dependency_order_linear_chain() {
    let processes = vec![
        test_process("a", vec![]),
        test_process("b", vec!["a".into()]),
        test_process("c", vec!["b".into()]),
    ];
    let order = resolve_dependency_order(&processes).unwrap();
    assert_eq!(order, vec!["a", "b", "c"]);
}

#[test]
fn resolve_dependency_order_diamond() {
    let processes = vec![
        test_process("a", vec![]),
        test_process("b", vec!["a".into()]),
        test_process("c", vec!["a".into()]),
        test_process("d", vec!["b".into(), "c".into()]),
    ];
    let order = resolve_dependency_order(&processes).unwrap();
    let a = order.iter().position(|n| n == "a").unwrap();
    let b = order.iter().position(|n| n == "b").unwrap();
    let c = order.iter().position(|n| n == "c").unwrap();
    let d = order.iter().position(|n| n == "d").unwrap();
    assert!(a < b);
    assert!(a < c);
    assert!(b < d);
    assert!(c < d);
}

#[test]
fn resolve_dependency_order_cycle_detected() {
    let processes = vec![
        test_process("a", vec!["b".into()]),
        test_process("b", vec!["a".into()]),
    ];
    let err = resolve_dependency_order(&processes).unwrap_err();
    assert!(err.contains("cycle"), "error should mention cycle: {err}");
}

#[test]
fn resolve_dependency_order_unknown_dep_rejected() {
    let processes = vec![test_process("a", vec!["missing".into()])];
    let err = resolve_dependency_order(&processes).unwrap_err();
    assert!(err.contains("unknown process"));
}

#[test]
fn resolve_dependency_order_self_dep_rejected() {
    let processes = vec![test_process("a", vec!["a".into()])];
    let err = resolve_dependency_order(&processes).unwrap_err();
    assert!(err.contains("depends on itself"));
}

// -- 失败传播测试 --

#[test]
fn propagate_failure_terminates_dependents() {
    let children = vec![
        PropagatableChild {
            name: "a".into(),
            dependencies: vec![],
            failure: "propagate".into(),
            finished: false,
        },
        PropagatableChild {
            name: "b".into(),
            dependencies: vec!["a".into()],
            failure: "propagate".into(),
            finished: false,
        },
        PropagatableChild {
            name: "c".into(),
            dependencies: vec!["b".into()],
            failure: "propagate".into(),
            finished: false,
        },
    ];
    let terminated = collect_propagated_failures(&children, "a");
    assert!(terminated.contains(&"b".to_string()));
    assert!(terminated.contains(&"c".to_string()));
}

#[test]
fn propagate_failure_stops_at_isolate() {
    let children = vec![
        PropagatableChild {
            name: "a".into(),
            dependencies: vec![],
            failure: "propagate".into(),
            finished: false,
        },
        PropagatableChild {
            name: "b".into(),
            dependencies: vec!["a".into()],
            failure: "isolate".into(),
            finished: false,
        },
        PropagatableChild {
            name: "c".into(),
            dependencies: vec!["b".into()],
            failure: "propagate".into(),
            finished: false,
        },
    ];
    let terminated = collect_propagated_failures(&children, "a");
    assert!(terminated.contains(&"b".to_string()));
    // c depends on b, but b is isolate so propagation stops
    assert!(!terminated.contains(&"c".to_string()));
}

#[test]
fn propagate_failure_skips_finished() {
    let children = vec![
        PropagatableChild {
            name: "a".into(),
            dependencies: vec![],
            failure: "propagate".into(),
            finished: false,
        },
        PropagatableChild {
            name: "b".into(),
            dependencies: vec!["a".into()],
            failure: "propagate".into(),
            finished: true, // already finished
        },
        PropagatableChild {
            name: "c".into(),
            dependencies: vec!["b".into()],
            failure: "propagate".into(),
            finished: false,
        },
    ];
    let terminated = collect_propagated_failures(&children, "a");
    // b is already finished, so it's skipped; c depends on b but b wasn't terminated
    assert!(!terminated.contains(&"b".to_string()));
    assert!(!terminated.contains(&"c".to_string()));
}

// -- Zenoh 自动配置测试 --

#[test]
fn should_auto_configure_zenoh_when_no_env_vars() {
    let _lock = ZENOH_ENV_LOCK.lock().expect("zenoh env lock should work");
    let _mode = EnvOverride::set("FLOWRT_ZENOH_MODE", None);
    let _listen = EnvOverride::set("FLOWRT_ZENOH_LISTEN", None);
    let _connect = EnvOverride::set("FLOWRT_ZENOH_CONNECT", None);

    assert!(should_auto_configure_zenoh());
}

#[test]
fn should_not_auto_configure_when_env_set() {
    let _lock = ZENOH_ENV_LOCK.lock().expect("zenoh env lock should work");
    let _mode = EnvOverride::set("FLOWRT_ZENOH_MODE", Some("peer"));

    assert!(!should_auto_configure_zenoh());
}

#[test]
fn zenoh_launch_env_hub_and_spoke() {
    let mut hub = test_process("hub", vec![]);
    hub.backend = "zenoh".into();
    let mut spoke = test_process("spoke", vec!["hub".into()]);
    spoke.backend = "zenoh".into();
    let inproc_only = test_process("inproc_only", vec![]);
    let processes = [hub, spoke, inproc_only];
    let refs: Vec<&LaunchProcess> = processes.iter().collect();
    let plan = zenoh_launch_env_for_graph(&refs).unwrap();
    let env = &plan.env;

    assert_eq!(env.len(), 2); // only zenoh processes
    let hub_env = env.get("hub").unwrap();
    let spoke_env = env.get("spoke").unwrap();
    assert!(hub_env.listen.starts_with("tcp/127.0.0.1:"));
    assert!(hub_env.connect.is_empty());
    assert!(spoke_env.listen.is_empty());
    assert_eq!(spoke_env.connect, hub_env.listen);
}

#[test]
fn zenoh_launch_env_empty_for_no_zenoh() {
    let processes = [test_process("a", vec![])];
    let refs: Vec<&LaunchProcess> = processes.iter().collect();
    let plan = zenoh_launch_env_for_graph(&refs).unwrap();
    assert!(plan.env.is_empty());
}

#[test]
fn zenoh_port_lease_skips_locked_port() {
    let first = reserve_zenoh_port_lease("hub").unwrap();
    let second = reserve_zenoh_port_lease("spoke").unwrap();

    assert_ne!(first.port, second.port);
}

// -- 依赖满足判断测试 --

#[test]
fn dependencies_satisfied_when_all_met_process_started() {
    let process = test_process("c", vec!["a".into(), "b".into()]);
    let mut spawned = BTreeSet::new();
    spawned.insert("a".into());
    spawned.insert("b".into());
    let ready = BTreeSet::new();
    // process_started gate：依赖只需已启动。
    assert!(process_dependencies_satisfied(&process, &spawned, &ready));
}

#[test]
fn dependencies_satisfied_when_all_met_runtime_ready() {
    let mut process = test_process("c", vec!["a".into(), "b".into()]);
    process.readiness = ReadinessGate::RuntimeReady;
    let mut spawned = BTreeSet::new();
    spawned.insert("a".into());
    spawned.insert("b".into());
    let mut ready = BTreeSet::new();
    ready.insert("a".into());
    ready.insert("b".into());
    assert!(process_dependencies_satisfied(&process, &spawned, &ready));
}

#[test]
fn dependencies_not_satisfied_for_runtime_ready_when_dep_only_spawned() {
    let mut process = test_process("c", vec!["a".into()]);
    process.readiness = ReadinessGate::RuntimeReady;
    let mut spawned = BTreeSet::new();
    spawned.insert("a".into());
    let ready = BTreeSet::new();
    // runtime_ready gate：依赖必须已通过 readiness，仅 spawned 不够。
    assert!(!process_dependencies_satisfied(&process, &spawned, &ready));
}

#[test]
fn dependencies_not_satisfied_when_missing() {
    let process = test_process("c", vec!["a".into(), "b".into()]);
    let mut spawned = BTreeSet::new();
    spawned.insert("a".into());
    let ready = BTreeSet::new();
    assert!(!process_dependencies_satisfied(&process, &spawned, &ready));
}
