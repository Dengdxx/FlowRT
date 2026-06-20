// FlowRT 管理产物。不要手工修改。
#include "flowrt_app/selfdesc.hpp"

#include <string_view>

namespace flowrt_app {
namespace {

#if defined(__GNUC__) || defined(__clang__)
[[gnu::used, gnu::section(".flowrt.selfdesc")]]
#endif
const char kFlowrtSelfDescription[] = R"({
  "self_description_version": "0.1",
  "ir_version": "0.1",
  "schema_version": "0.1",
  "source_hash": "8e6bb88bbae216b02d4aae83e072fe7ffe1cc1a56d4b1a24ae579533cd38210b",
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
    "name": "bounded_operation_iox2_cpp",
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
        "cpp"
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
          "runtime": "cpp",
          "params": []
        },
        {
          "name": "navigator",
          "component": "navigator",
          "process": "server_proc",
          "target": "linux",
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
          "backend_source": "profile_default",
          "goal_frame": {
            "message_type": "PlanGoal",
            "encoding": "canonical_frame_v1",
            "variable": true,
            "bounded": true,
            "max_size_bytes": 16,
            "iox2_slot_cap_bytes": null
          },
          "start_request_frame": {
            "message_type": "OperationStartRequest<PlanGoal>",
            "encoding": "canonical_frame_v1",
            "variable": true,
            "bounded": true,
            "max_size_bytes": 40,
            "iox2_slot_cap_bytes": 40
          },
          "timeout_ms": 5000,
          "concurrency": "reject",
          "preempt": "reject",
          "queue_depth": 4,
          "max_in_flight": 1,
          "feedback": "latest",
          "result_retention_ms": 60000,
          "lowering": {
            "start_service": "FlowRT/service/__flowrt_operation_controller_plan_start",
            "start_key_expr": "",
            "cancel_service": "FlowRT/service/__flowrt_operation_controller_plan_cancel",
            "cancel_key_expr": "",
            "status_service": "FlowRT/service/__flowrt_operation_controller_plan_status",
            "status_key_expr": "",
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
      "language": "cpp",
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
      "language": "cpp",
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
      "header_size_bytes": 8,
      "max_size_bytes": 16,
      "variable": true,
      "fields": [
        {
          "name": "target",
          "type": "string<max=8>",
          "header_offset_bytes": 0,
          "header_size_bytes": 8,
          "tail_max_bytes": 8
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
)";

const char kFlowrtSelfDescriptionHash[] = "0871d77ad09391c4a63c93f2f5ea95ef79e44f44f989feadac325fe1886cfec3";

}  // namespace

std::string_view self_description_json() noexcept {
    return std::string_view{kFlowrtSelfDescription, sizeof(kFlowrtSelfDescription) - 1};
}

std::string_view self_description_hash() noexcept {
    return std::string_view{kFlowrtSelfDescriptionHash, sizeof(kFlowrtSelfDescriptionHash) - 1};
}

}  // namespace flowrt_app
