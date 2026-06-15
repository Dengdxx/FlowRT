use super::*;

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
fn echo_summarizes_long_variable_numeric_sequence_by_default_and_raw_keeps_values() {
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
      "from": "source.scan",
      "to": "sink.scan",
      "message_type": "Scan"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "Scan",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 8,
    "max_size_bytes": null,
    "variable": true,
    "fields": [{
      "name": "ranges",
      "type": "sequence<f32>",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": null
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-sequence-summary");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 901,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&68u32.to_le_bytes());
    for value in 0..17 {
        payload.extend_from_slice(&(value as f32).to_le_bytes());
    }

    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes("source.scan_to_sink.scan", "Scan", payload, Some(123));
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let summary = echo_channel_from_image(&selfdesc, "source.scan", Some(&socket)).unwrap();
    assert!(summary.contains(
        "ranges=sequence_summary(count=17,min=0.0,max=16.0,mean=8.0,first=0.0,last=16.0)"
    ));
    assert!(!summary.contains("ranges=[0.0,1.0"));

    let raw = echo_channel_from_image_with_options(
        &selfdesc,
        "source.scan",
        Some(&socket),
        EchoFormatOptions { raw: true },
    )
    .unwrap();
    assert!(raw.contains("ranges=[0.0,1.0"));
    assert!(raw.contains(",16.0]"));
    assert!(!raw.contains("sequence_summary"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_summarizes_long_variable_named_fixed_struct_sequence_by_default() {
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
      "from": "source.path",
      "to": "sink.path",
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
    "header_size_bytes": 8,
    "max_size_bytes": null,
    "variable": true,
    "fields": [{
      "name": "points",
      "type": "sequence<Point>",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": null
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-named-struct-sequence-summary");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 903,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&136u32.to_le_bytes());
    for value in 0..17 {
        payload.extend_from_slice(&(value as f32).to_le_bytes());
        payload.extend_from_slice(&((value + 100) as f32).to_le_bytes());
    }

    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes("source.path_to_sink.path", "PathFrame", payload, Some(123));
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let summary = echo_channel_from_image(&selfdesc, "source.path", Some(&socket)).unwrap();

    assert!(
        summary.contains(
            "points=sequence_summary(count=17,first={x=0.0,y=100.0},last={x=16.0,y=116.0})"
        )
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_multiple_channels_keep_prefix_when_sequence_is_summarized() {
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
      "from": "source.a",
      "to": "sink.a",
      "message_type": "Scan"
    }, {
      "from": "source.b",
      "to": "sink.b",
      "message_type": "Scan"
    }]
  }],
  "message_abi": [],
  "message_frames": [{
    "type_name": "Scan",
    "encoding": "canonical_frame_v1",
    "header_size_bytes": 8,
    "max_size_bytes": null,
    "variable": true,
    "fields": [{
      "name": "ranges",
      "type": "sequence<u32>",
      "header_offset_bytes": 0,
      "header_size_bytes": 8,
      "tail_max_bytes": null
    }]
  }]
}
"#;
    let root = temp_test_dir("echo-sequence-summary-multi");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 902,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let mut payload = Vec::new();
    payload.extend_from_slice(&0u32.to_le_bytes());
    payload.extend_from_slice(&68u32.to_le_bytes());
    for value in 0..17 {
        payload.extend_from_slice(&(value as u32).to_le_bytes());
    }

    let state = flowrt::IntrospectionState::new();
    state.record_channel_publish_bytes("source.a_to_sink.a", "Scan", payload.clone(), Some(10));
    state.record_channel_publish_bytes("source.b_to_sink.b", "Scan", payload, Some(20));
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channels(
        &EchoSelection {
            image: Some(selfdesc.clone()),
            channels: vec!["source.a".to_string(), "source.b".to_string()],
        },
        Some(&socket),
        EchoFormatOptions::default(),
    )
    .unwrap();

    let lines = output.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("channel=source.a_to_sink.a "));
    assert!(lines[0].contains("ranges=sequence_summary(count=17"));
    assert!(lines[1].starts_with("channel=source.b_to_sink.b "));
    assert!(lines[1].contains("ranges=sequence_summary(count=17"));

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
        EchoFormatOptions::default(),
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
        .env_remove("CARGO_TARGET_DIR")
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
fn echo_multiple_channels_from_image_prefixes_each_line() {
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
      "from": "source.a",
      "to": "sink.a",
      "message_type": "Count"
    }, {
      "from": "source.b",
      "to": "sink.b",
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
    let root = temp_test_dir("echo-multi-channel");
    let selfdesc = root.join("selfdesc.json");
    let socket = root.join("main.sock");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&selfdesc, source).unwrap();

    let handshake = flowrt::IntrospectionHandshake {
        protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),
        pid: 91,
        started_at_unix_ms: 1234,
        self_description_hash: self_description_hash(source.as_bytes()),
        package: "robot_demo".to_string(),
        process: "main".to_string(),
        runtime: "rust".to_string(),
    };
    let state = flowrt::IntrospectionState::new();
    state.register_channel("source.a_to_sink.a", "Count");
    state.register_channel("source.b_to_sink.b", "Count");
    state.record_channel_publish_bytes(
        "source.a_to_sink.a",
        "Count",
        1u32.to_le_bytes().to_vec(),
        Some(10),
    );
    state.record_channel_publish_bytes(
        "source.b_to_sink.b",
        "Count",
        2u32.to_le_bytes().to_vec(),
        Some(20),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channels(
        &EchoSelection {
            image: Some(selfdesc.clone()),
            channels: vec!["source.a".to_string(), "source.b".to_string()],
        },
        Some(&socket),
        EchoFormatOptions::default(),
    )
    .unwrap();

    let lines = output.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("channel=source.a_to_sink.a "));
    assert!(lines[0].contains("fields={value=1}"));
    assert!(lines[1].starts_with("channel=source.b_to_sink.b "));
    assert!(lines[1].contains("fields={value=2}"));

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
        EchoFormatOptions::default(),
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
        EchoFormatOptions::default(),
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
fn echo_prefers_canonical_frame_for_fixed_message_with_native_padding() {
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
      "message_type": "Padded",
      "backend": "zenoh"
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
    let root = temp_test_dir("echo-fixed-frame-padding");
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
    state.record_channel_publish_bytes(
        "source.packet_to_sink.packet",
        "Padded",
        vec![7, 0, 0, 0xc0, 0x3f],
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "source.packet", Some(&socket)).unwrap();

    assert!(output.contains("frame_max_size=5 variable=false"));
    assert!(output.contains("payload_len=5"));
    assert!(output.contains("fields={flag=7,value=1.5}"));
    assert!(output.contains("raw=070000c03f"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn echo_keeps_native_abi_for_inproc_fixed_message_with_padding() {
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
      "message_type": "Padded",
      "backend": "inproc"
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
    let root = temp_test_dir("echo-inproc-fixed-padding");
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
    state.record_channel_publish_bytes(
        "source.packet_to_sink.packet",
        "Padded",
        vec![7, 0, 0, 0, 0, 0, 0xc0, 0x3f],
        Some(123),
    );
    let server = flowrt::spawn_status_server_at(socket.clone(), handshake, state)
        .expect("status server should start");

    let output = echo_channel_from_image(&selfdesc, "source.packet", Some(&socket)).unwrap();

    assert!(output.contains("abi_size=8"));
    assert!(output.contains("payload_len=8"));
    assert!(output.contains("fields={flag=7,value=1.5}"));
    assert!(output.contains("raw=070000000000c03f"));

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn auto_socket_discovery_removes_stale_runtime_socket_candidates() {
    let _lock = XDG_RUNTIME_DIR_ENV_LOCK
        .lock()
        .expect("runtime dir env lock should not be poisoned");
    let root = temp_test_dir("echo-stale-socket-cleanup");
    let runtime_root = root.join("xdg-runtime");
    let _env = EnvOverride::set("XDG_RUNTIME_DIR", Some(runtime_root.as_os_str()));
    let socket_dir = flowrt::runtime_socket_dir();
    std::fs::create_dir_all(&socket_dir).unwrap();

    let stale_socket = socket_dir.join("999999.sock");
    std::fs::write(&stale_socket, "not a unix socket").unwrap();

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
        "stale runtime socket candidate should be removed"
    );

    drop(server);
    let _ = std::fs::remove_dir_all(&root);
}
