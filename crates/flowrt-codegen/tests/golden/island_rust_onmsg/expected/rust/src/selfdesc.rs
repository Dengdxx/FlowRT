// FlowRT 管理产物。不要手工修改。

#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 4302] = *br#"{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "515793ee29da352cce90dc7681aa7e0456e8addce6cf347cf90fb12198b28cf7",
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
    "name": "island_rust_demo",
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
        "rust"
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
          "runtime": "rust",
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
          "message_type": "Sample"
        },
        {
          "canonical_id": "boundary_bcd811ea02d297e7",
          "name": "echo_out",
          "direction": "output",
          "endpoint": "consumer.echo",
          "instance": "consumer",
          "port": "echo",
          "message_type": "Sample"
        }
      ],
      "services": [],
      "operations": []
    }
  ],
  "component_types": [
    {
      "name": "consumer",
      "language": "rust",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [
        {
          "name": "sample",
          "type": "Sample"
        }
      ],
      "outputs": [
        {
          "name": "echo",
          "type": "Sample"
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
      "type_name": "Sample",
      "size_bytes": 4,
      "align_bytes": 4,
      "empty": false,
      "fields": [
        {
          "name": "value",
          "type": "u32",
          "offset_bytes": 0,
          "size_bytes": 4,
          "align_bytes": 4
        }
      ]
    }
  ],
  "message_frames": [
    {
      "type_name": "Sample",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 4,
      "max_size_bytes": 4,
      "variable": false,
      "fields": [
        {
          "name": "value",
          "type": "u32",
          "header_offset_bytes": 0,
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
    "ceb417f887535f4e0c73c765a5a1d8b4385060ad82ddc9d401b1539dac21d734"
}
