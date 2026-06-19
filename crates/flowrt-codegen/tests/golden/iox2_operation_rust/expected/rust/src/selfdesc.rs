// FlowRT 管理产物。不要手工修改。

#[used]
#[unsafe(link_section = ".flowrt.selfdesc")]
static FLOWRT_SELF_DESCRIPTION: [u8; 7651] = *br#"{
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "f56582f3a60beaf8a3d2e8150945aa68372dddf6535ec10020377cd9dba5c901",
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
    "name": "iox2_operation_rust",
    "version": null,
    "rsdl_version": "0.1"
  },
  "profiles": [
    {
      "name": "default",
      "mode": "strict",
      "backend": "iox2"
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
        "iox2"
      ]
    }
  ],
  "deployments": [
    {
      "graph": "default",
      "profile": "default",
      "target": "linux",
      "backend": "iox2",
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
            "name": "navigator_serial",
            "kind": "serial",
            "instance": "navigator"
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
            "period_ms": 100,
            "deadline_ms": null,
            "priority": null
          },
          {
            "name": "main",
            "instance": "navigator",
            "lane": "navigator_serial",
            "trigger": "periodic",
            "readiness": "any_ready",
            "concurrency": "exclusive",
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
          "name": "controller",
          "component": "controller",
          "process": "client_proc",
          "target": "linux",
          "runtime": "rust",
          "params": []
        },
        {
          "name": "navigator",
          "component": "navigator",
          "process": "server_proc",
          "target": "linux",
          "runtime": "rust",
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
          "period_ms": 100,
          "deadline_ms": null,
          "lane": "controller_serial",
          "priority": null,
          "inputs": [],
          "outputs": []
        },
        {
          "name": "main",
          "instance": "navigator",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": 1000,
          "deadline_ms": null,
          "lane": "navigator_serial",
          "priority": null,
          "inputs": [],
          "outputs": []
        }
      ],
      "channels": [],
      "boundary_endpoints": [],
      "services": [],
      "operations": [
        {
          "name": "controller.plan",
          "canonical_id": "operation_6a76898c2802c4e0",
          "client_instance": "controller",
          "client_port": "plan",
          "server_instance": "navigator",
          "server_port": "plan",
          "goal_type": "PlanGoal",
          "feedback_type": "PlanFeedback",
          "result_type": "PlanResult",
          "backend": "iox2",
          "timeout_ms": 5000,
          "concurrency": "reject",
          "preempt": "reject",
          "queue_depth": 4,
          "max_in_flight": 1,
          "feedback": "latest",
          "result_retention_ms": 60000,
          "lowering": {
            "start_service": "__flowrt_operation_controller_plan_start",
            "cancel_service": "__flowrt_operation_controller_plan_cancel",
            "status_service": "__flowrt_operation_controller_plan_status",
            "feedback_channel": "__flowrt_operation_controller_plan_feedback",
            "result_channel": "__flowrt_operation_controller_plan_result"
          }
        }
      ]
    }
  ],
  "component_types": [
    {
      "name": "controller",
      "language": "rust",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [],
      "outputs": [],
      "service_clients": [],
      "service_servers": [],
      "operation_clients": [
        {
          "name": "plan",
          "goal_type": "PlanGoal",
          "feedback_type": "PlanFeedback",
          "result_type": "PlanResult"
        }
      ],
      "operation_servers": [],
      "params": []
    },
    {
      "name": "navigator",
      "language": "rust",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [],
      "outputs": [],
      "service_clients": [],
      "service_servers": [],
      "operation_clients": [],
      "operation_servers": [
        {
          "name": "plan",
          "goal_type": "PlanGoal",
          "feedback_type": "PlanFeedback",
          "result_type": "PlanResult"
        }
      ],
      "params": []
    }
  ],
  "message_abi": [
    {
      "type_name": "PlanFeedback",
      "size_bytes": 4,
      "align_bytes": 4,
      "empty": false,
      "fields": [
        {
          "name": "progress",
          "type": "f32",
          "offset_bytes": 0,
          "size_bytes": 4,
          "align_bytes": 4
        }
      ]
    },
    {
      "type_name": "PlanGoal",
      "size_bytes": 4,
      "align_bytes": 4,
      "empty": false,
      "fields": [
        {
          "name": "target",
          "type": "u32",
          "offset_bytes": 0,
          "size_bytes": 4,
          "align_bytes": 4
        }
      ]
    },
    {
      "type_name": "PlanResult",
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
      "type_name": "PlanFeedback",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 4,
      "max_size_bytes": 4,
      "variable": false,
      "fields": [
        {
          "name": "progress",
          "type": "f32",
          "header_offset_bytes": 0,
          "header_size_bytes": 4,
          "tail_max_bytes": null
        }
      ]
    },
    {
      "type_name": "PlanGoal",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 4,
      "max_size_bytes": 4,
      "variable": false,
      "fields": [
        {
          "name": "target",
          "type": "u32",
          "header_offset_bytes": 0,
          "header_size_bytes": 4,
          "tail_max_bytes": null
        }
      ]
    },
    {
      "type_name": "PlanResult",
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
    "d635d2d32671b4c5a06cfedfea3a8d4d2220e249408759786450baf3e0507593"
}
