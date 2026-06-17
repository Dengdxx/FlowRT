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
  "source_hash": "1b6436242c95f3ad727e4b5a31aaa9c36de2ce646dce697aa6218d8c9f0deb11",
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
    "name": "instance_degrade_cpp_demo",
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
            "name": "consumer_serial",
            "kind": "serial",
            "instance": "consumer"
          },
          {
            "name": "monitor_serial",
            "kind": "serial",
            "instance": "monitor"
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
          },
          {
            "name": "main",
            "instance": "monitor",
            "lane": "monitor_serial",
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
          "name": "consumer",
          "component": "sink",
          "process": "main",
          "target": null,
          "runtime": "cpp",
          "params": []
        },
        {
          "name": "monitor",
          "component": "producer",
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
          "outputs": []
        },
        {
          "name": "main",
          "instance": "monitor",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": 10,
          "deadline_ms": null,
          "lane": "monitor_serial",
          "priority": null,
          "inputs": [],
          "outputs": [
            "sample"
          ]
        }
      ],
      "channels": [
        {
          "from": "monitor.sample",
          "to": "consumer.sample",
          "message_type": "Sample",
          "backend": "inproc",
          "thread_affinity": "send_safe",
          "service": null,
          "key_expr": null,
          "channel": "latest",
          "depth": 1,
          "overflow": "drop_oldest",
          "stale_policy": "hold_last",
          "max_age_ms": 50
        }
      ],
      "boundary_endpoints": [],
      "services": [],
      "operations": []
    }
  ],
  "component_types": [
    {
      "name": "producer",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [],
      "outputs": [
        {
          "name": "sample",
          "type": "Sample"
        }
      ],
      "service_clients": [],
      "service_servers": [],
      "operation_clients": [],
      "operation_servers": [],
      "params": []
    },
    {
      "name": "sink",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [
        {
          "name": "sample",
          "type": "Sample"
        }
      ],
      "outputs": [],
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
)";

const char kFlowrtSelfDescriptionHash[] = "095fc4389eca95d2a4c3a6dc7249b150c765a8c34bd293e67b19533baf8876d0";

}  // namespace

std::string_view self_description_json() noexcept {
    return std::string_view{kFlowrtSelfDescription, sizeof(kFlowrtSelfDescription) - 1};
}

std::string_view self_description_hash() noexcept {
    return std::string_view{kFlowrtSelfDescriptionHash, sizeof(kFlowrtSelfDescriptionHash) - 1};
}

}  // namespace flowrt_app
