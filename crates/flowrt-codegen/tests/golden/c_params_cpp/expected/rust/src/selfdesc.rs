// FlowRT 管理产物。不要手工修改。

#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 5240] = *br#"{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "47b1773f4572030b9e9fd09006152e4a3917dbd0443c44957d8787f4f33cc479",
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
    "name": "c_params_demo",
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
  "targets": [],
  "deployments": [],
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
            "period_ms": 5,
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
          "runtime": "c",
          "params": [
            {
              "name": "enabled",
              "type": "bool",
              "update": "startup",
              "current": true,
              "min": null,
              "max": null,
              "choices": []
            },
            {
              "name": "gain",
              "type": "f32",
              "update": "on_tick",
              "current": 2.0,
              "min": 0.0,
              "max": 10.0,
              "choices": []
            },
            {
              "name": "limits",
              "type": "array",
              "update": "startup",
              "current": [
                1,
                2,
                3
              ],
              "min": null,
              "max": null,
              "choices": []
            },
            {
              "name": "mode",
              "type": "string",
              "update": "startup",
              "current": "normal",
              "min": null,
              "max": null,
              "choices": []
            }
          ]
        }
      ],
      "tasks": [
        {
          "name": "main",
          "instance": "controller",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": 5,
          "deadline_ms": null,
          "lane": "controller_serial",
          "priority": null,
          "inputs": [],
          "outputs": [
            "cmd"
          ]
        }
      ],
      "channels": [],
      "boundary_endpoints": [],
      "services": [],
      "operations": []
    }
  ],
  "component_types": [
    {
      "name": "controller",
      "language": "c",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [],
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
      "params": [
        {
          "name": "enabled",
          "type": "bool",
          "update": "startup",
          "default": true,
          "min": null,
          "max": null,
          "choices": []
        },
        {
          "name": "gain",
          "type": "f32",
          "update": "on_tick",
          "default": 1.0,
          "min": 0.0,
          "max": 10.0,
          "choices": []
        },
        {
          "name": "limits",
          "type": "array",
          "update": "startup",
          "default": [
            1,
            2,
            3
          ],
          "min": null,
          "max": null,
          "choices": []
        },
        {
          "name": "mode",
          "type": "string",
          "update": "startup",
          "default": "normal",
          "min": null,
          "max": null,
          "choices": []
        }
      ]
    }
  ],
  "message_abi": [
    {
      "type_name": "Cmd",
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
      "type_name": "Cmd",
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
    "f94952cb4a77a24beea7d14da5edf0f120f6a4f4293dc67ec4c7ebfb6cc970c8"
}
