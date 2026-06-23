use super::*;

mod frame_transport;

fn spawn_stalled_introspection_socket(socket: std::path::PathBuf) -> std::sync::mpsc::Sender<()> {
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        if let Ok((stream, _addr)) = listener.accept() {
            let _stream = stream;
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
        }
    });
    release_tx
}

fn write_json_line(
    stream: &mut std::os::unix::net::UnixStream,
    response: &flowrt::IntrospectionResponse,
) {
    let line = serde_json::to_string(response).unwrap();
    use std::io::Write as _;
    stream.write_all(line.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
}

fn read_request_line(stream: &std::os::unix::net::UnixStream) {
    let mut request = String::new();
    let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
    std::io::BufRead::read_line(&mut reader, &mut request).unwrap();
    assert!(
        !request.is_empty(),
        "introspection client must send a request before the fake server responds"
    );
}

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
      "message_type": "Imu",
      "backend": "iox2",
      "thread_affinity": "scheduler_local_commit"
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
    assert!(list.contains("backend=iox2 thread_affinity=scheduler_local_commit"));
    assert!(list.contains("message Imu size=8"));
    assert!(nodes.contains("source process=main runtime=rust component=imu_sim"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn self_description_summary_shows_island_boundary_endpoints() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "artifact": {
    "mode": "island",
    "temporary_island": true,
    "test_only": true,
    "temporary_overlay": {
      "kind": "temporary_island",
      "original_profile_mode": "strict",
      "generated_by": { "command": "flowrt prepare", "source": "demo.rsdl" },
      "boundary_mappings": [{
        "direction": "output",
        "name": "sample_out",
        "endpoint": "producer.sample",
        "source": "--boundary-output"
      }]
    },
    "clock": { "source": "simulated_replay", "unit": "ms", "field": "tick_time_ms" }
  },
  "package": { "name": "island_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "targets": [],
  "deployments": [],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "instances": [],
    "tasks": [],
    "channels": [],
    "boundary_endpoints": [{
      "name": "sample_in",
      "canonical_id": "boundary_0123456789abcdef",
      "direction": "input",
      "endpoint": "consumer.sample",
      "instance": "consumer",
      "port": "sample",
      "message_type": "Sample"
    }]
  }],
  "message_abi": [{ "type_name": "Sample", "size_bytes": 4 }]
}
"#;
    let root = temp_test_dir("selfdesc-island-boundary");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);

    assert!(list.contains("profiles=1 island_profiles=1"));
    assert!(list.contains(
        "artifact_mode=island temporary_island=true test_only=true temporary_overlay=true"
    ));
    assert!(list.contains("clock_source=simulated_replay clock_unit=ms clock_field=tick_time_ms"));
    assert!(list.contains("graph default mode=island"));
    assert!(list.contains("boundary input sample_in endpoint=consumer.sample type=Sample"));

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
        .env_remove("CARGO_TARGET_DIR")
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

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

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
fn live_status_summary_displays_channel_input_and_route_diagnostics() {
    let root = temp_test_dir("live-status-runtime-diagnostics");
    let socket = root.join("main.sock");
    let selfdesc_json = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "artifact": { "mode": "strict", "temporary_island": false, "test_only": false },
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "graphs": [{
    "name": "default",
    "mode": "strict",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.packet",
      "to": "sink.packet",
      "message_type": "Packet",
      "backend": "zenoh",
      "thread_affinity": "send_safe"
    }]
  }]
}
"#;
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 78,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(selfdesc_json.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.set_self_description_json(selfdesc_json);
    state.record_tick_at(250, "simulated_replay");
    state.register_route(flowrt::IntrospectionRouteStatus {
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
        backend_health_state: "reconnecting".to_string(),
        backend_health_error: Some("publish Zenoh sample: session closed".to_string()),
        backend_reconnect_attempt: 2,
        backend_next_retry_unix_ms: Some(4_200),
        backend_recoverable: true,
    });
    state.record_input_status(flowrt::IntrospectionInputStatus {
        task: "sink.main".to_string(),
        input: "packet".to_string(),
        channel: "source.packet_to_sink.packet".to_string(),
        message_type: "Packet".to_string(),
        present: false,
        stale: true,
        last_revision: Some(9),
        last_read_ms: Some(125),
        updated_unix_ms: Some(2000),
        dropped_samples: 0,
        backpressure_count: 0,
        overflow_count: 0,
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("channel=source.packet_to_sink.packet"));
    assert!(output.contains("type=Packet"));
    assert!(output.contains("route=source.packet_to_sink.packet"));
    assert!(output.contains("backend=zenoh"));
    assert!(output.contains("thread_affinity=send_safe"));
    assert!(output.contains("selected_reason=variable_frame_auto_fallback"));
    assert!(output.contains("dropped_samples=1"));
    assert!(output.contains("backpressure=2"));
    assert!(output.contains("overflow=3"));
    assert!(output.contains("last_publish_ms=120"));
    assert!(output.contains("last_error=queue overflow"));
    assert!(output.contains("backend_health=reconnecting"));
    assert!(output.contains("backend_recoverable=true"));
    assert!(output.contains("backend_reconnect_attempt=2"));
    assert!(output.contains("backend_next_retry_unix_ms=4200"));
    assert!(output.contains("backend_health_error=publish Zenoh sample: session closed"));
    assert!(output.contains("input=sink.main.packet"));
    assert!(output.contains("present=false"));
    assert!(output.contains("stale=true"));
    assert!(output.contains("last_revision=9"));
    assert!(output.contains("last_read_ms=125"));
    assert!(output.contains("diagnostic=source.packet_to_sink.packet"));
    assert!(output.contains("category=route"));
    assert!(output.contains("severity=error"));
    assert!(output.contains("latest_age_ms"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_labels_static_facts_and_keeps_live_route_authoritative() {
    let root = temp_test_dir("live-status-static-live-split");
    let socket = root.join("main.sock");
    let selfdesc_json = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "artifact": { "mode": "strict", "temporary_island": false, "test_only": false },
  "package": { "name": "robot_demo", "version": null, "rsdl_version": "0.1" },
  "graphs": [{
    "name": "default",
    "mode": "strict",
    "instances": [],
    "tasks": [],
    "channels": [{
      "from": "source.packet",
      "to": "sink.packet",
      "message_type": "PacketStatic",
      "backend": "iox2",
      "thread_affinity": "scheduler_local_commit"
    }]
  }]
}
"#;
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 82,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(selfdesc_json.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.set_self_description_json(selfdesc_json);
    state.register_route(flowrt::IntrospectionRouteStatus {
        name: "source.packet_to_sink.packet".to_string(),
        from: "source.packet".to_string(),
        to: "sink.packet".to_string(),
        message_type: "PacketLive".to_string(),
        backend: "zenoh".to_string(),
        selected_reason: "runtime_probe".to_string(),
        published_count: 11,
        dropped_samples: 0,
        backpressure_count: 0,
        overflow_count: 0,
        last_publish_ms: Some(512),
        last_error: None,
        ..Default::default()
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("static_selfdesc=loaded"), "{output}");
    assert!(output.contains(
        "route=source.packet_to_sink.packet from=source.packet to=sink.packet type=PacketLive backend=zenoh"
    ), "{output}");
    assert!(output.contains("selected_reason=runtime_probe"), "{output}");
    assert!(
        output.contains(
            "thread_affinity=scheduler_local_commit static_thread_affinity=scheduler_local_commit"
        ),
        "{output}"
    );
    assert!(
        !output.contains("type=PacketStatic backend=iox2"),
        "{output}"
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_json_exposes_machine_readable_diagnostics() {
    let root = temp_test_dir("live-status-json-diagnostics");
    let socket = root.join("main.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 79,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.record_tick_at(250, "simulated_replay");
    state.register_route(flowrt::IntrospectionRouteStatus {
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
    state.record_task_health(flowrt::IntrospectionTaskHealth {
        name: "control.loop".to_string(),
        lane: "control".to_string(),
        inflight: false,
        scheduled_time_ms: Some(1_000),
        observed_time_ms: Some(1_033),
        lateness_ms: Some(33),
        missed_periods: Some(3),
        overrun: Some(true),
        ..Default::default()
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = crate::introspection::live_status_json_for_sockets(vec![socket], false).unwrap();
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(value[0]["live"], true);
    assert_eq!(value[0]["status"]["clock"]["source"], "simulated_replay");
    assert_eq!(
        value[0]["status"]["tasks"][0]["scheduled_time_ms"],
        serde_json::json!(1_000)
    );
    assert_eq!(
        value[0]["status"]["tasks"][0]["observed_time_ms"],
        serde_json::json!(1_033)
    );
    assert_eq!(
        value[0]["status"]["tasks"][0]["lateness_ms"],
        serde_json::json!(33)
    );
    assert_eq!(
        value[0]["status"]["tasks"][0]["missed_periods"],
        serde_json::json!(3)
    );
    assert_eq!(
        value[0]["status"]["tasks"][0]["overrun"],
        serde_json::json!(true)
    );
    assert!(
        value[0]["status"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["category"] == "route"
                && diagnostic["entity_id"] == "source.packet_to_sink.packet"
                && diagnostic["reason"] == "queue overflow")
    );
    assert!(
        value[0]["status"]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["category"] == "task"
                && diagnostic["entity_id"] == "control.loop"
                && diagnostic["reason"] == "runtime observed task timing issue")
    );

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
        readiness_wait: None,
        resource_placement: Some(
            flowrt::supervisor::resource_placement::ResourcePlacementStatus {
                desired: flowrt::supervisor::resource_placement::ResourcePlacement {
                    cpu_affinity: vec![0, 1],
                    nice: Some(5),
                    ..Default::default()
                },
                applied: Default::default(),
            },
        ),
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("supervisor_process=sensors"));
    assert!(output.contains("state=stale"));
    assert!(output.contains("pid=77"));
    assert!(output.contains("restarts=2"));
    assert!(output.contains("ticks=10"));
    assert!(output.contains("tick_stale=true"));
    assert!(output.contains("resource_placement="));
    assert!(output.contains("\"cpu_affinity\":[0,1]"));
    assert!(output.contains("\"nice\":5"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_displays_io_boundary_health() {
    let root = temp_test_dir("live-status-boundary-diagnostics");
    let socket = root.join("main.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 72,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_io_boundary(
        "camera",
        "CameraDriver",
        vec![flowrt::IntrospectionIoBoundaryResourceStatus {
            name: "sensor".to_string(),
            kind: "device".to_string(),
            ready: false,
            message: Some("waiting for /dev/video0".to_string()),
            last_error: Some("open failed".to_string()),
            updated_unix_ms: Some(3000),
        }],
    );
    state.mark_io_boundary_ready("camera", false);
    state.record_io_boundary_error("camera", "frame timeout");
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("io_boundary=camera"));
    assert!(output.contains("component=CameraDriver"));
    assert!(output.contains("ready=false"));
    assert!(output.contains("healthy=false"));
    assert!(output.contains("last_error=frame timeout"));
    assert!(output.contains("io_boundary_resource=camera.sensor"));
    assert!(output.contains("kind=device"));
    assert!(output.contains("message=waiting for /dev/video0"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_displays_resource_state() {
    let root = temp_test_dir("live-status-resource-state");
    let socket = root.join("main.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 74,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_resource(flowrt::IntrospectionResourceStatus {
        name: "sensor.lidar_uart".to_string(),
        capability: "perception.lidar.samples".to_string(),
        state: "pending".to_string(),
        required: true,
        source: Some("contract".to_string()),
        owner_process: Some("main".to_string()),
        last_error: Some("provider not reported ready".to_string()),
        updated_unix_ms: Some(4000),
        ..Default::default()
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("resource=sensor.lidar_uart"));
    assert!(output.contains("capability=perception.lidar.samples"));
    assert!(output.contains("state=pending"));
    assert!(output.contains("required=true"));
    assert!(output.contains("readiness=none"));
    assert!(output.contains("on_failure=none"));
    assert!(output.contains("provider=none"));
    assert!(output.contains("source=contract"));
    assert!(output.contains("owner_process=main"));
    assert!(output.contains("last_error=provider not reported ready"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_enriches_io_boundary_resource_descriptor_schema() {
    let root = temp_test_dir("live-status-boundary-descriptor");
    let socket = root.join("main.sock");
    let selfdesc_json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "robot_demo" },
  "component_types": [{
    "name": "CameraDriver",
    "language": "rust",
    "kind": "io_boundary",
    "resources": [{
      "name": "frames",
      "kind": "shm",
      "required": true,
      "descriptor": {
        "kind": "frame",
        "port": "frame",
        "format": "rgb8",
        "encoding": "row_major",
        "metadata": { "width": "640", "height": "480" },
        "record_payload": false
      }
    }]
  }],
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "camera",
      "component": "CameraDriver",
      "process": "camera_proc",
      "runtime": "rust"
    }],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}"#;
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 73,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(selfdesc_json.as_bytes()),
        package: "robot_demo".to_string(),
        process: "camera_proc".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.set_self_description_json(selfdesc_json);
    state.register_io_boundary(
        "camera",
        "CameraDriver",
        vec![flowrt::IntrospectionIoBoundaryResourceStatus {
            name: "frames".to_string(),
            kind: "shm".to_string(),
            ready: true,
            message: None,
            last_error: None,
            updated_unix_ms: Some(3000),
        }],
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("io_boundary_resource=camera.frames"));
    assert!(output.contains("descriptor_kind=frame"));
    assert!(output.contains("descriptor_port=frame"));
    assert!(output.contains("descriptor_format=rgb8"));
    assert!(output.contains("descriptor_encoding=row_major"));
    assert!(output.contains("descriptor_record_payload=false"));
    assert!(output.contains("descriptor_payload_capture=none"));
    assert!(output.contains("descriptor_metadata=[height:480,width:640]"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_displays_recorder_health() {
    let root = temp_test_dir("live-status-recorder-health");
    let socket = root.join("main.sock");
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 71,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.start_recorder(flowrt::IntrospectionRecorderStart {
        output: Some("run.mcap".to_string()),
        filters: vec!["channel:source.imu_to_sink.imu".to_string()],
        queue_depth: Some(4),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime_pid: 71,
        selfdesc_hash: "feedface".to_string(),
    });
    state.try_record_channel_sample_bytes("source.imu_to_sink.imu", "Imu", &[1, 2], Some(10));
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("recorder enabled=true"));
    assert!(output.contains("output=run.mcap"));
    assert!(output.contains("dropped_count=0"));
    assert!(output.contains("bytes_written=2"));
    assert!(output.contains("queued_events=1"));
    assert!(output.contains("active_filters=[channel:source.imu_to_sink.imu]"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_live_only_hides_stale_sockets() {
    let root = temp_test_dir("live-status-live-only");
    let socket = root.join("missing.sock");

    let default_output = live_status_summary_for_sockets(vec![socket.clone()], false).unwrap();
    let live_only_output = live_status_summary_for_sockets(vec![socket], true).unwrap();

    assert!(default_output.contains("stale socket="));
    assert_eq!(live_only_output, "no live FlowRT processes");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_reports_stalled_socket_as_stale() {
    let root = temp_test_dir("live-status-stalled-socket");
    let socket = root.join("stalled.sock");
    let release = spawn_stalled_introspection_socket(socket.clone());

    let default_output = live_status_summary_for_sockets(vec![socket.clone()], false).unwrap();
    let live_only_output = live_status_summary_for_sockets(vec![socket], true).unwrap();

    assert!(default_output.contains("stale socket="));
    assert_eq!(live_only_output, "no live FlowRT processes");
    let _ = release.send(());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_keeps_status_when_static_selfdesc_stalls() {
    let root = temp_test_dir("live-status-stalled-selfdesc");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let handshake = flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 81,
            started_at_unix_ms: 1234,
            self_description_hash: "feedface".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        if let Ok((mut stream, _addr)) = listener.accept() {
            read_request_line(&stream);
            write_json_line(
                &mut stream,
                &flowrt::IntrospectionResponse::Status {
                    handshake,
                    status: flowrt::IntrospectionStatus::default(),
                },
            );
        }
        if let Ok((stream, _addr)) = listener.accept() {
            let _stream = stream;
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
        }
    });

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("pid=81"));
    assert!(output.contains("package=robot_demo"));
    assert!(!output.contains("stale socket="));
    let _ = release_tx.send(());
    server.join().unwrap();
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
            clock: Default::default(),
            channels: vec![flowrt::IntrospectionChannelStatus {
                name: "source.imu_to_sink.imu".to_string(),
                message_type: "Imu".to_string(),
                published_count: 100,
                last_payload_len: None,
                active_observers: 0,
                dropped_samples: 0,
            }],
            processes: Vec::new(),
            resources: Vec::new(),
            inputs: Vec::new(),
            routes: Vec::new(),
            io_boundaries: Vec::new(),
            services: Vec::new(),
            operations: Vec::new(),
            tasks: Vec::new(),
            lanes: Vec::new(),
            recorder: Default::default(),
            params: Vec::new(),
            instances: Vec::new(),
            failovers: Vec::new(),
            critical_instances: Vec::new(),
            graph_health: "healthy".to_string(),
            graph_critical_health: "healthy".to_string(),
            diagnostics: Vec::new(),
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
            clock: Default::default(),
            channels: vec![flowrt::IntrospectionChannelStatus {
                name: "source.imu_to_sink.imu".to_string(),
                message_type: "Imu".to_string(),
                published_count: 150,
                last_payload_len: None,
                active_observers: 0,
                dropped_samples: 0,
            }],
            processes: Vec::new(),
            resources: Vec::new(),
            inputs: Vec::new(),
            routes: Vec::new(),
            io_boundaries: Vec::new(),
            services: Vec::new(),
            operations: Vec::new(),
            tasks: Vec::new(),
            lanes: Vec::new(),
            recorder: Default::default(),
            params: Vec::new(),
            instances: Vec::new(),
            failovers: Vec::new(),
            critical_instances: Vec::new(),
            graph_health: "healthy".to_string(),
            graph_critical_health: "healthy".to_string(),
            diagnostics: Vec::new(),
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
fn live_hz_summary_includes_route_drop_and_overflow_delta() {
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 77,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let route_name = "source.packet_to_sink.packet".to_string();
    let first = flowrt::IntrospectionResponse::Status {
        handshake: handshake.clone(),
        status: flowrt::IntrospectionStatus {
            tick_count: 10,
            channels: vec![flowrt::IntrospectionChannelStatus {
                name: route_name.clone(),
                message_type: "Packet".to_string(),
                published_count: 10,
                last_payload_len: None,
                active_observers: 0,
                dropped_samples: 0,
            }],
            routes: vec![flowrt::IntrospectionRouteStatus {
                name: route_name.clone(),
                from: "source.packet".to_string(),
                to: "sink.packet".to_string(),
                message_type: "Packet".to_string(),
                backend: "zenoh".to_string(),
                selected_reason: "variable_frame_auto_fallback".to_string(),
                published_count: 10,
                dropped_samples: 2,
                backpressure_count: 4,
                overflow_count: 6,
                last_publish_ms: Some(100),
                last_error: None,
                ..Default::default()
            }],
            ..Default::default()
        },
    };
    let second = flowrt::IntrospectionResponse::Status {
        handshake,
        status: flowrt::IntrospectionStatus {
            tick_count: 20,
            channels: vec![flowrt::IntrospectionChannelStatus {
                name: route_name.clone(),
                message_type: "Packet".to_string(),
                published_count: 30,
                last_payload_len: None,
                active_observers: 0,
                dropped_samples: 0,
            }],
            routes: vec![flowrt::IntrospectionRouteStatus {
                name: route_name,
                from: "source.packet".to_string(),
                to: "sink.packet".to_string(),
                message_type: "Packet".to_string(),
                backend: "zenoh".to_string(),
                selected_reason: "variable_frame_auto_fallback".to_string(),
                published_count: 30,
                dropped_samples: 5,
                backpressure_count: 6,
                overflow_count: 11,
                last_publish_ms: Some(200),
                last_error: None,
                ..Default::default()
            }],
            ..Default::default()
        },
    };

    let output = format_hz_summary_from_status_pair(&first, &second, Duration::from_millis(1000))
        .expect("hz summary should include route diagnostics");

    assert!(output.contains("channel=source.packet_to_sink.packet"));
    assert!(output.contains("delta=20"));
    assert!(output.contains("dropped_delta=3"));
    assert!(output.contains("backpressure_delta=2"));
    assert!(output.contains("overflow_delta=5"));
}

#[test]
fn live_hz_summary_includes_route_only_publish_delta() {
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 77,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let route_name = "ros2_bridge_1".to_string();
    let first = flowrt::IntrospectionResponse::Status {
        handshake: handshake.clone(),
        status: flowrt::IntrospectionStatus {
            tick_count: 10,
            routes: vec![flowrt::IntrospectionRouteStatus {
                name: route_name.clone(),
                from: "source.pose".to_string(),
                to: "ros2:/flowrt/pose".to_string(),
                message_type: "Pose".to_string(),
                backend: "zenoh".to_string(),
                selected_reason: "ros2_bridge".to_string(),
                published_count: 10,
                dropped_samples: 1,
                backpressure_count: 0,
                overflow_count: 0,
                last_publish_ms: Some(100),
                last_error: None,
                ..Default::default()
            }],
            ..Default::default()
        },
    };
    let second = flowrt::IntrospectionResponse::Status {
        handshake,
        status: flowrt::IntrospectionStatus {
            tick_count: 20,
            routes: vec![flowrt::IntrospectionRouteStatus {
                name: route_name,
                from: "source.pose".to_string(),
                to: "ros2:/flowrt/pose".to_string(),
                message_type: "Pose".to_string(),
                backend: "zenoh".to_string(),
                selected_reason: "ros2_bridge".to_string(),
                published_count: 16,
                dropped_samples: 2,
                backpressure_count: 1,
                overflow_count: 0,
                last_publish_ms: Some(200),
                last_error: None,
                ..Default::default()
            }],
            ..Default::default()
        },
    };

    let output = format_hz_summary_from_status_pair(&first, &second, Duration::from_millis(500))
        .expect("hz summary should include route-only diagnostics");

    assert!(output.contains("channel=ros2_bridge_1"));
    assert!(output.contains("type=Pose"));
    assert!(output.contains("delta=6"));
    assert!(output.contains("hz=12.00"));
    assert!(output.contains("dropped_delta=1"));
    assert!(output.contains("backpressure_delta=1"));
    assert!(output.contains("overflow_delta=0"));
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

#[test]
fn live_hz_summary_reports_stalled_socket_without_failing_scan() {
    let root = temp_test_dir("live-hz-stalled");
    let socket = root.join("stalled.sock");
    let release = spawn_stalled_introspection_socket(socket.clone());

    let output = live_hz_summary_for_sockets(None, vec![socket.clone()], Duration::from_millis(1))
        .expect("stalled socket should be reported as stale");

    assert!(output.contains(&format!("stale socket={}", socket.display())));
    let _ = release.send(());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_hz_summary_reports_non_status_response_as_stale() {
    let root = temp_test_dir("live-hz-non-status");
    let socket = root.join("non-status.sock");
    std::fs::create_dir_all(&root).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let response = flowrt::IntrospectionResponse::SelfDescription {
        handshake: flowrt::IntrospectionHandshake {
            protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 78,
            started_at_unix_ms: 1234,
            self_description_hash: "feedface".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        },
        json: "{}".to_string(),
    };
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        let mut reader = std::io::BufReader::new(stream.try_clone().unwrap());
        std::io::BufRead::read_line(&mut reader, &mut request).unwrap();
        let response = serde_json::to_string(&response).unwrap();
        std::io::Write::write_all(&mut stream, response.as_bytes()).unwrap();
        std::io::Write::write_all(&mut stream, b"\n").unwrap();
    });

    let output = live_hz_summary_for_sockets(None, vec![socket.clone()], Duration::from_millis(1))
        .expect("non-status response should be reported as stale");

    assert!(output.contains(&format!(
        "stale socket={} error=unexpected introspection response",
        socket.display()
    )));
    server.join().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn self_description_summary_displays_service_endpoints() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "svc_demo" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [],
    "services": [{
      "name": "planner.plan_to_executor.execute",
      "canonical_id": "svc_001",
      "client_instance": "planner",
      "client_port": "plan",
      "server_instance": "executor",
      "server_port": "execute",
      "request_type": "PlanRequest",
      "response_type": "PlanResponse",
      "backend": "zenoh",
      "key_expr": "flowrt/service/planner.plan/request"
    }]
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("selfdesc-services");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);

    assert!(list.contains("services=1"));
    assert!(list.contains("service planner.plan_to_executor.execute"));
    assert!(list.contains("client=planner.plan"));
    assert!(list.contains("server=executor.execute"));
    assert!(list.contains("request=PlanRequest"));
    assert!(list.contains("response=PlanResponse"));
    assert!(list.contains("backend=zenoh"));
    assert!(list.contains("key_expr=flowrt/service/planner.plan/request"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn self_description_summary_separates_iox2_service_name_and_zenoh_key_expr() {
    let iox2_source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "iox2_svc_demo" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [],
    "services": [{
      "name": "planner.plan_to_executor.execute",
      "client_instance": "planner",
      "client_port": "plan",
      "server_instance": "executor",
      "server_port": "execute",
      "request_type": "PlanRequest",
      "response_type": "PlanResponse",
      "backend": "iox2",
      "service": "FlowRT/service/planner_plan"
    }]
  }],
  "message_abi": []
}
"#;
    let iox2: flowrt_selfdesc::SelfDescription = serde_json::from_str(iox2_source).unwrap();
    let iox2_list = self_description_summary(&iox2);

    assert!(iox2_list.contains("backend=iox2"));
    assert!(iox2_list.contains("service=FlowRT/service/planner_plan"));
    assert!(!iox2_list.contains("key_expr="));

    let zenoh_source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "zenoh_svc_demo" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [],
    "services": [{
      "name": "planner.plan_to_executor.execute",
      "client_instance": "planner",
      "client_port": "plan",
      "server_instance": "executor",
      "server_port": "execute",
      "request_type": "PlanRequest",
      "response_type": "PlanResponse",
      "backend": "zenoh",
      "key_expr": "flowrt/service/planner.plan/request"
    }]
  }],
  "message_abi": []
}
"#;
    let zenoh: flowrt_selfdesc::SelfDescription = serde_json::from_str(zenoh_source).unwrap();
    let zenoh_list = self_description_summary(&zenoh);

    assert!(zenoh_list.contains("backend=zenoh"));
    assert!(zenoh_list.contains("key_expr=flowrt/service/planner.plan/request"));
    assert!(!zenoh_list.contains("service=FlowRT/service/"));
}

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

#[test]
fn live_status_summary_displays_service_health() {
    let root = temp_test_dir("live-status-service-health");
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
    state.register_service("planner.plan_to_executor.execute");
    state.record_service_health(flowrt::IntrospectionServiceStatus {
        name: "planner.plan_to_executor.execute".to_string(),
        ready: true,
        in_flight: 2,
        queued: 1,
        total_requests: 100,
        timeout_count: 3,
        busy_count: 1,
        unavailable_count: 0,
        late_drop_count: 2,
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("service=planner.plan_to_executor.execute"));
    assert!(output.contains("ready=true"));
    assert!(output.contains("in_flight=2"));
    assert!(output.contains("queued=1"));
    assert!(output.contains("total_requests=100"));
    assert!(output.contains("timeout=3"));
    assert!(output.contains("busy=1"));
    assert!(output.contains("late_drop=2"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_enriches_island_boundary_endpoints() {
    let root = temp_test_dir("live-status-island-boundary");
    let socket = root.join("main.sock");
    let selfdesc_json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "artifact": {
    "mode": "island",
    "temporary_island": true,
    "test_only": true,
    "temporary_overlay": {
      "kind": "temporary_island",
      "original_profile_mode": "strict",
      "generated_by": { "command": "flowrt prepare", "source": "demo.rsdl" },
      "boundary_mappings": [{
        "direction": "output",
        "name": "sample_out",
        "endpoint": "producer.sample",
        "source": "--boundary-output"
      }]
    },
    "clock": { "source": "simulated_replay", "unit": "ms", "field": "tick_time_ms" }
  },
  "package": { "name": "island_demo" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "boundary_endpoints": [{
      "name": "sample_out",
      "direction": "output",
      "endpoint": "producer.sample",
      "instance": "producer",
      "port": "sample",
      "message_type": "Sample"
    }]
  }],
  "message_abi": []
}"#;
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(selfdesc_json.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.set_self_description_json(selfdesc_json);
    state.record_tick_at(25, "simulated_replay");
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(output.contains("graph=default mode=island boundary_endpoints=1"));
    assert!(output.contains(
        "clock_source=simulated_replay tick_time_ms=25 clock_unit=ms clock_field=tick_time_ms"
    ));
    assert!(output.contains(
        "artifact_mode=island temporary_island=true test_only=true temporary_overlay=true"
    ));
    assert!(output.contains(
        "boundary_endpoint=sample_out direction=output endpoint=producer.sample type=Sample graph=default mode=island"
    ));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_associates_service_health_with_instances() {
    let root = temp_test_dir("live-status-svc-instance");
    let socket = root.join("main.sock");
    let selfdesc_json = r#"{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "robot_demo" },
  "graphs": [{
    "name": "default",
    "instances": [],
    "tasks": [],
    "channels": [],
    "services": [{
      "name": "planner.plan_to_executor.execute",
      "client_instance": "planner",
      "client_port": "plan",
      "server_instance": "executor",
      "server_port": "execute",
      "request_type": "PlanReq",
      "response_type": "PlanResp",
      "backend": "inproc"
    }]
  }],
  "message_abi": []
}"#;
    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 88,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(selfdesc_json.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.set_self_description_json(selfdesc_json);
    state.register_service("planner.plan_to_executor.execute");
    state.record_service_health(flowrt::IntrospectionServiceStatus {
        name: "planner.plan_to_executor.execute".to_string(),
        ready: true,
        in_flight: 1,
        queued: 0,
        total_requests: 50,
        timeout_count: 0,
        busy_count: 0,
        unavailable_count: 0,
        late_drop_count: 0,
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(
        output.contains("client_instance=planner"),
        "expected client_instance=planner in output: {output}"
    );
    assert!(
        output.contains("server_instance=executor"),
        "expected server_instance=executor in output: {output}"
    );
    assert!(output.contains("service=planner.plan_to_executor.execute"));
    assert!(output.contains("ready=true"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn self_description_summary_shows_component_types() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "comp_demo" },
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "src",
      "component": "sensor",
      "process": "main",
      "runtime": "rust"
    }],
    "tasks": [],
    "channels": []
  }],
  "component_types": [{
    "name": "sensor",
    "language": "rust",
    "kind": "native",
    "inputs": [],
    "outputs": [{ "name": "imu", "type": "Imu" }],
    "service_clients": [],
    "service_servers": [],
    "params": [{ "name": "rate", "type": "f64", "update": "on_tick" }]
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("selfdesc-component-types");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);

    assert!(list.contains("component_types=1"));
    assert!(list.contains("component sensor language=rust kind=native"));
    assert!(list.contains("outputs: imu:Imu"));
    assert!(list.contains("params: rate:f64 update=on_tick"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn list_summary_shows_resource_contract() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "resource_demo" },
  "graphs": [{
    "name": "default",
    "resource_contract": {
      "resource_contract_version": "0.1",
      "providers": [{
        "name": "lidar_provider",
        "scope": "process",
        "capabilities": ["perception.lidar.samples"],
        "target": null,
        "process": "sensor_proc",
        "external_package": null,
        "readiness_source": "provider_ready",
        "health_source": "provider_health"
      }],
      "requirements": [{
        "instance": "sensor",
        "component": "sensor",
        "name": "lidar_uart",
        "capability": "perception.lidar.samples",
        "access": "read_write",
        "required": true,
        "readiness": "before_start",
        "health": "required",
        "on_failure": "stop_process",
        "satisfaction": "satisfied",
        "provider": "lidar_provider",
        "diagnostic": null
      }, {
        "instance": "detector",
        "component": "detector",
        "name": "accelerator",
        "capability": "compute.acceleration.inference",
        "access": "read",
        "required": false,
        "readiness": "lazy",
        "health": "optional",
        "on_failure": "degrade",
        "satisfaction": "optional_unsatisfied",
        "provider": null,
        "diagnostic": "optional resource has no provider"
      }],
      "satisfactions": []
    },
    "instances": [],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("selfdesc-resource-contract");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);

    assert!(list.contains("resource_contract_version=0.1"));
    assert!(list.contains("resource_provider lidar_provider scope=process capabilities=perception.lidar.samples readiness_source=provider_ready health_source=provider_health process=sensor_proc"));
    assert!(list.contains("resource_requirement sensor.lidar_uart component=sensor capability=perception.lidar.samples access=read_write required=true readiness=before_start health=required on_failure=stop_process satisfaction=satisfied provider=lidar_provider"));
    assert!(list.contains("resource_requirement detector.accelerator component=detector capability=compute.acceleration.inference access=read required=false readiness=lazy health=optional on_failure=degrade satisfaction=optional_unsatisfied provider=none diagnostic=optional resource has no provider"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn self_description_loader_rejects_unsupported_resource_contract_version() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "resource_demo" },
  "graphs": [{
    "name": "default",
    "resource_contract": {
      "resource_contract_version": "9.9",
      "providers": [],
      "requirements": [],
      "satisfactions": []
    },
    "instances": [],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("selfdesc-resource-contract-version");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let error = format!("{:#}", load_self_description(&path).unwrap_err());

    assert!(error.contains("unsupported FlowRT resource contract version `9.9`"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn self_description_summary_shows_per_instance_component_view() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "view_demo" },
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "planner",
      "component": "planner_comp",
      "process": "main",
      "runtime": "rust",
      "params": [{ "name": "goal_x", "type": "f64", "update": "on_tick", "current": 1.0 }]
    }, {
      "name": "executor",
      "component": "executor_comp",
      "process": "main",
      "runtime": "rust"
    }],
    "tasks": [{
      "name": "plan_task",
      "instance": "planner",
      "trigger": "on_message",
      "lane": "plan_lane"
    }],
    "channels": [{
      "from": "planner.cmd",
      "to": "executor.cmd",
      "message_type": "Cmd",
      "backend": "inproc"
    }],
    "services": [{
      "name": "planner.plan_to_executor.execute",
      "client_instance": "planner",
      "client_port": "plan",
      "server_instance": "executor",
      "server_port": "execute",
      "request_type": "PlanReq",
      "response_type": "PlanResp",
      "backend": "inproc"
    }]
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("selfdesc-component-view");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);

    // instance 下应展示 task、channel、service 和 param。
    assert!(list.contains("instance planner component=planner_comp process=main runtime=rust"));
    assert!(list.contains("task plan_task trigger=on_message lane=plan_lane"));
    assert!(list.contains("channel planner.cmd -> executor.cmd type=Cmd backend=inproc"));
    assert!(list.contains("service planner.plan_to_executor.execute"));
    assert!(list.contains("param goal_x:f64 update=on_tick current=1.0"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn self_description_summary_handles_no_component_types() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "old_demo" },
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "src",
      "component": "imu_sim",
      "process": "main",
      "runtime": "rust"
    }],
    "tasks": [],
    "channels": []
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("selfdesc-no-comp-types");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let list = self_description_summary(&self_description);

    // 旧版 JSON 没有 component_types 字段，serde(default) 给空 Vec。
    assert!(list.contains("component_types=0"));
    assert!(list.contains("instance src component=imu_sim"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn self_description_nodes_shows_kind_when_available() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "kind_demo" },
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "sensor",
      "component": "imu_sensor",
      "process": "main",
      "runtime": "rust"
    }]
  }],
  "component_types": [{
    "name": "imu_sensor",
    "language": "rust",
    "kind": "native"
  }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("selfdesc-nodes-kind");
    let path = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&path, source).unwrap();

    let self_description = load_self_description(&path).unwrap();
    let nodes = self_description_nodes(&self_description);

    assert!(nodes.contains("sensor process=main runtime=rust component=imu_sensor kind=native"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_displays_task_and_lane_health() {
    let root = temp_test_dir("live-status-health");
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
    state.record_task_health(flowrt::IntrospectionTaskHealth {
        name: "imu_task".to_string(),
        lane: "sensor_lane".to_string(),
        inflight: true,
        scheduled_time_ms: Some(1000),
        observed_time_ms: Some(1012),
        lateness_ms: Some(12),
        missed_periods: Some(1),
        overrun: Some(true),
        deadline_missed: 3,
        stale_input: 1,
        backpressure: 0,
        overflow: 2,
        fairness_violations: 0,
        run_count: 100,
        success_count: 97,
        consecutive_failures: 0,
        last_run_ms: Some(1000),
        last_success_ms: Some(999),
    });
    state.record_lane_health(flowrt::IntrospectionLaneHealth {
        name: "sensor_lane".to_string(),
        queue_depth: 2,
        dispatched_count: 500,
        fairness_violations: 0,
    });

    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(
        output.contains("task_health=imu_task"),
        "expected task_health=imu_task in output: {output}"
    );
    assert!(
        output.contains("lane=sensor_lane"),
        "expected lane=sensor_lane in output: {output}"
    );
    assert!(
        output.contains("inflight=true"),
        "expected inflight=true in output: {output}"
    );
    assert!(
        output.contains("scheduled_time_ms=1000"),
        "expected scheduled_time_ms=1000 in output: {output}"
    );
    assert!(
        output.contains("observed_time_ms=1012"),
        "expected observed_time_ms=1012 in output: {output}"
    );
    assert!(
        output.contains("lateness_ms=12"),
        "expected lateness_ms=12 in output: {output}"
    );
    assert!(
        output.contains("missed_periods=1"),
        "expected missed_periods=1 in output: {output}"
    );
    assert!(
        output.contains("overrun=true"),
        "expected overrun=true in output: {output}"
    );
    assert!(
        output.contains("timing=runtime_observed"),
        "expected timing=runtime_observed in output: {output}"
    );
    assert!(
        output.contains("deadline_missed=3"),
        "expected deadline_missed=3 in output: {output}"
    );
    assert!(
        output.contains("stale_input=1"),
        "expected stale_input=1 in output: {output}"
    );
    assert!(
        output.contains("overflow=2"),
        "expected overflow=2 in output: {output}"
    );
    assert!(
        output.contains("fairness_violations=0"),
        "expected fairness_violations=0 in output: {output}"
    );
    assert!(
        output.contains("runs=100"),
        "expected runs=100 in output: {output}"
    );
    assert!(
        output.contains("successes=97"),
        "expected successes=97 in output: {output}"
    );
    assert!(
        output.contains("lane_health=sensor_lane"),
        "expected lane_health=sensor_lane in output: {output}"
    );
    assert!(
        output.contains("queue_depth=2"),
        "expected queue_depth=2 in output: {output}"
    );
    assert!(
        output.contains("dispatched_count=500"),
        "expected dispatched_count=500 in output: {output}"
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn live_status_summary_displays_graph_health_metrics() {
    let root = temp_test_dir("live-status-graph-health");
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
    state.register_critical_instances(["controller_a", "controller_b"]);
    state.record_lifecycle_transition(
        "controller_a",
        flowrt::LifecycleState::Running,
        Some(1),
        None,
    );
    state.record_lifecycle_transition(
        "controller_b",
        flowrt::LifecycleState::Running,
        Some(1),
        None,
    );
    state.record_lifecycle_transition(
        "monitor",
        flowrt::LifecycleState::Faulted,
        Some(7),
        Some("sensor_timeout"),
    );
    state.record_instance_restart("monitor");

    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = live_status_summary_for_sockets(vec![socket], false).unwrap();

    assert!(
        output.contains("graph_health=faulted"),
        "expected graph_health=faulted in output: {output}"
    );
    assert!(
        output.contains("graph_critical_health=healthy"),
        "expected graph_critical_health=healthy in output: {output}"
    );
    assert!(
        output.contains("critical_instances=controller_a,controller_b"),
        "expected critical_instances in output: {output}"
    );
    assert!(
        output.contains("instance=monitor lifecycle=faulted restart_count=1"),
        "expected monitor lifecycle metrics in output: {output}"
    );
    assert!(
        output.contains("last_fault_reason=sensor_timeout"),
        "expected last_fault_reason in output: {output}"
    );
    assert!(
        output.contains("last_fault_tick=7"),
        "expected last_fault_tick in output: {output}"
    );
    assert!(
        output.contains("last_transition_tick=7"),
        "expected last_transition_tick in output: {output}"
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}
