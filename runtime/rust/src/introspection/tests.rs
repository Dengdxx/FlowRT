use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;
use std::time::Duration;

use crate::{BackendHealthSnapshot, BackendHealthState};

use super::*;

mod status_facts;

#[test]
fn socket_path_uses_pid_name_under_runtime_dir() {
    let dir = runtime_socket_dir();
    let path = runtime_socket_path_for_pid(1234);

    assert_eq!(path, dir.join("1234.sock"));
}

#[test]
fn status_server_returns_handshake_and_snapshot() {
    let root =
        std::env::temp_dir().join(format!("flowrt-introspection-test-{}", std::process::id()));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    for _ in 0..7 {
        state.record_tick_at(10, "simulated_replay");
    }
    state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![1u8; 48], Some(7));
    state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![2u8; 48], Some(8));
    state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![3u8; 48], Some(9));

    let server = spawn_status_server_at(socket.clone(), handshake.clone(), state.clone())
        .expect("server should start");
    let IntrospectionResponse::Status {
        handshake: response_handshake,
        status,
    } = request_status(server.path()).expect("status request should succeed")
    else {
        panic!("status request returned wrong response")
    };

    assert_eq!(response_handshake, handshake);
    assert_eq!(status.tick_count, 7);
    assert_eq!(status.clock.source, "simulated_replay");
    assert_eq!(status.clock.tick_time_ms, Some(10));
    assert_eq!(status.clock.unit, "ms");
    assert_eq!(
        status.channels,
        vec![IntrospectionChannelStatus {
            name: "source.imu_to_sink.imu".to_string(),
            message_type: "Imu".to_string(),
            published_count: 3,
            last_payload_len: Some(48),
            active_observers: 0,
            dropped_samples: 0,
        }]
    );

    state.record_tick_at(11, "simulated_replay");
    let IntrospectionResponse::Status { status, .. } =
        request_status(server.path()).expect("second status request should succeed")
    else {
        panic!("status request returned wrong response")
    };
    assert_eq!(status.tick_count, 8);
    assert_eq!(status.clock.tick_time_ms, Some(11));

    let IntrospectionResponse::ChannelSnapshot { channel, .. } =
        request_channel_snapshot(server.path(), "source.imu_to_sink.imu")
            .expect("snapshot request should succeed")
    else {
        panic!("snapshot request returned wrong response")
    };
    assert_eq!(channel.published_count, 3);
    assert_eq!(channel.payload, Some(vec![3u8; 48]));
    assert_eq!(channel.published_at_ms, Some(9));
    let channel_json = serde_json::to_value(&channel).unwrap();
    assert!(channel_json.get("name").is_none());
    assert!(channel_json.get("message_type").is_none());

    let IntrospectionResponse::Error { message, .. } =
        request_channel_snapshot(server.path(), "missing.channel")
            .expect("missing channel should return structured error response")
    else {
        panic!("missing channel request returned wrong response")
    };
    assert_eq!(message, "unknown FlowRT channel");

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn status_server_returns_registered_self_description_json() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-selfdesc-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.set_self_description_json(r#"{"package":{"name":"robot_demo"}}"#);
    let server = spawn_status_server_at(socket.clone(), handshake.clone(), state)
        .expect("server should start");

    let IntrospectionResponse::SelfDescription {
        handshake: response_handshake,
        json,
    } = request_self_description(server.path()).expect("self-description request should succeed")
    else {
        panic!("self-description request returned wrong response")
    };

    assert_eq!(response_handshake, handshake);
    assert_eq!(json, r#"{"package":{"name":"robot_demo"}}"#);

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn status_includes_supervisor_process_health() {
    let state = IntrospectionState::new();

    state.record_process_health(IntrospectionProcessStatus {
        name: "sensors".to_string(),
        state: "running".to_string(),
        pid: Some(42),
        restart_count: 1,
        tick_count: Some(7),
        last_seen_unix_ms: Some(1000),
        tick_stale: false,
        exit_code: None,
        readiness_wait: None,
        resource_placement: None,
    });

    assert_eq!(
        state.status().processes,
        vec![IntrospectionProcessStatus {
            name: "sensors".to_string(),
            state: "running".to_string(),
            pid: Some(42),
            restart_count: 1,
            tick_count: Some(7),
            last_seen_unix_ms: Some(1000),
            tick_stale: false,
            exit_code: None,
            readiness_wait: None,
            resource_placement: None,
        }]
    );
}

#[test]
fn status_server_reports_missing_self_description() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-missing-selfdesc-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let server = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
        .expect("server should start");

    let IntrospectionResponse::Error { message, .. } = request_self_description(server.path())
        .expect("missing self-description should return structured error")
    else {
        panic!("missing self-description request returned wrong response")
    };

    assert_eq!(message, "FlowRT self-description is not registered");

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn state_recovers_after_mutex_poison() {
    let state = IntrospectionState::new();
    let poison_state = state.clone();
    let poison_thread = thread::spawn(move || {
        let _guard = poison_state.inner.lock().unwrap();
        panic!("poison introspection state for test");
    });
    assert!(poison_thread.join().is_err());

    state.register_channel("source.count_to_sink.count", "Count");
    state.record_tick();
    state.record_channel_publish_bytes("source.count_to_sink.count", "Count", vec![7], Some(1));
    state.register_param(IntrospectionParamSchema {
        name: "controller.kp".to_string(),
        ty: "f32".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: None,
        max: None,
        choices: Vec::new(),
    });

    let status = state.status();
    assert_eq!(status.tick_count, 1);
    assert_eq!(status.channels.len(), 1);
    assert_eq!(
        state
            .channel_snapshot("source.count_to_sink.count")
            .unwrap()
            .payload,
        Some(vec![7])
    );
    assert!(
        state
            .set_param_pending("controller.kp", serde_json::json!(2.0))
            .is_ok()
    );
    assert_eq!(
        state.pending_param("controller.kp"),
        Some(serde_json::json!(2.0))
    );
    assert_eq!(
        state.take_pending_param("controller.kp"),
        Some(serde_json::json!(2.0))
    );
    state.record_param_applied("controller.kp", serde_json::json!(2.0));
    assert_eq!(
        state.param("controller.kp").unwrap().current,
        serde_json::json!(2.0)
    );
}

#[test]
fn probe_recording_is_disabled_until_observer_guard_is_active() {
    let state = IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");

    assert!(
        !state
            .try_probe_channel_publish_bytes(
                "source.imu_to_sink.imu",
                "Imu",
                &[1, 2, 3, 4],
                Some(10)
            )
            .recorded
    );
    let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
    assert_eq!(snapshot.published_count, 0);
    assert_eq!(snapshot.payload, None);

    let guard = state
        .observe_channel("source.imu_to_sink.imu")
        .expect("registered channel should be observable");
    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(1));
    assert!(
        state
            .try_probe_channel_publish_bytes(
                "source.imu_to_sink.imu",
                "Imu",
                &[5, 6, 7, 8],
                Some(11)
            )
            .recorded
    );
    let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
    assert_eq!(snapshot.published_count, 0);
    assert_eq!(snapshot.payload, Some(vec![5, 6, 7, 8]));
    assert_eq!(snapshot.published_at_ms, Some(11));

    drop(guard);
    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));
    assert!(
        !state
            .try_probe_channel_publish_bytes(
                "source.imu_to_sink.imu",
                "Imu",
                &[9, 10, 11, 12],
                Some(12)
            )
            .recorded
    );
    let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
    assert_eq!(snapshot.published_count, 0);
    assert_eq!(snapshot.payload, Some(vec![5, 6, 7, 8]));
}

#[test]
fn publish_event_updates_status_count_without_payload_or_observer() {
    let state = IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    let probe = state
        .channel_probe("source.imu_to_sink.imu")
        .expect("registered channel should expose probe");

    probe.record_publish_event();
    probe.record_publish_event();

    let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
    assert_eq!(snapshot.published_count, 2);
    assert_eq!(snapshot.payload, None);
    assert_eq!(snapshot.published_at_ms, None);
    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));
}

#[test]
fn bounded_probe_drops_oversized_payload_and_reports_drop_count() {
    let state = IntrospectionState::new();
    state.register_channel_with_probe_capacity("source.image_to_sink.image", "Image", Some(4));
    let guard = state
        .observe_channel("source.image_to_sink.image")
        .expect("registered channel should be observable");

    let record = state.try_probe_channel_publish_bytes(
        "source.image_to_sink.image",
        "Image",
        &[1, 2, 3, 4, 5],
        Some(10),
    );
    let snapshot = state
        .channel_snapshot("source.image_to_sink.image")
        .expect("registered channel should have snapshot state");
    let status = state
        .channel_status("source.image_to_sink.image")
        .expect("registered channel should have status");

    assert_eq!(
        record,
        IntrospectionProbeRecord {
            recorded: false,
            dropped: true,
        }
    );
    assert_eq!(snapshot.published_count, 0);
    assert_eq!(snapshot.payload.as_deref(), Some([].as_slice()));
    assert_eq!(status.active_observers, 1);
    assert_eq!(status.dropped_samples, 1);

    drop(guard);
    assert_eq!(
        state.active_probe_count("source.image_to_sink.image"),
        Some(0)
    );
}

#[test]
fn observe_channel_socket_enables_probe_until_connection_closes() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-observe-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("server should start");

    let mut stream = UnixStream::connect(server.path()).unwrap();
    stream
        .write_all(
            br#"{"command":"observe_channel","channel":"source.imu_to_sink.imu","mode":"latest"}
"#,
        )
        .unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains(r#""response":"observe_ready""#));

    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(1));
    assert!(
        state
            .try_probe_channel_publish_bytes("source.imu_to_sink.imu", "Imu", &[1, 2, 3], Some(7))
            .recorded
    );
    assert_eq!(
        state
            .channel_snapshot("source.imu_to_sink.imu")
            .unwrap()
            .payload,
        Some(vec![1, 2, 3])
    );

    drop(reader);
    drop(stream);
    for _ in 0..100 {
        if state.active_probe_count("source.imu_to_sink.imu") == Some(0) {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn observe_unknown_channel_returns_error_without_enabling_probe() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-observe-missing-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("server should start");

    let (_stream, response) = observe_channel_stream(server.path(), "missing.channel")
        .expect("missing channel should return structured error");

    assert!(matches!(
        response,
        IntrospectionResponse::Error { message, .. } if message == "unknown FlowRT channel"
    ));
    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));
    assert_eq!(state.active_probe_count("missing.channel"), None);

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn observe_channel_stream_helper_keeps_probe_enabled_until_stream_drops() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-observe-helper-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 43,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("server should start");

    let (stream, response) =
        observe_channel_stream(server.path(), "source.imu_to_sink.imu").unwrap();
    assert!(matches!(
        response,
        IntrospectionResponse::ObserveReady { .. }
    ));
    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(1));
    assert!(
        state
            .try_probe_channel_publish_bytes("source.imu_to_sink.imu", "Imu", &[9, 8, 7], Some(8))
            .recorded
    );

    drop(stream);
    for _ in 0..100 {
        if state.active_probe_count("source.imu_to_sink.imu") == Some(0) {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn observe_connections_do_not_exhaust_status_control_plane() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-observe-cap-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 44,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("server should start");

    let mut streams = Vec::new();
    for _ in 0..MAX_INTROSPECTION_OBSERVERS {
        let (stream, response) =
            observe_channel_stream(server.path(), "source.imu_to_sink.imu").unwrap();
        assert!(matches!(
            response,
            IntrospectionResponse::ObserveReady { .. }
        ));
        streams.push(stream);
    }
    assert_eq!(
        state.active_probe_count("source.imu_to_sink.imu"),
        Some(MAX_INTROSPECTION_OBSERVERS as u64)
    );

    let response = request_status_with_timeout(server.path(), Duration::from_millis(100))
        .expect("status request should remain available while observe streams are open");
    assert!(matches!(response, IntrospectionResponse::Status { .. }));

    let (_extra_stream, response) = observe_channel_stream(server.path(), "source.imu_to_sink.imu")
        .expect("excess observe should receive structured error");
    assert!(matches!(
        response,
        IntrospectionResponse::Error { message, .. }
            if message == "FlowRT introspection observe connection limit reached"
    ));

    drop(streams);
    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn idle_clients_are_closed_by_initial_request_timeout() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-idle-cap-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 45,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let server = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
        .expect("server should start");

    let mut idle_streams = Vec::new();
    for _ in 0..MAX_INTROSPECTION_CLIENT_THREADS {
        idle_streams.push(UnixStream::connect(server.path()).unwrap());
    }
    thread::sleep(INTROSPECTION_INITIAL_REQUEST_TIMEOUT + Duration::from_millis(100));

    let response = request_status_with_timeout(server.path(), Duration::from_millis(100))
        .expect("idle clients should time out and release connection slots");
    assert!(matches!(response, IntrospectionResponse::Status { .. }));

    drop(idle_streams);
    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn status_server_refuses_to_replace_live_socket() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-live-socket-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let first =
        spawn_status_server_at(socket.clone(), handshake.clone(), IntrospectionState::new())
            .expect("first server should start");

    let error = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
        .expect_err("live socket must not be replaced by a second server");

    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    assert!(request_status(first.path()).is_ok());

    drop(first);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn introspection_client_permit_limits_and_releases_active_connections() {
    let active = Arc::new(AtomicUsize::new(0));
    let first = try_acquire_introspection_client_permit(&active, 1)
        .expect("first client should acquire permit");

    assert_eq!(active.load(Ordering::Acquire), 1);
    assert!(try_acquire_introspection_client_permit(&active, 1).is_none());
    assert_eq!(active.load(Ordering::Acquire), 1);

    drop(first);
    assert_eq!(active.load(Ordering::Acquire), 0);
    assert!(try_acquire_introspection_client_permit(&active, 1).is_some());
}

#[test]
fn status_server_removes_stale_socket_file_before_binding() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-stale-socket-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    fs::create_dir_all(&root).unwrap();
    fs::write(&socket, b"stale").unwrap();
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };

    let server = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
        .expect("stale socket path should be reclaimed");

    assert!(request_status(server.path()).is_ok());
    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn status_server_handles_runtime_parameter_requests() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-params-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.register_param(IntrospectionParamSchema {
        name: "controller.kp".to_string(),
        ty: "f32".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: Some(serde_json::json!(0.0)),
        max: Some(serde_json::json!(10.0)),
        choices: Vec::new(),
    });
    state.register_param(IntrospectionParamSchema {
        name: "controller.mode".to_string(),
        ty: "string".to_string(),
        update: "startup".to_string(),
        current: serde_json::json!("normal"),
        min: None,
        max: None,
        choices: vec![serde_json::json!("normal"), serde_json::json!("safe")],
    });

    let server = spawn_status_server_at(socket.clone(), handshake.clone(), state.clone())
        .expect("server should start");

    let IntrospectionResponse::ParamList { params, .. } =
        request_param_list(server.path()).expect("param list request should succeed")
    else {
        panic!("param list returned wrong response")
    };
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].name, "controller.kp");
    assert_eq!(params[0].current, serde_json::json!(1.0));
    assert!(params[0].pending.is_none());

    let IntrospectionResponse::ParamValue { param, .. } =
        request_param_get(server.path(), "controller.kp")
            .expect("param get request should succeed")
    else {
        panic!("param get returned wrong response")
    };
    assert_eq!(param.current, serde_json::json!(1.0));

    let IntrospectionResponse::ParamValue { param, .. } =
        request_param_set(server.path(), "controller.kp", serde_json::json!(2.5))
            .expect("param set request should succeed")
    else {
        panic!("param set returned wrong response")
    };
    assert_eq!(param.current, serde_json::json!(1.0));
    assert_eq!(param.pending, Some(serde_json::json!(2.5)));
    assert_eq!(
        state.pending_param("controller.kp"),
        Some(serde_json::json!(2.5))
    );
    assert_eq!(
        state.peek_pending_param("controller.kp"),
        Some(serde_json::json!(2.5))
    );
    state.record_param_applied("controller.kp", serde_json::json!(2.5));
    assert_eq!(state.pending_param("controller.kp"), None);

    let IntrospectionResponse::Error { message, .. } =
        request_param_set(server.path(), "controller.mode", serde_json::json!("safe"))
            .expect("startup param set should return structured error")
    else {
        panic!("startup param set returned wrong response")
    };
    assert_eq!(
        message,
        "FlowRT parameter `controller.mode` is startup-only"
    );

    let IntrospectionResponse::Error { message, .. } =
        request_param_set(server.path(), "controller.kp", serde_json::json!(12.0))
            .expect("out-of-range param set should return structured error")
    else {
        panic!("out-of-range param set returned wrong response")
    };
    assert_eq!(message, "FlowRT parameter `controller.kp` is above maximum");

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn params_apply_state_preserves_newer_pending_and_rejects_without_current_change() {
    let state = IntrospectionState::new();
    state.register_param(IntrospectionParamSchema {
        name: "controller.kp".to_string(),
        ty: "f32".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: Some(serde_json::json!(0.0)),
        max: Some(serde_json::json!(10.0)),
        choices: Vec::new(),
    });

    state
        .set_param_pending("controller.kp", serde_json::json!(2.0))
        .expect("first pending value should be accepted");
    let boundary_value = state
        .peek_pending_param("controller.kp")
        .expect("scheduler boundary should inspect pending without clearing it");
    assert_eq!(
        state.pending_param("controller.kp"),
        Some(boundary_value.clone())
    );

    state
        .set_param_pending("controller.kp", serde_json::json!(3.0))
        .expect("newer pending value should remain observable");
    state.record_param_applied("controller.kp", boundary_value);
    let status = state.param("controller.kp").unwrap();
    assert_eq!(status.current, serde_json::json!(2.0));
    assert_eq!(status.pending, Some(serde_json::json!(3.0)));

    let rejected = state.peek_pending_param("controller.kp").unwrap();
    state.record_param_rejected("controller.kp", rejected, "callback_rejected");
    let status = state.param("controller.kp").unwrap();
    assert_eq!(status.current, serde_json::json!(2.0));
    assert_eq!(status.pending, None);
}

#[test]
fn param_runtime_validation_compares_large_integer_bounds_exactly() {
    let param = ParamState {
        ty: "u64".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(9007199254740992_u64),
        pending: None,
        apply_state: "applied".to_string(),
        last_reject_reason: None,
        updated_unix_ms: None,
        min: None,
        max: Some(serde_json::json!(9007199254740992_u64)),
        choices: vec![],
    };

    let error = validate_param_json_value(
        "controller.limit",
        &param,
        &serde_json::json!(9007199254740993_u64),
    )
    .expect_err("value above a large integer max must be rejected exactly");

    assert_eq!(
        error,
        "FlowRT parameter `controller.limit` is above maximum"
    );
}

#[test]
fn status_server_reports_and_cancels_operation_status() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-operation-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.register_operation("controller.plan");
    state.record_operation_health(IntrospectionOperationStatus {
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

    let server = spawn_status_server_at(socket.clone(), handshake.clone(), state.clone())
        .expect("server should start");

    let IntrospectionResponse::Status { status, .. } =
        request_status(server.path()).expect("operation status request should succeed")
    else {
        panic!("status returned wrong response")
    };
    assert_eq!(status.operations.len(), 1);
    assert_eq!(status.operations[0].name, "controller.plan");
    assert_eq!(status.operations[0].running, 1);
    assert_eq!(
        status.operations[0].current_operation_ids,
        vec!["111:7:3".to_string()]
    );

    let IntrospectionResponse::OperationValue { operation, .. } =
        request_operation_status(server.path(), "111:7:3")
            .expect("operation status by id request should succeed")
    else {
        panic!("operation status by id returned wrong response")
    };
    assert_eq!(operation.name, "controller.plan");
    assert_eq!(operation.running, 1);
    assert_eq!(operation.current_state.as_deref(), Some("running"));
    assert_eq!(operation.current_operation_ids, vec!["111:7:3".to_string()]);

    let IntrospectionResponse::OperationValue { operation, .. } =
        request_operation_cancel(server.path(), "111:7:3")
            .expect("operation cancel request should succeed")
    else {
        panic!("operation cancel returned wrong response")
    };
    assert_eq!(operation.name, "controller.plan");
    assert_eq!(operation.running, 1);
    assert_eq!(operation.canceled_count, 0);
    assert_eq!(operation.current_state.as_deref(), Some("cancel_requested"));
    assert_eq!(operation.current_operation_ids, vec!["111:7:3".to_string()]);

    let IntrospectionResponse::OperationValue { operation, .. } =
        request_operation_cancel(server.path(), "111:7:3")
            .expect("repeated current cancel should be idempotent")
    else {
        panic!("second operation cancel returned wrong response")
    };
    assert_eq!(operation.current_state.as_deref(), Some("cancel_requested"));

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn status_server_starts_operation_with_registered_handler() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-operation-start-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.register_operation("controller.plan");
    state.register_operation_start_handler("controller.plan", |payload, timeout_ms, owner| {
        assert_eq!(payload, vec![7, 0, 0, 0]);
        assert_eq!(timeout_ms, Some(2500));
        assert_eq!(owner.as_deref(), Some("flowrt.cli"));
        Ok(IntrospectionOperationStartStatus {
            operation_id: "111:7:3".to_string(),
            operation: IntrospectionOperationStatus {
                name: "controller.plan".to_string(),
                ready: true,
                running: 1,
                queued: 0,
                current_operation_ids: vec!["111:7:3".to_string()],
                total_started: 1,
                current_state: Some("starting".to_string()),
                current_owner: Some("flowrt.cli".to_string()),
                current_deadline_ms: Some(2500),
                last_event: Some("flowrt.operation.state_changed".to_string()),
                ..Default::default()
            },
        })
    });

    let server = spawn_status_server_at(socket.clone(), handshake.clone(), state.clone())
        .expect("server should start");

    let IntrospectionResponse::OperationStarted { started, .. } = request_operation_start(
        server.path(),
        "controller.plan",
        vec![7, 0, 0, 0],
        Some(2500),
        Some("flowrt.cli".to_string()),
    )
    .expect("operation start request should succeed") else {
        panic!("operation start returned wrong response")
    };
    assert_eq!(started.operation_id, "111:7:3");
    assert_eq!(started.operation.name, "controller.plan");
    assert_eq!(started.operation.current_state.as_deref(), Some("starting"));

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn operation_result_request_returns_retained_payload() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-operation-result-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    state.record_operation_result_payload(
        "controller.plan",
        "111:7:3",
        "succeeded",
        None,
        Some(vec![1, 0, 0, 0]),
    );

    let server =
        spawn_status_server_at(socket.clone(), handshake, state).expect("server should start");

    let IntrospectionResponse::OperationResult { result, .. } =
        request_operation_result(server.path(), "111:7:3")
            .expect("operation result request should succeed")
    else {
        panic!("operation result returned wrong response")
    };
    assert_eq!(result.operation_id, "111:7:3");
    assert_eq!(result.operation, "controller.plan");
    assert_eq!(result.state, "succeeded");
    assert_eq!(result.result.as_deref(), Some("succeeded"));
    assert_eq!(result.error, None);
    assert_eq!(result.payload, Some(vec![1, 0, 0, 0]));

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn operation_result_retention_evicts_result_and_events() {
    let state = IntrospectionState::new();
    state.record_operation_transition(
        "controller.plan",
        "111:7:3",
        "running",
        Some("controller.plan"),
        Some(50),
    );
    state.record_operation_progress_payload("controller.plan", "111:7:3", 1, Some(vec![1, 2]));
    state.record_operation_result_payload_with_retention(
        "controller.plan",
        "111:7:3",
        "succeeded",
        None,
        Some(vec![3, 4]),
        Some(10),
    );

    let result = state
        .result_operation("111:7:3")
        .expect("retained result should be visible before expiry");
    assert_eq!(result.payload, Some(vec![3, 4]));
    assert_eq!(
        result.expires_unix_ms,
        result.completed_unix_ms.map(|now| now + 10)
    );
    let (events, _, terminal) = state
        .observe_operation("111:7:3", 0, None)
        .expect("retained events should be visible before expiry");
    assert_eq!(events.len(), 3);
    assert!(terminal);

    state.evict_retained_operation_observations(
        result
            .expires_unix_ms
            .expect("retained result should carry expiry")
            .saturating_add(1),
    );

    assert!(state.result_operation("111:7:3").is_err());
    assert!(state.observe_operation("111:7:3", 0, None).is_err());
}

#[test]
fn operation_result_zero_retention_drops_result_and_events_immediately() {
    let state = IntrospectionState::new();
    state.record_operation_transition(
        "controller.plan",
        "111:7:4",
        "running",
        Some("controller.plan"),
        Some(50),
    );
    state.record_operation_progress_payload("controller.plan", "111:7:4", 1, Some(vec![1, 2]));
    state.record_operation_result_payload_with_retention(
        "controller.plan",
        "111:7:4",
        "succeeded",
        None,
        Some(vec![3, 4]),
        Some(0),
    );

    assert!(state.result_operation("111:7:4").is_err());
    assert!(state.observe_operation("111:7:4", 0, None).is_err());
}

#[test]
fn operation_observe_request_returns_ordered_events_and_terminal_flag() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-operation-observe-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
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
        Some(vec![1, 0, 0, 0]),
    );

    let server =
        spawn_status_server_at(socket.clone(), handshake, state).expect("server should start");

    let IntrospectionResponse::OperationEvents {
        operation_id,
        events,
        next_sequence,
        terminal,
        ..
    } = request_operation_observe(server.path(), "111:7:3", 0, Some(10))
        .expect("operation observe request should succeed")
    else {
        panic!("operation observe returned wrong response")
    };
    assert_eq!(operation_id, "111:7:3");
    assert!(terminal);
    assert_eq!(next_sequence, 3);
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].sequence, 0);
    assert_eq!(events[0].kind, "state");
    assert_eq!(events[0].state.as_deref(), Some("running"));
    assert_eq!(events[1].sequence, 1);
    assert_eq!(events[1].kind, "progress");
    assert_eq!(events[1].progress_sequence, Some(0));
    assert_eq!(events[1].payload, Some(vec![7, 0, 0, 0]));
    assert_eq!(events[2].sequence, 2);
    assert_eq!(events[2].kind, "result");
    assert_eq!(events[2].state.as_deref(), Some("succeeded"));
    assert_eq!(events[2].payload, Some(vec![1, 0, 0, 0]));

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn task_health_recording_and_status_snapshot() {
    let state = IntrospectionState::new();

    state.record_task_health(IntrospectionTaskHealth {
        name: "imu_task".to_string(),
        lane: "sensor_lane".to_string(),
        inflight: true,
        scheduled_time_ms: Some(1_000),
        observed_time_ms: Some(1_012),
        lateness_ms: Some(12),
        missed_periods: Some(1),
        overrun: Some(true),
        deadline_missed: 3,
        stale_input: 1,
        backpressure: 0,
        overflow: 0,
        fairness_violations: 0,
        run_count: 100,
        success_count: 97,
        consecutive_failures: 0,
        last_run_ms: Some(1000),
        last_success_ms: Some(1000),
    });

    state.record_task_health(IntrospectionTaskHealth {
        name: "control_task".to_string(),
        lane: "control_lane".to_string(),
        deadline_missed: 0,
        stale_input: 0,
        backpressure: 5,
        overflow: 2,
        fairness_violations: 1,
        run_count: 50,
        success_count: 48,
        consecutive_failures: 1,
        last_run_ms: Some(2000),
        last_success_ms: Some(1900),
        ..Default::default()
    });

    let status = state.status();
    assert_eq!(status.tasks.len(), 2);

    let imu = state.task_health("imu_task").unwrap();
    assert_eq!(imu.name, "imu_task");
    assert_eq!(imu.deadline_missed, 3);
    assert!(imu.inflight);
    assert_eq!(imu.scheduled_time_ms, Some(1_000));
    assert_eq!(imu.observed_time_ms, Some(1_012));
    assert_eq!(imu.lateness_ms, Some(12));
    assert_eq!(imu.missed_periods, Some(1));
    assert_eq!(imu.overrun, Some(true));
    assert_eq!(imu.run_count, 100);
    assert_eq!(imu.success_count, 97);
    assert_eq!(imu.consecutive_failures, 0);

    let control = state.task_health("control_task").unwrap();
    assert_eq!(control.backpressure, 5);
    assert_eq!(control.overflow, 2);
    assert_eq!(control.consecutive_failures, 1);
    assert_eq!(control.last_success_ms, Some(1900));

    assert!(state.task_health("missing_task").is_none());
}

#[test]
fn io_boundary_health_recording_and_status_snapshot() {
    let state = IntrospectionState::new();
    state.register_io_boundary(
        "camera",
        "CameraDriver",
        vec![IntrospectionIoBoundaryResourceStatus {
            name: "camera_shm".to_string(),
            kind: "shm".to_string(),
            ..Default::default()
        }],
    );

    state.mark_io_boundary_ready("camera", true);
    state.record_io_boundary_resource_ready("camera", "camera_shm", true, None);
    state.record_io_boundary_error("camera", "frame timeout");

    let status = state.status();
    assert_eq!(status.io_boundaries.len(), 1);
    let boundary = &status.io_boundaries[0];
    assert_eq!(boundary.name, "camera");
    assert_eq!(boundary.component, "CameraDriver");
    assert!(boundary.ready);
    assert!(!boundary.healthy);
    assert_eq!(boundary.last_error.as_deref(), Some("frame timeout"));
    assert_eq!(boundary.resources.len(), 1);
    assert_eq!(boundary.resources[0].name, "camera_shm");
    assert_eq!(boundary.resources[0].kind, "shm");
    assert!(boundary.resources[0].ready);
}

#[test]
fn boundary_publish_request_invokes_registered_handler() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-boundary-pub-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
    let captured_for_handler = Arc::clone(&captured);
    state.register_boundary_input_handler("sample_in", "Sample", move |payload, _| {
        *captured_for_handler.lock().unwrap() = payload.to_vec();
        Ok(7)
    });

    let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("server should start");

    let IntrospectionResponse::BoundaryPublish { boundary, .. } =
        request_boundary_publish(server.path(), "sample_in", vec![1, 2, 3, 4], Some(123))
            .expect("boundary publish request should succeed")
    else {
        panic!("boundary publish returned wrong response")
    };
    assert_eq!(boundary.endpoint, "sample_in");
    assert_eq!(boundary.message_type, "Sample");
    assert_eq!(boundary.revision, 7);
    assert_eq!(boundary.published_at_ms, Some(123));
    assert_eq!(*captured.lock().unwrap(), vec![1, 2, 3, 4]);

    let IntrospectionResponse::Error { message, .. } =
        request_boundary_publish(server.path(), "missing", vec![9], None)
            .expect("unknown boundary publish should return structured error")
    else {
        panic!("unknown boundary publish returned wrong response")
    };
    assert_eq!(message, "unknown FlowRT boundary input `missing`");

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn lane_health_recording_and_status_snapshot() {
    let state = IntrospectionState::new();

    state.record_lane_health(IntrospectionLaneHealth {
        name: "sensor_lane".to_string(),
        queue_depth: 2,
        dispatched_count: 500,
        fairness_violations: 0,
    });

    state.record_lane_health(IntrospectionLaneHealth {
        name: "control_lane".to_string(),
        queue_depth: 0,
        dispatched_count: 250,
        fairness_violations: 3,
    });

    let status = state.status();
    assert_eq!(status.lanes.len(), 2);

    let sensor = state.lane_health("sensor_lane").unwrap();
    assert_eq!(sensor.queue_depth, 2);
    assert_eq!(sensor.dispatched_count, 500);

    let control = state.lane_health("control_lane").unwrap();
    assert_eq!(control.queue_depth, 0);
    assert_eq!(control.dispatched_count, 250);

    assert!(state.lane_health("missing_lane").is_none());
}

#[test]
fn health_fields_serialize_with_defaults_for_backward_compat() {
    // 旧版 JSON 不含 operations/tasks/lanes 和新增 instance metrics 字段时应解析为默认值。
    let status: IntrospectionStatus =
        serde_json::from_str(
            r#"{"tick_count":1,"channels":[],"processes":[],"services":[],"instances":[{"instance":"controller","lifecycle_state":"running"}]}"#,
        )
        .unwrap();
    assert!(status.inputs.is_empty());
    assert!(status.routes.is_empty());
    assert!(status.operations.is_empty());
    assert!(status.tasks.is_empty());
    assert!(status.lanes.is_empty());
    assert_eq!(status.instances[0].restart_count, 0);
    assert_eq!(status.instances[0].last_fault_reason, None);
    assert_eq!(status.instances[0].last_fault_tick, None);
    assert_eq!(status.instances[0].last_transition_tick, None);
}

#[test]
fn recorder_disabled_does_not_capture_channel_payload() {
    let state = IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");

    let outcome = state.try_record_channel_sample_bytes(
        "source.imu_to_sink.imu",
        "Imu",
        &[1, 2, 3, 4],
        Some(10),
    );

    assert!(!outcome.recorded);
    assert!(!outcome.dropped);
    let status = state.status();
    assert!(!status.recorder.enabled);
    assert_eq!(status.recorder.bytes_written, 0);
    assert_eq!(state.drain_recorder_events().len(), 0);
}

#[test]
fn publish_boundary_input_records_canonical_stimulus_for_replay() {
    let state = IntrospectionState::new();
    state.register_boundary_input_handler("sample_in", "Sample", |_payload, _timestamp| Ok(1));
    state.start_recorder(IntrospectionRecorderStart {
        output: Some("memory://replay.mcap".to_string()),
        filters: vec!["channel:sample_in".to_string()],
        queue_depth: Some(4),
        package: "demo".to_string(),
        process: "main".to_string(),
        runtime_pid: 7,
        selfdesc_hash: "abc".to_string(),
    });

    state
        .publish_boundary_input("sample_in", vec![9, 8, 7, 6], Some(123))
        .expect("publish boundary input");

    let events = state.drain_recorder_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].event_kind,
        flowrt_record::RecordEventKind::ChannelSample
    );
    assert_eq!(events[0].entity.name, "sample_in");
    assert_eq!(events[0].payload, vec![9, 8, 7, 6]);
    assert_eq!(
        events[0].payload_encoding,
        flowrt_record::PayloadEncoding::CanonicalFrame
    );
}

#[test]
fn publish_boundary_input_records_sensor_sample_time_ns() {
    // 声明了 timestamp 源的 boundary 经 typed 提取器注册；publish 录制的 envelope 带 sample_time_ns，
    // 与生成 shell 调 register_boundary_input_with_sample_time 的路径一致（提取器读字段 × unit→ns）。
    let state = IntrospectionState::new();
    let input: crate::BoundaryInput<u32> = crate::BoundaryInput::default();
    state.register_boundary_input_with_sample_time::<u32, _>(
        "imu_in",
        "ImuSample",
        input,
        |payload| {
            <u32 as crate::FrameCodec>::decode_frame(payload)
                .ok()
                .map(|micros| (micros as u64).saturating_mul(1000))
        },
    );
    state.start_recorder(IntrospectionRecorderStart {
        output: Some("memory://sample_time.mcap".to_string()),
        filters: vec!["channel:imu_in".to_string()],
        queue_depth: Some(4),
        package: "sensor".to_string(),
        process: "main".to_string(),
        runtime_pid: 7,
        selfdesc_hash: "stamp".to_string(),
    });

    // 2000 微秒（LE u32）× 1000 → 2_000_000 纳秒；receive-time(published_at_ms=50) 独立。
    state
        .publish_boundary_input("imu_in", 2000u32.to_le_bytes().to_vec(), Some(50))
        .expect("publish boundary input");

    let events = state.drain_recorder_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].sample_time_ns, Some(2_000_000));
    assert_eq!(events[0].monotonic_ns, 50_000_000);
    assert_eq!(
        events[0].payload_encoding,
        flowrt_record::PayloadEncoding::CanonicalFrame
    );
}

#[test]
fn recorder_start_captures_channel_sample_and_reports_status() {
    let state = IntrospectionState::new();
    state.start_recorder(IntrospectionRecorderStart {
        output: Some("memory://test.mcap".to_string()),
        filters: vec!["channel:source.imu_to_sink.imu".to_string()],
        queue_depth: Some(4),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime_pid: 42,
        selfdesc_hash: "abc123".to_string(),
    });

    let outcome = state.try_record_channel_sample_bytes(
        "source.imu_to_sink.imu",
        "Imu",
        &[1, 2, 3, 4],
        Some(10),
    );

    assert!(outcome.recorded);
    assert!(!outcome.dropped);
    let events = state.drain_recorder_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].entity.name, "source.imu_to_sink.imu");
    assert_eq!(events[0].payload, vec![1, 2, 3, 4]);

    let status = state.status();
    assert!(status.recorder.enabled);
    assert_eq!(
        status.recorder.output.as_deref(),
        Some("memory://test.mcap")
    );
    assert_eq!(status.recorder.dropped_count, 0);
    assert_eq!(
        status.recorder.active_filters,
        vec!["channel:source.imu_to_sink.imu"]
    );
}

#[test]
fn recorder_captures_operation_start_and_cancel_commands() {
    let state = IntrospectionState::new();
    state.start_recorder(IntrospectionRecorderStart {
        output: Some("memory://operation-commands.mcap".to_string()),
        filters: vec!["operation:controller.plan".to_string()],
        queue_depth: Some(8),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime_pid: 42,
        selfdesc_hash: "abc123".to_string(),
    });
    state.register_operation_start_handler("controller.plan", |payload, timeout_ms, owner| {
        assert_eq!(payload, vec![7, 0, 0, 0]);
        assert_eq!(timeout_ms, Some(2500));
        assert_eq!(owner.as_deref(), Some("flowrt.cli"));
        Ok(IntrospectionOperationStartStatus {
            operation_id: "111:7:3".to_string(),
            operation: IntrospectionOperationStatus {
                name: "controller.plan".to_string(),
                ready: true,
                running: 1,
                current_operation_ids: vec!["111:7:3".to_string()],
                current_state: Some("starting".to_string()),
                current_owner: owner,
                current_deadline_ms: timeout_ms,
                ..Default::default()
            },
        })
    });
    state.register_operation_cancel_handler("controller.plan", |operation_id| {
        assert_eq!(operation_id, "111:7:3");
        Ok(IntrospectionOperationStatus {
            name: "controller.plan".to_string(),
            ready: true,
            current_state: Some("cancel_requested".to_string()),
            ..Default::default()
        })
    });
    state.record_operation_transition(
        "controller.plan",
        "111:7:3",
        "running",
        Some("flowrt.cli"),
        Some(2500),
    );
    let _ = state.drain_recorder_events();

    state
        .start_operation(
            "controller.plan",
            vec![7, 0, 0, 0],
            Some(2500),
            Some("flowrt.cli".to_string()),
        )
        .expect("operation start should be accepted");
    state
        .cancel_operation("111:7:3")
        .expect("operation cancel should be accepted");

    let events = state.drain_recorder_events();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0].payload_schema,
        flowrt_record::OPERATION_COMMAND_START_SCHEMA_NAME
    );
    assert_eq!(
        events[1].payload_schema,
        flowrt_record::OPERATION_COMMAND_CANCEL_SCHEMA_NAME
    );
    let start: flowrt_record::OperationStartCommandPayload =
        serde_json::from_slice(&events[0].payload).unwrap();
    assert_eq!(start.operation_id, "111:7:3");
    assert_eq!(start.goal_payload, vec![7, 0, 0, 0]);
    assert_eq!(start.timeout_ms, Some(2500));
    assert_eq!(start.owner.as_deref(), Some("flowrt.cli"));
    let cancel: flowrt_record::OperationCancelCommandPayload =
        serde_json::from_slice(&events[1].payload).unwrap();
    assert_eq!(cancel.operation_id, "111:7:3");
}

#[test]
fn recorder_marks_variable_channel_frame_payload_encoding() {
    let state = IntrospectionState::new();
    state.start_recorder(IntrospectionRecorderStart {
        output: None,
        filters: vec!["channel".to_string()],
        queue_depth: Some(4),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime_pid: 42,
        selfdesc_hash: "abc123".to_string(),
    });

    let outcome = state.try_record_channel_sample_frame_bytes(
        "source.packet_to_sink.packet",
        "Packet",
        &[1, 2, 3, 4],
        Some(10),
    );

    assert!(outcome.recorded);
    let events = state.drain_recorder_events();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].payload_encoding,
        flowrt_record::PayloadEncoding::CanonicalFrame
    );
}

#[test]
fn recorder_makes_probe_publish_report_recorded_without_echo_observer() {
    let state = IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    state.start_recorder(IntrospectionRecorderStart {
        output: None,
        filters: vec!["channel".to_string()],
        queue_depth: Some(4),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime_pid: 42,
        selfdesc_hash: "abc123".to_string(),
    });

    let outcome =
        state.try_probe_channel_publish_bytes("source.imu_to_sink.imu", "Imu", &[9, 8], Some(10));

    assert!(outcome.recorded);
    assert!(!outcome.dropped);
    assert_eq!(
        state
            .channel_snapshot("source.imu_to_sink.imu")
            .unwrap()
            .published_count,
        0
    );
    let events = state.drain_recorder_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload, vec![9, 8]);
}

#[test]
fn recorder_bounded_queue_reports_dropped_count() {
    let state = IntrospectionState::new();
    state.start_recorder(IntrospectionRecorderStart {
        output: None,
        filters: vec!["all".to_string()],
        queue_depth: Some(1),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime_pid: 42,
        selfdesc_hash: "abc123".to_string(),
    });

    let first = state.try_record_channel_sample_bytes("a.out_to_b.in", "Msg", &[1], Some(1));
    let second = state.try_record_channel_sample_bytes("a.out_to_b.in", "Msg", &[2], Some(2));

    assert!(first.recorded);
    assert!(!first.dropped);
    assert!(!second.recorded);
    assert!(second.dropped);
    let status = state.status();
    assert_eq!(status.recorder.dropped_count, 1);
    assert_eq!(status.recorder.queued_events, 1);
    assert_eq!(state.drain_recorder_events().len(), 1);
}

#[test]
fn status_server_controls_recorder_and_drains_events() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-recorder-test-{}",
        std::process::id()
    ));
    let socket = root.join("worker.sock");
    let handshake = IntrospectionHandshake {
        protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 42,
        started_at_unix_ms: 1000,
        self_description_hash: "abc123".to_string(),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = IntrospectionState::new();
    let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
        .expect("status server should start");

    let started = request_recorder_start(
        &socket,
        Some("memory://socket.mcap".to_string()),
        vec!["channel:source.imu_to_sink.imu".to_string()],
        Some(4),
    )
    .expect("recorder start request should succeed");
    let IntrospectionResponse::RecorderValue { recorder, .. } = started else {
        panic!("recorder start returned wrong response")
    };
    assert!(recorder.enabled);
    assert_eq!(recorder.output.as_deref(), Some("memory://socket.mcap"));

    state.try_record_channel_sample_bytes("source.imu_to_sink.imu", "Imu", &[9, 8], Some(11));
    let drained = request_recorder_drain(&socket).expect("recorder drain request should succeed");
    let IntrospectionResponse::RecorderEvents {
        events, recorder, ..
    } = drained
    else {
        panic!("recorder drain returned wrong response")
    };
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].entity.name, "source.imu_to_sink.imu");
    assert_eq!(recorder.queued_events, 0);

    let stopped = request_recorder_stop(&socket).expect("recorder stop request should succeed");
    let IntrospectionResponse::RecorderValue { recorder, .. } = stopped else {
        panic!("recorder stop returned wrong response")
    };
    assert!(!recorder.enabled);

    drop(server);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn registered_service_is_not_ready_until_marked() {
    let state = IntrospectionState::new();
    state.register_service("planner.plan");

    let status = state.status();
    assert_eq!(status.services.len(), 1);
    assert_eq!(status.services[0].name, "planner.plan");
    assert!(!status.services[0].ready);

    state.mark_service_ready("planner.plan");

    let status = state.status();
    assert!(status.services[0].ready);
}

#[test]
fn request_status_with_timeout_returns_when_peer_stalls() {
    let root = std::env::temp_dir().join(format!(
        "flowrt-introspection-stall-test-{}",
        std::process::id()
    ));
    let socket = root.join("stall.sock");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("test temp dir should be created");
    let listener = UnixListener::bind(&socket).expect("test listener should bind");
    let handle = thread::spawn(move || {
        let (_stream, _addr) = listener.accept().expect("test listener should accept");
        thread::sleep(Duration::from_millis(100));
    });

    let error = request_status_with_timeout(&socket, Duration::from_millis(10))
        .expect_err("stalled peer should time out");

    assert!(
        matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
        "unexpected error kind: {error:?}"
    );
    handle.join().expect("stall thread should exit");
    let _ = fs::remove_dir_all(root);
}
