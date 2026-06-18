// FlowRT 管理产物。不要手工修改。

#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 6492] = *br#"{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "bfc001a8e3472abcd25f2136350dc8b7c972fa089402a409bf0a5a14f11a6794",
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
    "name": "zenoh_service_rust",
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
        "rust"
      ],
      "backends": [
        "inproc",
        "zenoh"
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
            "name": "plan_client_serial",
            "kind": "serial",
            "instance": "plan_client"
          },
          {
            "name": "plan_svc_serial",
            "kind": "serial",
            "instance": "plan_svc"
          }
        ],
        "tasks": [
          {
            "name": "main",
            "instance": "plan_client",
            "lane": "plan_client_serial",
            "trigger": "periodic",
            "readiness": "any_ready",
            "concurrency": "exclusive",
            "period_ms": 100,
            "deadline_ms": null,
            "priority": null
          },
          {
            "name": "main",
            "instance": "plan_svc",
            "lane": "plan_svc_serial",
            "trigger": "periodic",
            "readiness": "any_ready",
            "concurrency": "parallel",
            "period_ms": 1000,
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
          "name": "plan_client",
          "component": "planner",
          "process": "client_proc",
          "target": "linux",
          "runtime": "rust",
          "params": []
        },
        {
          "name": "plan_svc",
          "component": "plan_service",
          "process": "server_proc",
          "target": "linux",
          "runtime": "rust",
          "params": []
        }
      ],
      "tasks": [
        {
          "name": "main",
          "instance": "plan_client",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": 100,
          "deadline_ms": null,
          "lane": "plan_client_serial",
          "priority": null,
          "inputs": [],
          "outputs": []
        },
        {
          "name": "main",
          "instance": "plan_svc",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "parallel",
          "period_ms": 1000,
          "deadline_ms": null,
          "lane": "plan_svc_serial",
          "priority": null,
          "inputs": [],
          "outputs": []
        }
      ],
      "channels": [],
      "boundary_endpoints": [],
      "services": [
        {
          "name": "plan_client.plan_to_plan_svc.plan",
          "canonical_id": "service_065ca539d970ad81",
          "client_instance": "plan_client",
          "client_port": "plan",
          "server_instance": "plan_svc",
          "server_port": "plan",
          "request_type": "PlanRequest",
          "response_type": "PlanResponse",
          "backend": "zenoh",
          "key_expr": "flowrt/service/plan_x5F_client.plan/request",
          "timeout_ms": 1000,
          "queue_depth": 16,
          "overflow": "busy",
          "lane": "",
          "max_in_flight": 64
        }
      ],
      "operations": []
    }
  ],
  "component_types": [
    {
      "name": "plan_service",
      "language": "rust",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [],
      "outputs": [],
      "service_clients": [],
      "service_servers": [
        {
          "name": "plan",
          "request_type": "PlanRequest",
          "response_type": "PlanResponse"
        }
      ],
      "operation_clients": [],
      "operation_servers": [],
      "params": []
    },
    {
      "name": "planner",
      "language": "rust",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [],
      "outputs": [],
      "service_clients": [
        {
          "name": "plan",
          "request_type": "PlanRequest",
          "response_type": "PlanResponse"
        }
      ],
      "service_servers": [],
      "operation_clients": [],
      "operation_servers": [],
      "params": []
    }
  ],
  "message_abi": [
    {
      "type_name": "PlanRequest",
      "size_bytes": 4,
      "align_bytes": 4,
      "empty": false,
      "fields": [
        {
          "name": "goal",
          "type": "u32",
          "offset_bytes": 0,
          "size_bytes": 4,
          "align_bytes": 4
        }
      ]
    },
    {
      "type_name": "PlanResponse",
      "size_bytes": 1,
      "align_bytes": 1,
      "empty": false,
      "fields": [
        {
          "name": "accepted",
          "type": "bool",
          "offset_bytes": 0,
          "size_bytes": 1,
          "align_bytes": 1
        }
      ]
    }
  ],
  "message_frames": [
    {
      "type_name": "PlanRequest",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 4,
      "max_size_bytes": 4,
      "variable": false,
      "fields": [
        {
          "name": "goal",
          "type": "u32",
          "header_offset_bytes": 0,
          "header_size_bytes": 4,
          "tail_max_bytes": null
        }
      ]
    },
    {
      "type_name": "PlanResponse",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 1,
      "max_size_bytes": 1,
      "variable": false,
      "fields": [
        {
          "name": "accepted",
          "type": "bool",
          "header_offset_bytes": 0,
          "header_size_bytes": 1,
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
    "eff7080a4f1eb98370f7bc197fd7870dc3b55cde405c9404de66424bfaa54311"
}
