// FlowRT 管理产物。不要手工修改。

#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 6723] = *br#"{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "3cd23f05341ec5314e387f75ac80226aee5a89e4a6613f401031326fa0c6e50b",
  "artifact": {
    "mode": "strict",
    "temporary_island": false,
    "test_only": false,
    "clock": {
      "source": "realtime",
      "unit": "ms",
      "field": "tick_time_ms"
    }
  },
  "package": {
    "name": "feedback_loop_cpp_demo",
    "version": null,
    "rsdl_version": "0.1"
  },
  "profiles": [
    {
      "name": "default",
      "mode": "strict",
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
      "profile": "default",
      "target": "linux",
      "backend": "inproc",
      "satisfied": true
    }
  ],
  "graphs": [
    {
      "name": "default",
      "mode": "strict",
      "scheduler": {
        "worker_threads": 1,
        "lanes": [
          {
            "name": "controller_serial",
            "kind": "serial",
            "instance": "controller"
          },
          {
            "name": "plant_serial",
            "kind": "serial",
            "instance": "plant"
          }
        ],
        "tasks": [
          {
            "name": "main",
            "instance": "controller",
            "lane": "controller_serial",
            "trigger": "periodic",
            "readiness": "any_ready",
            "concurrency": "exclusive",
            "period_ms": 10,
            "deadline_ms": null,
            "priority": null
          },
          {
            "name": "main",
            "instance": "plant",
            "lane": "plant_serial",
            "trigger": "periodic",
            "readiness": "any_ready",
            "concurrency": "exclusive",
            "period_ms": 10,
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
          "name": "controller",
          "component": "controller",
          "process": "main",
          "target": null,
          "runtime": "cpp",
          "params": []
        },
        {
          "name": "plant",
          "component": "plant",
          "process": "main",
          "target": null,
          "runtime": "cpp",
          "params": []
        }
      ],
      "tasks": [
        {
          "name": "main",
          "instance": "controller",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": 10,
          "deadline_ms": null,
          "lane": "controller_serial",
          "priority": null,
          "inputs": [
            "state"
          ],
          "outputs": [
            "cmd"
          ]
        },
        {
          "name": "main",
          "instance": "plant",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": 10,
          "deadline_ms": null,
          "lane": "plant_serial",
          "priority": null,
          "inputs": [
            "cmd"
          ],
          "outputs": [
            "state"
          ]
        }
      ],
      "channels": [
        {
          "from": "controller.cmd",
          "to": "plant.cmd",
          "message_type": "Cmd",
          "backend": "inproc",
          "thread_affinity": "send_safe",
          "service": null,
          "key_expr": null,
          "channel": "latest",
          "depth": 1,
          "overflow": "drop_oldest",
          "stale_policy": "warn",
          "max_age_ms": null
        },
        {
          "from": "plant.state",
          "to": "controller.state",
          "message_type": "State",
          "backend": "inproc",
          "thread_affinity": "send_safe",
          "service": null,
          "key_expr": null,
          "channel": "latest",
          "depth": 1,
          "overflow": "drop_oldest",
          "stale_policy": "warn",
          "max_age_ms": null
        }
      ],
      "boundary_endpoints": [],
      "services": [],
      "operations": []
    }
  ],
  "component_types": [
    {
      "name": "controller",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [
        {
          "name": "state",
          "type": "State"
        }
      ],
      "outputs": [
        {
          "name": "cmd",
          "type": "Cmd"
        }
      ],
      "service_clients": [],
      "service_servers": [],
      "operation_clients": [],
      "operation_servers": [],
      "params": []
    },
    {
      "name": "plant",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [
        {
          "name": "cmd",
          "type": "Cmd"
        }
      ],
      "outputs": [
        {
          "name": "state",
          "type": "State"
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
      "type_name": "Cmd",
      "size_bytes": 8,
      "align_bytes": 8,
      "empty": false,
      "fields": [
        {
          "name": "u",
          "type": "f64",
          "offset_bytes": 0,
          "size_bytes": 8,
          "align_bytes": 8
        }
      ]
    },
    {
      "type_name": "State",
      "size_bytes": 8,
      "align_bytes": 8,
      "empty": false,
      "fields": [
        {
          "name": "x",
          "type": "f64",
          "offset_bytes": 0,
          "size_bytes": 8,
          "align_bytes": 8
        }
      ]
    }
  ],
  "message_frames": [
    {
      "type_name": "Cmd",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 8,
      "max_size_bytes": 8,
      "variable": false,
      "fields": [
        {
          "name": "u",
          "type": "f64",
          "header_offset_bytes": 0,
          "header_size_bytes": 8,
          "tail_max_bytes": null
        }
      ]
    },
    {
      "type_name": "State",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 8,
      "max_size_bytes": 8,
      "variable": false,
      "fields": [
        {
          "name": "x",
          "type": "f64",
          "header_offset_bytes": 0,
          "header_size_bytes": 8,
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
    "477ddebf06b7db7aac326fb8b8d751bc61eceff09e458929041da8ca17c96dd3"
}
