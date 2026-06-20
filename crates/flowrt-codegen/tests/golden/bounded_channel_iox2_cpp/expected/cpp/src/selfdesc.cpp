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
  "source_hash": "8c9c77ec30737cf3a05e04e01271ee9d514831907af3cd3ba43f4d8f08a768c9",
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
    "name": "bounded_channel_iox2_cpp",
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
            "name": "sink_serial",
            "kind": "serial",
            "instance": "sink"
          },
          {
            "name": "source_serial",
            "kind": "serial",
            "instance": "source"
          }
        ],
        "tasks": [
          {
            "name": "main",
            "instance": "sink",
            "lane": "sink_serial",
            "trigger": "on_message",
            "readiness": "any_ready",
            "concurrency": "exclusive",
            "period_ms": null,
            "deadline_ms": null,
            "priority": null
          },
          {
            "name": "main",
            "instance": "source",
            "lane": "source_serial",
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
          "name": "sink",
          "component": "sink",
          "process": "sink_proc",
          "target": "linux",
          "runtime": "cpp",
          "params": []
        },
        {
          "name": "source",
          "component": "source",
          "process": "source_proc",
          "target": "linux",
          "runtime": "cpp",
          "params": []
        }
      ],
      "tasks": [
        {
          "name": "main",
          "instance": "sink",
          "trigger": "on_message",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": null,
          "deadline_ms": null,
          "lane": "sink_serial",
          "priority": null,
          "inputs": [
            "packet"
          ],
          "outputs": []
        },
        {
          "name": "main",
          "instance": "source",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": 5,
          "deadline_ms": null,
          "lane": "source_serial",
          "priority": null,
          "inputs": [],
          "outputs": [
            "packet"
          ]
        }
      ],
      "channels": [
        {
          "from": "source.packet",
          "to": "sink.packet",
          "message_type": "Packet",
          "backend": "iox2",
          "backend_source": "profile_default",
          "frame": {
            "message_type": "Packet",
            "encoding": "canonical_frame_v1",
            "variable": true,
            "bounded": true,
            "max_size_bytes": 60,
            "iox2_slot_cap_bytes": 60
          },
          "thread_affinity": "scheduler_local_commit",
          "service": "FlowRT/bounded_channel_iox2_cpp/default/bind_0/source_packet_to_sink_packet",
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
      "name": "sink",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [
        {
          "name": "packet",
          "type": "Packet"
        }
      ],
      "outputs": [],
      "service_clients": [],
      "service_servers": [],
      "operation_clients": [],
      "operation_servers": [],
      "params": []
    },
    {
      "name": "source",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [],
      "outputs": [
        {
          "name": "packet",
          "type": "Packet"
        }
      ],
      "service_clients": [],
      "service_servers": [],
      "operation_clients": [],
      "operation_servers": [],
      "params": []
    }
  ],
  "message_abi": [],
  "message_frames": [
    {
      "type_name": "Packet",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 24,
      "max_size_bytes": 60,
      "variable": true,
      "fields": [
        {
          "name": "payload",
          "type": "bytes<max=8>",
          "header_offset_bytes": 0,
          "header_size_bytes": 8,
          "tail_max_bytes": 8
        },
        {
          "name": "label",
          "type": "string<max=12>",
          "header_offset_bytes": 8,
          "header_size_bytes": 8,
          "tail_max_bytes": 12
        },
        {
          "name": "samples",
          "type": "sequence<u32,max=4>",
          "header_offset_bytes": 16,
          "header_size_bytes": 8,
          "tail_max_bytes": 16
        }
      ]
    }
  ]
}
)";

const char kFlowrtSelfDescriptionHash[] = "35b3fb42e95457a911292ef429f46f87a130be12ed860193d063a68abd90c73a";

}  // namespace

std::string_view self_description_json() noexcept {
    return std::string_view{kFlowrtSelfDescription, sizeof(kFlowrtSelfDescription) - 1};
}

std::string_view self_description_hash() noexcept {
    return std::string_view{kFlowrtSelfDescriptionHash, sizeof(kFlowrtSelfDescriptionHash) - 1};
}

}  // namespace flowrt_app
