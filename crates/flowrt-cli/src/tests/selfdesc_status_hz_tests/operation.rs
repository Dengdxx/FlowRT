use super::*;

#[test]
fn self_description_summary_displays_operation_endpoints() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "op_demo" },
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "controller",
      "component": "controller_comp",
      "process": "main",
      "runtime": "rust"
    }, {
      "name": "navigator",
      "component": "navigator_comp",
      "process": "main",
      "runtime": "rust"
    }],
    "tasks": [],
    "channels": [],
    "operations": [{
      "name": "controller.plan",
      "canonical_id": "operation.default.controller.plan_to_navigator.plan",
      "client_instance": "controller",
      "client_port": "plan",
      "server_instance": "navigator",
      "server_port": "plan",
      "goal_type": "PlanGoal",
      "feedback_type": "PlanFeedback",
      "result_type": "PlanResult",
      "backend": "inproc",
      "timeout_ms": 5000,
      "concurrency": "reject",
      "preempt": "reject",
      "queue_depth": 4,
      "max_in_flight": 1,
      "feedback": "latest",
      "result_retention_ms": null,
      "lowering": {
        "start_service": "__flowrt_operation_controller_plan_start",
        "cancel_service": "__flowrt_operation_controller_plan_cancel",
        "status_service": "__flowrt_operation_controller_plan_status",
        "feedback_channel": "__flowrt_operation_controller_plan_feedback",
        "result_channel": "__flowrt_operation_controller_plan_result"
      }
    }]
  }],
  "component_types": [{
    "name": "controller_comp",
    "language": "rust",
    "kind": "native",
    "operation_clients": [{
      "name": "plan",
      "goal_type": "PlanGoal",
      "feedback_type": "PlanFeedback",
      "result_type": "PlanResult"
    }]
  }, {
    "name": "navigator_comp",
    "language": "rust",
    "kind": "native",
    "operation_servers": [{
      "name": "plan",
      "goal_type": "PlanGoal",
      "feedback_type": "PlanFeedback",
      "result_type": "PlanResult"
    }]
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("selfdesc-operations");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);

    assert!(list.contains("operations=1"));
    assert!(list.contains("operation controller.plan"));
    assert!(list.contains("client=controller.plan"));
    assert!(list.contains("server=navigator.plan"));
    assert!(list.contains("goal=PlanGoal"));
    assert!(list.contains("feedback=PlanFeedback"));
    assert!(list.contains("result=PlanResult"));
    assert!(list.contains("backend=inproc"));
    assert!(list.contains("operation_clients: plan:PlanGoal->PlanFeedback->PlanResult"));

    let json_output = operation_list_json(Some(&path), None).unwrap();
    let value: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    assert_eq!(value["response"], "operation_list");
    assert_eq!(value["package"], "op_demo");
    assert_eq!(value["operations"][0]["name"], "controller.plan");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_topology_summary_separates_iox2_service_name_and_zenoh_key_expr() {
    let iox2_source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "iox2_op_demo" },
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
      "backend": "iox2",
      "lowering": {
        "start_service": "FlowRT/service/__flowrt_operation_controller_plan_start",
        "start_key_expr": "",
        "cancel_service": "FlowRT/service/__flowrt_operation_controller_plan_cancel",
        "cancel_key_expr": "",
        "status_service": "FlowRT/service/__flowrt_operation_controller_plan_status",
        "status_key_expr": ""
      }
    }]
  }],
  "message_abi": []
}
"#;
    let iox2: flowrt_selfdesc::SelfDescription = serde_json::from_str(iox2_source).unwrap();
    let iox2_list = crate::introspection::operation_topology_summary(&iox2);

    assert!(iox2_list.contains("backend=iox2"));
    assert!(
        iox2_list.contains("start_service=FlowRT/service/__flowrt_operation_controller_plan_start")
    );
    assert!(!iox2_list.contains("start_key_expr="));

    let zenoh_source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "zenoh_op_demo" },
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
      "lowering": {
        "start_service": "",
        "start_key_expr": "flowrt/service/__flowrt_operation_controller_plan_start/request",
        "cancel_service": "",
        "cancel_key_expr": "flowrt/service/__flowrt_operation_controller_plan_cancel/request",
        "status_service": "",
        "status_key_expr": "flowrt/service/__flowrt_operation_controller_plan_status/request"
      }
    }]
  }],
  "message_abi": []
}
"#;
    let zenoh: flowrt_selfdesc::SelfDescription = serde_json::from_str(zenoh_source).unwrap();
    let zenoh_list = crate::introspection::operation_topology_summary(&zenoh);

    assert!(zenoh_list.contains("backend=zenoh"));
    assert!(zenoh_list.contains(
        "start_key_expr=flowrt/service/__flowrt_operation_controller_plan_start/request"
    ));
    assert!(!zenoh_list.contains("start_service=FlowRT/service/"));
}

#[test]
fn self_description_summary_handles_no_services() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "no_svc" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("selfdesc-no-services");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);

    assert!(list.contains("services=0"));
    assert!(!list.contains("service "));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_displays_operation_health() {
    let root = temp_test_dir("live-status-operation-health");
    let socket = root.join("main.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 77,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_operation_health(flowrt::IntrospectionOperationStatus {
        name: "controller.plan".to_string(),
        ready: true,
        running: 1,
        queued: 2,
        current_operation_ids: vec!["111:7:3".to_string()],
        total_started: 9,
        succeeded_count: 5,
        failed_count: 1,
        canceled_count: 0,
        timeout_count: 1,
        preempted_count: 0,
        current_state: Some("running".to_string()),
        current_owner: Some("controller.plan".to_string()),
        current_deadline_ms: Some(1500),
        last_event: Some("flowrt.operation.state_changed".to_string()),
        last_error: None,
        last_transition_ms: Some(12345),
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("operation=controller.plan"));
    assert!(output.contains("ready=true"));
    assert!(output.contains("running=1"));
    assert!(output.contains("queued=2"));
    assert!(output.contains("current_operation_ids=[111:7:3]"));
    assert!(output.contains("total_started=9"));
    assert!(output.contains("succeeded=5"));
    assert!(output.contains("timeout=1"));
    assert!(output.contains("state=running"));
    assert!(output.contains("owner=controller.plan"));
    assert!(output.contains("deadline_ms=1500"));
    assert!(output.contains("last_event=flowrt.operation.state_changed"));
    assert!(output.contains("last_transition_ms=12345"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_cli_status_and_cancel_use_runtime_socket() {
    let root = temp_test_dir("operation-cli");
    let socket = root.join("main.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_operation_health(flowrt::IntrospectionOperationStatus {
        name: "controller.plan".to_string(),
        ready: true,
        running: 1,
        queued: 0,
        current_operation_ids: vec!["111:7:3".to_string()],
        total_started: 1,
        succeeded_count: 0,
        failed_count: 0,
        canceled_count: 0,
        timeout_count: 0,
        preempted_count: 0,
        current_state: Some("cancel_requested".to_string()),
        current_owner: Some("controller.plan".to_string()),
        current_deadline_ms: Some(1500),
        last_event: Some("flowrt.operation.state_changed".to_string()),
        last_error: None,
        last_transition_ms: Some(12345),
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let status = operation_status_summary_for_sockets(None, vec![socket.clone()]).unwrap();
    assert!(status.contains("operation=controller.plan"));
    assert!(status.contains("current_operation_ids=[111:7:3]"));

    let status_by_id =
        operation_status_summary_for_sockets(Some("111:7:3"), vec![socket.clone()]).unwrap();
    assert!(status_by_id.contains("operation_id=111:7:3"));
    assert!(status_by_id.contains("operation=controller.plan"));
    assert!(status_by_id.contains("state=cancel_requested"));

    let status_json = operation_status_json(Some(&socket), Some("111:7:3")).unwrap();
    let status_value: serde_json::Value = serde_json::from_str(&status_json).unwrap();
    assert_eq!(status_value["response"], "operation_status");
    assert_eq!(status_value["operation_id"], "111:7:3");
    assert_eq!(
        status_value["entries"][0]["operation"]["name"],
        "controller.plan"
    );

    let canceled = operation_cancel("111:7:3", Some(&socket)).unwrap();
    assert!(canceled.contains("operation=controller.plan"));
    assert!(canceled.contains("operation_id=111:7:3"));
    assert!(canceled.contains("state=cancel_requested"));
    assert!(canceled.contains("canceled=0"));

    let cancel_json = operation_cancel_json("111:7:3", Some(&socket)).unwrap();
    let cancel_value: serde_json::Value = serde_json::from_str(&cancel_json).unwrap();
    assert_eq!(cancel_value["response"], "operation_value");
    assert_eq!(cancel_value["operation_id"], "111:7:3");
    assert_eq!(cancel_value["operation"]["name"], "controller.plan");

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_start_encodes_goal_json_and_returns_operation_id() {
    let root = temp_test_dir("operation-cli-start");
    let path = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let source = r#"
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
      "backend": "inproc",
      "timeout_ms": 5000,
      "concurrency": "reject",
      "preempt": "reject",
      "queue_depth": 4,
      "max_in_flight": 1,
      "feedback": "latest",
      "result_retention_ms": null
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
  }]
}
"#;
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
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
            operation: flowrt::IntrospectionOperationStatus {
                name: "controller.plan".to_string(),
                ready: true,
                running: 1,
                queued: 0,
                current_operation_ids: vec!["111:7:3".to_string()],
                total_started: 1,
                succeeded_count: 0,
                failed_count: 0,
                canceled_count: 0,
                timeout_count: 0,
                preempted_count: 0,
                current_state: Some("starting".to_string()),
                current_owner: Some("flowrt.cli".to_string()),
                current_deadline_ms: Some(2500),
                last_event: Some("flowrt.operation.state_changed".to_string()),
                last_error: None,
                last_transition_ms: Some(12345),
            },
        })
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = operation_start(
        &path,
        "controller.plan",
        r#"{"target":7}"#,
        Some(&socket),
        Some(2500),
    )
    .unwrap();

    assert!(output.contains("operation_id=111:7:3"));
    assert!(output.contains("operation=controller.plan"));
    assert!(output.contains("state=starting"));

    let json_output = operation_start_json(
        &path,
        "controller.plan",
        r#"{"target":7}"#,
        Some(&socket),
        Some(2500),
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&json_output).unwrap();
    assert_eq!(value["response"], "operation_started");
    assert_eq!(value["operation_id"], "111:7:3");
    assert_eq!(value["operation"]["name"], "controller.plan");

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_result_decodes_payload_json() {
    let root = temp_test_dir("operation-cli-result");
    let path = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let source = r#"
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
      "backend": "inproc",
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
"#;
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_operation_result_payload(
        "controller.plan",
        "111:7:3",
        "succeeded",
        None,
        Some(vec![1]),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = operation_result(&path, "111:7:3", Some(&socket), false, None, 5000).unwrap();

    assert!(output.contains("operation_id=111:7:3"));
    assert!(output.contains("state=succeeded"));
    assert!(output.contains("result={\"accepted\":true}"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_result_json_decodes_payload_value() {
    let root = temp_test_dir("operation-cli-result-json");
    let path = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let source = r#"
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
      "backend": "inproc",
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
"#;
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_operation_result_payload(
        "controller.plan",
        "111:7:3",
        "succeeded",
        None,
        Some(vec![1]),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = operation_result_json(&path, "111:7:3", Some(&socket), false, None, 5000).unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(value["response"], "operation_result");
    assert_eq!(value["result"]["operation_id"], "111:7:3");
    assert_eq!(value["result"]["state"], "succeeded");
    assert_eq!(value["result"]["payload"], serde_json::json!([1]));
    assert_eq!(
        value["result"]["value"],
        serde_json::json!({"accepted": true})
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_follow_decodes_progress_and_result_json() {
    let root = temp_test_dir("operation-cli-follow");
    let path = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let source = r#"
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
      "backend": "inproc",
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
"#;
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_operation_transition(
        "controller.plan",
        "111:7:3",
        "running",
        Some("controller.plan"),
        Some(1500),
    );
    state.record_operation_progress_payload(
        "controller.plan",
        "111:7:3",
        0,
        Some(vec![7, 0, 0, 0]),
    );
    state.record_operation_result_payload(
        "controller.plan",
        "111:7:3",
        "succeeded",
        None,
        Some(vec![1]),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = operation_follow(&path, "111:7:3", Some(&socket), false, None, 5000).unwrap();

    assert!(output.contains("operation_id=111:7:3 state=running"));
    assert!(output.contains("operation_id=111:7:3 progress_sequence=0 progress={\"progress\":7}"));
    assert!(output.contains("operation_id=111:7:3 state=succeeded result={\"accepted\":true}"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_follow_json_decodes_progress_and_result_values() {
    let root = temp_test_dir("operation-cli-follow-json");
    let path = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let source = r#"
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
      "backend": "inproc",
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
"#;
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_operation_transition(
        "controller.plan",
        "111:7:3",
        "running",
        Some("controller.plan"),
        Some(1500),
    );
    state.record_operation_progress_payload(
        "controller.plan",
        "111:7:3",
        0,
        Some(vec![7, 0, 0, 0]),
    );
    state.record_operation_result_payload(
        "controller.plan",
        "111:7:3",
        "succeeded",
        None,
        Some(vec![1]),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = operation_follow_json(&path, "111:7:3", Some(&socket), false, None, 5000).unwrap();
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

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

fn operation_variable_frame_source() -> &'static str {
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
      "backend": "inproc",
      "timeout_ms": 5000,
      "concurrency": "reject",
      "preempt": "reject",
      "queue_depth": 4,
      "max_in_flight": 1,
      "feedback": "latest",
      "result_retention_ms": 10000
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "PlanFeedback",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 8,
    "max_size_bytes": 64,
    "variable": true,
    "fields": [{
      "name": "label",
      "type": "string<max=32>",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": 32
    }]
  }, {
    "type_name": "PlanResult",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 16,
    "max_size_bytes": 64,
    "variable": true,
    "fields": [{
      "name": "label",
      "type": "string<max=32>",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": 32
    }, {
      "name": "samples",
      "type": "sequence<u32,max=8>",
      "header_offset_bytes": 8,
      "header_size_bytes": 8,
      "tail_max_bytes": 32
    }]
  }]
}
"#
}

fn frame_with_label(label: &str) -> Vec<u8> {
    let mut header = vec![0u8; 8];
    let mut tail = Vec::new();
    let label_span = flowrt::append_tail_block(&mut tail, label.as_bytes()).unwrap();
    label_span.encode(&mut header[..8]).unwrap();
    header.extend_from_slice(&tail);
    header
}

fn frame_with_label_and_samples(label: &str, samples: &[u32]) -> Vec<u8> {
    let mut header = vec![0u8; 16];
    let mut tail = Vec::new();
    let label_span = flowrt::append_tail_block(&mut tail, label.as_bytes()).unwrap();
    label_span.encode(&mut header[..8]).unwrap();

    let mut sample_bytes = Vec::new();
    for sample in samples {
        sample_bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let samples_span = flowrt::append_tail_block(&mut tail, &sample_bytes).unwrap();
    samples_span.encode(&mut header[8..16]).unwrap();

    header.extend_from_slice(&tail);
    header
}

#[test]
fn operation_result_json_decodes_variable_frame_payload_value() {
    let root = temp_test_dir("operation-cli-variable-result-json");
    let path = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let source = operation_variable_frame_source();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_operation_result_payload(
        "controller.plan",
        "111:7:3",
        "succeeded",
        None,
        Some(frame_with_label_and_samples("done", &[10, 20])),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = operation_result_json(&path, "111:7:3", Some(&socket), false, None, 5000).unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(value["response"], "operation_result");
    assert_eq!(
        value["result"]["value"],
        serde_json::json!({"label": "done", "samples": [10, 20]})
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_start_encodes_bounded_variable_frame_goal_json() {
    let root = temp_test_dir("operation-cli-bounded-variable-start");
    let path = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let source = r#"
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
      "backend": "iox2",
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
  }],
  "message_frames": [{
    "type_name": "PlanGoal",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 8,
    "max_size_bytes": 16,
    "variable": true,
    "fields": [{
      "name": "target",
      "type": "string<max=8>",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": 8
    }]
  }]
}
"#;
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_operation_start_handler("controller.plan", |payload, timeout_ms, owner| {
        assert_eq!(payload, frame_with_label("nav"));
        assert_eq!(timeout_ms, Some(2500));
        assert_eq!(owner.as_deref(), Some("flowrt.cli"));
        Ok(flowrt::IntrospectionOperationStartStatus {
            operation_id: "111:7:3".to_string(),
            operation: flowrt::IntrospectionOperationStatus {
                name: "controller.plan".to_string(),
                ready: true,
                running: 1,
                queued: 0,
                current_operation_ids: vec!["111:7:3".to_string()],
                total_started: 1,
                succeeded_count: 0,
                failed_count: 0,
                canceled_count: 0,
                timeout_count: 0,
                preempted_count: 0,
                current_state: Some("starting".to_string()),
                current_owner: Some("flowrt.cli".to_string()),
                current_deadline_ms: Some(2500),
                last_event: Some("flowrt.operation.state_changed".to_string()),
                last_error: None,
                last_transition_ms: Some(12345),
            },
        })
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = operation_start_json(
        &path,
        "controller.plan",
        r#"{"target":"nav"}"#,
        Some(&socket),
        Some(2500),
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(value["response"], "operation_started");
    assert_eq!(value["operation_id"], "111:7:3");

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_follow_json_decodes_variable_frame_progress_and_result_values() {
    let root = temp_test_dir("operation-cli-variable-follow-json");
    let path = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let source = operation_variable_frame_source();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_operation_transition(
        "controller.plan",
        "111:7:3",
        "running",
        Some("controller.plan"),
        Some(1500),
    );
    state.record_operation_progress_payload(
        "controller.plan",
        "111:7:3",
        0,
        Some(frame_with_label("half")),
    );
    state.record_operation_result_payload(
        "controller.plan",
        "111:7:3",
        "succeeded",
        None,
        Some(frame_with_label_and_samples("done", &[10, 20])),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = operation_follow_json(&path, "111:7:3", Some(&socket), false, None, 5000).unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();

    assert_eq!(value["response"], "operation_events");
    assert_eq!(
        value["events"][1]["value"],
        serde_json::json!({"label": "half"})
    );
    assert_eq!(
        value["events"][2]["value"],
        serde_json::json!({"label": "done", "samples": [10, 20]})
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn operation_cancel_without_socket_refuses_ambiguous_id_without_side_effects() {
    let root = temp_test_dir("operation-cli-ambiguous-cancel");
    let socket_a = root.join("main-a.sock");
    let socket_b = root.join("main-b.sock");
    let handshake_a = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main_a".to_string(),
        runtime: "rust".to_string(),
    };
    let handshake_b = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 89,
        started_at_unix_ms: 1235,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main_b".to_string(),
        runtime: "rust".to_string(),
    };
    let state_a = flowrt::IntrospectionState::new();
    let state_b = flowrt::IntrospectionState::new();
    for state in [&state_a, &state_b] {
        state.record_operation_health(flowrt::IntrospectionOperationStatus {
            name: "controller.plan".to_string(),
            ready: true,
            running: 1,
            queued: 0,
            current_operation_ids: vec!["111:7:3".to_string()],
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
            last_transition_ms: Some(12345),
        });
    }
    let server_a = flowrt::spawn_status_server_at(socket_a.clone(), handshake_a, state_a)
        .expect("first status server should start");
    let server_b = flowrt::spawn_status_server_at(socket_b.clone(), handshake_b, state_b)
        .expect("second status server should start");

    let error = operation_cancel_for_sockets("111:7:3", vec![socket_a.clone(), socket_b.clone()])
        .expect_err("ambiguous operation id must require --socket");
    assert!(error.to_string().contains("multiple live FlowRT processes"));

    let status_a = operation_status_summary_for_sockets(None, vec![socket_a.clone()]).unwrap();
    let status_b = operation_status_summary_for_sockets(None, vec![socket_b.clone()]).unwrap();
    assert!(status_a.contains("running=1"));
    assert!(status_a.contains("canceled=0"));
    assert!(status_a.contains("current_operation_ids=[111:7:3]"));
    assert!(status_b.contains("running=1"));
    assert!(status_b.contains("canceled=0"));
    assert!(status_b.contains("current_operation_ids=[111:7:3]"));

    drop(server_a);
    drop(server_b);
    let _ = std::fs::remove_dir_all(&root);
}
