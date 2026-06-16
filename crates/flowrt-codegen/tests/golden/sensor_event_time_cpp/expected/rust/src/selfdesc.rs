// FlowRT 管理产物。不要手工修改。

#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 4655] = *br#"{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "571ad42c1715412213e277f923d5a06d4409b403afc5752fc2d4ea436145d404",
  "artifact": {
    "mode": "island",
    "temporary_island": false,
    "test_only": false,
    "clock": {
      "source": "realtime",
      "unit": "ms",
      "field": "tick_time_ms"
    }
  },
  "package": {
    "name": "island_sensor_cpp_demo",
    "version": null,
    "rsdl_version": "0.1"
  },
  "profiles": [
    {
      "name": "dev",
      "mode": "island",
      "backend": "inproc"
    }
  ],
  "targets": [
    {
      "name": "linux",
      "platform": null,
      "runtimes": [
        "cpp"
      ],
      "backends": [
        "inproc"
      ]
    }
  ],
  "deployments": [
    {
      "graph": "default",
      "profile": "dev",
      "target": "linux",
      "backend": "inproc",
      "satisfied": true
    }
  ],
  "graphs": [
    {
      "name": "default",
      "mode": "island",
      "scheduler": {
        "worker_threads": 1,
        "lanes": [
          {
            "name": "consumer_serial",
            "kind": "serial",
            "instance": "consumer"
          }
        ],
        "tasks": [
          {
            "name": "main",
            "instance": "consumer",
            "lane": "consumer_serial",
            "trigger": "on_message",
            "readiness": "any_ready",
            "concurrency": "exclusive",
            "period_ms": null,
            "deadline_ms": null,
            "priority": null
          }
        ]
      },
      "resource_contract": {
        "resource_contract_version": "0.1",
        "requirements": [],
        "providers": [],
        "satisfactions": []
      },
      "external_processes": [],
      "instances": [
        {
          "name": "consumer",
          "component": "consumer",
          "process": "main",
          "target": null,
          "runtime": "cpp",
          "params": []
        }
      ],
      "tasks": [
        {
          "name": "main",
          "instance": "consumer",
          "trigger": "on_message",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": null,
          "deadline_ms": null,
          "lane": "consumer_serial",
          "priority": null,
          "inputs": [
            "sample"
          ],
          "outputs": [
            "echo"
          ]
        }
      ],
      "channels": [],
      "boundary_endpoints": [
        {
          "canonical_id": "boundary_82eb2c6e51af3a5b",
          "name": "sample_in",
          "direction": "input",
          "endpoint": "consumer.sample",
          "instance": "consumer",
          "port": "sample",
          "message_type": "ImuSample"
        },
        {
          "canonical_id": "boundary_bcd811ea02d297e7",
          "name": "echo_out",
          "direction": "output",
          "endpoint": "consumer.echo",
          "instance": "consumer",
          "port": "echo",
          "message_type": "ImuSample"
        }
      ],
      "services": [],
      "operations": []
    }
  ],
  "component_types": [
    {
      "name": "consumer",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [
        {
          "name": "sample",
          "type": "ImuSample"
        }
      ],
      "outputs": [
        {
          "name": "echo",
          "type": "ImuSample"
        }
      ],
      "service_clients": [],
      "service_servers": [],
      "operation_clients": [],
      "operation_servers": [],
      "params": []
    }
  ],
  "message_abi": [
    {
      "type_name": "ImuSample",
      "size_bytes": 8,
      "align_bytes": 4,
      "empty": false,
      "fields": [
        {
          "name": "stamp_us",
          "type": "u32",
          "offset_bytes": 0,
          "size_bytes": 4,
          "align_bytes": 4
        },
        {
          "name": "ax",
          "type": "f32",
          "offset_bytes": 4,
          "size_bytes": 4,
          "align_bytes": 4
        }
      ]
    }
  ],
  "message_frames": [
    {
      "type_name": "ImuSample",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 8,
      "max_size_bytes": 8,
      "variable": false,
      "fields": [
        {
          "name": "stamp_us",
          "type": "u32",
          "header_offset_bytes": 0,
          "header_size_bytes": 4,
          "tail_max_bytes": null
        },
        {
          "name": "ax",
          "type": "f32",
          "header_offset_bytes": 4,
          "header_size_bytes": 4,
          "tail_max_bytes": null
        }
      ]
    }
  ]
}
"#;

#[allow(dead_code)]
pub fn self_description_json() -> &'static str {
    std::str::from_utf8(&FLOWRT_SELF_DESCRIPTION).expect("generated FlowRT self-description is UTF-8")
}

#[allow(dead_code)]
pub fn self_description_hash() -> &'static str {
    "8350bcd8135abc6754f446822233fd71d2ecb474a88011661e4f7a4cc9419453"
}
