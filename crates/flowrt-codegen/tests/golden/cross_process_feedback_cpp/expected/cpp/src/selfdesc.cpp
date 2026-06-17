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
  "source_hash": "57975ca20664ac145b2b2f3eb73276a677da38cc40e4dc4c5341353197305470",
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
    "name": "xproc_feedback_cpp",
    "version": null,
    "rsdl_version": "0.1"
  },
  "profiles": [
    {
      "name": "default",
      "mode": "strict",
      "backend": "zenoh"
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
        "zenoh"
      ]
    }
  ],
  "deployments": [
    {
      "graph": "default",
      "profile": "default",
      "target": "linux",
      "backend": "zenoh",
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
          "process": "ctrl_proc",
          "target": null,
          "runtime": "cpp",
          "params": []
        },
        {
          "name": "plant",
          "component": "plant",
          "process": "plant_proc",
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
          "backend": "zenoh",
          "thread_affinity": "send_safe",
          "service": null,
          "key_expr": "flowrt/xproc_feedback_cpp/default/default/bind_0/controller_cmd_to_plant_cmd",
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
          "backend": "zenoh",
          "thread_affinity": "send_safe",
          "service": null,
          "key_expr": "flowrt/xproc_feedback_cpp/default/default/bind_1/plant_state_to_controller_state",
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
)";

const char kFlowrtSelfDescriptionHash[] = "5a6431a1447c370c646e2f297911d5a3ce0902beffb53be67d86dbd2d30ed26e";

}  // namespace

std::string_view self_description_json() noexcept {
    return std::string_view{kFlowrtSelfDescription, sizeof(kFlowrtSelfDescription) - 1};
}

std::string_view self_description_hash() noexcept {
    return std::string_view{kFlowrtSelfDescriptionHash, sizeof(kFlowrtSelfDescriptionHash) - 1};
}

}  // namespace flowrt_app
