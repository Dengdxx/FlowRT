use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    BackendThreadAffinity, BoundaryDirection, BoundaryEndpointIr, ComponentIr, ComponentKind,
    ContractIr, DeterminismMode, DeterminismTimeoutPolicy, ExternalHealthKind, ExternalProcessIr,
    ExternalWorkingDir, GraphIr, GraphMode, InstanceIr, IoBoundaryHealth, IoBoundaryReadiness,
    IoBoundaryShutdown, IoSideEffect, OperationConcurrencyPolicy, OperationFeedbackPolicy,
    OperationPreemptPolicy, ProcessIr, ResourceProviderIr, ResourceRequirementIr,
    ResourceSatisfactionIr, ServicePortIr, TaskIr, derived::GraphDerivedFacts,
};

use crate::resource_names::{
    OBSERVABILITY_TRACE_CAPABILITY, descriptor_payload_capture_name, resource_access_name,
    resource_descriptor_kind_name, resource_failure_name, resource_health_name,
    resource_provider_scope_name, resource_readiness_name, resource_satisfaction_status,
};
use crate::runtime_plan::{bridge_runtime_plans, contract_derived_facts, graph_derived_facts};
use crate::selfdesc::{
    channel_backend_source_name, channel_message_expr, frame_transport_for_expr,
    frame_transport_for_expr_with_backend_slot, operation_backend_source_name,
    operation_start_request_frame_transport, service_backend_source_name,
    unbounded_frame_fallback_diagnostic,
};
use crate::{
    CodegenError, Result, component_by_name, iox2_service_name_for_edge, language_name,
    ros2_bridge_key_expr, zenoh_key_expr_for_edge,
};

pub(super) fn emit_launch_manifest(contract: &ContractIr) -> Result<String> {
    let facts = contract_derived_facts(contract)?;
    let graphs = contract
        .graphs
        .iter()
        .map(|graph| {
            let graph_facts = graph_derived_facts(&facts, graph);
            let mut graph_json = serde_json::json!({
                "name": graph.name,
                "mode": graph_mode_name(contract_artifact_mode(contract)),
                "scheduler": launch_scheduler(contract, graph),
                "resource_contract": launch_resource_contract(graph, graph_facts),
                "processes": launch_processes(contract, graph, graph_facts),
                "channels": launch_channels(contract, graph, graph_facts),
                "boundary_endpoints": launch_boundary_endpoints(graph),
                "services": launch_services(contract, graph)?,
                "operations": launch_operations(contract, graph),
                "ros2_bridges": launch_ros2_bridges(contract, graph),
                "instances": graph.instances.iter().map(|instance| {
                    let component = component_by_name(contract, &instance.component.name);
                    serde_json::json!({
                        "name": instance.name,
                        "component": instance.component.name,
                        "runtime": language_name(component.language),
                        "process": instance.process,
                        "target": instance.target.as_ref().map(|target| &target.name),
                    })
                }).collect::<Vec<_>>(),
                "tasks": graph.tasks.iter().map(launch_task).collect::<Vec<_>>(),
            });
            if let Some(tracing) = launch_tracing(graph_facts) {
                graph_json
                    .as_object_mut()
                    .expect("launch graph must be a JSON object")
                    .insert("tracing".to_string(), tracing);
            }
            Ok(graph_json)
        })
        .collect::<Result<Vec<_>>>()?;
    let launch = serde_json::json!({
        "package": contract.package.name,
        "ir_version": contract.ir_version,
        "artifact": {
            "mode": graph_mode_name(contract_artifact_mode(contract)),
            "temporary_island": contract.artifact.temporary_island,
            "test_only": contract.artifact.test_only,
            "temporary_overlay": launch_temporary_overlay(contract),
            "clock": {
                "source": clock_source_name(contract),
                "unit": "ms",
                "field": "tick_time_ms",
            },
        },
        "profiles": contract.profiles.iter().map(|profile| &profile.name).collect::<Vec<_>>(),
        "determinism": launch_determinism(contract),
        "profile_modes": contract.profiles.iter().map(|profile| serde_json::json!({
            "name": profile.name,
            "mode": graph_mode_name(profile.mode),
        })).collect::<Vec<_>>(),
        "targets": contract.targets.iter().map(|target| &target.name).collect::<Vec<_>>(),
        "graphs": graphs,
    });
    let mut output = serde_json::to_string_pretty(&launch)?;
    output.push('\n');
    Ok(output)
}

fn launch_determinism(contract: &ContractIr) -> serde_json::Value {
    let determinism = contract
        .profiles
        .first()
        .map(|profile| &profile.determinism);
    let Some(determinism) = determinism else {
        return serde_json::json!({
            "mode": "process_local",
            "tick_timeout_ms": 0,
            "on_timeout": "fault_graph",
            "processes": [],
        });
    };
    serde_json::json!({
        "mode": determinism_mode_name(determinism.mode),
        "tick_timeout_ms": determinism.timeout_ms.unwrap_or(0),
        "on_timeout": determinism_timeout_policy_name(determinism.on_timeout),
        "processes": determinism.processes,
    })
}

fn determinism_mode_name(mode: DeterminismMode) -> &'static str {
    match mode {
        DeterminismMode::ProcessLocal => "process_local",
        DeterminismMode::GlobalTick => "global_tick",
    }
}

fn determinism_timeout_policy_name(policy: DeterminismTimeoutPolicy) -> &'static str {
    match policy {
        DeterminismTimeoutPolicy::FaultGraph => "fault_graph",
        DeterminismTimeoutPolicy::StopGraph => "stop_graph",
    }
}

fn launch_resource_contract(graph: &GraphIr, graph_facts: &GraphDerivedFacts) -> serde_json::Value {
    serde_json::json!({
        "resource_contract_version": flowrt_selfdesc::RESOURCE_CONTRACT_SCHEMA_VERSION,
        "requirements": graph_facts
            .resources
            .satisfactions
            .iter()
            .map(launch_resource_requirement_binding)
            .collect::<Vec<_>>(),
        "providers": graph
            .resource_providers
            .iter()
            .map(launch_resource_provider)
            .collect::<Vec<_>>(),
        "satisfactions": graph_facts
            .resources
            .satisfactions
            .iter()
            .map(launch_resource_satisfaction)
            .collect::<Vec<_>>(),
    })
}

fn launch_resource_requirement_binding(satisfaction: &ResourceSatisfactionIr) -> serde_json::Value {
    serde_json::json!({
        "instance": satisfaction.instance.name,
        "component": satisfaction.component.name,
        "name": satisfaction.resource,
        "capability": satisfaction.capability.0.as_str(),
        "access": resource_access_name(satisfaction.access),
        "required": satisfaction.required,
        "readiness": resource_readiness_name(satisfaction.readiness),
        "health": resource_health_name(satisfaction.health),
        "on_failure": resource_failure_name(satisfaction.on_failure),
        "satisfaction": resource_satisfaction_status(satisfaction),
        "provider": satisfaction.provider.as_ref().map(|provider| provider.name.as_str()),
        "diagnostic": satisfaction.diagnostic.as_deref(),
    })
}

fn launch_resource_provider(provider: &ResourceProviderIr) -> serde_json::Value {
    serde_json::json!({
        "name": provider.name,
        "scope": resource_provider_scope_name(provider.scope),
        "capabilities": provider
            .capabilities
            .iter()
            .map(|capability| capability.0.as_str())
            .collect::<Vec<_>>(),
        "target": provider.target.as_ref().map(|target| target.name.as_str()),
        "process": provider.process.as_deref(),
        "external_package": provider.external_package.as_deref(),
        "readiness_source": provider.readiness_source,
        "health_source": provider.health_source,
    })
}

fn launch_resource_satisfaction(satisfaction: &ResourceSatisfactionIr) -> serde_json::Value {
    serde_json::json!({
        "instance": satisfaction.instance.name,
        "component": satisfaction.component.name,
        "resource": satisfaction.resource,
        "capability": satisfaction.capability.0.as_str(),
        "access": resource_access_name(satisfaction.access),
        "required": satisfaction.required,
        "readiness": resource_readiness_name(satisfaction.readiness),
        "health": resource_health_name(satisfaction.health),
        "on_failure": resource_failure_name(satisfaction.on_failure),
        "status": resource_satisfaction_status(satisfaction),
        "satisfied": satisfaction.satisfied,
        "provider": satisfaction.provider.as_ref().map(|provider| provider.name.as_str()),
        "diagnostic": satisfaction.diagnostic.as_deref(),
    })
}

fn launch_tracing(graph_facts: &GraphDerivedFacts) -> Option<serde_json::Value> {
    let satisfaction = tracing_satisfaction(graph_facts)?;
    Some(serde_json::json!({
        "enabled": true,
        "capability": OBSERVABILITY_TRACE_CAPABILITY,
        "provider": satisfaction.provider.as_ref().map(|provider| provider.name.as_str()),
        "endpoint": null,
    }))
}

fn tracing_satisfaction(graph_facts: &GraphDerivedFacts) -> Option<&ResourceSatisfactionIr> {
    graph_facts
        .resources
        .satisfactions
        .iter()
        .find(|satisfaction| {
            satisfaction.satisfied
                && satisfaction.capability.0.as_str() == OBSERVABILITY_TRACE_CAPABILITY
        })
}

fn launch_services(contract: &ContractIr, graph: &GraphIr) -> Result<Vec<serde_json::Value>> {
    let components = contract
        .components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    let instances = graph
        .instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();

    graph
        .services
        .iter()
        .map(|service| {
            let client = service_port_for_instance(
                &components,
                &instances,
                service.client.instance.name.as_str(),
                service.client.port.as_str(),
                ServicePortRole::Client,
            );
            let server = service_port_for_instance(
                &components,
                &instances,
                service.server.instance.name.as_str(),
                service.server.port.as_str(),
                ServicePortRole::Server,
            );
            if client.request != server.request {
                return Err(CodegenError::InvalidLaunchManifest {
                    message: format!(
                        "service `{}` request type mismatch: client `{}` uses `{}`, server `{}` uses `{}`",
                        service.id.0,
                        service.client.port,
                        client.request.canonical_syntax(),
                        service.server.port,
                        server.request.canonical_syntax()
                    ),
                });
            }
            if client.response != server.response {
                return Err(CodegenError::InvalidLaunchManifest {
                    message: format!(
                        "service `{}` response type mismatch: client `{}` uses `{}`, server `{}` uses `{}`",
                        service.id.0,
                        service.client.port,
                        client.response.canonical_syntax(),
                        service.server.port,
                        server.response.canonical_syntax()
                    ),
                });
            }
            let name = format!("{}.{}", service.client.instance.name, service.client.port);
            let transport_endpoint =
                crate::runtime_plan::transport_endpoint(&service.backend.0, &name);
            let request_type = client.request.canonical_syntax();
            let response_type = client.response.canonical_syntax();
            let request_frame = frame_transport_for_expr_with_backend_slot(
                contract,
                &client.request,
                &request_type,
                &service.backend.0,
            );
            let response_frame = frame_transport_for_expr_with_backend_slot(
                contract,
                &client.response,
                &response_type,
                &service.backend.0,
            );
            let has_frame = request_frame.is_some() || response_frame.is_some();
            let diagnostic = unbounded_frame_fallback_diagnostic(
                contract,
                &client.request,
                &request_type,
                &name,
                &service.backend.0,
                service.backend_source == flowrt_ir::ServiceBackendSource::AutoFallback,
            )
            .or_else(|| {
                unbounded_frame_fallback_diagnostic(
                    contract,
                    &client.response,
                    &response_type,
                    &name,
                    &service.backend.0,
                    service.backend_source == flowrt_ir::ServiceBackendSource::AutoFallback,
                )
            });
            let mut endpoint = serde_json::json!({
                "name": name,
                "client": format!("{}.{}", service.client.instance.name, service.client.port),
                "client_instance": service.client.instance.name,
                "client_port": service.client.port,
                "server": format!("{}.{}", service.server.instance.name, service.server.port),
                "server_instance": service.server.instance.name,
                "server_port": service.server.port,
                "request": request_type,
                "response": response_type,
                "backend": service.backend.0,
                "timeout_ms": service.policy.timeout_ms,
                "queue_depth": service.policy.queue_depth,
                "overflow": service.policy.overflow,
                "lane": service.policy.lane,
                "max_in_flight": service.policy.max_in_flight,
            });
            if let Some(key_expr) = transport_endpoint.key_expr() {
                endpoint.as_object_mut().expect("service endpoint must be a JSON object").insert(
                    "key_expr".to_string(),
                    serde_json::Value::String(key_expr.to_string()),
                );
            }
            if let Some(service_name) = transport_endpoint.service_name() {
                endpoint.as_object_mut().expect("service endpoint must be a JSON object").insert(
                    "service".to_string(),
                    serde_json::Value::String(service_name.to_string()),
                );
            }
            if has_frame {
                endpoint.as_object_mut().expect("service endpoint must be a JSON object").insert(
                    "backend_source".to_string(),
                    serde_json::Value::String(service_backend_source_name(service.backend_source).to_string()),
                );
            }
            if let Some(diagnostic) = diagnostic {
                endpoint.as_object_mut().expect("service endpoint must be a JSON object").insert(
                    "diagnostic".to_string(),
                    serde_json::Value::String(diagnostic),
                );
            }
            if let Some(frame) = request_frame {
                endpoint.as_object_mut().expect("service endpoint must be a JSON object").insert(
                    "request_frame".to_string(),
                    serde_json::to_value(frame).expect("frame transport must serialize"),
                );
            }
            if let Some(frame) = response_frame {
                endpoint.as_object_mut().expect("service endpoint must be a JSON object").insert(
                    "response_frame".to_string(),
                    serde_json::to_value(frame).expect("frame transport must serialize"),
                );
            }
            Ok(endpoint)
        })
        .collect()
}

fn launch_operations(contract: &ContractIr, graph: &GraphIr) -> Vec<serde_json::Value> {
    crate::runtime_plan::operation_runtime_plans(contract, graph)
        .iter()
        .map(|plan| {
            let operation_edge = &graph.operations[plan.index];
            let goal_type = plan.goal_type.canonical_syntax();
            let goal_frame = frame_transport_for_expr(contract, &plan.goal_type, &goal_type, None);
            let start_request_frame =
                operation_start_request_frame_transport(contract, &plan.goal_type, &plan.backend.0);
            let has_frame = goal_frame.is_some() || start_request_frame.is_some();
            let diagnostic = unbounded_frame_fallback_diagnostic(
                contract,
                &plan.goal_type,
                &goal_type,
                &plan.operation_name,
                &plan.backend.0,
                operation_edge.backend_source == flowrt_ir::OperationBackendSource::AutoFallback,
            );
            let mut operation = serde_json::json!({
                "name": plan.operation_name,
                "client": format!("{}.{}", plan.client_instance, plan.client_port),
                "client_instance": plan.client_instance,
                "client_port": plan.client_port,
                "server": format!("{}.{}", plan.server_instance, plan.server_port),
                "server_instance": plan.server_instance,
                "server_port": plan.server_port,
                "goal": goal_type,
                "feedback": plan.feedback_type.canonical_syntax(),
                "result": plan.result_type.canonical_syntax(),
                "backend": plan.backend.0,
                "timeout_ms": plan.timeout_ms,
                "concurrency": operation_concurrency_name(plan.concurrency),
                "preempt": operation_preempt_name(plan.preempt),
                "queue_depth": plan.queue_depth,
                "max_in_flight": plan.max_in_flight,
                "feedback_policy": operation_feedback_name(plan.feedback),
                "result_retention_ms": plan.result_retention_ms,
            });
            insert_operation_endpoint(&mut operation, "start", &plan.start_endpoint);
            insert_operation_endpoint(&mut operation, "cancel", &plan.cancel_endpoint);
            insert_operation_endpoint(&mut operation, "status", &plan.status_endpoint);
            if has_frame {
                operation
                    .as_object_mut()
                    .expect("operation endpoint must be a JSON object")
                    .insert(
                        "backend_source".to_string(),
                        serde_json::Value::String(
                            operation_backend_source_name(operation_edge.backend_source)
                                .to_string(),
                        ),
                    );
            }
            if let Some(diagnostic) = diagnostic {
                operation
                    .as_object_mut()
                    .expect("operation endpoint must be a JSON object")
                    .insert(
                        "diagnostic".to_string(),
                        serde_json::Value::String(diagnostic),
                    );
            }
            if let Some(frame) = goal_frame {
                operation
                    .as_object_mut()
                    .expect("operation endpoint must be a JSON object")
                    .insert(
                        "goal_frame".to_string(),
                        serde_json::to_value(frame).expect("frame transport must serialize"),
                    );
            }
            if let Some(frame) = start_request_frame {
                operation
                    .as_object_mut()
                    .expect("operation endpoint must be a JSON object")
                    .insert(
                        "start_request_frame".to_string(),
                        serde_json::to_value(frame).expect("frame transport must serialize"),
                    );
            }
            operation
        })
        .collect()
}

fn insert_operation_endpoint(
    operation: &mut serde_json::Value,
    prefix: &str,
    endpoint: &crate::runtime_plan::TransportEndpoint,
) {
    let object = operation
        .as_object_mut()
        .expect("operation endpoint must be a JSON object");
    if let Some(service_name) = endpoint.service_name() {
        object.insert(
            format!("{prefix}_service"),
            serde_json::Value::String(service_name.to_string()),
        );
    }
    if let Some(key_expr) = endpoint.key_expr() {
        object.insert(
            format!("{prefix}_key_expr"),
            serde_json::Value::String(key_expr.to_string()),
        );
    }
}

fn operation_concurrency_name(policy: OperationConcurrencyPolicy) -> &'static str {
    match policy {
        OperationConcurrencyPolicy::Reject => "reject",
        OperationConcurrencyPolicy::Queue => "queue",
    }
}

fn operation_preempt_name(policy: OperationPreemptPolicy) -> &'static str {
    match policy {
        OperationPreemptPolicy::Reject => "reject",
        OperationPreemptPolicy::CancelRunning => "cancel_running",
    }
}

fn operation_feedback_name(policy: OperationFeedbackPolicy) -> &'static str {
    match policy {
        OperationFeedbackPolicy::Latest => "latest",
        OperationFeedbackPolicy::Fifo => "fifo",
    }
}

fn launch_boundary_endpoints(graph: &GraphIr) -> Vec<serde_json::Value> {
    graph
        .boundary_endpoints
        .iter()
        .map(launch_boundary_endpoint)
        .collect()
}

fn launch_boundary_endpoint(endpoint: &BoundaryEndpointIr) -> serde_json::Value {
    serde_json::json!({
        "name": endpoint.name,
        "canonical_id": endpoint.id.0,
        "direction": boundary_direction_name(endpoint.direction),
        "endpoint": format!("{}.{}", endpoint.port.instance.name, endpoint.port.port),
        "instance": endpoint.port.instance.name,
        "port": endpoint.port.port,
        "message_type": endpoint.ty.canonical_syntax(),
    })
}

fn graph_mode_name(mode: GraphMode) -> &'static str {
    match mode {
        GraphMode::Strict => "strict",
        GraphMode::Island => "island",
    }
}

fn clock_source_name(contract: &ContractIr) -> &'static str {
    contract.artifact.clock_source.label()
}

fn launch_temporary_overlay(contract: &ContractIr) -> serde_json::Value {
    let Some(overlay) = contract.artifact.temporary_overlay.as_ref() else {
        return serde_json::Value::Null;
    };
    serde_json::json!({
        "kind": overlay.kind,
        "original_profile_mode": graph_mode_name(overlay.original_profile_mode),
        "generated_by": {
            "command": overlay.generated_by.command,
            "source": overlay.generated_by.source,
        },
        "boundary_mappings": overlay.boundary_mappings.iter().map(|mapping| serde_json::json!({
            "direction": boundary_direction_name(mapping.direction),
            "name": mapping.name,
            "endpoint": mapping.endpoint,
            "source": mapping.source,
        })).collect::<Vec<_>>(),
    })
}

fn contract_artifact_mode(contract: &ContractIr) -> GraphMode {
    if contract.artifact.mode == GraphMode::Island
        || contract
            .profiles
            .iter()
            .any(|profile| profile.mode == GraphMode::Island)
    {
        GraphMode::Island
    } else {
        GraphMode::Strict
    }
}

fn boundary_direction_name(direction: BoundaryDirection) -> &'static str {
    match direction {
        BoundaryDirection::Input => "input",
        BoundaryDirection::Output => "output",
    }
}

fn service_port_for_instance<'a>(
    components: &BTreeMap<&str, &'a ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    instance_name: &str,
    port_name: &str,
    role: ServicePortRole,
) -> &'a ServicePortIr {
    let instance = instances
        .get(instance_name)
        .expect("validated service bind must reference an existing instance");
    let component = components
        .get(instance.component.name.as_str())
        .expect("validated service bind must reference an existing component");
    let ports = match role {
        ServicePortRole::Client => &component.service_clients,
        ServicePortRole::Server => &component.service_servers,
    };
    ports
        .iter()
        .find(|port| port.name == port_name)
        .expect("validated service bind must reference an existing service port")
}

#[derive(Debug, Clone, Copy)]
enum ServicePortRole {
    Client,
    Server,
}

fn launch_scheduler(contract: &ContractIr, graph: &GraphIr) -> serde_json::Value {
    serde_json::json!({
        "worker_threads": contract
            .profiles
            .first()
            .map(|profile| profile.scheduler.worker_threads)
            .unwrap_or(1),
        "lanes": scheduler_lanes(graph),
        "tasks": graph.tasks.iter().map(scheduler_task).collect::<Vec<_>>(),
    })
}

fn scheduler_lanes(graph: &GraphIr) -> Vec<serde_json::Value> {
    let mut lanes = BTreeMap::<String, String>::new();
    for task in &graph.tasks {
        lanes.insert(task_lane_name(task), task.instance.name.clone());
    }

    lanes
        .into_iter()
        .map(|(name, instance)| {
            serde_json::json!({
                "name": name,
                "kind": "serial",
                "instance": instance,
            })
        })
        .collect()
}

fn scheduler_task(task: &TaskIr) -> serde_json::Value {
    serde_json::json!({
        "name": task.name,
        "instance": task.instance.name,
        "lane": task_lane_name(task),
        "trigger": task.trigger,
        "readiness": task.readiness,
        "period_ms": task.period_ms,
        "deadline_ms": task.deadline_ms,
        "priority": task.priority,
    })
}

fn task_lane_name(task: &TaskIr) -> String {
    crate::runtime_plan::resolved_task_lane_name(task)
}

fn launch_ros2_bridges(contract: &ContractIr, graph: &GraphIr) -> Vec<serde_json::Value> {
    bridge_runtime_plans(contract, graph)
        .iter()
        .map(|bridge| {
            let flowrt = bridge_flowrt_endpoint(bridge);
            serde_json::json!({
                "name": bridge.name,
                "flowrt": flowrt,
                "boundary_endpoint": bridge.boundary_endpoint.as_deref(),
                "ros2_topic": bridge.ros2_topic,
                "ros2_type": bridge.ros2_type,
                "direction": ros2_bridge_direction_name(bridge.direction),
                "field": bridge.field,
                "backend": "zenoh",
                "key_expr": ros2_bridge_key_expr(contract, graph, bridge),
            })
        })
        .collect()
}

fn bridge_flowrt_endpoint(bridge: &crate::runtime_plan::BridgeRuntimePlan) -> String {
    bridge.boundary_endpoint.as_ref().map_or_else(
        || format!("{}.{}", bridge.source_instance, bridge.source_port),
        |endpoint| format!("boundary:{endpoint}"),
    )
}

fn ros2_bridge_direction_name(direction: flowrt_ir::Ros2BridgeDirection) -> &'static str {
    match direction {
        flowrt_ir::Ros2BridgeDirection::FlowrtToRos2 => "flowrt_to_ros2",
        flowrt_ir::Ros2BridgeDirection::Ros2ToFlowrt => "ros2_to_flowrt",
    }
}

fn launch_channels(
    contract: &ContractIr,
    graph: &GraphIr,
    graph_facts: &GraphDerivedFacts,
) -> Vec<serde_json::Value> {
    let route_facts = graph_facts
        .routes
        .iter()
        .map(|route| (route.bind_id.0.as_str(), route))
        .collect::<BTreeMap<_, _>>();
    graph
        .binds
        .iter()
        .enumerate()
        .map(|(index, bind)| {
            let route = route_facts
                .get(bind.id.0.as_str())
                .copied()
                .expect("derived route facts must contain every launch channel");
            let backend = route.backend.0.as_str();
            let service = (backend == "iox2")
                .then(|| iox2_service_name_for_edge(contract, graph, index, bind));
            let key_expr =
                (backend == "zenoh").then(|| zenoh_key_expr_for_edge(contract, graph, index, bind));
            let message_expr = channel_message_expr(contract, graph, bind);
            let message_type = message_expr.canonical_syntax();
            let frame = frame_transport_for_expr_with_backend_slot(
                contract,
                &message_expr,
                &message_type,
                backend,
            );
            let diagnostic = unbounded_frame_fallback_diagnostic(
                contract,
                &message_expr,
                &message_type,
                &format!(
                    "{}.{} -> {}.{}",
                    bind.from.instance.name, bind.from.port, bind.to.instance.name, bind.to.port
                ),
                backend,
                route.backend_source == flowrt_ir::ChannelBackendSource::AutoFallback,
            );
            let mut channel = serde_json::json!({
                "from": format!("{}.{}", bind.from.instance.name, bind.from.port),
                "to": format!("{}.{}", bind.to.instance.name, bind.to.port),
                "backend": backend,
                "thread_affinity": route.thread_affinity.map(thread_affinity_name),
                "service": service,
                "key_expr": key_expr,
                "channel": bind.channel,
                "depth": bind.depth,
                "overflow": bind.overflow,
                "stale_policy": bind.stale,
                "max_age_ms": bind.max_age_ms,
            });
            if frame.is_some() {
                channel
                    .as_object_mut()
                    .expect("channel endpoint must be a JSON object")
                    .insert(
                        "backend_source".to_string(),
                        serde_json::Value::String(
                            channel_backend_source_name(route.backend_source).to_string(),
                        ),
                    );
            }
            if let Some(diagnostic) = diagnostic {
                channel
                    .as_object_mut()
                    .expect("channel endpoint must be a JSON object")
                    .insert(
                        "diagnostic".to_string(),
                        serde_json::Value::String(diagnostic),
                    );
            }
            if let Some(frame) = frame {
                channel
                    .as_object_mut()
                    .expect("channel endpoint must be a JSON object")
                    .insert(
                        "frame".to_string(),
                        serde_json::to_value(frame).expect("frame transport must serialize"),
                    );
            }
            channel
        })
        .collect()
}

fn thread_affinity_name(affinity: BackendThreadAffinity) -> &'static str {
    match affinity {
        BackendThreadAffinity::SendSafe => "send_safe",
        BackendThreadAffinity::SchedulerLocalCommit => "scheduler_local_commit",
    }
}

fn launch_processes(
    contract: &ContractIr,
    graph: &GraphIr,
    graph_facts: &GraphDerivedFacts,
) -> Vec<serde_json::Value> {
    let mut processes = BTreeMap::<String, Vec<&InstanceIr>>::new();
    let external_processes = graph
        .external_processes
        .iter()
        .map(|external| (external.process.as_str(), external))
        .collect::<BTreeMap<_, _>>();
    for instance in &graph.instances {
        processes
            .entry(
                instance
                    .process
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
            )
            .or_default()
            .push(instance);
    }

    let mut launch_processes = processes
        .into_iter()
        .map(|(name, instances)| {
            let instance_names = instances
                .iter()
                .map(|instance| instance.name.as_str())
                .collect::<BTreeSet<_>>();
            let runtimes = process_runtimes(contract, &instances);
            let target = common_process_target(&instances);
            let orchestration = process_orchestration(graph, &name);
            let external = external_processes
                .get(name.as_str())
                .map(|external| launch_external_process(external));
            let backend = process_backend(
                graph,
                graph_facts,
                &instance_names,
                external_processes.get(name.as_str()).copied(),
            );
            serde_json::json!({
                "name": name,
                "backend": backend,
                "target": target,
                "runtimes": runtimes,
                "runtime_kind": process_runtime_kind(&runtimes),
                "external": external,
                "depends_on": orchestration.depends_on,
                "restart": {
                    "policy": orchestration.restart.policy,
                    "max_restarts": orchestration.restart.max_restarts,
                    "initial_delay_ms": orchestration.restart.initial_delay_ms,
                    "max_delay_ms": orchestration.restart.max_delay_ms,
                },
                "failure": orchestration.failure_propagation,
                "readiness": orchestration.readiness,
                "startup_delay_ms": orchestration.startup_delay_ms,
                "env": orchestration.env,
                "resource_placement": {
                    "cpu_affinity": orchestration.cpu_affinity,
                    "nice": orchestration.nice,
                    "rt_policy": orchestration.rt_policy,
                    "rt_priority": orchestration.rt_priority,
                },
                "io_boundaries": launch_io_boundaries(contract, &instances),
                "instances": instances.iter().map(|instance| &instance.name).collect::<Vec<_>>(),
                "tasks": graph.tasks.iter().filter(|task| instance_names.contains(task.instance.name.as_str())).map(launch_task).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    if !graph.ros2_bridges.is_empty() {
        launch_processes.push(serde_json::json!({
            "name": "ros2_bridge",
            "backend": "zenoh",
            "target": null,
        "runtimes": ["ros2_bridge"],
        "runtime_kind": "ros2_bridge",
        "external": null,
        "depends_on": [],
            "restart": {
                "policy": "on_failure",
                "max_restarts": 3,
                "initial_delay_ms": 100,
                "max_delay_ms": 1000,
            },
            "failure": "propagate",
            "readiness": "process_started",
            "startup_delay_ms": 0,
            "env": {},
            "resource_placement": {
                "cpu_affinity": [],
                "nice": null,
                "rt_policy": null,
                "rt_priority": null,
            },
            "instances": [],
            "tasks": [],
        }));
    }

    launch_processes
}

fn launch_io_boundaries(
    contract: &ContractIr,
    instances: &[&InstanceIr],
) -> Vec<serde_json::Value> {
    instances
        .iter()
        .filter_map(|instance| {
            let component = component_by_name(contract, &instance.component.name);
            if component.kind != ComponentKind::IoBoundary {
                return None;
            }
            let policy = component.io_boundary.as_ref()?;
            Some(serde_json::json!({
                "instance": instance.name,
                "component": component.name,
                "side_effects": policy
                    .side_effects
                    .iter()
                    .map(|effect| io_side_effect_name(*effect))
                    .collect::<Vec<_>>(),
                "readiness": io_readiness_name(policy.readiness),
                "health": io_health_name(policy.health),
                "shutdown": io_shutdown_name(policy.shutdown),
                "resources": component
                    .resources
                    .iter()
                    .map(launch_resource_requirement)
                    .collect::<Vec<_>>(),
            }))
        })
        .collect()
}

fn launch_resource_requirement(resource: &ResourceRequirementIr) -> serde_json::Value {
    let mut value = serde_json::json!({
        "name": resource.name,
        "capability": resource.capability.0.as_str(),
        "access": resource_access_name(resource.access),
        "required": resource.required,
        "readiness": resource_readiness_name(resource.readiness),
        "health": resource_health_name(resource.health),
        "on_failure": resource_failure_name(resource.on_failure),
    });
    if let Some(descriptor) = &resource.descriptor {
        value["descriptor"] = serde_json::json!({
            "kind": resource_descriptor_kind_name(descriptor.kind),
            "port": descriptor.port,
            "format": descriptor.format,
            "encoding": descriptor.encoding,
            "metadata": descriptor.metadata,
            "record_payload": descriptor.record_payload,
            "payload_capture": descriptor_payload_capture_name(descriptor.payload_capture),
        });
    }
    value
}

fn launch_external_process(external: &ExternalProcessIr) -> serde_json::Value {
    serde_json::json!({
        "package": &external.package,
        "executable": &external.executable,
        "args": &external.args,
        "working_dir": external_working_dir_name(external.working_dir),
        "health": external_health_name(external.health),
        "required_backends": external
            .required_backends
            .iter()
            .map(|backend| backend.0.as_str())
            .collect::<Vec<_>>(),
    })
}

fn external_working_dir_name(kind: ExternalWorkingDir) -> &'static str {
    match kind {
        ExternalWorkingDir::Package => "package",
        ExternalWorkingDir::Workspace => "workspace",
    }
}

fn external_health_name(kind: ExternalHealthKind) -> &'static str {
    match kind {
        ExternalHealthKind::ProcessStarted => "process_started",
        ExternalHealthKind::RuntimeSocket => "runtime_socket",
    }
}

fn io_side_effect_name(kind: IoSideEffect) -> &'static str {
    match kind {
        IoSideEffect::Read => "read",
        IoSideEffect::Write => "write",
        IoSideEffect::Network => "network",
        IoSideEffect::Filesystem => "filesystem",
        IoSideEffect::Device => "device",
        IoSideEffect::Compute => "compute",
    }
}

fn io_readiness_name(kind: IoBoundaryReadiness) -> &'static str {
    match kind {
        IoBoundaryReadiness::ComponentStarted => "component_started",
        IoBoundaryReadiness::ResourceReady => "resource_ready",
    }
}

fn io_health_name(kind: IoBoundaryHealth) -> &'static str {
    match kind {
        IoBoundaryHealth::RuntimeReported => "runtime_reported",
        IoBoundaryHealth::ProcessStatus => "process_status",
    }
}

fn io_shutdown_name(kind: IoBoundaryShutdown) -> &'static str {
    match kind {
        IoBoundaryShutdown::Cooperative => "cooperative",
        IoBoundaryShutdown::BestEffort => "best_effort",
    }
}

fn process_orchestration<'a>(graph: &'a GraphIr, process_name: &str) -> &'a ProcessIr {
    graph
        .processes
        .iter()
        .find(|process| process.name == process_name)
        .expect("normalized graph must contain process orchestration for every process")
}

fn process_backend(
    graph: &GraphIr,
    graph_facts: &GraphDerivedFacts,
    instance_names: &BTreeSet<&str>,
    external: Option<&ExternalProcessIr>,
) -> String {
    if graph.ros2_bridges.iter().any(|bridge| {
        bridge.backend.0 == "zenoh" && instance_names.contains(bridge.flowrt.instance.name.as_str())
    }) {
        return "zenoh".to_string();
    }
    if graph_facts.routes.iter().any(|route| {
        route.backend.0 == "zenoh"
            && (instance_names.contains(route.from.instance.name.as_str())
                || instance_names.contains(route.to.instance.name.as_str()))
    }) {
        return "zenoh".to_string();
    }
    if graph.services.iter().any(|service| {
        service.backend.0 == "zenoh"
            && (instance_names.contains(service.client.instance.name.as_str())
                || instance_names.contains(service.server.instance.name.as_str()))
    }) {
        return "zenoh".to_string();
    }
    if graph.operations.iter().any(|operation| {
        operation.backend.0 == "zenoh"
            && (instance_names.contains(operation.client.instance.name.as_str())
                || instance_names.contains(operation.server.instance.name.as_str()))
    }) {
        return "zenoh".to_string();
    }
    if graph_facts.routes.iter().any(|route| {
        route.backend.0 == "iox2"
            && (instance_names.contains(route.from.instance.name.as_str())
                || instance_names.contains(route.to.instance.name.as_str()))
    }) {
        return "iox2".to_string();
    }
    if let Some(external) = external {
        if external
            .required_backends
            .iter()
            .any(|backend| backend.0 == "zenoh")
        {
            return "zenoh".to_string();
        }
        if let Some(backend) = external.required_backends.first() {
            return backend.0.clone();
        }
    }
    "inproc".to_string()
}

fn launch_task(task: &TaskIr) -> serde_json::Value {
    serde_json::json!({
        "name": task.name,
        "instance": task.instance.name,
        "trigger": task.trigger,
        "readiness": task.readiness,
        "period_ms": task.period_ms,
        "deadline_ms": task.deadline_ms,
        "lane": task_lane_name(task),
        "priority": task.priority,
        "inputs": task.inputs,
        "outputs": task.outputs,
    })
}

fn process_runtimes(contract: &ContractIr, instances: &[&InstanceIr]) -> Vec<&'static str> {
    instances
        .iter()
        .map(|instance| component_by_name(contract, &instance.component.name))
        .map(|component| language_name(component.language))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn process_runtime_kind(runtimes: &[&'static str]) -> &'static str {
    if runtimes.len() == 1 {
        runtimes[0]
    } else {
        "mixed"
    }
}

fn common_process_target(instances: &[&InstanceIr]) -> Option<String> {
    let mut targets = instances
        .iter()
        .filter_map(|instance| instance.target.as_ref().map(|target| target.name.clone()))
        .collect::<BTreeSet<_>>();

    if targets.len() == 1 {
        targets.pop_first()
    } else {
        None
    }
}
