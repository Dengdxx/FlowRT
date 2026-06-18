// FlowRT 管理产物。不要手工修改。

#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 4299] = *br#"{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "21ad07313143b59d87a7db6969aa4722a06636dd5fdce8314f4ac8d44247ae8a",
  "artifact": {
    "mode": "island",
    "temporary_island": false,
    "test_only": true,
    "clock": {
      "source": "simulated_replay",
      "unit": "ms",
      "field": "tick_time_ms"
    }
  },
  "package": {
    "name": "fault_injection_degrade_demo",
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
            "name": "monitor_serial",
            "kind": "serial",
            "instance": "monitor"
          }
        ],
        "tasks": [
          {
            "name": "main",
            "instance": "monitor",
            "lane": "monitor_serial",
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
          "name": "monitor",
          "component": "monitor",
          "process": "main",
          "target": null,
          "runtime": "rust",
          "params": []
        }
      ],
      "tasks": [
        {
          "name": "main",
          "instance": "monitor",
          "trigger": "on_message",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": null,
          "deadline_ms": null,
          "lane": "monitor_serial",
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
          "canonical_id": "boundary_1781ee2eddd122f3",
          "name": "feed",
          "direction": "input",
          "endpoint": "monitor.sample",
          "instance": "monitor",
          "port": "sample",
          "message_type": "Sample"
        },
        {
          "canonical_id": "boundary_d2f6c7298dbf5f9b",
          "name": "emit",
          "direction": "output",
          "endpoint": "monitor.echo",
          "instance": "monitor",
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
      "name": "monitor",
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
    "073eed72c1bf6d6d7ba84b984eab3770b8e3564cb8fbc648f11cac775f6f6418"
}
