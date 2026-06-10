use super::*;

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
fn live_status_summary_keeps_status_when_selfdesc_enrichment_stalls() {
    let root = temp_test_dir("live-status-stalled-selfdesc");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
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
            io_boundaries: Vec::new(),
            services: Vec::new(),
            operations: Vec::new(),
            tasks: Vec::new(),
            lanes: Vec::new(),
            recorder: Default::default(),
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
            io_boundaries: Vec::new(),
            services: Vec::new(),
            operations: Vec::new(),
            tasks: Vec::new(),
            lanes: Vec::new(),
            recorder: Default::default(),
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
      "response_type": "PlanResponse"
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

    let _ = std::fs::remove_dir_all(&root);
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
      "result_retention_ms": 60000,
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

    let _ = std::fs::remove_dir_all(&root);
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
        last_transition_ms: Some(12345),
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let status = operation_status_summary_for_sockets(None, vec![socket.clone()]).unwrap();
    assert!(status.contains("operation=controller.plan"));
    assert!(status.contains("current_operation_ids=[111:7:3]"));

    let canceled = operation_cancel("111:7:3", Some(&socket)).unwrap();
    assert!(canceled.contains("operation=controller.plan"));
    assert!(canceled.contains("operation_id=111:7:3"));
    assert!(canceled.contains("canceled=1"));

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
