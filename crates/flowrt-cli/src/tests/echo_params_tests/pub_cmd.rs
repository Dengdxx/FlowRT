use super::*;

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
fn pub_prefers_canonical_frame_for_fixed_message_with_native_padding() {
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
      "message_type": "Padded"
    }]
  }],
  "message_abi": [{
    "type_name": "Padded",
    "size_bytes": 8,
    "align_bytes": 4,
    "fields": [{
      "name": "flag",
      "type": "u8",
      "offset_bytes": 0,
      "size_bytes": 1,
      "align_bytes": 1
    }, {
      "name": "value",
      "type": "f32",
      "offset_bytes": 4,
      "size_bytes": 4,
      "align_bytes": 4
    }]
  }],
  "message_frames": [{
    "type_name": "Padded",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 5,
    "max_size_bytes": 5,
    "variable": false,
    "fields": [{
      "name": "flag",
      "type": "u8",
      "header_offset_bytes": 0,
      "header_size_bytes": 1,
      "tail_max_bytes": null
    }, {
      "name": "value",
      "type": "f32",
      "header_offset_bytes": 1,
      "header_size_bytes": 4,
      "tail_max_bytes": null
    }]
  }]
}
"#;
    let root = temp_test_dir("pub-fixed-frame-padding");
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
    state.register_boundary_input_handler("sample_in", "Padded", move |payload, _| {
        *captured_for_handler.lock().unwrap() = payload.to_vec();
        Ok(1)
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    boundary_publish(
        "sample_in",
        r#"{"flag": 7, "value": 1.5}"#,
        Some(&selfdesc),
        Some(&socket),
        None,
    )
    .unwrap();

    assert_eq!(*captured.lock().unwrap(), vec![7, 0, 0, 0xc0, 0x3f]);

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
fn pub_injects_variable_canonical_frame_json_into_boundary_input() {
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
fn pub_file_injects_jsonl_into_boundary_input() {
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
      "canonical_id": "boundary_sample_0123456789abcdef",
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
    let root = temp_test_dir("pub-file-jsonl");
    let selfdesc = root.join("selfdesc.json");
    let input = root.join("samples.jsonl");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(&input, "{\"value\":7}\n\n{\"value\":8}\n").unwrap();

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
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));
    let captured_for_handler = captured.clone();
    state.register_boundary_input_handler("sample_in", "Sample", move |payload, timestamp| {
        assert_eq!(timestamp, Some(789));
        let bytes: [u8; 4] = payload.try_into().expect("u32 payload");
        let value = u32::from_le_bytes(bytes);
        let mut captured = captured_for_handler.lock().unwrap();
        captured.push(value);
        Ok(captured.len() as u64)
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = boundary_publish_from_file(
        "sample_in",
        &input,
        Some(&selfdesc),
        Some(&socket),
        Some(789),
        None,
    )
    .unwrap();

    assert!(output.contains("revision=1"));
    assert!(output.contains("revision=2"));
    assert!(output.contains("summary: endpoint=sample_in sent=2"));
    assert_eq!(*captured.lock().unwrap(), vec![7, 8]);

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_file_injects_json_array_into_boundary_input_with_freq() {
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
      "canonical_id": "boundary_sample_0123456789abcdef",
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
    let root = temp_test_dir("pub-file-array");
    let selfdesc = root.join("selfdesc.json");
    let input = root.join("samples.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();
    std::fs::write(&input, r#"[{"value":10},{"value":11}]"#).unwrap();

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
    let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));
    let captured_for_handler = captured.clone();
    state.register_boundary_input_handler("sample_in", "Sample", move |payload, _| {
        let bytes: [u8; 4] = payload.try_into().expect("u32 payload");
        let value = u32::from_le_bytes(bytes);
        let mut captured = captured_for_handler.lock().unwrap();
        captured.push(value);
        Ok(captured.len() as u64)
    });
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = boundary_publish_from_file(
        "sample_in",
        &input,
        Some(&selfdesc),
        Some(&socket),
        None,
        Some(10_000.0),
    )
    .unwrap();

    assert!(output.contains("summary: endpoint=sample_in sent=2"));
    assert_eq!(*captured.lock().unwrap(), vec![10, 11]);

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pub_file_reports_jsonl_line_for_invalid_json() {
    let root = temp_test_dir("pub-file-bad-jsonl");
    let input = root.join("samples.jsonl");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&input, "{bad json}\n").unwrap();

    let error =
        boundary_publish_from_file("sample_in", &input, None, None, None, None).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("line 1"), "unexpected error: {message}");
    assert!(
        message.contains("must be valid JSON"),
        "unexpected error: {message}"
    );

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
