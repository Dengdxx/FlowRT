use super::*;

#[test]
fn replay_fixture_drives_multiple_boundary_inputs() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
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
      "name": "scan_in",
      "canonical_id": "boundary_scan",
      "direction": "input",
      "endpoint": "lidar.scan",
      "instance": "lidar",
      "port": "scan",
      "message_type": "Sample"
    }, {
      "name": "pose_in",
      "canonical_id": "boundary_pose",
      "direction": "input",
      "endpoint": "localizer.pose",
      "instance": "localizer",
      "port": "pose",
      "message_type": "Sample"
    }]
  }],
  "message_abi": [{
    "type_name": "Sample",
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
    let root = temp_test_dir("replay-multi-boundary");
    let selfdesc = root.join("selfdesc.json");
    let fixture = root.join("fixture.jsonl");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(
        &fixture,
        r#"{"boundary":"pose_in","at_ms":20,"payload":{"value":2}}
{"boundary":"scan_in","at_ms":10,"payload":{"value":1}}
{"boundary":"scan_in","dt_ms":5,"payload":{"value":3}}
"#,
    )
    .unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 910,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let captured = std::sync::Arc::new(std::sync::Mutex::new(
        Vec::<(String, u32, Option<u64>)>::new(),
    ));
    for endpoint in ["scan_in", "pose_in"] {
        let captured_for_handler = captured.clone();
        state.register_boundary_input_handler(endpoint, "Sample", move |payload, timestamp| {
            let bytes: [u8; 4] = payload.try_into().expect("u32 payload");
            captured_for_handler.lock().unwrap().push((
                endpoint.to_string(),
                u32::from_le_bytes(bytes),
                timestamp,
            ));
            Ok(1)
        });
    }
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = replay_fixture(&fixture, &selfdesc, Some(&socket), 1.0, true).unwrap();

    assert!(output.contains("replay source="));
    assert!(output.contains("events=3"));
    assert!(output.contains("boundaries=2"));
    assert!(output.contains("duration_ms=20"));
    assert_eq!(
        *captured.lock().unwrap(),
        vec![
            ("scan_in".to_string(), 1, Some(10)),
            ("scan_in".to_string(), 3, Some(15)),
            ("pose_in".to_string(), 2, Some(20)),
        ]
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replay_variable_frame_fixture_drives_boundary_input() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
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
      "name": "path_in",
      "canonical_id": "boundary_path",
      "direction": "input",
      "endpoint": "planner.path",
      "instance": "planner",
      "port": "path",
      "message_type": "PathFrame"
    }]
  }],
  "message_abi": [{
    "type_name": "Point",
    "size_bytes": 8,
    "align_bytes": 4,
    "fields": [
      { "name": "x", "type": "f32", "offset_bytes": 0, "size_bytes": 4, "align_bytes": 4 },
      { "name": "y", "type": "f32", "offset_bytes": 4, "size_bytes": 4, "align_bytes": 4 }
    ]
  }],
  "message_frames": [{
    "type_name": "PathFrame",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 24,
    "max_size_bytes": null,
    "variable": true,
    "fields": [{
      "name": "label",
      "type": "string",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": null
    }, {
      "name": "payload",
      "type": "bytes",
      "header_offset_bytes": 8,
      "header_size_bytes": 8,
      "tail_max_bytes": null
    }, {
      "name": "points",
      "type": "sequence<Point>",
      "header_offset_bytes": 16,
      "header_size_bytes": 8,
      "tail_max_bytes": null
    }]
  }]
}
"#;
    let root = temp_test_dir("replay-variable-frame-boundary");
    let selfdesc = root.join("selfdesc.json");
    let fixture = root.join("fixture.jsonl");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(
        &fixture,
        r#"{"boundary":"path_in","at_ms":0,"payload":{"label":"","payload":[],"points":[]}}
{"boundary":"path_in","dt_ms":5,"payload":{"label":"utf8-\u03bc","payload":[1,2,3],"points":[{"x":1.0,"y":2.0},{"x":3.0,"y":4.0}]}}
"#,
    )
    .unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 911,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(Vec<u8>, Option<u64>)>::new()));
    let captured_for_handler = captured.clone();
    state.register_boundary_input_handler("path_in", "PathFrame", move |payload, timestamp| {
        captured_for_handler
            .lock()
            .unwrap()
            .push((payload.to_vec(), timestamp));
        Ok(1)
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = replay_fixture(&fixture, &selfdesc, Some(&socket), 1.0, true).unwrap();

    let empty = vec![0u8; 24];
    let mut expected = vec![0u8; 24];
    let mut tail = Vec::new();
    let label_span = flowrt::append_tail_block(&mut tail, "utf8-\u{03bc}".as_bytes()).unwrap();
    let payload_span = flowrt::append_tail_block(&mut tail, &[1, 2, 3]).unwrap();
    let mut points_tail = Vec::new();
    points_tail.extend_from_slice(&1.0f32.to_le_bytes());
    points_tail.extend_from_slice(&2.0f32.to_le_bytes());
    points_tail.extend_from_slice(&3.0f32.to_le_bytes());
    points_tail.extend_from_slice(&4.0f32.to_le_bytes());
    let points_span = flowrt::append_tail_block(&mut tail, &points_tail).unwrap();
    label_span.encode(&mut expected[0..8]).unwrap();
    payload_span.encode(&mut expected[8..16]).unwrap();
    points_span.encode(&mut expected[16..24]).unwrap();
    expected.extend_from_slice(&tail);

    assert!(output.contains("events=2"));
    assert_eq!(
        *captured.lock().unwrap(),
        vec![(empty, Some(0)), (expected, Some(5))]
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replay_rejects_strict_self_description() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "strict_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [{ "name": "default", "backend": "inproc", "mode": "strict" }],
  "targets": [],
  "deployments": [],
  "graphs": [{ "name": "default", "mode": "strict", "instances": [], "tasks": [], "channels": [], "boundary_endpoints": [] }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("replay-strict-reject");
    let selfdesc = root.join("selfdesc.json");
    let fixture = root.join("fixture.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(&fixture, r#"[{"boundary":"scan_in","payload":{}}]"#).unwrap();

    let error = replay_fixture(&fixture, &selfdesc, None, 1.0, true).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("not island mode; flowrt replay only writes island boundary input")
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replay_rejects_unknown_boundary_input() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
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
      "name": "known_in",
      "canonical_id": "boundary_known",
      "direction": "input",
      "endpoint": "source.sample",
      "instance": "source",
      "port": "sample",
      "message_type": "Sample"
    }]
  }],
  "message_abi": [{
    "type_name": "Sample",
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
    let root = temp_test_dir("replay-unknown-boundary");
    let selfdesc = root.join("selfdesc.json");
    let fixture = root.join("fixture.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(
        &fixture,
        r#"[{"boundary":"missing_in","payload":{"value":1}}]"#,
    )
    .unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 911,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let error = replay_fixture(&fixture, &selfdesc, Some(&socket), 1.0, true).unwrap_err();

    let message = format!("{error:#}");
    assert!(message.contains("missing_in"));
    assert!(message.contains("unknown FlowRT boundary input"));
    assert!(message.contains("only writes typed boundary input"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replay_rejects_dataflow_channel_endpoint_with_boundary_input_error() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "island_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "channels": [{
      "from": "producer.sample",
      "to": "consumer.sample",
      "message_type": "Sample",
      "backend": "inproc",
      "channel": "latest"
    }],
    "boundary_endpoints": [{
      "name": "sample_in",
      "canonical_id": "boundary_known",
      "direction": "input",
      "endpoint": "consumer.boundary",
      "instance": "consumer",
      "port": "boundary",
      "message_type": "Sample"
    }]
  }],
  "message_abi": [{
    "type_name": "Sample",
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
    let root = temp_test_dir("replay-dataflow-channel-reject");
    let selfdesc = root.join("selfdesc.json");
    let fixture = root.join("fixture.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(
        &fixture,
        r#"[{"boundary":"producer.sample","payload":{"value":1}}]"#,
    )
    .unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 912,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let error = replay_fixture(&fixture, &selfdesc, Some(&socket), 1.0, true).unwrap_err();
    let message = format!("{error:#}");

    assert!(message.contains("producer.sample"));
    assert!(message.contains("dataflow channel"));
    assert!(message.contains("only writes typed boundary input"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replay_rejects_service_and_operation_endpoint_with_boundary_input_error() {
    let source = r#"
{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "island_demo", "version": null, "rsdl_version": "0.1" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "services": [{
      "name": "controller.plan",
      "client": "controller.plan",
      "server": "planner.plan",
      "request": "Sample",
      "response": "Sample",
      "backend": "inproc"
    }],
    "operations": [{
      "name": "controller.navigate",
      "client_instance": "controller",
      "client_port": "navigate",
      "server_instance": "navigator",
      "server_port": "navigate",
      "goal_type": "Sample",
      "feedback_type": "Sample",
      "result_type": "Sample",
      "backend": "inproc"
    }],
    "boundary_endpoints": [{
      "name": "sample_in",
      "canonical_id": "boundary_known",
      "direction": "input",
      "endpoint": "sensor.sample",
      "instance": "sensor",
      "port": "sample",
      "message_type": "Sample"
    }]
  }],
  "message_abi": [{
    "type_name": "Sample",
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
    let root = temp_test_dir("replay-control-endpoint-reject");
    let selfdesc = root.join("selfdesc.json");
    let service_fixture = root.join("service.json");
    let operation_fixture = root.join("operation.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(
        &service_fixture,
        r#"[{"boundary":"controller.plan","payload":{"value":1}}]"#,
    )
    .unwrap();
    std::fs::write(
        &operation_fixture,
        r#"[{"boundary":"controller.navigate","payload":{"value":1}}]"#,
    )
    .unwrap();

    let service_error = replay_fixture(&service_fixture, &selfdesc, None, 1.0, true).unwrap_err();
    let operation_error =
        replay_fixture(&operation_fixture, &selfdesc, None, 1.0, true).unwrap_err();
    let service_message = format!("{service_error:#}");
    let operation_message = format!("{operation_error:#}");

    assert!(
        service_message.contains("service endpoint"),
        "{service_message}"
    );
    assert!(
        service_message.contains("only writes typed boundary input"),
        "{service_message}"
    );
    assert!(
        operation_message.contains("operation endpoint"),
        "{operation_message}"
    );
    assert!(
        operation_message.contains("only writes typed boundary input"),
        "{operation_message}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replay_mcap_operation_commands_start_and_cancel_invocation() {
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
    let root = temp_test_dir("replay-mcap-operation-commands");
    let selfdesc = root.join("selfdesc.json");
    let recording = root.join("run.mcap");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    {
        let mut writer =
            flowrt_record::FlowrtMcapWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        let channel = writer
            .register_channel(
                "flowrt/record/operation_event",
                flowrt_record::RecordEventKind::OperationEvent,
            )
            .unwrap();
        let entity = flowrt_record::RecordEntity {
            kind: flowrt_record::RecordEntityKind::Operation,
            name: "controller.plan".to_string(),
            instance: Some("controller".to_string()),
            task: None,
            type_name: None,
        };
        let start_payload = flowrt_record::OperationStartCommandPayload {
            operation_id: "111:7:3".to_string(),
            goal_payload: vec![7, 0, 0, 0],
            timeout_ms: Some(2500),
            owner: Some("flowrt.cli".to_string()),
        };
        let start = flowrt_record::RecordEnvelope {
            schema_version: flowrt_record::RECORD_SCHEMA_VERSION,
            event_kind: flowrt_record::RecordEventKind::OperationEvent,
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime_pid: 88,
            selfdesc_hash: self_description_hash(source.as_bytes()),
            monotonic_ns: 0,
            sample_time_ns: None,
            wall_unix_ns: 0,
            sequence: 0,
            entity: entity.clone(),
            payload_encoding: flowrt_record::PayloadEncoding::Json,
            payload_schema: flowrt_record::OPERATION_COMMAND_START_SCHEMA_NAME.to_string(),
            payload: serde_json::to_vec(&start_payload).unwrap(),
        };
        let cancel = flowrt_record::RecordEnvelope {
            payload_schema: flowrt_record::OPERATION_COMMAND_CANCEL_SCHEMA_NAME.to_string(),
            payload: br#"{"operation_id":"111:7:3"}"#.to_vec(),
            monotonic_ns: 1_000_000,
            sequence: 1,
            ..start.clone()
        };
        writer.write_event(channel, &start).unwrap();
        writer.write_event(channel, &cancel).unwrap();
        writer.flush().unwrap();
        let bytes = writer.finish_into_inner().unwrap().into_inner();
        std::fs::write(&recording, bytes).unwrap();
    }

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
    let start_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cancel_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let state_for_start = state.clone();
    let start_count_for_handler = start_count.clone();
    state.register_operation_start_handler("controller.plan", move |payload, timeout_ms, owner| {
        start_count_for_handler.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(payload, vec![7, 0, 0, 0]);
        assert_eq!(timeout_ms, Some(2500));
        assert_eq!(owner.as_deref(), Some("flowrt.cli"));
        state_for_start.record_operation_transition(
            "controller.plan",
            "111:7:3",
            "running",
            owner.as_deref(),
            timeout_ms,
        );
        Ok(flowrt::IntrospectionOperationStartStatus {
            operation_id: "111:7:3".to_string(),
            operation: flowrt::IntrospectionOperationStatus {
                name: "controller.plan".to_string(),
                ready: true,
                running: 1,
                current_operation_ids: vec!["111:7:3".to_string()],
                current_state: Some("running".to_string()),
                current_owner: owner,
                current_deadline_ms: timeout_ms,
                ..Default::default()
            },
        })
    });
    let cancel_count_for_handler = cancel_count.clone();
    state.register_operation_cancel_handler("controller.plan", move |operation_id| {
        cancel_count_for_handler.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(operation_id, "111:7:3");
        Ok(flowrt::IntrospectionOperationStatus {
            name: "controller.plan".to_string(),
            ready: true,
            current_state: Some("cancel_requested".to_string()),
            ..Default::default()
        })
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = replay_fixture(&recording, &selfdesc, Some(&socket), 1.0, true).unwrap();

    assert!(output.contains("operation_commands=2"));
    assert_eq!(start_count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(cancel_count.load(std::sync::atomic::Ordering::SeqCst), 1);

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn replay_reports_bad_jsonl_line_number() {
    let root = temp_test_dir("replay-bad-jsonl");
    let selfdesc = root.join("selfdesc.json");
    let fixture = root.join("fixture.jsonl");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, "{}").unwrap();
    std::fs::write(&fixture, "{\"boundary\":\"a\",\"payload\":{}}\nnot-json\n").unwrap();

    let error = replay_fixture(&fixture, &selfdesc, None, 1.0, true).unwrap_err();

    assert!(error.to_string().contains("line 2"));

    let _ = std::fs::remove_dir_all(&root);
}
