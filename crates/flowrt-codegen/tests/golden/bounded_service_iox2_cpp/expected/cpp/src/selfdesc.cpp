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
  "source_hash": "745a68577ecf460449365b7d76eef9b3d1b1faabd6321b8dd93ef4c6d4103066",
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
    "name": "bounded_service_iox2_cpp",
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
          "runtime": "cpp",
          "params": []
        },
        {
          "name": "plan_svc",
          "component": "plan_service",
          "process": "server_proc",
          "target": "linux",
          "runtime": "cpp",
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
          "backend": "iox2",
          "backend_source": "profile_default",
          "request_frame": {
            "message_type": "PlanRequest",
            "encoding": "canonical_frame_v1",
            "variable": true,
            "bounded": true,
            "max_size_bytes": 44,
            "iox2_slot_cap_bytes": 44
          },
          "response_frame": {
            "message_type": "PlanResponse",
            "encoding": "canonical_frame_v1",
            "variable": true,
            "bounded": true,
            "max_size_bytes": 21,
            "iox2_slot_cap_bytes": 21
          },
          "service": "FlowRT/service/plan_client_plan",
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
      "language": "cpp",
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
      "language": "cpp",
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
  "message_abi": [],
  "message_frames": [
    {
      "type_name": "PlanRequest",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 20,
      "max_size_bytes": 44,
      "variable": true,
      "fields": [
        {
          "name": "goal",
          "type": "u32",
          "header_offset_bytes": 0,
          "header_size_bytes": 4,
          "tail_max_bytes": null
        },
        {
          "name": "label",
          "type": "string<max=8>",
          "header_offset_bytes": 4,
          "header_size_bytes": 8,
          "tail_max_bytes": 8
        },
        {
          "name": "samples",
          "type": "sequence<u32,max=4>",
          "header_offset_bytes": 12,
          "header_size_bytes": 8,
          "tail_max_bytes": 16
        }
      ]
    },
    {
      "type_name": "PlanResponse",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 9,
      "max_size_bytes": 21,
      "variable": true,
      "fields": [
        {
          "name": "accepted",
          "type": "bool",
          "header_offset_bytes": 0,
          "header_size_bytes": 1,
          "tail_max_bytes": null
        },
        {
          "name": "detail",
          "type": "string<max=12>",
          "header_offset_bytes": 1,
          "header_size_bytes": 8,
          "tail_max_bytes": 12
        }
      ]
    }
  ]
}
)";

const char kFlowrtSelfDescriptionHash[] = "c0539542331875d9f9e94037ef547372b098f47620a34b95b005ed136e1f8a47";

}  // namespace

std::string_view self_description_json() noexcept {
    return std::string_view{kFlowrtSelfDescription, sizeof(kFlowrtSelfDescription) - 1};
}

std::string_view self_description_hash() noexcept {
    return std::string_view{kFlowrtSelfDescriptionHash, sizeof(kFlowrtSelfDescriptionHash) - 1};
}

}  // namespace flowrt_app
