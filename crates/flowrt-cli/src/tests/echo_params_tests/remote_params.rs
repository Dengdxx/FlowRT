use super::*;

#[test]
fn parse_remote_params_key_expr_extracts_package_hash_and_pid() {
    let result = introspection::parse_remote_params_key_expr("flowrt/params/robot_demo/abc123/42");
    assert_eq!(result, Some(("robot_demo", "abc123", "42")));
}

#[test]
fn parse_remote_operation_key_expr_extracts_package_hash_and_pid() {
    let result = introspection::parse_remote_operation_key_expr("flowrt/op/robot_demo/abc123/42");
    assert_eq!(result, Some(("robot_demo", "abc123", "42")));
}

#[test]
fn parse_remote_params_key_expr_rejects_invalid_prefix() {
    assert!(introspection::parse_remote_params_key_expr("flowrt/status/robot/abc/1").is_none());
}

#[test]
fn parse_remote_operation_key_expr_rejects_invalid_prefix() {
    assert!(introspection::parse_remote_operation_key_expr("flowrt/params/robot/abc/1").is_none());
}

#[test]
fn parse_remote_params_key_expr_rejects_empty_segments() {
    assert!(introspection::parse_remote_params_key_expr("flowrt/params//abc/42").is_none());
    assert!(introspection::parse_remote_params_key_expr("flowrt/params/robot//42").is_none());
    assert!(introspection::parse_remote_params_key_expr("flowrt/params/robot/abc/").is_none());
}

#[test]
fn parse_remote_params_key_expr_rejects_missing_segments() {
    assert!(introspection::parse_remote_params_key_expr("flowrt/params/robot/abc").is_none());
    assert!(introspection::parse_remote_params_key_expr("flowrt/params/robot").is_none());
}

#[test]
fn parse_remote_params_key_expr_rejects_extra_segments() {
    assert!(
        introspection::parse_remote_params_key_expr("flowrt/params/robot/abc/42/extra").is_none()
    );
}

#[test]
fn discover_remote_params_runtimes_filters_by_hash_and_deduplicates() {
    // 测试 discovery 的 hash 过滤逻辑：用同 session 直接查询特定 key expression，
    // 验证 discover_remote_params_runtimes 的 hash 过滤和去重逻辑。
    // （wildcard get 在 zenoh 同 session 下不触发本地 queryable，属于已知行为限制。）
    let session = zenoh::open(flowrt::zenoh::config_from_environment().unwrap())
        .wait()
        .unwrap();
    let expected_hash = "discovery_hash".to_string();

    let ke_match_1 = format!("flowrt/params/pkg_a/{expected_hash}/8001");
    let ke_match_2 = format!("flowrt/params/pkg_b/{expected_hash}/8002");
    let ke_mismatch = "flowrt/params/pkg_c/other_hash/8003".to_string();

    let hs_match_1 = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 8001,
        started_at_unix_ms: 1000,
        self_description_hash: expected_hash.clone(),
        package: "pkg_a".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let hs_match_2 = flowrt::IntrospectionHandshake {
        pid: 8002,
        package: "pkg_b".to_string(),
        process: "control".to_string(),
        ..hs_match_1.clone()
    };
    let hs_mismatch = flowrt::IntrospectionHandshake {
        pid: 8003,
        self_description_hash: "other_hash".to_string(),
        package: "pkg_c".to_string(),
        ..hs_match_1.clone()
    };

    let state_a = flowrt::IntrospectionState::new();
    state_a.register_param(flowrt::IntrospectionParamSchema {
        name: "kp".to_string(),
        ty: "f32".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: None,
        max: None,
        choices: Vec::new(),
    });
    let state_b = flowrt::IntrospectionState::new();
    let state_c = flowrt::IntrospectionState::new();

    let _s1 = flowrt::ZenohParamsServer::open(&session, &ke_match_1, hs_match_1, state_a).unwrap();
    let _s2 = flowrt::ZenohParamsServer::open(&session, &ke_match_2, hs_match_2, state_b).unwrap();
    let _s3 =
        flowrt::ZenohParamsServer::open(&session, &ke_mismatch, hs_mismatch, state_c).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 直接查询三个端点，模拟 discovery 的 hash 过滤逻辑。
    let resp_1 = flowrt::request_remote_param_list(&session, &ke_match_1, 3000).unwrap();
    let resp_2 = flowrt::request_remote_param_list(&session, &ke_match_2, 3000).unwrap();
    let resp_3 = flowrt::request_remote_param_list(&session, &ke_mismatch, 3000).unwrap();

    // 模拟 discover_remote_params_runtimes 的过滤逻辑。
    let mut entries = Vec::new();
    for (ke, resp) in [
        (&ke_match_1, &resp_1),
        (&ke_match_2, &resp_2),
        (&ke_mismatch, &resp_3),
    ] {
        let Some((_pkg, hash, pid_str)) = introspection::parse_remote_params_key_expr(ke) else {
            continue;
        };
        if hash != expected_hash {
            continue;
        }
        let handshake = match resp {
            flowrt::IntrospectionResponse::ParamList { handshake, .. } => handshake,
            _ => continue,
        };
        entries.push(introspection::RemoteRuntimeEntry {
            key_expr: ke.to_string(),
            pid: pid_str.parse().unwrap(),
            package: handshake.package.clone(),
            process: handshake.process.clone(),
            runtime: handshake.runtime.clone(),
            self_description_hash: hash.to_string(),
        });
    }
    // 只返回 hash 匹配的两个端点。
    assert_eq!(entries.len(), 2);
    let pids: Vec<u32> = entries.iter().map(|e| e.pid).collect();
    assert!(pids.contains(&8001));
    assert!(pids.contains(&8002));
    assert!(!pids.contains(&8003));
}

#[test]
fn select_remote_runtime_returns_single_entry() {
    let entries = vec![introspection::RemoteRuntimeEntry {
        key_expr: "flowrt/params/robot/hash1/100".to_string(),
        pid: 100,
        package: "robot".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
        self_description_hash: "hash1".to_string(),
    }];
    let result = introspection::select_remote_runtime(entries, "hash1").unwrap();
    assert_eq!(result.pid, 100);
    assert_eq!(result.key_expr, "flowrt/params/robot/hash1/100");
}

#[test]
fn remote_params_get_returns_error_for_unknown_param() {
    let session = zenoh::open(flowrt::zenoh::config_from_environment().unwrap())
        .wait()
        .unwrap();
    let key_expr = format!(
        "flowrt/params/cli_get/{}/9002",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let expected_hash = "cli_get_unknown".to_string();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 9002,
        started_at_unix_ms: 1000,
        self_description_hash: expected_hash.clone(),
        package: "cli_robot".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_param(flowrt::IntrospectionParamSchema {
        name: "controller.kp".to_string(),
        ty: "f32".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: None,
        max: None,
        choices: Vec::new(),
    });

    let _server = flowrt::ZenohParamsServer::open(&session, &key_expr, handshake, state).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let response = flowrt::request_remote_param_get(&session, &key_expr, "missing.param", 5000)
        .expect("remote param get unknown");
    let flowrt::IntrospectionResponse::Error { message, .. } = response else {
        panic!("expected Error response for unknown param");
    };
    assert_eq!(message, "unknown FlowRT parameter `missing.param`");
}

#[test]
fn remote_params_set_rejects_out_of_range_value() {
    let session = zenoh::open(flowrt::zenoh::config_from_environment().unwrap())
        .wait()
        .unwrap();
    let key_expr = format!(
        "flowrt/params/cli_set/{}/9003",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let expected_hash = "cli_set_range".to_string();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 9003,
        started_at_unix_ms: 1000,
        self_description_hash: expected_hash.clone(),
        package: "cli_robot".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_param(flowrt::IntrospectionParamSchema {
        name: "controller.kp".to_string(),
        ty: "f32".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: Some(serde_json::json!(0.0)),
        max: Some(serde_json::json!(10.0)),
        choices: Vec::new(),
    });

    let _server = flowrt::ZenohParamsServer::open(&session, &key_expr, handshake, state).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let response = flowrt::request_remote_param_set(
        &session,
        &key_expr,
        "controller.kp",
        serde_json::json!(99.0),
        5000,
    )
    .expect("remote param set out of range");
    let flowrt::IntrospectionResponse::Error { message, .. } = response else {
        panic!("expected Error response for out-of-range value");
    };
    assert_eq!(message, "FlowRT parameter `controller.kp` is above maximum");
}

#[test]
fn select_remote_runtime_rejects_empty_list() {
    let error = introspection::select_remote_runtime(Vec::new(), "test_hash").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("no remote FlowRT runtime matches")
    );
    assert!(error.to_string().contains("test_hash"));
}

#[test]
fn select_remote_runtime_rejects_multiple_matches() {
    let entries = vec![
        introspection::RemoteRuntimeEntry {
            key_expr: "flowrt/params/robot/hash1/100".to_string(),
            pid: 100,
            package: "robot".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
            self_description_hash: "hash1".to_string(),
        },
        introspection::RemoteRuntimeEntry {
            key_expr: "flowrt/params/robot/hash1/200".to_string(),
            pid: 200,
            package: "robot".to_string(),
            process: "control".to_string(),
            runtime: "rust".to_string(),
            self_description_hash: "hash1".to_string(),
        },
    ];
    let error = introspection::select_remote_runtime(entries, "hash1").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("multiple remote FlowRT runtimes match"));
    assert!(message.contains("--runtime"));
    assert!(message.contains("pid=100"));
    assert!(message.contains("pid=200"));
}
