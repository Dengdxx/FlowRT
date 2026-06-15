use super::*;

#[test]
fn auto_socket_discovery_removes_stale_unix_socket_candidates() {
    let _lock = XDG_RUNTIME_DIR_ENV_LOCK
        .lock()
        .expect("runtime dir env lock should not be poisoned");
    let root = temp_test_dir("e-stale-unix");
    let runtime_root = root.join("xdg-runtime");
    let _env = EnvOverride::set("XDG_RUNTIME_DIR", Some(runtime_root.as_os_str()));
    let socket_dir = flowrt::runtime_socket_dir();
    std::fs::create_dir_all(&socket_dir).unwrap();

    let stale_socket = socket_dir.join("999999.sock");
    {
        let listener = std::os::unix::net::UnixListener::bind(&stale_socket)
            .expect("test should create unix socket file");
        drop(listener);
    }

    let live_socket = socket_dir.join("1000.sock");
    let self_description_hash = "feedface".to_string();
    let server = flowrt::spawn_status_server_at(
        live_socket.clone(),
        flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 1000,
            started_at_unix_ms: 1,
            self_description_hash: self_description_hash.clone(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        },
        flowrt::IntrospectionState::new(),
    )
    .expect("status server should start");

    let selected = select_echo_socket(None, &self_description_hash).unwrap();

    assert_eq!(selected, live_socket);
    assert!(
        !stale_socket.exists(),
        "stale unix socket candidate should be removed"
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn explicit_discoverable_socket_removes_stale_unix_socket_and_reports_it() {
    let _lock = XDG_RUNTIME_DIR_ENV_LOCK
        .lock()
        .expect("runtime dir env lock should not be poisoned");
    let root = temp_test_dir("e-exp-stale");
    let runtime_root = root.join("xdg-runtime");
    let _env = EnvOverride::set("XDG_RUNTIME_DIR", Some(runtime_root.as_os_str()));
    let socket_dir = flowrt::runtime_socket_dir();
    std::fs::create_dir_all(&socket_dir).unwrap();

    let stale_socket = socket_dir.join("999999.sock");
    {
        let listener = std::os::unix::net::UnixListener::bind(&stale_socket)
            .expect("test should create unix socket file");
        drop(listener);
    }

    let error = select_echo_socket(Some(&stale_socket), "feedface")
        .expect_err("stale explicit socket should fail with cleanup hint");
    let message = error.to_string();

    assert!(message.contains("stale FlowRT runtime socket"));
    assert!(message.contains(&stale_socket.display().to_string()));
    assert!(
        !stale_socket.exists(),
        "stale explicit socket should be removed"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_selfdesc_explicit_socket_removes_stale_unix_socket_and_reports_it() {
    let _lock = XDG_RUNTIME_DIR_ENV_LOCK
        .lock()
        .expect("runtime dir env lock should not be poisoned");
    let root = temp_test_dir("e-live-exp-stale");
    let runtime_root = root.join("xdg-runtime");
    let _env = EnvOverride::set("XDG_RUNTIME_DIR", Some(runtime_root.as_os_str()));
    let socket_dir = flowrt::runtime_socket_dir();
    std::fs::create_dir_all(&socket_dir).unwrap();

    let stale_socket = socket_dir.join("999999.sock");
    {
        let listener = std::os::unix::net::UnixListener::bind(&stale_socket)
            .expect("test should create unix socket file");
        drop(listener);
    }

    let error = introspection::load_echo_context_from_live_socket(Some(&stale_socket))
        .expect_err("stale explicit socket should fail with cleanup hint");
    let message = error.to_string();

    assert!(message.contains("stale FlowRT runtime socket"));
    assert!(message.contains(&stale_socket.display().to_string()));
    assert!(
        !stale_socket.exists(),
        "stale explicit socket should be removed"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn params_commands_use_selfdesc_matched_runtime_socket() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "param_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "controller",
      "component": "controller",
      "process": "main",
      "runtime": "rust",
	      "params": [{
	        "name": "kp",
	        "type": "f32",
	        "update": "on_tick"
	      }, {
	        "name": "mode",
	        "type": "string",
	        "update": "startup"
	      }]
    }],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("params-cli");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 87,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "param_demo".to_string(),
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
    state.register_param(flowrt::IntrospectionParamSchema {
        name: "controller.mode".to_string(),
        ty: "string".to_string(),
        update: "startup".to_string(),
        current: serde_json::json!("safe"),
        min: None,
        max: None,
        choices: Vec::new(),
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let list = params_list(&selfdesc, Some(&socket)).unwrap();
    assert!(list.contains("controller.kp type=f32 update=on_tick current=1.0"));

    let get = params_get(&selfdesc, "controller.kp", Some(&socket)).unwrap();
    assert!(get.contains("pending=none"));
    assert!(get.contains("apply_state=applied"));
    assert!(get.contains("runtime_update=pending-on-tick"));

    let startup_get = params_get(&selfdesc, "controller.mode", Some(&socket)).unwrap();
    assert!(startup_get.contains("update=startup"));
    assert!(startup_get.contains("apply_state=startup-only"));
    assert!(startup_get.contains("runtime_update=startup-only"));

    let set = params_set(&selfdesc, "controller.kp", "2.5", Some(&socket)).unwrap();
    assert!(set.contains("current=1.0"));
    assert!(set.contains("pending=2.5"));
    assert!(set.contains("apply_state=pending"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn params_set_file_applies_json_object_entries() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "param_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "controller",
      "component": "controller",
      "process": "main",
      "runtime": "rust",
      "params": [{
        "name": "kp",
        "type": "f32",
        "update": "on_tick"
      }, {
        "name": "enabled",
        "type": "bool",
        "update": "on_tick"
      }]
    }],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("params-file-object");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let params_file = root.join("params.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(
        &params_file,
        r#"{"controller.kp":2.5,"controller.enabled":true}"#,
    )
    .unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "param_demo".to_string(),
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
    state.register_param(flowrt::IntrospectionParamSchema {
        name: "controller.enabled".to_string(),
        ty: "bool".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(false),
        min: None,
        max: None,
        choices: Vec::new(),
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("status server should start");

    let result =
        introspection::params_set_from_file(&selfdesc, &params_file, Some(&socket)).unwrap();

    assert!(!result.has_errors);
    assert!(
        result
            .output
            .contains("controller.kp: ok: controller.kp type=f32")
    );
    assert!(result.output.contains("pending=2.5"));
    assert!(
        result
            .output
            .contains("controller.enabled: ok: controller.enabled type=bool")
    );
    assert!(result.output.contains("pending=true"));
    assert!(result.output.contains("summary: ok=2 error=0"));
    assert_eq!(
        state.pending_param("controller.kp"),
        Some(serde_json::json!(2.5))
    );
    assert_eq!(
        state.pending_param("controller.enabled"),
        Some(serde_json::json!(true))
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn params_set_file_reports_partial_failures_for_json_array() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "param_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "controller",
      "component": "controller",
      "process": "main",
      "runtime": "rust",
      "params": [{
        "name": "kp",
        "type": "f32",
        "update": "on_tick"
      }, {
        "name": "mode",
        "type": "string",
        "update": "startup"
      }]
    }],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("params-file-array");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    let params_file = root.join("params.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(
        &params_file,
        r#"[{"name":"controller.kp","value":2.5},{"name":"controller.mode","value":"normal"},{"name":"controller.kp","value":3.5}]"#,
    )
    .unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 89,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "param_demo".to_string(),
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
    state.register_param(flowrt::IntrospectionParamSchema {
        name: "controller.mode".to_string(),
        ty: "string".to_string(),
        update: "startup".to_string(),
        current: serde_json::json!("safe"),
        min: None,
        max: None,
        choices: vec![serde_json::json!("safe"), serde_json::json!("normal")],
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("status server should start");

    let result =
        introspection::params_set_from_file(&selfdesc, &params_file, Some(&socket)).unwrap();

    assert!(result.has_errors);
    let lines: Vec<&str> = result.output.lines().collect();
    assert_eq!(lines.len(), 4);
    assert!(lines[0].starts_with("controller.kp: ok: "));
    assert!(
        lines[1]
            .contains("controller.mode: error: failed to set FlowRT parameter `controller.mode`")
    );
    assert!(lines[1].contains("startup-only"));
    assert!(lines[2].starts_with("controller.kp: ok: "));
    assert_eq!(lines[3], "summary: ok=2 error=1");
    assert_eq!(
        state.pending_param("controller.kp"),
        Some(serde_json::json!(3.5))
    );
    assert_eq!(state.pending_param("controller.mode"), None);

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_endpoint_alias_reports_ambiguous_channels() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "left_sink.imu",
      "message_type": "Imu"
    }, {
      "from": "source.imu",
      "to": "right_sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let self_description: SelfDescription = serde_json::from_str(source).unwrap();

    let error = find_echo_channel(&self_description, "source.imu").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("contains multiple channels named `source.imu`")
    );
}

#[test]
fn echo_reports_no_payload_when_snapshot_is_empty() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-no-payload");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 82,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output =
        echo_channel_snapshot_from_image(&selfdesc, "source.imu_to_sink.imu", Some(&socket))
            .unwrap();

    assert!(output.contains("payload_len=0"));
    assert!(output.contains("no payload"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_rejects_payload_length_that_does_not_match_message_abi() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-bad-payload-len");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 83,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![0x01, 0x02], None);
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let error =
        echo_channel_snapshot_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("payload length 2"));
    assert!(message.contains("Message ABI size 4"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_checks_explicit_socket_hash_before_snapshot_request() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-wrong-socket-hash");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 84,
        started_at_unix_ms: 1234,
        self_description_hash: "different_hash".to_string(),
        package: "other_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let error =
        echo_channel_snapshot_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("self-description hash `different_hash` does not match"));
    assert!(!message.contains("failed to request channel snapshot"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_reports_structured_live_channel_errors() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("echo-live-channel-error");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 85,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let error =
        echo_channel_snapshot_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("failed to read channel snapshot `source.imu_to_sink.imu`"));
    assert!(message.contains("unknown FlowRT channel"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

// ── 远程参数控制面测试 ──────────────────────────────────────────────────
