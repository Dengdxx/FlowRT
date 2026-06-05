use super::*;

#[test]
fn self_description_sidecar_drives_list_and_nodes_output() {
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
    "instances": [{
      "name": "source",
      "component": "imu_sim",
      "process": "main",
      "target": null,
      "runtime": "rust"
    }],
    "tasks": [{ "instance": "source", "trigger": "periodic" }],
    "channels": [{
      "from": "source.imu",
      "to": "sink.imu",
      "message_type": "Imu"
    }]
  }],
  "message_abi": [{ "type_name": "Imu", "size_bytes": 8 }]
}
"#;
    let root = temp_test_dir("selfdesc-sidecar");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);
    let nodes = self_description_nodes(&self_description);

    assert!(list.contains("package=robot_demo"));
    assert!(list.contains("channel source.imu -> sink.imu type=Imu"));
    assert!(list.contains("message Imu size=8"));
    assert!(nodes.contains("source process=main runtime=rust component=imu_sim"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn reads_self_description_from_object_section() {
    let root = temp_test_dir("selfdesc-section");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "selfdesc-section-test"
version = "0.1.0"
edition = "2024"

[workspace]
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        r##"
#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 253] = *br#"{
  "self_description_version": "0.1",
  "source_hash": "feedface",
  "package": { "name": "binary_demo" },
  "graphs": [{ "name": "default", "instances": [], "tasks": [], "channels": [] }],
  "message_abi": [{ "type_name": "Ping", "size_bytes": 4 }]
}
"#;

fn main() {}
"##,
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
        "selfdesc-section-test.exe"
    } else {
        "selfdesc-section-test"
    };
    let binary = root.join("target/debug").join(binary_name);
    let self_description = load_self_description(&binary).unwrap();

    assert_eq!(self_description.package.name, "binary_demo");
    assert_eq!(self_description.message_abi[0].type_name, "Ping");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_reads_runtime_socket_handshake() {
    let root = temp_test_dir("live-status");
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
    state.register_channel("source.imu_to_sink.imu", "Imu");
    for _ in 0..9 {
        state.record_tick();
    }
    for _ in 0..4 {
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![0u8; 48], None);
    }
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket]).unwrap();

    assert!(output.contains("pid=77"));
    assert!(output.contains("package=robot_demo"));
    assert!(output.contains("process=main"));
    assert!(output.contains("selfdesc=feedface"));
    assert!(output.contains("ticks=9"));
    assert!(output.contains("channels=1"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_displays_supervisor_process_health() {
    let root = temp_test_dir("live-status-supervisor-health");
    let socket = root.join("supervisor.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 70,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "flowrt_supervisor".to_string(),
        runtime: "supervisor".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_process_health(flowrt::IntrospectionProcessStatus {
        name: "sensors".to_string(),
        state: "stale".to_string(),
        pid: Some(77),
        restart_count: 2,
        tick_count: Some(10),
        last_seen_unix_ms: Some(2000),
        tick_stale: true,
        exit_code: None,
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket]).unwrap();

    assert!(output.contains("supervisor_process=sensors"));
    assert!(output.contains("state=stale"));
    assert!(output.contains("pid=77"));
    assert!(output.contains("restarts=2"));
    assert!(output.contains("ticks=10"));
    assert!(output.contains("tick_stale=true"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_hz_summary_formats_channel_delta_rate() {
    let first = flowrt::IntrospectionResponse::Status {
        handshake: flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 77,
            started_at_unix_ms: 1234,
            self_description_hash: "feedface".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        },
        status: flowrt::IntrospectionStatus {
            tick_count: 10,
            channels: vec![flowrt::IntrospectionChannelStatus {
                name: "source.imu_to_sink.imu".to_string(),
                message_type: "Imu".to_string(),
                published_count: 100,
                last_payload_len: None,
                active_observers: 0,
                dropped_samples: 0,
            }],
            processes: Vec::new(),
        },
    };
    let second = flowrt::IntrospectionResponse::Status {
        handshake: flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 77,
            started_at_unix_ms: 1234,
            self_description_hash: "feedface".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        },
        status: flowrt::IntrospectionStatus {
            tick_count: 20,
            channels: vec![flowrt::IntrospectionChannelStatus {
                name: "source.imu_to_sink.imu".to_string(),
                message_type: "Imu".to_string(),
                published_count: 150,
                last_payload_len: None,
                active_observers: 0,
                dropped_samples: 0,
            }],
            processes: Vec::new(),
        },
    };

    let output = format_hz_summary_from_status_pair(&first, &second, Duration::from_millis(500))
        .expect("hz summary should format status pair");

    assert!(output.contains("channel=source.imu_to_sink.imu"));
    assert!(output.contains("type=Imu"));
    assert!(output.contains("delta=50"));
    assert!(output.contains("hz=100.00"));
}

#[test]
fn live_hz_summary_reads_status_without_enabling_probe() {
    let root = temp_test_dir("live-hz");
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
    state.register_channel("source.imu_to_sink.imu", "Imu");
    state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![0u8; 48], None);
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("status server should start");
    let publish_state = state.clone();
    let publisher = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        for _ in 0..5 {
            publish_state.record_channel_publish_bytes(
                "source.imu_to_sink.imu",
                "Imu",
                vec![0u8; 48],
                None,
            );
        }
    });

    let output = live_hz_summary_for_sockets(
        Some("source.imu_to_sink.imu"),
        vec![socket],
        Duration::from_millis(50),
    )
    .unwrap();
    publisher.join().unwrap();

    assert!(output.contains("channel=source.imu_to_sink.imu"));
    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_hz_summary_reports_stale_socket_without_failing_scan() {
    let root = temp_test_dir("live-hz-stale");
    let socket = root.join("missing.sock");
    std::fs::create_dir_all(&root).unwrap();

    let output = live_hz_summary_for_sockets(None, vec![socket.clone()], Duration::from_millis(1))
        .expect("stale socket should be reported as a line");

    assert!(output.contains(&format!("stale socket={}", socket.display())));

    let _ = std::fs::remove_dir_all(&root);
}
