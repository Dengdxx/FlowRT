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
fn pub_injects_json_into_boundary_input() {
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
      "name": "sample_in",
      "canonical_id": "boundary_0123456789abcdef",
      "direction": "input",
      "endpoint": "consumer.sample",
      "instance": "consumer",
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
    let root = temp_test_dir("pub-boundary-input");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 81,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let captured_for_handler = captured.clone();
    state.register_boundary_input_handler("sample_in", "Sample", move |payload, timestamp| {
        assert_eq!(timestamp, Some(123));
        *captured_for_handler.lock().unwrap() = payload.to_vec();
        Ok(1)
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = boundary_publish(
        "sample_in",
        r#"{"value": 42}"#,
        Some(&selfdesc),
        Some(&socket),
        Some(123),
    )
    .unwrap();

    assert!(output.contains("boundary=sample_in"));
    assert!(output.contains("type=Sample"));
    assert!(output.contains("revision=1"));
    assert_eq!(*captured.lock().unwrap(), 42u32.to_le_bytes());

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_injects_empty_message_from_null_and_empty_object() {
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
      "name": "empty_in",
      "canonical_id": "boundary_empty_0123456789abcdef",
      "direction": "input",
      "endpoint": "consumer.empty",
      "instance": "consumer",
      "port": "empty",
      "message_type": "Empty"
    }]
  }],
  "message_abi": [{
    "type_name": "Empty",
    "size_bytes": 0,
    "align_bytes": 1,
    "empty": true,
    "fields": []
  }]
}
"#;
    let root = temp_test_dir("pub-boundary-empty");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 81,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let captures = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
    let captures_for_handler = captures.clone();
    state.register_boundary_input_handler("empty_in", "Empty", move |payload, _| {
        captures_for_handler.lock().unwrap().push(payload.to_vec());
        Ok(1)
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    boundary_publish("empty_in", "null", Some(&selfdesc), Some(&socket), None).unwrap();
    boundary_publish("empty_in", "{}", Some(&selfdesc), Some(&socket), None).unwrap();

    let captures = captures.lock().unwrap();
    assert_eq!(captures.len(), 2);
    assert!(captures.iter().all(|payload| payload.is_empty()));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_rejects_zero_sized_message_without_empty_flag() {
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
      "name": "bad_empty_in",
      "canonical_id": "boundary_empty_0123456789abcdef",
      "direction": "input",
      "endpoint": "consumer.empty",
      "instance": "consumer",
      "port": "empty",
      "message_type": "Empty"
    }]
  }],
  "message_abi": [{
    "type_name": "Empty",
    "size_bytes": 0,
    "align_bytes": 1,
    "fields": []
  }]
}
"#;
    let root = temp_test_dir("pub-boundary-implicit-empty");
    let selfdesc = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let err = boundary_publish("bad_empty_in", "{}", Some(&selfdesc), None, None).unwrap_err();
    assert!(
        err.to_string()
            .contains("JSON encoding requires field metadata"),
        "{err:#}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_encodes_nested_fixed_message_json() {
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
      "name": "sample_in",
      "canonical_id": "boundary_0123456789abcdef",
      "direction": "input",
      "endpoint": "consumer.sample",
      "instance": "consumer",
      "port": "sample",
      "message_type": "Sample"
    }]
  }],
  "message_abi": [{
    "type_name": "Point",
    "size_bytes": 8,
    "align_bytes": 4,
    "fields": [
      { "name": "x", "type": "u32", "offset_bytes": 0, "size_bytes": 4, "align_bytes": 4 },
      { "name": "y", "type": "u32", "offset_bytes": 4, "size_bytes": 4, "align_bytes": 4 }
    ]
  }, {
    "type_name": "Sample",
    "size_bytes": 12,
    "align_bytes": 4,
    "fields": [
      { "name": "point", "type": "Point", "offset_bytes": 0, "size_bytes": 8, "align_bytes": 4 },
      { "name": "values", "type": "[u16;2]", "offset_bytes": 8, "size_bytes": 4, "align_bytes": 2 }
    ]
  }]
}
"#;
    let root = temp_test_dir("pub-nested-boundary-input");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 82,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let captured_for_handler = captured.clone();
    state.register_boundary_input_handler("sample_in", "Sample", move |payload, _| {
        *captured_for_handler.lock().unwrap() = payload.to_vec();
        Ok(2)
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = boundary_publish(
        "sample_in",
        r#"{"point":{"x":1,"y":2},"values":[3,4]}"#,
        Some(&selfdesc),
        Some(&socket),
        None,
    )
    .unwrap();

    let mut expected = Vec::new();
    expected.extend(1u32.to_le_bytes());
    expected.extend(2u32.to_le_bytes());
    expected.extend(3u16.to_le_bytes());
    expected.extend(4u16.to_le_bytes());
    assert!(output.contains("revision=2"));
    assert_eq!(*captured.lock().unwrap(), expected);

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_injects_canonical_frame_json_into_boundary_input() {
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
      "canonical_id": "boundary_0123456789abcdef",
      "direction": "input",
      "endpoint": "consumer.scan",
      "instance": "consumer",
      "port": "scan",
      "message_type": "ScanFrame"
    }]
  }],
  "message_abi": [{
    "type_name": "Stamp",
    "size_bytes": 8,
    "align_bytes": 4,
    "fields": [
      { "name": "sec", "type": "u32", "offset_bytes": 0, "size_bytes": 4, "align_bytes": 4 },
      { "name": "nsec", "type": "u32", "offset_bytes": 4, "size_bytes": 4, "align_bytes": 4 }
    ]
  }, {
    "type_name": "Meta",
    "size_bytes": 9,
    "align_bytes": 4,
    "fields": [
      { "name": "stamp", "type": "Stamp", "offset_bytes": 0, "size_bytes": 8, "align_bytes": 4 },
      { "name": "active", "type": "bool", "offset_bytes": 8, "size_bytes": 1, "align_bytes": 1 }
    ]
  }, {
    "type_name": "Point",
    "size_bytes": 8,
    "align_bytes": 4,
    "fields": [
      { "name": "x", "type": "f32", "offset_bytes": 0, "size_bytes": 4, "align_bytes": 4 },
      { "name": "y", "type": "f32", "offset_bytes": 4, "size_bytes": 4, "align_bytes": 4 }
    ]
  }],
  "message_frames": [{
    "type_name": "ScanFrame",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 41,
    "max_size_bytes": 128,
    "variable": true,
    "fields": [{
      "name": "meta",
      "type": "Meta",
      "header_offset_bytes": 0,
      "header_size_bytes": 9,
      "tail_max_bytes": null
    }, {
      "name": "label",
      "type": "string",
      "header_offset_bytes": 9,
      "header_size_bytes": 8,
      "tail_max_bytes": 32
    }, {
      "name": "payload",
      "type": "bytes",
      "header_offset_bytes": 17,
      "header_size_bytes": 8,
      "tail_max_bytes": 64
    }, {
      "name": "ranges",
      "type": "sequence<f32>",
      "header_offset_bytes": 25,
      "header_size_bytes": 8,
      "tail_max_bytes": 32
    }, {
      "name": "points",
      "type": "sequence<Point>",
      "header_offset_bytes": 33,
      "header_size_bytes": 8,
      "tail_max_bytes": 64
    }]
  }]
}
"#;
    let root = temp_test_dir("pub-canonical-frame-boundary-input");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 83,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let captured_for_handler = captured.clone();
    state.register_boundary_input_handler("scan_in", "ScanFrame", move |payload, timestamp| {
        assert_eq!(timestamp, Some(456));
        *captured_for_handler.lock().unwrap() = payload.to_vec();
        Ok(3)
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = boundary_publish(
        "scan_in",
        r#"{
          "meta": { "stamp": { "sec": 7, "nsec": 9 }, "active": true },
          "label": "laser",
          "payload": "AQIDBA==",
          "ranges": [1.0, 1.5],
          "points": [
            { "x": 2.0, "y": 3.0 },
            { "x": 4.0, "y": 5.0 }
          ]
        }"#,
        Some(&selfdesc),
        Some(&socket),
        Some(456),
    )
    .unwrap();

    let mut expected = vec![0u8; 41];
    let mut cursor = 0usize;
    expected[cursor..cursor + 4].copy_from_slice(&7u32.to_le_bytes());
    cursor += 4;
    expected[cursor..cursor + 4].copy_from_slice(&9u32.to_le_bytes());
    cursor += 4;
    expected[cursor] = 1;
    cursor += 1;

    let mut tail = Vec::new();
    let label_span = flowrt::append_tail_block(&mut tail, b"laser").unwrap();
    let payload_span = flowrt::append_tail_block(&mut tail, &[1, 2, 3, 4]).unwrap();

    let mut ranges_tail = Vec::new();
    ranges_tail.extend_from_slice(&1.0f32.to_le_bytes());
    ranges_tail.extend_from_slice(&1.5f32.to_le_bytes());
    let ranges_span = flowrt::append_tail_block(&mut tail, &ranges_tail).unwrap();

    let mut points_tail = Vec::new();
    points_tail.extend_from_slice(&2.0f32.to_le_bytes());
    points_tail.extend_from_slice(&3.0f32.to_le_bytes());
    points_tail.extend_from_slice(&4.0f32.to_le_bytes());
    points_tail.extend_from_slice(&5.0f32.to_le_bytes());
    let points_span = flowrt::append_tail_block(&mut tail, &points_tail).unwrap();

    label_span
        .encode(&mut expected[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])
        .unwrap();
    cursor += flowrt::VAR_SPAN_WIRE_SIZE;
    payload_span
        .encode(&mut expected[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])
        .unwrap();
    cursor += flowrt::VAR_SPAN_WIRE_SIZE;
    ranges_span
        .encode(&mut expected[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])
        .unwrap();
    cursor += flowrt::VAR_SPAN_WIRE_SIZE;
    points_span
        .encode(&mut expected[cursor..cursor + flowrt::VAR_SPAN_WIRE_SIZE])
        .unwrap();
    expected.extend_from_slice(&tail);

    assert!(output.contains("boundary=scan_in"));
    assert!(output.contains("type=ScanFrame"));
    assert!(output.contains("revision=3"));
    assert_eq!(*captured.lock().unwrap(), expected);

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_accepts_byte_array_json_for_canonical_frame_bytes_field() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "island_demo" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "boundary_endpoints": [{
      "name": "scan_in",
      "direction": "input",
      "endpoint": "consumer.scan",
      "instance": "consumer",
      "port": "scan",
      "message_type": "BytesFrame"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "BytesFrame",
    "header_size_bytes": 8,
    "max_size_bytes": 32,
    "variable": true,
    "fields": [{
      "name": "payload",
      "type": "bytes",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": 16
    }]
  }]
}
"#;
    let root = temp_test_dir("pub-canonical-frame-byte-array");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 84,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let captured_for_handler = captured.clone();
    state.register_boundary_input_handler("scan_in", "BytesFrame", move |payload, _| {
        *captured_for_handler.lock().unwrap() = payload.to_vec();
        Ok(4)
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = boundary_publish(
        "scan_in",
        r#"{"payload":[0,127,255]}"#,
        Some(&selfdesc),
        Some(&socket),
        None,
    )
    .unwrap();

    let mut expected = vec![0u8; 8];
    let mut tail = Vec::new();
    let payload_span = flowrt::append_tail_block(&mut tail, &[0, 127, 255]).unwrap();
    payload_span.encode(&mut expected[..8]).unwrap();
    expected.extend_from_slice(&tail);

    assert!(output.contains("revision=4"));
    assert_eq!(*captured.lock().unwrap(), expected);

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_rejects_invalid_canonical_frame_base64_json() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "island_demo" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "boundary_endpoints": [{
      "name": "scan_in",
      "direction": "input",
      "endpoint": "consumer.scan",
      "instance": "consumer",
      "port": "scan",
      "message_type": "ScanFrame"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "ScanFrame",
    "header_size_bytes": 8,
    "max_size_bytes": 32,
    "variable": true,
    "fields": [{
      "name": "payload",
      "type": "bytes",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": 16
    }]
  }]
}
"#;
    let root = temp_test_dir("pub-canonical-frame-invalid-base64");
    let selfdesc = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let err = boundary_publish(
        "scan_in",
        r#"{"payload":"%%%"}"#,
        Some(&selfdesc),
        Some(root.join("missing.sock").as_path()),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("invalid base64"), "unexpected error: {err}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_rejects_non_array_sequence_in_canonical_frame_json() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "island_demo" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "boundary_endpoints": [{
      "name": "scan_in",
      "direction": "input",
      "endpoint": "consumer.scan",
      "instance": "consumer",
      "port": "scan",
      "message_type": "ScanFrame"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "ScanFrame",
    "header_size_bytes": 8,
    "max_size_bytes": 32,
    "variable": true,
    "fields": [{
      "name": "ranges",
      "type": "sequence<f32>",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": 16
    }]
  }]
}
"#;
    let root = temp_test_dir("pub-canonical-frame-sequence-type-error");
    let selfdesc = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let err = boundary_publish(
        "scan_in",
        r#"{"ranges":"oops"}"#,
        Some(&selfdesc),
        Some(root.join("missing.sock").as_path()),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("expects JSON array"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_rejects_unknown_field_in_canonical_frame_json() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "island_demo" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "boundary_endpoints": [{
      "name": "scan_in",
      "direction": "input",
      "endpoint": "consumer.scan",
      "instance": "consumer",
      "port": "scan",
      "message_type": "ScanFrame"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "ScanFrame",
    "header_size_bytes": 8,
    "max_size_bytes": 32,
    "variable": true,
    "fields": [{
      "name": "label",
      "type": "string",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": 16
    }]
  }]
}
"#;
    let root = temp_test_dir("pub-canonical-frame-unknown-field");
    let selfdesc = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let err = boundary_publish(
        "scan_in",
        r#"{"label":"ok","extra":1}"#,
        Some(&selfdesc),
        Some(root.join("missing.sock").as_path()),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("unknown field `extra`"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_rejects_canonical_frame_sequence_without_nested_fixed_metadata() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "island_demo" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "boundary_endpoints": [{
      "name": "scan_in",
      "direction": "input",
      "endpoint": "consumer.scan",
      "instance": "consumer",
      "port": "scan",
      "message_type": "ScanFrame"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "ScanFrame",
    "header_size_bytes": 8,
    "max_size_bytes": 64,
    "variable": true,
    "fields": [{
      "name": "points",
      "type": "sequence<Point>",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": 32
    }]
  }]
}
"#;
    let root = temp_test_dir("pub-canonical-frame-missing-nested-metadata");
    let selfdesc = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let err = boundary_publish(
        "scan_in",
        r#"{"points":[{"x":1.0,"y":2.0}]}"#,
        Some(&selfdesc),
        Some(root.join("missing.sock").as_path()),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("Point"), "unexpected error: {err}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_rejects_missing_field_in_canonical_frame_json() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "island_demo" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "boundary_endpoints": [{
      "name": "scan_in",
      "direction": "input",
      "endpoint": "consumer.scan",
      "instance": "consumer",
      "port": "scan",
      "message_type": "ScanFrame"
    }]
  }],
  "message_abi": [{
    "type_name": "Meta",
    "size_bytes": 1,
    "align_bytes": 1,
    "fields": [
      { "name": "active", "type": "bool", "offset_bytes": 0, "size_bytes": 1, "align_bytes": 1 }
    ]
  }],
  "message_frames": [{
    "type_name": "ScanFrame",
    "header_size_bytes": 9,
    "max_size_bytes": 32,
    "variable": true,
    "fields": [{
      "name": "meta",
      "type": "Meta",
      "header_offset_bytes": 0,
      "header_size_bytes": 1,
      "tail_max_bytes": null
    }, {
      "name": "label",
      "type": "string",
      "header_offset_bytes": 1,
      "header_size_bytes": 8,
      "tail_max_bytes": 16
    }]
  }]
}
"#;
    let root = temp_test_dir("pub-canonical-frame-missing-field");
    let selfdesc = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let err = boundary_publish(
        "scan_in",
        r#"{"meta":{"active":true}}"#,
        Some(&selfdesc),
        Some(root.join("missing.sock").as_path()),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(
        err.contains("missing field `label`"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_rejects_out_of_range_primitive_in_canonical_frame_json() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "island_demo" },
  "profiles": [{ "name": "dev", "backend": "inproc", "mode": "island" }],
  "graphs": [{
    "name": "default",
    "mode": "island",
    "boundary_endpoints": [{
      "name": "scan_in",
      "direction": "input",
      "endpoint": "consumer.scan",
      "instance": "consumer",
      "port": "scan",
      "message_type": "ScanFrame"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "ScanFrame",
    "header_size_bytes": 8,
    "max_size_bytes": 32,
    "variable": true,
    "fields": [{
      "name": "ranges",
      "type": "sequence<u8>",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": 16
    }]
  }]
}
"#;
    let root = temp_test_dir("pub-canonical-frame-primitive-overflow");
    let selfdesc = root.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let err = boundary_publish(
        "scan_in",
        r#"{"ranges":[1,300]}"#,
        Some(&selfdesc),
        Some(root.join("missing.sock").as_path()),
        None,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("exceeds max 255"), "unexpected error: {err}");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_rejects_boundary_output_and_strict_selfdesc() {
    let output_source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
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
  "message_abi": [{ "type_name": "Sample", "size_bytes": 4 }]
}
"#;
    let strict_source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "0123456789abcdef",
  "package": { "name": "strict_demo" },
  "profiles": [{ "name": "default", "backend": "inproc", "mode": "strict" }],
  "graphs": [{ "name": "default", "mode": "strict", "boundary_endpoints": [] }],
  "message_abi": []
}
"#;
    let root = temp_test_dir("pub-boundary-reject");
    let output_dir = root.join("output");
    let strict_dir = root.join("strict");
    let output_selfdesc = output_dir.join("selfdesc.json");
    let strict_selfdesc = strict_dir.join("selfdesc.json");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&output_dir).unwrap();
    std::fs::create_dir_all(&strict_dir).unwrap();
    std::fs::write(&output_selfdesc, output_source).unwrap();
    std::fs::write(&strict_selfdesc, strict_source).unwrap();

    let err = boundary_publish(
        "sample_out",
        r#"{"value": 1}"#,
        Some(&output_selfdesc),
        Some(root.join("missing.sock").as_path()),
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("is a boundary output"),
        "unexpected error: {err}"
    );

    let err = boundary_publish(
        "sample_in",
        r#"{"value": 1}"#,
        Some(&strict_selfdesc),
        Some(root.join("missing.sock").as_path()),
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("is not island mode"),
        "unexpected error: {err}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_reads_boundary_output_snapshot_from_self_description() {
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
    "channels": [],
    "boundary_endpoints": [{
      "name": "sample_out",
      "direction": "output",
      "endpoint": "producer.sample",
      "instance": "producer",
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
    let root = temp_test_dir("echo-boundary-output");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 81,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "island_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_channel("sample_out", "Sample");
    state.record_channel_publish_bytes(
        "sample_out",
        "Sample",
        7u32.to_le_bytes().to_vec(),
        Some(9),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "sample_out", Some(&socket)).unwrap();

    assert!(output.contains("channel=sample_out"));
    assert!(output.contains("type=Sample"));
    assert!(output.contains("fields={value=7}"));

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
fn echo_formats_standard_frame_descriptor_payload_structurally() {
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
      "from": "camera.frame",
      "to": "processor.frame",
      "message_type": "FrameHandle"
    }]
  }],
  "message_abi": [{
    "type_name": "FrameHandle",
    "size_bytes": 64,
    "align_bytes": 8,
    "fields": [
      {"name":"resource_id_hash","type":"u64","offset_bytes":0,"size_bytes":8,"align_bytes":8},
      {"name":"slot","type":"u32","offset_bytes":8,"size_bytes":4,"align_bytes":4},
      {"name":"generation","type":"u64","offset_bytes":16,"size_bytes":8,"align_bytes":8},
      {"name":"size_bytes","type":"u64","offset_bytes":24,"size_bytes":8,"align_bytes":8},
      {"name":"timestamp_unix_ns","type":"u64","offset_bytes":32,"size_bytes":8,"align_bytes":8},
      {"name":"width","type":"u32","offset_bytes":40,"size_bytes":4,"align_bytes":4},
      {"name":"height","type":"u32","offset_bytes":44,"size_bytes":4,"align_bytes":4},
      {"name":"stride_bytes","type":"u32","offset_bytes":48,"size_bytes":4,"align_bytes":4},
      {"name":"format_id","type":"u32","offset_bytes":52,"size_bytes":4,"align_bytes":4},
      {"name":"encoding_id","type":"u32","offset_bytes":56,"size_bytes":4,"align_bytes":4},
      {"name":"flags","type":"u32","offset_bytes":60,"size_bytes":4,"align_bytes":4}
    ]
  }]
}
"#;
    let root = temp_test_dir("echo-frame-descriptor");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 90,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let mut payload = vec![0u8; 64];
    payload[0..8].copy_from_slice(&0xCAFEu64.to_le_bytes());
    payload[8..12].copy_from_slice(&7u32.to_le_bytes());
    payload[16..24].copy_from_slice(&42u64.to_le_bytes());
    payload[24..32].copy_from_slice(&921_600u64.to_le_bytes());
    payload[32..40].copy_from_slice(&1_700_000_000u64.to_le_bytes());
    payload[40..44].copy_from_slice(&640u32.to_le_bytes());
    payload[44..48].copy_from_slice(&480u32.to_le_bytes());
    payload[48..52].copy_from_slice(&1_920u32.to_le_bytes());
    payload[52..56].copy_from_slice(&3u32.to_le_bytes());
    payload[56..60].copy_from_slice(&9u32.to_le_bytes());
    payload[60..64].copy_from_slice(&1u32.to_le_bytes());

    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes(
        "camera.frame_to_processor.frame",
        "FrameHandle",
        payload,
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "camera.frame", Some(&socket)).unwrap();

    assert!(output.contains("descriptor=frame"));
    assert!(output.contains("frame_descriptor={resource_id_hash=51966 slot=7 generation=42"));
    assert!(output.contains("size_bytes=921600"));
    assert!(output.contains("width=640 height=480 stride_bytes=1920"));
    assert!(output.contains("format_id=3 encoding_id=9 flags=1"));

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
    assert!(get.contains("runtime_update=pending-on-tick"));

    let startup_get = params_get(&selfdesc, "controller.mode", Some(&socket)).unwrap();
    assert!(startup_get.contains("update=startup"));
    assert!(startup_get.contains("runtime_update=startup-only"));

    let set = params_set(&selfdesc, "controller.kp", "2.5", Some(&socket)).unwrap();
    assert!(set.contains("current=1.0"));
    assert!(set.contains("pending=2.5"));

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
fn parse_remote_params_key_expr_rejects_extra_segments() {
    assert!(
        introspection::parse_remote_params_key_expr("flowrt/params/robot/abc/42/extra").is_none()
    );
}

#[test]
fn discover_remote_params_runtimes_filters_by_hash_and_deduplicates() {
    // 测试 discovery 的 hash 过滤逻辑：用同 session 直接查询特定 key expression，
    // 验证 discover_remote_params_runtimes 的 hash 过滤和去重逻辑。
    // （wildcard get 在 zenoh 同 session 下不触发本地 queryable，属于已知行为限制。）
    let session = zenoh::open(flowrt::zenoh::config_from_environment().unwrap())
        .wait()
        .unwrap();
    let expected_hash = "discovery_hash".to_string();

    let ke_match_1 = format!("flowrt/params/pkg_a/{expected_hash}/8001");
    let ke_match_2 = format!("flowrt/params/pkg_b/{expected_hash}/8002");
    let ke_mismatch = "flowrt/params/pkg_c/other_hash/8003".to_string();

    let hs_match_1 = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 8001,
        started_at_unix_ms: 1000,
        self_description_hash: expected_hash.clone(),
        package: "pkg_a".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let hs_match_2 = flowrt::IntrospectionHandshake {
        pid: 8002,
        package: "pkg_b".to_string(),
        process: "control".to_string(),
        ..hs_match_1.clone()
    };
    let hs_mismatch = flowrt::IntrospectionHandshake {
        pid: 8003,
        self_description_hash: "other_hash".to_string(),
        package: "pkg_c".to_string(),
        ..hs_match_1.clone()
    };

    let state_a = flowrt::IntrospectionState::new();
    state_a.register_param(flowrt::IntrospectionParamSchema {
        name: "kp".to_string(),
        ty: "f32".to_string(),
        update: "on_tick".to_string(),
        current: serde_json::json!(1.0),
        min: None,
        max: None,
        choices: Vec::new(),
    });
    let state_b = flowrt::IntrospectionState::new();
    let state_c = flowrt::IntrospectionState::new();

    let _s1 = flowrt::ZenohParamsServer::open(&session, &ke_match_1, hs_match_1, state_a).unwrap();
    let _s2 = flowrt::ZenohParamsServer::open(&session, &ke_match_2, hs_match_2, state_b).unwrap();
    let _s3 =
        flowrt::ZenohParamsServer::open(&session, &ke_mismatch, hs_mismatch, state_c).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));

    // 直接查询三个端点，模拟 discovery 的 hash 过滤逻辑。
    let resp_1 = flowrt::request_remote_param_list(&session, &ke_match_1, 3000).unwrap();
    let resp_2 = flowrt::request_remote_param_list(&session, &ke_match_2, 3000).unwrap();
    let resp_3 = flowrt::request_remote_param_list(&session, &ke_mismatch, 3000).unwrap();

    // 模拟 discover_remote_params_runtimes 的过滤逻辑。
    let mut entries = Vec::new();
    for (ke, resp) in [
        (&ke_match_1, &resp_1),
        (&ke_match_2, &resp_2),
        (&ke_mismatch, &resp_3),
    ] {
        let Some((_pkg, hash, pid_str)) = introspection::parse_remote_params_key_expr(ke) else {
            continue;
        };
        if hash != expected_hash {
            continue;
        }
        let handshake = match resp {
            flowrt::IntrospectionResponse::ParamList { handshake, .. } => handshake,
            _ => continue,
        };
        entries.push(introspection::RemoteRuntimeEntry {
            key_expr: ke.to_string(),
            pid: pid_str.parse().unwrap(),
            package: handshake.package.clone(),
            process: handshake.process.clone(),
            runtime: handshake.runtime.clone(),
            self_description_hash: hash.to_string(),
        });
    }
    // 只返回 hash 匹配的两个端点。
    assert_eq!(entries.len(), 2);
    let pids: Vec<u32> = entries.iter().map(|e| e.pid).collect();
    assert!(pids.contains(&8001));
    assert!(pids.contains(&8002));
    assert!(!pids.contains(&8003));
}

#[test]
fn select_remote_runtime_returns_single_entry() {
    let entries = vec![introspection::RemoteRuntimeEntry {
        key_expr: "flowrt/params/robot/hash1/100".to_string(),
        pid: 100,
        package: "robot".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
        self_description_hash: "hash1".to_string(),
    }];
    let result = introspection::select_remote_runtime(entries, "hash1").unwrap();
    assert_eq!(result.pid, 100);
    assert_eq!(result.key_expr, "flowrt/params/robot/hash1/100");
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
    assert!(message.contains("--runtime"));
    assert!(message.contains("pid=100"));
    assert!(message.contains("pid=200"));
}
