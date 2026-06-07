use super::*;
use zenoh::Wait;

#[test]
fn echo_reads_channel_snapshot_from_fake_status_server() {
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
    let root = temp_test_dir("echo-snapshot");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 81,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes(
        "source.imu_to_sink.imu",
        "Imu",
        vec![0x01, 0x02, 0x0a, 0xff],
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "source.imu", Some(&socket)).unwrap();

    assert!(output.contains("channel=source.imu_to_sink.imu"));
    assert!(output.contains("type=Imu"));
    assert!(output.contains("published_count=1"));
    assert!(output.contains("published_at_ms=123"));
    assert!(output.contains("payload_len=4"));
    assert!(output.contains("raw=01020aff"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_formats_fixed_abi_fields_from_self_description_layout() {
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
      "from": "source.count",
      "to": "sink.count",
      "message_type": "Count"
    }]
  }],
  "message_abi": [{
    "type_name": "Count",
    "size_bytes": 4,
    "align_bytes": 4,
    "fields": [{
      "name": "value",
      "type": "u32",
      "offset_bytes": 0,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-format-fields");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

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
    state.record_channel_publish_bytes(
        "source.count_to_sink.count",
        "Count",
        vec![0x01, 0x02, 0x03, 0x04],
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "source.count", Some(&socket)).unwrap();

    assert!(output.contains("fields={value=67305985}"));
    assert!(output.contains("raw=01020304"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_formats_variable_frame_fields_from_self_description_layout() {
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
      "from": "source.packet",
      "to": "sink.packet",
      "message_type": "Packet"
    }]
  }],
  "message_abi": [],
    "message_frames": [{
    "type_name": "Packet",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 17,
    "max_size_bytes": null,
    "variable": true,
    "fields": [{
      "name": "valid",
      "type": "bool",
      "header_offset_bytes": 0,
      "header_size_bytes": 1,
      "tail_max_bytes": null
    }, {
      "name": "label",
      "type": "string",
      "header_offset_bytes": 1,
      "header_size_bytes": 8,
      "tail_max_bytes": null
    }, {
      "name": "samples",
      "type": "sequence<u32>",
      "header_offset_bytes": 9,
      "header_size_bytes": 8,
      "tail_max_bytes": null
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-format-frame");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 89,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let mut payload = Vec::new();
    payload.push(1);
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&2u32.to_le_bytes());
    payload.extend_from_slice(&2u32.to_le_bytes());
    payload.extend_from_slice(&8u32.to_le_bytes());
    payload.extend_from_slice(b"ok");
    payload.extend_from_slice(&10u32.to_le_bytes());
    payload.extend_from_slice(&20u32.to_le_bytes());

    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes(
        "source.packet_to_sink.packet",
        "Packet",
        payload,
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "source.packet", Some(&socket)).unwrap();

    assert!(output.contains("fields={valid=true,label=\"ok\",samples=[10,20]}"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_online_loads_self_description_and_enables_probe() {
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
      "from": "source.count",
      "to": "sink.count",
      "message_type": "Count"
    }]
  }],
  "message_abi": [{
    "type_name": "Count",
    "size_bytes": 4,
    "align_bytes": 4,
    "fields": [{
      "name": "value",
      "type": "u32",
      "offset_bytes": 0,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-online-selfdesc");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 90,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.set_self_description_json(source);
    state.register_channel("source.count_to_sink.count", "Count");
    assert!(
        !state
            .try_probe_channel_publish_bytes(
                "source.count_to_sink.count",
                "Count",
                &[0, 0, 0, 0],
                Some(100)
            )
            .recorded
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("status server should start");

    let publisher = std::thread::spawn({
        let state = state.clone();
        move || {
            for _ in 0..100 {
                if state.active_probe_count("source.count_to_sink.count") == Some(1) {
                    let record = state.try_probe_channel_publish_bytes(
                        "source.count_to_sink.count",
                        "Count",
                        &[0x2a, 0x00, 0x00, 0x00],
                        Some(124),
                    );
                    assert!(record.recorded);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("echo did not enable channel probe");
        }
    });

    let output = echo_channel(
        &EchoTarget {
            image: None,
            channel: "source.count".to_string(),
        },
        Some(&socket),
    )
    .unwrap();
    publisher.join().unwrap();

    assert!(output.contains("fields={value=42}"));
    assert!(output.contains("published_at_ms=124"));
    assert!(output.contains("raw=2a000000"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_with_binary_image_matches_section_selfdesc_hash() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "binary_echo_demo", "version": null, "rsdl_version": "0.1" },
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
  "message_abi": [{
    "type_name": "Imu",
    "size_bytes": 4,
    "align_bytes": 4,
    "fields": [{
      "name": "value",
      "type": "u32",
      "offset_bytes": 0,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-binary-selfdesc-hash");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "echo-binary-selfdesc-hash"
version = "0.1.0"
edition = "2024"

[workspace]
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        format!(
            r##"
#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; {}] = *br#"{source}"#;

fn main() {{}}
"##,
            source.len()
        ),
    )
    .unwrap();

    let status = ProcessCommand::new("cargo")
        .arg("build")
        .arg("--quiet")
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(status.success());
    let binary_name = if cfg!(windows) {
        "echo-binary-selfdesc-hash.exe"
    } else {
        "echo-binary-selfdesc-hash"
    };
    let binary = root.join("target/debug").join(binary_name);

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 91,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "binary_echo_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes(
        "source.imu_to_sink.imu",
        "Imu",
        vec![0x01, 0x02, 0x03, 0x04],
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&binary, "source.imu", Some(&socket)).unwrap();

    assert!(output.contains("fields={value=67305985}"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_follow_outputs_changed_snapshots_from_fake_status_server() {
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
    let root = temp_test_dir("echo-follow");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 86,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes(
        "source.imu_to_sink.imu",
        "Imu",
        vec![0x01, 0x02, 0x03, 0x04],
        Some(10),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("status server should start");
    let mut output = Vec::new();

    echo_channel_follow_for_polls(
        &EchoTarget {
            image: Some(selfdesc.clone()),
            channel: "source.imu".to_string(),
        },
        Some(&socket),
        std::time::Duration::from_millis(0),
        1,
        &mut output,
    )
    .unwrap();
    state.record_channel_publish_bytes(
        "source.imu_to_sink.imu",
        "Imu",
        vec![0x05, 0x06, 0x07, 0x08],
        Some(11),
    );
    echo_channel_follow_for_polls(
        &EchoTarget {
            image: Some(selfdesc.clone()),
            channel: "source.imu".to_string(),
        },
        Some(&socket),
        std::time::Duration::from_millis(0),
        2,
        &mut output,
    )
    .unwrap();

    let output = String::from_utf8(output).unwrap();
    let lines = output.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("published_count=1"));
    assert!(lines[0].contains("published_at_ms=10"));
    assert!(lines[0].contains("raw=01020304"));
    assert!(lines[1].contains("published_count=2"));
    assert!(lines[1].contains("published_at_ms=11"));
    assert!(lines[1].contains("raw=05060708"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_auto_socket_requires_explicit_socket_for_multiple_matches() {
    let root = temp_test_dir("echo-multiple-sockets");
    let first_socket = root.join("first.sock");
    let second_socket = root.join("second.sock");
    std::fs::create_dir_all(&root).unwrap();

    let self_description_hash = "feedface".to_string();
    let state = flowrt::IntrospectionState::new();
    let first = flowrt::spawn_status_server_at(
        first_socket.clone(),
        flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 91,
            started_at_unix_ms: 1,
            self_description_hash: self_description_hash.clone(),
            package: "robot_demo".to_string(),
            process: "first".to_string(),
            runtime: "rust".to_string(),
        },
        state.clone(),
    )
    .expect("first status server should start");
    let second = flowrt::spawn_status_server_at(
        second_socket.clone(),
        flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 92,
            started_at_unix_ms: 2,
            self_description_hash: self_description_hash.clone(),
            package: "robot_demo".to_string(),
            process: "second".to_string(),
            runtime: "rust".to_string(),
        },
        state,
    )
    .expect("second status server should start");

    let error = select_matching_runtime_socket(
        &self_description_hash,
        vec![first_socket.clone(), second_socket.clone()],
    )
    .expect_err("multiple matching sockets should require explicit selection");

    let message = error.to_string();
    assert!(message.contains("multiple live FlowRT processes match self-description hash"));
    assert!(message.contains("--socket"));
    assert!(message.contains(&first_socket.display().to_string()));
    assert!(message.contains(&second_socket.display().to_string()));

    drop(first);
    drop(second);
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
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let list = params_list(&selfdesc, Some(&socket)).unwrap();
    assert!(list.contains("controller.kp type=f32 update=on_tick current=1.0"));

    let get = params_get(&selfdesc, "controller.kp", Some(&socket)).unwrap();
    assert!(get.contains("pending=none"));

    let set = params_set(&selfdesc, "controller.kp", "2.5", Some(&socket)).unwrap();
    assert!(set.contains("current=1.0"));
    assert!(set.contains("pending=2.5"));

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

#[test]
fn parse_remote_params_key_expr_extracts_package_hash_and_pid() {
    let result = introspection::parse_remote_params_key_expr("flowrt/params/robot_demo/abc123/42");
    assert_eq!(result, Some(("robot_demo", "abc123", "42")));
}

#[test]
fn parse_remote_params_key_expr_rejects_invalid_prefix() {
    assert!(introspection::parse_remote_params_key_expr("flowrt/status/robot/abc/1").is_none());
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
fn remote_params_list_via_zenoh_returns_registered_params() {
    // 使用同一 session 测试远程参数查询路径，与 runtime params_remote 测试对齐。
    let session = zenoh::open(flowrt::zenoh::config_from_environment().unwrap())
        .wait()
        .unwrap();
    let key_expr = format!(
        "flowrt/params/cli_test/{}/9001",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let expected_hash = "cli_test_hash".to_string();
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 9001,
        started_at_unix_ms: 1000,
        self_description_hash: expected_hash.clone(),
        package: "cli_robot".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_param(flowrt::IntrospectionParamSchema {
        name: "planner.kp".to_string(),
        ty: "f64".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.5),
        min: Some(serde_json::json!(0.0)),
        max: Some(serde_json::json!(100.0)),
        choices: Vec::new(),
    });

    let _server = flowrt::ZenohParamsServer::open(&session, &key_expr, handshake, state).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 直接查询特定 key expression 验证远程参数路径。
    let response = flowrt::request_remote_param_list(&session, &key_expr, 5000)
        .expect("remote param list should succeed");
    let flowrt::IntrospectionResponse::ParamList { params, .. } = response else {
        panic!("expected ParamList response");
    };
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].name, "planner.kp");
    assert_eq!(params[0].current, serde_json::json!(1.5));
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
    assert!(message.contains("--socket"));
    assert!(message.contains("pid=100"));
    assert!(message.contains("pid=200"));
}
