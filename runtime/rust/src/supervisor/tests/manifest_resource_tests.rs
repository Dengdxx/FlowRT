use super::prelude::*;

// -- manifest 反序列化测试 --

#[test]
fn manifest_deserialization_with_defaults() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [{
                "name": "client.plan",
                "client": "client.plan",
                "client_instance": "client",
                "client_port": "plan",
                "server": "server.plan",
                "server_instance": "server",
                "server_port": "plan",
                "request": "PlanRequest",
                "response": "PlanResponse",
                "backend": "inproc",
                "timeout_ms": 5000,
                "queue_depth": 32,
                "overflow": "busy",
                "lane": null,
                "max_in_flight": 64
            }],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "sensor",
                "backend": "zenoh",
                "target": null,
                "runtimes": ["rust"],
                "runtime_kind": "rust",
                "tasks": []
            }]
        }]
    }"#;
    let manifest = parse_launch_manifest(json).unwrap();
    assert!(manifest.profile_modes.is_empty());
    assert_eq!(manifest.graphs[0].mode, "strict");
    assert!(manifest.graphs[0].boundary_endpoints.is_empty());
    let process = &manifest.graphs[0].processes[0];
    assert_eq!(process.name, "sensor");
    assert!(process.target.is_none());
    assert_eq!(process.runtimes, vec!["rust"]);
    assert!(process.tasks.is_empty());
    assert!(process.depends_on.is_empty());
    assert_eq!(process.restart.policy, RestartPolicyKind::OnFailure);
    assert_eq!(process.restart.max_restarts, 3);
    assert_eq!(process.failure, "propagate");
    assert_eq!(process.readiness, ReadinessGate::ProcessStarted);
    assert!(process.env.is_empty());
    assert_eq!(process.startup_delay_ms, 0);
    assert_eq!(process.resource_placement, ResourcePlacement::default());
    let service = &manifest.graphs[0].services[0];
    assert_eq!(service.lane, None);
    assert_eq!(service.max_in_flight, 64);
}

#[test]
fn manifest_deserialization_rejects_unknown_fields() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "sensor",
                "backend": "zenoh",
                "runtime_kind": "rust",
                "depends_onn": ["driver"]
            }]
        }]
    }"#;

    let error = parse_launch_manifest(json).expect_err("unknown manifest fields must fail");

    assert!(error.contains("unknown field"), "unexpected error: {error}");
}

#[test]
fn manifest_deserialization_accepts_launch_artifact_metadata() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "artifact": {
            "mode": "island",
            "temporary_island": true,
            "test_only": true,
            "temporary_overlay": {
                "kind": "temporary_island",
                "original_profile_mode": "strict",
                "generated_by": {
                    "command": "flowrt run --temporary-island",
                    "source": "cli"
                },
                "boundary_mappings": [{
                    "direction": "input",
                    "name": "sensor_in",
                    "endpoint": "boundary.sensor_in",
                    "source": "cli"
                }]
            }
        },
        "profiles": ["dev"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": []
        }]
    }"#;

    let manifest = parse_launch_manifest(json).unwrap();

    assert_eq!(manifest.artifact.mode, "island");
    assert!(manifest.artifact.temporary_island);
    assert!(manifest.artifact.test_only);
    let overlay = manifest
        .artifact
        .temporary_overlay
        .as_ref()
        .expect("temporary overlay metadata should be accepted");
    assert_eq!(overlay["kind"], "temporary_island");
    assert_eq!(overlay["boundary_mappings"][0]["name"], "sensor_in");
    assert_eq!(manifest.artifact.clock.source, "realtime");
    assert_eq!(manifest.artifact.clock.unit, "ms");
    assert_eq!(manifest.artifact.clock.field, "tick_time_ms");
}

#[test]
fn manifest_deserialization_accepts_artifact_clock_metadata() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "artifact": {
            "mode": "strict",
            "temporary_island": false,
            "test_only": false,
            "clock": {
                "source": "simulated_replay",
                "unit": "ms",
                "field": "tick_time_ms"
            }
        },
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": []
        }]
    }"#;

    let manifest = parse_launch_manifest(json).unwrap();

    assert_eq!(manifest.artifact.clock.source, "simulated_replay");
    assert_eq!(manifest.artifact.clock.unit, "ms");
    assert_eq!(manifest.artifact.clock.field, "tick_time_ms");
}

#[test]
fn manifest_deserialization_accepts_island_boundary_metadata() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["dev"],
        "profile_modes": [{ "name": "dev", "mode": "island" }],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "mode": "island",
            "scheduler": {},
            "channels": [],
            "boundary_endpoints": [{
                "name": "sample_in",
                "canonical_id": "boundary_0123456789abcdef",
                "direction": "input",
                "endpoint": "consumer.sample",
                "instance": "consumer",
                "port": "sample",
                "message_type": "Sample"
            }],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": []
        }]
    }"#;

    let manifest = parse_launch_manifest(json).unwrap();

    assert_eq!(manifest.profile_modes[0].name, "dev");
    assert_eq!(manifest.profile_modes[0].mode, "island");
    assert_eq!(manifest.graphs[0].mode, "island");
    assert_eq!(manifest.graphs[0].boundary_endpoints[0].name, "sample_in");
    assert_eq!(
        manifest.graphs[0].boundary_endpoints[0].endpoint,
        "consumer.sample"
    );
}

#[test]
fn manifest_deserialization_accepts_control_plane_frame_metadata() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["linux"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "boundary_endpoints": [],
            "services": [{
                "name": "planner.plan",
                "client": "planner.plan",
                "client_instance": "planner",
                "client_port": "plan",
                "server": "plan_service.plan",
                "server_instance": "plan_service",
                "server_port": "plan",
                "request": "PlanRequest",
                "response": "PlanResponse",
                "backend": "iox2",
                "backend_source": "explicit",
                "request_frame": {
                    "message_type": "PlanRequest",
                    "encoding": "canonical_frame_v1",
                    "variable": true,
                    "bounded": true,
                    "max_size_bytes": 16,
                    "iox2_slot_cap_bytes": 16
                },
                "response_frame": {
                    "message_type": "PlanResponse",
                    "encoding": "canonical_frame_v1",
                    "variable": true,
                    "bounded": true,
                    "max_size_bytes": 21,
                    "iox2_slot_cap_bytes": 21
                },
                "service": "FlowRT/service/planner_plan",
                "timeout_ms": 1000,
                "queue_depth": 4,
                "overflow": "busy",
                "max_in_flight": 64
            }],
            "operations": [{
                "name": "controller.nav",
                "client": "controller.nav",
                "client_instance": "controller",
                "client_port": "nav",
                "server": "navigator.nav",
                "server_instance": "navigator",
                "server_port": "nav",
                "goal": "PlanGoal",
                "feedback": "PlanFeedback",
                "result": "PlanResult",
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
                "start_service": "FlowRT/service/__flowrt_operation_controller_nav_start",
                "cancel_service": "FlowRT/service/__flowrt_operation_controller_nav_cancel",
                "status_service": "FlowRT/service/__flowrt_operation_controller_nav_status",
                "timeout_ms": 5000,
                "concurrency": "reject",
                "preempt": "reject",
                "queue_depth": 4,
                "max_in_flight": 1,
                "feedback_policy": "latest",
                "result_retention_ms": 60000
            }],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": []
        }]
    }"#;

    let manifest = parse_launch_manifest(json).unwrap();

    assert_eq!(manifest.graphs[0].services[0].backend, "iox2");
    assert_eq!(manifest.graphs[0].operations[0].backend, "iox2");
}

#[test]
fn manifest_deserialization_rejects_unsupported_ir_version() {
    let json = r#"{
        "package": "demo",
        "ir_version": "9.9",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": []
        }]
    }"#;

    let error = parse_launch_manifest(json).expect_err("future manifest version must fail");

    assert!(
        error.contains("unsupported FlowRT launch manifest IR version `9.9`"),
        "unexpected error: {error}"
    );
}

#[test]
fn manifest_deserialization_with_process_env() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "control",
                "backend": "inproc",
                "runtime_kind": "rust",
                "env": {
                    "APP_MODE": "control",
                    "LOG_LEVEL": "debug"
                }
            }]
        }]
    }"#;
    let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
    let process = &manifest.graphs[0].processes[0];

    assert_eq!(process.env["APP_MODE"], "control");
    assert_eq!(process.env["LOG_LEVEL"], "debug");
}

#[test]
fn manifest_deserialization_with_external_process_metadata() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "camera_proc",
                "backend": "zenoh",
                "runtime_kind": "external",
                "external": {
                    "package": "camera_driver",
                    "executable": "bin/camera-node",
                    "args": ["--device", "/dev/video0"],
                    "working_dir": "workspace",
                    "health": "process_started",
                    "required_backends": ["zenoh"]
                }
            }]
        }]
    }"#;
    let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
    let process = &manifest.graphs[0].processes[0];
    let external = process.external.as_ref().unwrap();

    assert_eq!(external.package, "camera_driver");
    assert_eq!(external.executable, "bin/camera-node");
    assert_eq!(external.args, vec!["--device", "/dev/video0"]);
    assert_eq!(external.working_dir, LaunchExternalWorkingDir::Workspace);
    assert_eq!(external.health, LaunchExternalHealth::ProcessStarted);
    assert_eq!(effective_readiness(process), ReadinessGate::ProcessStarted);
}

#[test]
fn external_runtime_socket_health_upgrades_default_readiness() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "camera_proc",
                "backend": "zenoh",
                "runtime_kind": "external",
                "external": {
                    "package": "camera_driver",
                    "executable": "bin/camera-node"
                }
            }]
        }]
    }"#;
    let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
    let process = &manifest.graphs[0].processes[0];

    assert_eq!(process.readiness, ReadinessGate::ProcessStarted);
    assert_eq!(effective_readiness(process), ReadinessGate::RuntimeReady);
}

#[test]
fn manifest_deserialization_with_custom_policy() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "control",
                "backend": "iox2",
                "runtime_kind": "cpp",
                "depends_on": ["sensor"],
                "restart": {
                    "policy": "never",
                    "max_restarts": 0,
                    "initial_delay_ms": 0,
                    "max_delay_ms": 0
                },
                "failure": "isolate"
            }]
        }]
    }"#;
    let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
    let process = &manifest.graphs[0].processes[0];
    assert_eq!(process.depends_on, vec!["sensor"]);
    assert_eq!(process.restart.policy, RestartPolicyKind::Never);
    assert_eq!(process.failure, "isolate");
    assert_eq!(process.readiness, ReadinessGate::ProcessStarted);
    assert_eq!(process.resource_placement, ResourcePlacement::default());
}

#[test]
fn manifest_deserialization_with_readiness_gate() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "control",
                "backend": "inproc",
                "runtime_kind": "rust",
                "readiness": "runtime_ready",
                "startup_delay_ms": 500
            }]
        }]
    }"#;
    let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
    let process = &manifest.graphs[0].processes[0];
    assert_eq!(process.readiness, ReadinessGate::RuntimeReady);
    assert_eq!(process.startup_delay_ms, 500);
}

#[test]
fn manifest_deserialization_service_ready_gate() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "server",
                "backend": "inproc",
                "runtime_kind": "rust",
                "readiness": "service_ready"
            }]
        }]
    }"#;
    let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
    let process = &manifest.graphs[0].processes[0];
    assert_eq!(process.readiness, ReadinessGate::ServiceReady);
}

#[test]
fn manifest_deserialization_with_resource_placement() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "control",
                "backend": "iox2",
                "runtime_kind": "rust",
                "resource_placement": {
                    "cpu_affinity": [0, 1],
                    "nice": -10,
                    "rt_policy": "fifo",
                    "rt_priority": 50
                }
            }]
        }]
    }"#;
    let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
    let process = &manifest.graphs[0].processes[0];
    assert_eq!(process.resource_placement.cpu_affinity, vec![0, 1]);
    assert_eq!(process.resource_placement.nice, Some(-10));
    assert_eq!(
        process.resource_placement.rt_policy,
        Some(resource_placement::RtPolicy::Fifo)
    );
    assert_eq!(process.resource_placement.rt_priority, Some(50));
}

#[test]
fn manifest_deserialization_accepts_io_resource_descriptor_schema() {
    let json = r#"{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["default"],
        "graphs": [{
            "name": "main",
            "scheduler": {},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{
                "name": "camera",
                "backend": "iox2",
                "runtime_kind": "rust",
                "io_boundaries": [{
                    "instance": "camera",
                    "component": "camera",
                    "side_effects": ["device", "read"],
                    "readiness": "resource_ready",
                    "health": "runtime_reported",
                    "shutdown": "cooperative",
                    "resources": [{
                        "name": "frames",
                        "kind": "shm",
                        "required": true,
                        "descriptor": {
                            "kind": "frame",
                            "port": "frame",
                            "format": "rgb8",
                            "encoding": "row_major",
                            "metadata": {
                                "width": "640",
                                "height": "480"
                            },
                            "record_payload": false
                        }
                    }]
                }]
            }]
        }]
    }"#;
    let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
    let resource = &manifest.graphs[0].processes[0].io_boundaries[0].resources[0];
    let descriptor = resource.descriptor.as_ref().unwrap();

    assert_eq!(resource.name, "frames");
    assert!(resource.required);
    assert_eq!(descriptor.kind, "frame");
    assert_eq!(descriptor.port, "frame");
    assert_eq!(descriptor.format, "rgb8");
    assert_eq!(descriptor.encoding.as_deref(), Some("row_major"));
    assert_eq!(descriptor.metadata["width"], "640");
    assert!(!descriptor.record_payload);
}

#[test]
fn manifest_deserialization_parses_resource_contract_fields() {
    let json = resource_gate_manifest_json(&ResourceGateFixtureOptions {
        readiness: "before_start",
        required: true,
        health: "required",
        on_failure: "stop_process",
        status: "satisfied",
        satisfied: true,
        provider: Some("sensor_provider"),
        diagnostic: None,
    });
    let manifest = parse_launch_manifest(&json).unwrap();
    let contract = &manifest.graphs[0].resource_contract;

    assert_eq!(contract.providers[0].name, "sensor_provider");
    assert_eq!(contract.providers[0].scope, "process");
    assert_eq!(
        contract.providers[0].process.as_deref(),
        Some("sensor_proc")
    );
    assert_eq!(
        contract.providers[0].readiness_source.as_deref(),
        Some("provider_ready")
    );
    assert_eq!(
        contract.providers[0].health_source.as_deref(),
        Some("provider_health")
    );

    let requirement = &contract.requirements[0];
    assert_eq!(requirement.name, "samples");
    assert_eq!(requirement.capability, "perception.samples");
    assert_eq!(requirement.access, "read_write");
    assert!(requirement.required);
    assert_eq!(requirement.readiness, "before_start");
    assert_eq!(requirement.health, "required");
    assert_eq!(requirement.on_failure, "stop_process");
    assert_eq!(requirement.satisfaction, "satisfied");

    let satisfaction = &contract.satisfactions[0];
    assert_eq!(satisfaction.resource, "samples");
    assert_eq!(satisfaction.status, "satisfied");
    assert!(satisfaction.satisfied);
    assert_eq!(satisfaction.provider.as_deref(), Some("sensor_provider"));
}

#[test]
fn resource_gate_allows_required_satisfied_before_start() {
    let (graph, process) = resource_gate_fixture(ResourceGateFixtureOptions {
        readiness: "before_start",
        required: true,
        health: "required",
        on_failure: "stop_process",
        status: "satisfied",
        satisfied: true,
        provider: Some("sensor_provider"),
        diagnostic: None,
    });

    let decision = evaluate_process_resource_gates(&graph, &process, ResourceGatePhase::Startup);

    assert_eq!(decision.action, ResourceGateAction::Start);
    assert_eq!(decision.statuses[0].name, "sensor.samples");
    assert_eq!(decision.statuses[0].state, "ready");
    assert_eq!(
        decision.statuses[0].on_failure.as_deref(),
        Some("stop_process")
    );
    assert_eq!(
        decision.statuses[0].provider.as_deref(),
        Some("sensor_provider")
    );
}

#[test]
fn resource_gate_keeps_lazy_unsatisfied_pending_without_blocking() {
    let (graph, process) = resource_gate_fixture(ResourceGateFixtureOptions {
        readiness: "lazy",
        required: true,
        health: "required",
        on_failure: "stop_process",
        status: "unsatisfied",
        satisfied: false,
        provider: None,
        diagnostic: Some("provider not ready"),
    });

    let decision = evaluate_process_resource_gates(&graph, &process, ResourceGatePhase::Startup);

    assert_eq!(decision.action, ResourceGateAction::Start);
    assert_eq!(decision.statuses[0].state, "pending");
    assert_eq!(
        decision.statuses[0].diagnostic.as_deref(),
        Some("provider not ready")
    );
}

#[test]
fn resource_gate_degrades_optional_unsatisfied_without_blocking() {
    let (graph, process) = resource_gate_fixture(ResourceGateFixtureOptions {
        readiness: "before_init",
        required: false,
        health: "optional",
        on_failure: "degrade",
        status: "optional_unsatisfied",
        satisfied: false,
        provider: None,
        diagnostic: Some("optional provider not configured"),
    });

    let decision = evaluate_process_resource_gates(&graph, &process, ResourceGatePhase::Startup);

    assert_eq!(decision.action, ResourceGateAction::Degrade);
    assert_eq!(decision.statuses[0].state, "degraded");
    assert_eq!(
        decision.statuses[0].last_error.as_deref(),
        Some("optional provider not configured")
    );
}

#[test]
fn resource_gate_blocks_required_stop_process_with_diagnostic() {
    let (graph, process) = resource_gate_fixture(ResourceGateFixtureOptions {
        readiness: "before_init",
        required: true,
        health: "required",
        on_failure: "stop_process",
        status: "unsatisfied",
        satisfied: false,
        provider: None,
        diagnostic: Some("provider missing"),
    });

    let decision = evaluate_process_resource_gates(&graph, &process, ResourceGatePhase::Startup);

    assert_eq!(decision.action, ResourceGateAction::StopProcess);
    let error = decision.error_message("sensor_proc").unwrap();
    assert!(error.contains("sensor.samples"));
    assert!(error.contains("perception.samples"));
    assert!(error.contains("before_init"));
    assert!(error.contains("stop_process"));
}

#[test]
fn resource_gate_blocks_required_stop_graph_with_diagnostic() {
    let (graph, process) = resource_gate_fixture(ResourceGateFixtureOptions {
        readiness: "before_start",
        required: true,
        health: "required",
        on_failure: "stop_graph",
        status: "unsatisfied",
        satisfied: false,
        provider: None,
        diagnostic: Some("target provider missing"),
    });

    let decision = evaluate_process_resource_gates(&graph, &process, ResourceGatePhase::Startup);

    assert_eq!(decision.action, ResourceGateAction::StopGraph);
    let error = decision.error_message("sensor_proc").unwrap();
    assert!(error.contains("stop_graph"));
    assert!(error.contains("target provider missing"));
}

#[test]
fn restart_process_waits_for_resource_then_allows_restart() {
    let (graph, process) = resource_gate_fixture(ResourceGateFixtureOptions {
        readiness: "before_start",
        required: true,
        health: "required",
        on_failure: "restart_process",
        status: "unsatisfied",
        satisfied: false,
        provider: None,
        diagnostic: Some("provider not ready"),
    });

    let blocked = evaluate_process_resource_gates(&graph, &process, ResourceGatePhase::Restart);
    assert_eq!(blocked.action, ResourceGateAction::WaitRestart);
    assert_eq!(blocked.statuses[0].state, "pending");

    let (ready_graph, ready_process) = resource_gate_fixture(ResourceGateFixtureOptions {
        readiness: "before_start",
        required: true,
        health: "required",
        on_failure: "restart_process",
        status: "satisfied",
        satisfied: true,
        provider: Some("sensor_provider"),
        diagnostic: None,
    });
    let ready =
        evaluate_process_resource_gates(&ready_graph, &ready_process, ResourceGatePhase::Restart);
    assert_eq!(ready.action, ResourceGateAction::Start);
}

#[test]
fn restart_child_resource_gate_uses_live_ready_status() {
    let (graph, process) = resource_gate_fixture(ResourceGateFixtureOptions {
        readiness: "before_start",
        required: true,
        health: "required",
        on_failure: "restart_process",
        status: "unsatisfied",
        satisfied: false,
        provider: None,
        diagnostic: Some("provider not ready"),
    });
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    let real_child = cmd.spawn().unwrap();
    let mut child = supervised_child_for_test("sensor_proc", real_child);
    child.resource_gate = process_resource_gate(&graph, &process);
    let supervisor_state = IntrospectionState::new();

    let blocked =
        evaluate_child_resource_gates(&supervisor_state, &child, ResourceGatePhase::Restart);
    assert_eq!(blocked.action, ResourceGateAction::WaitRestart);

    supervisor_state.record_resource_status(IntrospectionResourceStatus {
        name: "sensor.samples".to_string(),
        capability: "perception.samples".to_string(),
        state: "ready".to_string(),
        required: true,
        satisfied: Some(true),
        source: Some("runtime".to_string()),
        owner_process: Some("sensor_proc".to_string()),
        updated_unix_ms: Some(55),
        ..Default::default()
    });

    let ready =
        evaluate_child_resource_gates(&supervisor_state, &child, ResourceGatePhase::Restart);
    assert_eq!(ready.action, ResourceGateAction::Start);
    assert_eq!(ready.statuses[0].state, "ready");
}

#[test]
fn runtime_reported_resource_status_is_enriched_with_owner() {
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    let real_child = cmd.spawn().unwrap();
    let mut child = supervised_child_for_test("sensor_proc", real_child);
    let supervisor_state = IntrospectionState::new();
    let status = crate::IntrospectionStatus {
        resources: vec![crate::IntrospectionResourceStatus {
            name: "sensor.samples".to_string(),
            capability: "perception.samples".to_string(),
            state: "degraded".to_string(),
            required: true,
            on_failure: Some("degrade".to_string()),
            last_error: Some("runtime health failed".to_string()),
            ..Default::default()
        }],
        ..Default::default()
    };

    record_child_reported_resource_statuses(&supervisor_state, &mut child, &status);

    let resource = &supervisor_state.status().resources[0];
    assert_eq!(resource.owner_process.as_deref(), Some("sensor_proc"));
    assert_eq!(resource.source.as_deref(), Some("runtime"));
    assert_eq!(resource.state, "degraded");
    assert!(child.resource_degraded);
}

#[test]
fn io_boundary_resource_error_updates_abstract_resource_status() {
    let (graph, process) = resource_gate_fixture(ResourceGateFixtureOptions {
        readiness: "before_start",
        required: true,
        health: "required",
        on_failure: "stop_process",
        status: "satisfied",
        satisfied: true,
        provider: Some("sensor_provider"),
        diagnostic: None,
    });
    let mut cmd = Command::new("sleep");
    cmd.arg("30");
    let real_child = cmd.spawn().unwrap();
    let mut child = supervised_child_for_test("sensor_proc", real_child);
    child.resource_gate = process_resource_gate(&graph, &process);
    let supervisor_state = IntrospectionState::new();
    let status = crate::IntrospectionStatus {
        io_boundaries: vec![crate::IntrospectionIoBoundaryStatus {
            name: "sensor".to_string(),
            component: "sensor".to_string(),
            ready: false,
            healthy: false,
            last_error: Some("boundary unhealthy".to_string()),
            resources: vec![crate::IntrospectionIoBoundaryResourceStatus {
                name: "samples".to_string(),
                kind: "perception.samples".to_string(),
                ready: false,
                message: Some("provider reported failure".to_string()),
                last_error: Some("resource failed".to_string()),
                updated_unix_ms: Some(55),
            }],
            updated_unix_ms: Some(55),
        }],
        ..Default::default()
    };

    record_child_reported_resource_statuses(&supervisor_state, &mut child, &status);

    let resource = &supervisor_state.status().resources[0];
    assert_eq!(resource.name, "sensor.samples");
    assert_eq!(resource.state, "failed");
    assert_eq!(resource.on_failure.as_deref(), Some("stop_process"));
    assert_eq!(resource.last_error.as_deref(), Some("resource failed"));
    assert!(child.resource_degraded);
}
