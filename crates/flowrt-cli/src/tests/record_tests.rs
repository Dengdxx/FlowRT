use super::*;

fn handshake(pid: u32, process: &str) -> flowrt::IntrospectionHandshake {
    flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid,
        started_at_unix_ms: 1234,
        self_description_hash: "feedface".to_string(),
        package: "robot_demo".to_string(),
        process: process.to_string(),
        runtime: "rust".to_string(),
    }
}

fn record_options(output: PathBuf, socket: Option<PathBuf>) -> record::RecordOptions {
    record::RecordOptions {
        output,
        socket,
        duration: Some(Duration::from_millis(20)),
        channels: Vec::new(),
        operations: Vec::new(),
        all: true,
        force: false,
        poll_interval: Duration::from_millis(1),
        shutdown: flowrt::ShutdownToken::new_for_test(),
    }
}

#[test]
fn record_writes_mcap_from_fake_runtime() {
    let root = temp_test_dir("record-writes-mcap");
    let socket = root.join("main.sock");
    let output = root.join("run.mcap");
    let state = flowrt::IntrospectionState::new();
    state.register_channel("source.imu_to_sink.imu", "Imu");
    let server =
        flowrt::spawn_status_server_at(socket.clone(), handshake(42, "main"), state.clone())
            .expect("status server should start");
    let producer_state = state.clone();
    let producer = std::thread::spawn(move || {
        for _ in 0..200 {
            if producer_state.status().recorder.enabled {
                producer_state.record_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    vec![1, 2, 3, 4],
                    Some(10),
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    let summary =
        record::record_runtime_for_sockets(record_options(output.clone(), None), vec![socket])
            .expect("record should write MCAP");

    producer.join().expect("producer thread should finish");
    assert!(summary.contains("event_count=1"));
    assert!(summary.contains("dropped_count=0"));
    let bytes = std::fs::read(&output).expect("MCAP output should exist");
    assert!(bytes.starts_with(flowrt_record::MCAP_MAGIC));
    assert!(!state.status().recorder.enabled);

    drop(server);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn record_refuses_existing_output_without_force() {
    let root = temp_test_dir("record-existing-output");
    let socket = root.join("main.sock");
    let output = root.join("run.mcap");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&output, b"existing").unwrap();
    let state = flowrt::IntrospectionState::new();
    let server =
        flowrt::spawn_status_server_at(socket.clone(), handshake(43, "main"), state.clone())
            .expect("status server should start");

    let error =
        record::record_runtime_for_sockets(record_options(output.clone(), None), vec![socket])
            .expect_err("record should refuse existing output");

    assert!(error.to_string().contains("already exists"));
    assert!(!state.status().recorder.enabled);

    drop(server);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn record_runtime_skips_discovery_when_socket_is_explicit() {
    let root = temp_test_dir("record-explicit-socket-no-discovery");
    let options = record_options(root.join("run.mcap"), Some(root.join("missing.sock")));

    let sockets = record::record_runtime_sockets_for_options(&options)
        .expect("explicit socket should not scan runtime socket directory");

    assert!(sockets.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn record_start_failure_does_not_leave_output_file() {
    let root = temp_test_dir("record-start-failure-clean-output");
    std::fs::create_dir_all(&root).unwrap();
    let socket = root.join("missing.sock");
    let output = root.join("run.mcap");
    let options = record_options(output.clone(), Some(socket));

    let error = record::record_runtime_for_sockets(options, Vec::new())
        .expect_err("missing socket should fail recorder start");

    assert!(
        error.to_string().contains("failed to start recorder"),
        "unexpected error: {error}"
    );
    assert!(
        !output.exists(),
        "failed record start must not leave output"
    );
    let leaked_temp = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains(".flowrt-record.tmp.")
        });
    assert!(!leaked_temp, "temporary record output should be removed");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn record_without_socket_rejects_multiple_live_runtimes() {
    let root = temp_test_dir("record-ambiguous-runtime");
    let socket_a = root.join("main-a.sock");
    let socket_b = root.join("main-b.sock");
    let state_a = flowrt::IntrospectionState::new();
    let state_b = flowrt::IntrospectionState::new();
    let server_a =
        flowrt::spawn_status_server_at(socket_a.clone(), handshake(44, "main_a"), state_a)
            .expect("first status server should start");
    let server_b =
        flowrt::spawn_status_server_at(socket_b.clone(), handshake(45, "main_b"), state_b)
            .expect("second status server should start");
    let output = root.join("run.mcap");

    let error =
        record::record_runtime_for_sockets(record_options(output, None), vec![socket_a, socket_b])
            .expect_err("record should require --socket for multiple runtimes");

    assert!(error.to_string().contains("multiple live FlowRT processes"));

    drop(server_a);
    drop(server_b);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn record_stops_when_shutdown_token_is_requested() {
    let root = temp_test_dir("record-shutdown-token");
    let socket = root.join("main.sock");
    let output = root.join("run.mcap");
    let state = flowrt::IntrospectionState::new();
    let server =
        flowrt::spawn_status_server_at(socket.clone(), handshake(46, "main"), state.clone())
            .expect("status server should start");
    let shutdown = flowrt::ShutdownToken::new_for_test();
    let request_shutdown = shutdown.clone();
    let mut options = record_options(output.clone(), None);
    options.duration = None;
    options.shutdown = shutdown;
    let producer_state = state.clone();
    let producer = std::thread::spawn(move || {
        for _ in 0..200 {
            if producer_state.status().recorder.enabled {
                request_shutdown.request();
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });

    let summary = record::record_runtime_for_sockets(options, vec![socket])
        .expect("record should stop on shutdown token");

    producer.join().expect("producer thread should finish");
    assert!(summary.contains("event_count=0"));
    assert!(!state.status().recorder.enabled);
    assert!(
        std::fs::read(&output)
            .expect("MCAP output should exist")
            .starts_with(flowrt_record::MCAP_MAGIC)
    );

    drop(server);
    let _ = std::fs::remove_dir_all(root);
}
