use super::*;

fn operation_self_description() -> String {
    r#"
{
  "self_description_version": "0.1",
  "source_hash": "feedface",
  "package": { "name": "robot_demo" },
  "graphs": [{
    "name": "default",
    "operations": [{
      "name": "controller.plan",
      "client_instance": "controller",
      "client_port": "plan",
      "server_instance": "navigator",
      "server_port": "plan",
      "goal_type": "PlanGoal",
      "feedback_type": "PlanFeedback",
      "result_type": "PlanResult",
      "backend": "zenoh",
      "timeout_ms": 5000,
      "concurrency": "reject",
      "preempt": "reject",
      "queue_depth": 4,
      "max_in_flight": 1,
      "feedback": "latest",
      "result_retention_ms": 10000
    }]
  }],
  "message_abi": [{
    "type_name": "PlanGoal",
    "size_bytes": 4,
    "align_bytes": 4,
    "fields": [{
      "name": "target",
      "type": "u32",
      "offset_bytes": 0,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }, {
    "type_name": "PlanFeedback",
    "size_bytes": 4,
    "align_bytes": 4,
    "fields": [{
      "name": "progress",
      "type": "u32",
      "offset_bytes": 0,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }, {
    "type_name": "PlanResult",
    "size_bytes": 1,
    "align_bytes": 1,
    "fields": [{
      "name": "accepted",
      "type": "bool",
      "offset_bytes": 0,
      "size_bytes": 1,
      "align_bytes": 1
    }]
  }]
}
"#
    .trim()
    .to_string()
}

fn remote_operation_status(state: &str) -> flowrt::IntrospectionOperationStatus {
    flowrt::IntrospectionOperationStatus {
        name: "controller.plan".to_string(),
        ready: true,
        running: u64::from(state == "running"),
        queued: 0,
        current_operation_ids: vec!["111:7:3".to_string()],
        total_started: 1,
        succeeded_count: u64::from(state == "succeeded"),
        failed_count: 0,
        canceled_count: 0,
        timeout_count: 0,
        preempted_count: 0,
        current_state: Some(state.to_string()),
        current_owner: Some("flowrt.cli".to_string()),
        current_deadline_ms: Some(2500),
        last_event: Some("flowrt.operation.state_changed".to_string()),
        last_error: None,
        last_transition_ms: Some(12345),
    }
}

fn unused_loopback_locator() -> String {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("test should reserve a local TCP port");
    let port = listener
        .local_addr()
        .expect("test listener should expose a local address")
        .port();
    drop(listener);
    format!("tcp/127.0.0.1:{port}")
}

fn zenoh_peer_config(listen: Option<&str>, connect: Option<&str>) -> zenoh::Config {
    let mut config = zenoh::Config::default();
    config.insert_json5("mode", "\"peer\"").unwrap();
    config
        .insert_json5("scouting/multicast/enabled", "false")
        .unwrap();
    if let Some(listen) = listen {
        config
            .insert_json5(
                "listen/endpoints",
                &serde_json::to_string(&[listen]).unwrap(),
            )
            .unwrap();
    }
    if let Some(connect) = connect {
        config
            .insert_json5(
                "connect/endpoints",
                &serde_json::to_string(&[connect]).unwrap(),
            )
            .unwrap();
    }
    config
}

fn open_remote_operation_server(
    source: &str,
    retain_result: bool,
) -> (
    std::path::PathBuf,
    zenoh::Session,
    zenoh::Session,
    flowrt::ZenohOperationServer,
    String,
) {
    let root = temp_test_dir("remote-operation-cli");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let hash = self_description_hash(source.as_bytes());
    let locator = unused_loopback_locator();
    let server_session = zenoh::open(zenoh_peer_config(Some(&locator), None))
        .wait()
        .unwrap();
    let key_expr = flowrt::operation_key_expr("robot_demo", &hash, std::process::id());
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: std::process::id(),
        started_at_unix_ms: 1234,
        self_description_hash: hash,
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_operation_start_handler("controller.plan", |payload, timeout_ms, owner| {
        assert_eq!(payload, vec![7, 0, 0, 0]);
        assert_eq!(timeout_ms, Some(2500));
        assert_eq!(owner.as_deref(), Some("flowrt.cli"));
        Ok(flowrt::IntrospectionOperationStartStatus {
            operation_id: "111:7:3".to_string(),
            operation: remote_operation_status("starting"),
        })
    });
    state.record_operation_health(remote_operation_status("running"));
    state.record_operation_transition(
        "controller.plan",
        "111:7:3",
        "running",
        Some("flowrt.cli"),
        Some(2500),
    );
    state.record_operation_progress_payload(
        "controller.plan",
        "111:7:3",
        0,
        Some(vec![7, 0, 0, 0]),
    );
    if retain_result {
        state.record_operation_result_payload(
            "controller.plan",
            "111:7:3",
            "succeeded",
            None,
            Some(vec![1]),
        );
    }
    let server =
        flowrt::ZenohOperationServer::open(&server_session, &key_expr, handshake, state).unwrap();
    let client_session = zenoh::open(zenoh_peer_config(None, Some(&locator)))
        .wait()
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));

    (path, server_session, client_session, server, key_expr)
}

fn expected_runtime_value(source: &str, key_expr: &str) -> serde_json::Value {
    serde_json::Value::String(format!(
        "pid={} package=robot_demo process=main runtime=rust selfdesc={} key={key_expr}",
        std::process::id(),
        self_description_hash(source.as_bytes())
    ))
}

#[test]
fn remote_operation_start_status_cancel_json_use_zenoh_control_plane() {
    let source = operation_self_description();
    let (path, _server_session, client_session, _server, key_expr) =
        open_remote_operation_server(&source, false);

    let started = introspection::remote_operation_start_json_with_session(
        &client_session,
        &path,
        "controller.plan",
        r#"{"target":7}"#,
        Some(&key_expr),
        Some(2500),
    )
    .unwrap();
    let started: serde_json::Value = serde_json::from_str(&started).unwrap();
    assert_eq!(started["response"], "operation_started");
    assert_eq!(started["operation_id"], "111:7:3");
    assert_eq!(started["operation"]["name"], "controller.plan");
    assert_eq!(started["operation"]["current_state"], "starting");
    assert_eq!(
        started["runtime"],
        expected_runtime_value(&source, &key_expr)
    );

    let hash = self_description_hash(source.as_bytes());
    let status = introspection::remote_operation_status_json_with_session(
        &client_session,
        &hash,
        "111:7:3",
        Some(&key_expr),
        5000,
    )
    .unwrap();
    let status: serde_json::Value = serde_json::from_str(&status).unwrap();
    assert_eq!(status["response"], "operation_value");
    assert_eq!(status["operation_id"], "111:7:3");
    assert_eq!(status["operation"]["current_state"], "running");

    let canceled = introspection::remote_operation_cancel_json_with_session(
        &client_session,
        &hash,
        "111:7:3",
        Some(&key_expr),
        5000,
    )
    .unwrap();
    let canceled: serde_json::Value = serde_json::from_str(&canceled).unwrap();
    assert_eq!(canceled["response"], "operation_value");
    assert_eq!(canceled["operation_id"], "111:7:3");
    assert_eq!(canceled["operation"]["current_state"], "cancel_requested");
}

#[test]
fn remote_operation_result_json_decodes_payload_value() {
    let source = operation_self_description();
    let (path, _server_session, client_session, _server, key_expr) =
        open_remote_operation_server(&source, true);
    let self_description = load_self_description(&path).unwrap();
    let hash = self_description_hash(source.as_bytes());

    let output = introspection::remote_operation_result_json_with_session(
        &client_session,
        &self_description,
        &hash,
        "111:7:3",
        Some(&key_expr),
        5000,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(value["response"], "operation_result");
    assert_eq!(value["result"]["operation_id"], "111:7:3");
    assert_eq!(value["result"]["state"], "succeeded");
    assert_eq!(value["result"]["payload"], serde_json::json!([1]));
    assert_eq!(
        value["result"]["value"],
        serde_json::json!({"accepted": true})
    );
    assert_eq!(value["runtime"], expected_runtime_value(&source, &key_expr));
}

#[test]
fn remote_operation_follow_json_decodes_progress_and_result_values() {
    let source = operation_self_description();
    let (path, _server_session, client_session, _server, key_expr) =
        open_remote_operation_server(&source, true);
    let self_description = load_self_description(&path).unwrap();
    let hash = self_description_hash(source.as_bytes());

    let output = introspection::remote_operation_follow_json_with_session(
        &client_session,
        &self_description,
        &hash,
        "111:7:3",
        Some(&key_expr),
        5000,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(value["response"], "operation_events");
    assert_eq!(value["operation_id"], "111:7:3");
    assert_eq!(value["terminal"], true);
    assert_eq!(value["events"][0]["kind"], "state");
    assert_eq!(value["events"][1]["kind"], "progress");
    assert_eq!(
        value["events"][1]["value"],
        serde_json::json!({"progress": 7})
    );
    assert_eq!(value["events"][2]["kind"], "result");
    assert_eq!(
        value["events"][2]["value"],
        serde_json::json!({"accepted": true})
    );
    assert_eq!(value["runtime"], expected_runtime_value(&source, &key_expr));
}
