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
  "source_hash": "0877d22c8f48000c79b36eb2a6d21c1befb040d5a83fb62b647d1dd378dd78bf",
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
    "name": "sync_fusion_cpp_demo",
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
            "name": "fusion_serial",
            "kind": "serial",
            "instance": "fusion"
          },
          {
            "name": "imu_src_serial",
            "kind": "serial",
            "instance": "imu_src"
          },
          {
            "name": "odom_src_serial",
            "kind": "serial",
            "instance": "odom_src"
          },
          {
            "name": "sink_serial",
            "kind": "serial",
            "instance": "sink"
          }
        ],
        "tasks": [
          {
            "name": "main",
            "instance": "fusion",
            "lane": "fusion_serial",
            "trigger": "on_synchronized",
            "readiness": "any_ready",
            "concurrency": "exclusive",
            "period_ms": null,
            "deadline_ms": null,
            "priority": null
          },
          {
            "name": "main",
            "instance": "imu_src",
            "lane": "imu_src_serial",
            "trigger": "periodic",
            "readiness": "any_ready",
            "concurrency": "exclusive",
            "period_ms": 10,
            "deadline_ms": null,
            "priority": null
          },
          {
            "name": "main",
            "instance": "odom_src",
            "lane": "odom_src_serial",
            "trigger": "periodic",
            "readiness": "any_ready",
            "concurrency": "exclusive",
            "period_ms": 10,
            "deadline_ms": null,
            "priority": null
          },
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
          "name": "fusion",
          "component": "fusion",
          "process": "main",
          "target": null,
          "runtime": "cpp",
          "params": []
        },
        {
          "name": "imu_src",
          "component": "imu_src",
          "process": "main",
          "target": null,
          "runtime": "cpp",
          "params": []
        },
        {
          "name": "odom_src",
          "component": "odom_src",
          "process": "main",
          "target": null,
          "runtime": "cpp",
          "params": []
        },
        {
          "name": "sink",
          "component": "sink",
          "process": "main",
          "target": null,
          "runtime": "cpp",
          "params": []
        }
      ],
      "tasks": [
        {
          "name": "main",
          "instance": "fusion",
          "trigger": "on_synchronized",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": null,
          "deadline_ms": null,
          "lane": "fusion_serial",
          "priority": null,
          "inputs": [],
          "outputs": [
            "estimate"
          ]
        },
        {
          "name": "main",
          "instance": "imu_src",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": 10,
          "deadline_ms": null,
          "lane": "imu_src_serial",
          "priority": null,
          "inputs": [],
          "outputs": [
            "imu"
          ]
        },
        {
          "name": "main",
          "instance": "odom_src",
          "trigger": "periodic",
          "readiness": "any_ready",
          "concurrency": "exclusive",
          "period_ms": 10,
          "deadline_ms": null,
          "lane": "odom_src_serial",
          "priority": null,
          "inputs": [],
          "outputs": [
            "odom"
          ]
        },
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
            "estimate"
          ],
          "outputs": []
        }
      ],
      "channels": [
        {
          "from": "fusion.estimate",
          "to": "sink.estimate",
          "message_type": "Estimate",
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
          "from": "imu_src.imu",
          "to": "fusion.imu",
          "message_type": "Imu",
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
          "from": "odom_src.odom",
          "to": "fusion.odom",
          "message_type": "Odom",
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
      "name": "fusion",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [
        {
          "name": "imu",
          "type": "Imu"
        },
        {
          "name": "odom",
          "type": "Odom"
        }
      ],
      "outputs": [
        {
          "name": "estimate",
          "type": "Estimate"
        }
      ],
      "service_clients": [],
      "service_servers": [],
      "operation_clients": [],
      "operation_servers": [],
      "params": []
    },
    {
      "name": "imu_src",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [],
      "outputs": [
        {
          "name": "imu",
          "type": "Imu"
        }
      ],
      "service_clients": [],
      "service_servers": [],
      "operation_clients": [],
      "operation_servers": [],
      "params": []
    },
    {
      "name": "odom_src",
      "language": "cpp",
      "kind": "native",
      "resources": [],
      "io_boundary": null,
      "inputs": [],
      "outputs": [
        {
          "name": "odom",
          "type": "Odom"
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
          "name": "estimate",
          "type": "Estimate"
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
      "type_name": "Estimate",
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
    },
    {
      "type_name": "Imu",
      "size_bytes": 16,
      "align_bytes": 8,
      "empty": false,
      "fields": [
        {
          "name": "ax",
          "type": "f64",
          "offset_bytes": 0,
          "size_bytes": 8,
          "align_bytes": 8
        },
        {
          "name": "stamp_ns",
          "type": "u64",
          "offset_bytes": 8,
          "size_bytes": 8,
          "align_bytes": 8
        }
      ]
    },
    {
      "type_name": "Odom",
      "size_bytes": 16,
      "align_bytes": 8,
      "empty": false,
      "fields": [
        {
          "name": "vx",
          "type": "f64",
          "offset_bytes": 0,
          "size_bytes": 8,
          "align_bytes": 8
        },
        {
          "name": "stamp_ns",
          "type": "u64",
          "offset_bytes": 8,
          "size_bytes": 8,
          "align_bytes": 8
        }
      ]
    }
  ],
  "message_frames": [
    {
      "type_name": "Estimate",
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
    },
    {
      "type_name": "Imu",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 16,
      "max_size_bytes": 16,
      "variable": false,
      "fields": [
        {
          "name": "ax",
          "type": "f64",
          "header_offset_bytes": 0,
          "header_size_bytes": 8,
          "tail_max_bytes": null
        },
        {
          "name": "stamp_ns",
          "type": "u64",
          "header_offset_bytes": 8,
          "header_size_bytes": 8,
          "tail_max_bytes": null
        }
      ]
    },
    {
      "type_name": "Odom",
      "encoding": "canonical_frame_v1",
      "header_size_bytes": 16,
      "max_size_bytes": 16,
      "variable": false,
      "fields": [
        {
          "name": "vx",
          "type": "f64",
          "header_offset_bytes": 0,
          "header_size_bytes": 8,
          "tail_max_bytes": null
        },
        {
          "name": "stamp_ns",
          "type": "u64",
          "header_offset_bytes": 8,
          "header_size_bytes": 8,
          "tail_max_bytes": null
        }
      ]
    }
  ]
}
)";

const char kFlowrtSelfDescriptionHash[] = "491b8abf29999932e6ea4ae3579293d68b395bf270bc20bac8fa1b2fbc584d28";

}  // namespace

std::string_view self_description_json() noexcept {
    return std::string_view{kFlowrtSelfDescription, sizeof(kFlowrtSelfDescription) - 1};
}

std::string_view self_description_hash() noexcept {
    return std::string_view{kFlowrtSelfDescriptionHash, sizeof(kFlowrtSelfDescriptionHash) - 1};
}

}  // namespace flowrt_app
