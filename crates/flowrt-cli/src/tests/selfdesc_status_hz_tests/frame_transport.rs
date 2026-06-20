use super::*;

#[test]
fn self_description_summary_displays_frame_transport_diagnostics() {
    let source = r#"
{
  "self_description_version": "0.1",
  "source_hash": "abc",
  "package": { "name": "frame_diag_demo" },
  "graphs": [{
    "name": "default",
    "instances": [{
      "name": "source",
      "component": "source_comp",
      "process": "main",
      "runtime": "rust"
    }, {
      "name": "sink",
      "component": "sink_comp",
      "process": "main",
      "runtime": "rust"
    }, {
      "name": "planner",
      "component": "planner_comp",
      "process": "main",
      "runtime": "rust"
    }, {
      "name": "executor",
      "component": "executor_comp",
      "process": "main",
      "runtime": "rust"
    }, {
      "name": "controller",
      "component": "controller_comp",
      "process": "main",
      "runtime": "rust"
    }, {
      "name": "navigator",
      "component": "navigator_comp",
      "process": "main",
      "runtime": "rust"
    }],
    "channels": [{
      "from": "source.packet",
      "to": "sink.packet",
      "message_type": "Packet",
      "backend": "zenoh",
      "backend_source": "auto_fallback",
      "diagnostic": "edge source.packet -> sink.packet 因字段 Packet.payload 无界落到 zenoh；加 max=N 可留在 iox2",
      "frame": {
        "message_type": "Packet",
        "encoding": "canonical_frame_v1",
        "variable": true,
        "bounded": false,
        "max_size_bytes": null,
        "iox2_slot_cap_bytes": null
      }
    }],
    "services": [{
      "name": "planner.plan_to_executor.plan",
      "client_instance": "planner",
      "client_port": "plan",
      "server_instance": "executor",
      "server_port": "plan",
      "request_type": "PlanRequest",
      "response_type": "PlanResponse",
      "backend": "iox2",
      "backend_source": "profile_default",
      "service": "FlowRT/service/planner_plan",
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
      }
    }],
    "operations": [{
      "name": "controller.plan",
      "client_instance": "controller",
      "client_port": "plan",
      "server_instance": "navigator",
      "server_port": "plan",
      "goal_type": "PlanGoal",
      "feedback_type": "PlanFeedback",
      "result_type": "PlanResult",
      "backend": "iox2",
      "backend_source": "explicit",
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
      "lowering": {
        "start_service": "__flowrt_operation_controller_plan_start",
        "cancel_service": "__flowrt_operation_controller_plan_cancel",
        "status_service": "__flowrt_operation_controller_plan_status"
      }
    }]
  }],
  "message_abi": []
}
"#;
    let self_description: flowrt_selfdesc::SelfDescription = serde_json::from_str(source).unwrap();
    let list = self_description_summary(&self_description);

    assert!(list.contains(
        "channel source.packet -> sink.packet type=Packet backend=zenoh backend_source=auto_fallback"
    ));
    assert!(list.contains("frame_type=Packet"));
    assert!(list.contains("frame_bounded=false"));
    assert!(list.contains("frame_max_size_bytes=none"));
    assert!(list.contains("diagnostic=edge source.packet -> sink.packet 因字段 Packet.payload 无界落到 zenoh；加 max=N 可留在 iox2"));
    assert!(list.contains("service planner.plan_to_executor.plan"));
    assert!(list.contains("backend=iox2 backend_source=profile_default"));
    assert!(list.contains("request_frame_type=PlanRequest"));
    assert!(list.contains("request_frame_max_size_bytes=44"));
    assert!(list.contains("response_frame_type=PlanResponse"));
    assert!(list.contains("response_frame_iox2_slot_cap_bytes=21"));
    assert!(list.contains("operation controller.plan"));
    assert!(list.contains("backend=iox2 backend_source=explicit"));
    assert!(list.contains("goal_frame_type=PlanGoal"));
    assert!(list.contains("goal_frame_iox2_slot_cap_bytes=none"));
    assert!(list.contains("start_request_frame_type=OperationStartRequest<PlanGoal>"));
    assert!(list.contains("start_request_frame_iox2_slot_cap_bytes=40"));
}
