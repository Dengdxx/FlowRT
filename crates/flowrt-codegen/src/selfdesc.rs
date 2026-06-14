use std::collections::BTreeMap;

use flowrt_conformance::MessageAbiExpectation;
use flowrt_ir::{
    BackendThreadAffinity, BoundaryDirection, BoundaryEndpointIr, ChannelEdgeIr, ChannelKind,
    ComponentIr, ComponentKind, ContractIr, GraphIr, GraphMode, InstanceIr, IoBoundaryHealth,
    IoBoundaryReadiness, IoBoundaryShutdown, IoSideEffect, OperationConcurrencyPolicy,
    OperationFeedbackPolicy, OperationPreemptPolicy, OverflowPolicy as IrOverflowPolicy,
    ResourceProviderIr, ResourceProviderScope, ResourceSatisfactionIr, ServiceOverflowPolicy,
    StalePolicy as IrStalePolicy, TaskConcurrency, TaskReadiness, TriggerKind, TypeExpr, TypeIr,
};
use flowrt_selfdesc::{
    SELF_DESCRIPTION_SCHEMA_VERSION, SELF_DESCRIPTION_SECTION, SelfDescription,
    SelfDescriptionArtifact, SelfDescriptionBoundaryEndpoint, SelfDescriptionChannel,
    SelfDescriptionClock, SelfDescriptionComponentType, SelfDescriptionDeployment,
    SelfDescriptionExternalProcess, SelfDescriptionFieldAbi, SelfDescriptionFrameField,
    SelfDescriptionGraph, SelfDescriptionInstance, SelfDescriptionIoBoundary,
    SelfDescriptionMessageAbi, SelfDescriptionMessageFrame, SelfDescriptionOperationEndpoint,
    SelfDescriptionOperationLowering, SelfDescriptionOperationPortDecl, SelfDescriptionPackage,
    SelfDescriptionParam, SelfDescriptionParamDecl, SelfDescriptionPortDecl,
    SelfDescriptionProfile, SelfDescriptionResourceContract, SelfDescriptionResourceDescriptor,
    SelfDescriptionResourceProvider, SelfDescriptionResourceRequirement,
    SelfDescriptionResourceRequirementBinding, SelfDescriptionResourceSatisfaction,
    SelfDescriptionScheduler, SelfDescriptionSchedulerLane, SelfDescriptionSchedulerTask,
    SelfDescriptionServiceEndpoint, SelfDescriptionServicePortDecl, SelfDescriptionTarget,
    SelfDescriptionTask, SelfDescriptionTemporaryOverlay,
    SelfDescriptionTemporaryOverlayBoundaryMapping, SelfDescriptionTemporaryOverlayGeneration,
};
use sha2::{Digest, Sha256};

use crate::{
    Result, component_by_name, fixed_message_abi_expectations, frame_header_size_for_expr,
    frame_header_size_for_type, frame_max_size_for_type, language_name, managed_header,
    param_type_name, param_update_name, param_value_for_instance, param_value_json,
    type_contains_variable_data, variable_tail_max_size,
};

pub(super) fn emit_self_description(contract: &ContractIr) -> Result<String> {
    let self_description = self_description(contract)?;
    let mut output = serde_json::to_string_pretty(&self_description)?;
    output.push('\n');
    Ok(output)
}

pub(super) fn emit_cpp_selfdesc_header(_contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str("#include <cstddef>\n#include <string_view>\n\n");
    output.push_str("namespace flowrt_app {\n\n");
    output.push_str("std::string_view self_description_json() noexcept;\n\n");
    output.push_str("std::string_view self_description_hash() noexcept;\n\n");
    output.push_str("}  // namespace flowrt_app\n");
    output
}

pub(super) fn emit_cpp_selfdesc_source(contract: &ContractIr) -> String {
    let json = emit_self_description(contract)
        .expect("validated contract self-description should serialize");
    let hash = hex_sha256(&json);
    let mut output = managed_header();
    output.push_str("#include \"flowrt_app/selfdesc.hpp\"\n\n");
    output.push_str("#include <string_view>\n\n");
    output.push_str("namespace flowrt_app {\nnamespace {\n\n");
    output.push_str(&format!(
        "#if defined(__GNUC__) || defined(__clang__)\n[[gnu::used, gnu::section(\"{SELF_DESCRIPTION_SECTION}\")]]\n#endif\n"
    ));
    output.push_str("const char kFlowrtSelfDescription[] = ");
    output.push_str(&cpp_raw_string_literal(&json));
    output.push_str(";\n\n");
    output.push_str("const char kFlowrtSelfDescriptionHash[] = ");
    output.push_str(&crate::cpp_string_literal(&hash));
    output.push_str(";\n\n");
    output.push_str("}  // namespace\n\n");
    output.push_str(
        "std::string_view self_description_json() noexcept {\n    return std::string_view{kFlowrtSelfDescription, sizeof(kFlowrtSelfDescription) - 1};\n}\n\n",
    );
    output.push_str(
        "std::string_view self_description_hash() noexcept {\n    return std::string_view{kFlowrtSelfDescriptionHash, sizeof(kFlowrtSelfDescriptionHash) - 1};\n}\n\n",
    );
    output.push_str("}  // namespace flowrt_app\n");
    output
}

pub(super) fn emit_rust_selfdesc(contract: &ContractIr) -> String {
    let json = emit_self_description(contract)
        .expect("validated contract self-description should serialize");
    let hash = hex_sha256(&json);
    let mut output = managed_header();
    output.push('\n');
    output.push_str(&format!(
        "#[used]\n#[unsafe(link_section = \"{SELF_DESCRIPTION_SECTION}\")]\nstatic FLOWRT_SELF_DESCRIPTION: [u8; {}] = *{};\n\n",
        json.len(),
        rust_byte_string_literal(&json),
    ));
    output.push_str(
        "#[allow(dead_code)]\npub fn self_description_json() -> &'static str {\n    std::str::from_utf8(&FLOWRT_SELF_DESCRIPTION).expect(\"generated FlowRT self-description is UTF-8\")\n}\n",
    );
    output.push_str(&format!(
        "\n#[allow(dead_code)]\npub fn self_description_hash() -> &'static str {{\n    {}\n}}\n",
        crate::rust_string_literal(&hash)
    ));
    output
}

fn self_description(contract: &ContractIr) -> Result<SelfDescription> {
    Ok(SelfDescription {
        self_description_version: SELF_DESCRIPTION_SCHEMA_VERSION.to_string(),
        ir_version: contract.ir_version.clone(),
        schema_version: contract.schema_version.clone(),
        source_hash: contract.source_hash.clone(),
        artifact: SelfDescriptionArtifact {
            mode: graph_mode_name(contract_artifact_mode(contract)).to_string(),
            temporary_island: contract.artifact.temporary_island,
            test_only: contract.artifact.test_only,
            temporary_overlay: contract
                .artifact
                .temporary_overlay
                .as_ref()
                .map(self_description_temporary_overlay),
            clock: SelfDescriptionClock {
                source: clock_source_name(contract).to_string(),
                unit: "ms".to_string(),
                field: "tick_time_ms".to_string(),
            },
        },
        package: SelfDescriptionPackage {
            name: contract.package.name.clone(),
            version: contract.package.version.clone(),
            rsdl_version: contract.package.rsdl_version.clone(),
        },
        profiles: contract
            .profiles
            .iter()
            .map(|profile| SelfDescriptionProfile {
                name: profile.name.clone(),
                mode: graph_mode_name(profile.mode).to_string(),
                backend: profile.backend.0.clone(),
            })
            .collect(),
        targets: contract
            .targets
            .iter()
            .map(|target| SelfDescriptionTarget {
                name: target.name.clone(),
                platform: target
                    .platform
                    .map(|platform| platform.as_str().to_string()),
                runtimes: target
                    .runtime
                    .iter()
                    .copied()
                    .map(|r| language_name(r).to_string())
                    .collect(),
                backends: target
                    .backends
                    .iter()
                    .map(|backend| backend.0.clone())
                    .collect(),
            })
            .collect(),
        deployments: contract
            .deployments
            .iter()
            .map(|deployment| SelfDescriptionDeployment {
                graph: deployment.graph.name.clone(),
                profile: deployment.profile.name.clone(),
                target: deployment.target.name.clone(),
                backend: deployment.backend.0.clone(),
                satisfied: deployment.satisfied,
            })
            .collect(),
        graphs: contract
            .graphs
            .iter()
            .map(|graph| self_description_graph(contract, graph))
            .collect(),
        component_types: contract
            .components
            .iter()
            .map(self_description_component_type)
            .collect(),
        message_abi: fixed_message_abi_expectations(contract)?
            .into_iter()
            .map(|expectation| self_description_message_abi(contract, expectation))
            .collect(),
        message_frames: contract
            .types
            .iter()
            .map(|ty| self_description_message_frame(contract, ty))
            .collect(),
    })
}

fn self_description_graph(contract: &ContractIr, graph: &GraphIr) -> SelfDescriptionGraph {
    SelfDescriptionGraph {
        name: graph.name.clone(),
        mode: graph_mode_name(contract_artifact_mode(contract)).to_string(),
        scheduler: self_description_scheduler(contract, graph),
        resource_contract: self_description_resource_contract(graph),
        external_processes: graph
            .external_processes
            .iter()
            .map(|external| SelfDescriptionExternalProcess {
                process: external.process.clone(),
                package: external.package.clone(),
                executable: external.executable.clone(),
                args: external.args.clone(),
                working_dir: external_working_dir_name(external.working_dir).to_string(),
                health: external_health_name(external.health).to_string(),
                required_backends: external
                    .required_backends
                    .iter()
                    .map(|backend| backend.0.clone())
                    .collect(),
            })
            .collect(),
        instances: graph
            .instances
            .iter()
            .map(|instance| {
                let component = component_by_name(contract, &instance.component.name);
                SelfDescriptionInstance {
                    name: instance.name.clone(),
                    component: instance.component.name.clone(),
                    process: instance
                        .process
                        .clone()
                        .unwrap_or_else(|| "main".to_string()),
                    target: instance.target.as_ref().map(|target| target.name.clone()),
                    runtime: language_name(component.language).to_string(),
                    params: self_description_params(component, instance),
                }
            })
            .collect(),
        tasks: graph
            .tasks
            .iter()
            .map(|task| SelfDescriptionTask {
                name: task.name.clone(),
                instance: task.instance.name.clone(),
                trigger: trigger_name(task.trigger).to_string(),
                readiness: readiness_name(task.readiness).to_string(),
                concurrency: task_concurrency_name(task.concurrency).to_string(),
                period_ms: task.period_ms,
                deadline_ms: task.deadline_ms,
                lane: task_lane_name(task),
                priority: task.priority,
                inputs: task.inputs.clone(),
                outputs: task.outputs.clone(),
            })
            .collect(),
        channels: graph
            .binds
            .iter()
            .enumerate()
            .map(|(index, bind)| {
                let backend = bind.backend.0.clone();
                SelfDescriptionChannel {
                    from: format!("{}.{}", bind.from.instance.name, bind.from.port),
                    to: format!("{}.{}", bind.to.instance.name, bind.to.port),
                    message_type: channel_message_type(contract, graph, bind),
                    service: if backend == "iox2" {
                        Some(crate::iox2_service_name_for_edge(
                            contract, graph, index, bind,
                        ))
                    } else {
                        None
                    },
                    key_expr: if backend == "zenoh" {
                        Some(crate::zenoh_key_expr_for_edge(contract, graph, index, bind))
                    } else {
                        None
                    },
                    channel: channel_name(bind.channel).to_string(),
                    depth: bind.depth,
                    overflow: overflow_name(bind.overflow).to_string(),
                    stale_policy: stale_name(bind.stale).to_string(),
                    max_age_ms: bind.max_age_ms,
                    backend,
                    thread_affinity: bind
                        .thread_affinity
                        .map(thread_affinity_name)
                        .unwrap_or_default()
                        .to_string(),
                }
            })
            .collect(),
        boundary_endpoints: graph
            .boundary_endpoints
            .iter()
            .map(self_description_boundary_endpoint)
            .collect(),
        services: graph
            .services
            .iter()
            .map(|service| self_description_service_endpoint(contract, graph, service))
            .collect(),
        operations: self_description_operation_endpoints(contract, graph),
    }
}

fn self_description_resource_contract(graph: &GraphIr) -> SelfDescriptionResourceContract {
    SelfDescriptionResourceContract {
        resource_contract_version: flowrt_selfdesc::RESOURCE_CONTRACT_SCHEMA_VERSION.to_string(),
        requirements: graph
            .resource_satisfactions
            .iter()
            .map(self_description_resource_requirement_binding)
            .collect(),
        providers: graph
            .resource_providers
            .iter()
            .map(self_description_resource_provider)
            .collect(),
        satisfactions: graph
            .resource_satisfactions
            .iter()
            .map(self_description_resource_satisfaction)
            .collect(),
    }
}

fn self_description_resource_requirement_binding(
    satisfaction: &ResourceSatisfactionIr,
) -> SelfDescriptionResourceRequirementBinding {
    SelfDescriptionResourceRequirementBinding {
        instance: satisfaction.instance.name.clone(),
        component: satisfaction.component.name.clone(),
        name: satisfaction.resource.clone(),
        capability: satisfaction.capability.0.clone(),
        access: resource_access_name(satisfaction.access).to_string(),
        required: satisfaction.required,
        readiness: resource_readiness_name(satisfaction.readiness).to_string(),
        health: resource_health_name(satisfaction.health).to_string(),
        on_failure: resource_failure_name(satisfaction.on_failure).to_string(),
        satisfaction: resource_satisfaction_status(satisfaction).to_string(),
        provider: satisfaction
            .provider
            .as_ref()
            .map(|provider| provider.name.clone()),
        diagnostic: satisfaction.diagnostic.clone(),
    }
}

fn self_description_resource_provider(
    provider: &ResourceProviderIr,
) -> SelfDescriptionResourceProvider {
    SelfDescriptionResourceProvider {
        name: provider.name.clone(),
        scope: resource_provider_scope_name(provider.scope).to_string(),
        capabilities: provider
            .capabilities
            .iter()
            .map(|capability| capability.0.clone())
            .collect(),
        target: provider.target.as_ref().map(|target| target.name.clone()),
        process: provider.process.clone(),
        external_package: provider.external_package.clone(),
        readiness_source: provider.readiness_source.clone(),
        health_source: provider.health_source.clone(),
    }
}

fn self_description_resource_satisfaction(
    satisfaction: &ResourceSatisfactionIr,
) -> SelfDescriptionResourceSatisfaction {
    SelfDescriptionResourceSatisfaction {
        instance: satisfaction.instance.name.clone(),
        component: satisfaction.component.name.clone(),
        resource: satisfaction.resource.clone(),
        capability: satisfaction.capability.0.clone(),
        access: resource_access_name(satisfaction.access).to_string(),
        required: satisfaction.required,
        readiness: resource_readiness_name(satisfaction.readiness).to_string(),
        health: resource_health_name(satisfaction.health).to_string(),
        on_failure: resource_failure_name(satisfaction.on_failure).to_string(),
        status: resource_satisfaction_status(satisfaction).to_string(),
        satisfied: satisfaction.satisfied,
        provider: satisfaction
            .provider
            .as_ref()
            .map(|provider| provider.name.clone()),
        diagnostic: satisfaction.diagnostic.clone(),
    }
}

fn self_description_boundary_endpoint(
    endpoint: &BoundaryEndpointIr,
) -> SelfDescriptionBoundaryEndpoint {
    SelfDescriptionBoundaryEndpoint {
        canonical_id: endpoint.id.0.clone(),
        name: endpoint.name.clone(),
        direction: boundary_direction_name(endpoint.direction).to_string(),
        endpoint: format!("{}.{}", endpoint.port.instance.name, endpoint.port.port),
        instance: endpoint.port.instance.name.clone(),
        port: endpoint.port.port.clone(),
        message_type: endpoint.ty.canonical_syntax(),
    }
}

fn graph_mode_name(mode: GraphMode) -> &'static str {
    match mode {
        GraphMode::Strict => "strict",
        GraphMode::Island => "island",
    }
}

fn clock_source_name(contract: &ContractIr) -> &'static str {
    if contract.artifact.temporary_overlay.is_some() {
        "simulated_replay"
    } else {
        "realtime"
    }
}

fn self_description_temporary_overlay(
    overlay: &flowrt_ir::TemporaryOverlayIr,
) -> SelfDescriptionTemporaryOverlay {
    SelfDescriptionTemporaryOverlay {
        kind: overlay.kind.clone(),
        original_profile_mode: graph_mode_name(overlay.original_profile_mode).to_string(),
        generated_by: SelfDescriptionTemporaryOverlayGeneration {
            command: overlay.generated_by.command.clone(),
            source: overlay.generated_by.source.clone(),
        },
        boundary_mappings: overlay
            .boundary_mappings
            .iter()
            .map(|mapping| SelfDescriptionTemporaryOverlayBoundaryMapping {
                direction: boundary_direction_name(mapping.direction).to_string(),
                name: mapping.name.clone(),
                endpoint: mapping.endpoint.clone(),
                source: mapping.source.clone(),
            })
            .collect(),
    }
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

fn thread_affinity_name(affinity: BackendThreadAffinity) -> &'static str {
    match affinity {
        BackendThreadAffinity::SendSafe => "send_safe",
        BackendThreadAffinity::SchedulerLocalCommit => "scheduler_local_commit",
    }
}

fn self_description_service_endpoint(
    contract: &ContractIr,
    graph: &GraphIr,
    service: &flowrt_ir::ServiceEdgeIr,
) -> SelfDescriptionServiceEndpoint {
    let instances = graph
        .instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();

    let (request_type, response_type) = instances
        .get(service.client.instance.name.as_str())
        .map(|instance| {
            let component = component_by_name(contract, &instance.component.name);
            let port = component
                .service_clients
                .iter()
                .find(|port| port.name == service.client.port)
                .expect("validated service bind must reference existing client service port");
            (
                port.request.canonical_syntax(),
                port.response.canonical_syntax(),
            )
        })
        .expect("validated service bind must reference existing client instance");

    let name = format!(
        "{}.{}_to_{}.{}",
        service.client.instance.name,
        service.client.port,
        service.server.instance.name,
        service.server.port
    );

    SelfDescriptionServiceEndpoint {
        name,
        canonical_id: service.id.0.clone(),
        client_instance: service.client.instance.name.clone(),
        client_port: service.client.port.clone(),
        server_instance: service.server.instance.name.clone(),
        server_port: service.server.port.clone(),
        request_type,
        response_type,
        backend: service.backend.0.clone(),
        timeout_ms: Some(service.policy.timeout_ms),
        queue_depth: Some(service.policy.queue_depth),
        overflow: service_overflow_name(service.policy.overflow).to_string(),
        lane: service.policy.lane.clone().unwrap_or_default(),
        max_in_flight: Some(service.policy.max_in_flight),
    }
}

fn service_overflow_name(policy: ServiceOverflowPolicy) -> &'static str {
    match policy {
        ServiceOverflowPolicy::Busy => "busy",
        ServiceOverflowPolicy::Error => "error",
    }
}

fn self_description_operation_endpoints(
    contract: &ContractIr,
    graph: &GraphIr,
) -> Vec<SelfDescriptionOperationEndpoint> {
    crate::runtime_plan::operation_runtime_plans(contract, graph)
        .iter()
        .map(|plan| {
            let operation = &graph.operations[plan.index];
            SelfDescriptionOperationEndpoint {
                name: plan.operation_name.clone(),
                canonical_id: operation.id.0.clone(),
                client_instance: plan.client_instance.clone(),
                client_port: plan.client_port.clone(),
                server_instance: plan.server_instance.clone(),
                server_port: plan.server_port.clone(),
                goal_type: plan.goal_type.canonical_syntax(),
                feedback_type: plan.feedback_type.canonical_syntax(),
                result_type: plan.result_type.canonical_syntax(),
                backend: plan.backend.0.clone(),
                timeout_ms: Some(plan.timeout_ms),
                concurrency: operation_concurrency_name(plan.concurrency).to_string(),
                preempt: operation_preempt_name(plan.preempt).to_string(),
                queue_depth: Some(plan.queue_depth),
                max_in_flight: Some(plan.max_in_flight),
                feedback: operation_feedback_name(plan.feedback).to_string(),
                result_retention_ms: Some(plan.result_retention_ms),
                lowering: SelfDescriptionOperationLowering {
                    start_service: operation_start_endpoint_name(plan),
                    cancel_service: operation_cancel_endpoint_name(plan),
                    status_service: operation_status_endpoint_name(plan),
                    feedback_channel: operation_feedback_endpoint_name(plan),
                    result_channel: operation_result_endpoint_name(plan),
                },
            }
        })
        .collect()
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

fn operation_start_endpoint_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_start",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn operation_cancel_endpoint_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_cancel",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn operation_status_endpoint_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_status",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn operation_feedback_endpoint_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_feedback",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn operation_result_endpoint_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_result",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn self_description_component_type(component: &ComponentIr) -> SelfDescriptionComponentType {
    SelfDescriptionComponentType {
        name: component.name.clone(),
        language: language_name(component.language).to_string(),
        kind: component_kind_name(component.kind).to_string(),
        resources: component
            .resources
            .iter()
            .map(|resource| SelfDescriptionResourceRequirement {
                name: resource.name.clone(),
                capability: resource.capability.0.clone(),
                access: resource_access_name(resource.access).to_string(),
                required: resource.required,
                readiness: resource_readiness_name(resource.readiness).to_string(),
                health: resource_health_name(resource.health).to_string(),
                on_failure: resource_failure_name(resource.on_failure).to_string(),
                descriptor: resource.descriptor.as_ref().map(|descriptor| {
                    SelfDescriptionResourceDescriptor {
                        kind: resource_descriptor_kind_name(descriptor.kind).to_string(),
                        port: descriptor.port.clone(),
                        format: descriptor.format.clone(),
                        encoding: descriptor.encoding.clone().unwrap_or_default(),
                        metadata: descriptor.metadata.clone(),
                        record_payload: descriptor.record_payload,
                    }
                }),
            })
            .collect(),
        io_boundary: component
            .io_boundary
            .as_ref()
            .map(|policy| SelfDescriptionIoBoundary {
                side_effects: policy
                    .side_effects
                    .iter()
                    .map(|effect| io_side_effect_name(*effect).to_string())
                    .collect(),
                readiness: io_readiness_name(policy.readiness).to_string(),
                health: io_health_name(policy.health).to_string(),
                shutdown: io_shutdown_name(policy.shutdown).to_string(),
            }),
        inputs: component
            .inputs
            .iter()
            .map(|port| SelfDescriptionPortDecl {
                name: port.name.clone(),
                ty: port.ty.canonical_syntax(),
            })
            .collect(),
        outputs: component
            .outputs
            .iter()
            .map(|port| SelfDescriptionPortDecl {
                name: port.name.clone(),
                ty: port.ty.canonical_syntax(),
            })
            .collect(),
        service_clients: component
            .service_clients
            .iter()
            .map(|port| SelfDescriptionServicePortDecl {
                name: port.name.clone(),
                request_type: port.request.canonical_syntax(),
                response_type: port.response.canonical_syntax(),
            })
            .collect(),
        service_servers: component
            .service_servers
            .iter()
            .map(|port| SelfDescriptionServicePortDecl {
                name: port.name.clone(),
                request_type: port.request.canonical_syntax(),
                response_type: port.response.canonical_syntax(),
            })
            .collect(),
        operation_clients: component
            .operation_clients
            .iter()
            .map(|port| SelfDescriptionOperationPortDecl {
                name: port.name.clone(),
                goal_type: port.goal.canonical_syntax(),
                feedback_type: port.feedback.canonical_syntax(),
                result_type: port.result.canonical_syntax(),
            })
            .collect(),
        operation_servers: component
            .operation_servers
            .iter()
            .map(|port| SelfDescriptionOperationPortDecl {
                name: port.name.clone(),
                goal_type: port.goal.canonical_syntax(),
                feedback_type: port.feedback.canonical_syntax(),
                result_type: port.result.canonical_syntax(),
            })
            .collect(),
        params: component
            .params
            .iter()
            .map(|param| SelfDescriptionParamDecl {
                name: param.name.clone(),
                ty: param_type_name(param.ty).to_string(),
                update: param_update_name(param.update).to_string(),
                default: Some(param_value_json(&param.default)),
                min: param.min.as_ref().map(param_value_json),
                max: param.max.as_ref().map(param_value_json),
                choices: param.choices.iter().map(param_value_json).collect(),
            })
            .collect(),
    }
}

fn component_kind_name(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Native => "native",
        ComponentKind::IoBoundary => "io_boundary",
        ComponentKind::External => "external",
    }
}

fn resource_descriptor_kind_name(kind: flowrt_ir::ResourceDescriptorKind) -> &'static str {
    match kind {
        flowrt_ir::ResourceDescriptorKind::Frame => "frame",
    }
}

fn resource_access_name(kind: flowrt_ir::ResourceAccess) -> &'static str {
    match kind {
        flowrt_ir::ResourceAccess::Read => "read",
        flowrt_ir::ResourceAccess::Write => "write",
        flowrt_ir::ResourceAccess::ReadWrite => "read_write",
        flowrt_ir::ResourceAccess::Exclusive => "exclusive",
    }
}

fn resource_readiness_name(kind: flowrt_ir::ResourceReadinessGate) -> &'static str {
    match kind {
        flowrt_ir::ResourceReadinessGate::BeforeInit => "before_init",
        flowrt_ir::ResourceReadinessGate::BeforeStart => "before_start",
        flowrt_ir::ResourceReadinessGate::Lazy => "lazy",
    }
}

fn resource_health_name(kind: flowrt_ir::ResourceHealthPolicy) -> &'static str {
    match kind {
        flowrt_ir::ResourceHealthPolicy::Required => "required",
        flowrt_ir::ResourceHealthPolicy::Optional => "optional",
        flowrt_ir::ResourceHealthPolicy::Ignored => "ignored",
    }
}

fn resource_failure_name(kind: flowrt_ir::ResourceFailurePolicy) -> &'static str {
    match kind {
        flowrt_ir::ResourceFailurePolicy::StopProcess => "stop_process",
        flowrt_ir::ResourceFailurePolicy::RestartProcess => "restart_process",
        flowrt_ir::ResourceFailurePolicy::Degrade => "degrade",
        flowrt_ir::ResourceFailurePolicy::StopGraph => "stop_graph",
    }
}

fn resource_provider_scope_name(kind: ResourceProviderScope) -> &'static str {
    match kind {
        ResourceProviderScope::Target => "target",
        ResourceProviderScope::Process => "process",
        ResourceProviderScope::ExternalPackage => "external_package",
    }
}

fn resource_satisfaction_status(satisfaction: &ResourceSatisfactionIr) -> &'static str {
    if satisfaction.satisfied {
        "satisfied"
    } else if satisfaction
        .diagnostic
        .as_deref()
        .is_some_and(|diagnostic| diagnostic.contains("conflict"))
    {
        "conflict"
    } else if !satisfaction.required {
        "optional_unsatisfied"
    } else {
        "unsatisfied"
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

fn external_working_dir_name(kind: flowrt_ir::ExternalWorkingDir) -> &'static str {
    match kind {
        flowrt_ir::ExternalWorkingDir::Package => "package",
        flowrt_ir::ExternalWorkingDir::Workspace => "workspace",
    }
}

fn external_health_name(kind: flowrt_ir::ExternalHealthKind) -> &'static str {
    match kind {
        flowrt_ir::ExternalHealthKind::ProcessStarted => "process_started",
        flowrt_ir::ExternalHealthKind::RuntimeSocket => "runtime_socket",
    }
}

fn self_description_scheduler(contract: &ContractIr, graph: &GraphIr) -> SelfDescriptionScheduler {
    let mut lanes = BTreeMap::<String, String>::new();
    for task in &graph.tasks {
        lanes.insert(task_lane_name(task), task.instance.name.clone());
    }

    SelfDescriptionScheduler {
        worker_threads: contract
            .profiles
            .first()
            .map(|profile| profile.scheduler.worker_threads)
            .unwrap_or(1),
        lanes: lanes
            .into_iter()
            .map(|(name, instance)| SelfDescriptionSchedulerLane {
                name,
                kind: "serial".to_string(),
                instance,
            })
            .collect(),
        tasks: graph
            .tasks
            .iter()
            .map(|task| SelfDescriptionSchedulerTask {
                name: task.name.clone(),
                instance: task.instance.name.clone(),
                lane: task_lane_name(task),
                trigger: trigger_name(task.trigger).to_string(),
                readiness: readiness_name(task.readiness).to_string(),
                concurrency: task_concurrency_name(task.concurrency).to_string(),
                period_ms: task.period_ms,
                deadline_ms: task.deadline_ms,
                priority: task.priority,
            })
            .collect(),
    }
}

fn task_lane_name(task: &flowrt_ir::TaskIr) -> String {
    crate::runtime_plan::resolved_task_lane_name(task)
}

fn task_concurrency_name(concurrency: TaskConcurrency) -> &'static str {
    match concurrency {
        TaskConcurrency::Exclusive => "exclusive",
        TaskConcurrency::Parallel => "parallel",
    }
}

fn self_description_message_abi(
    contract: &ContractIr,
    expectation: MessageAbiExpectation,
) -> SelfDescriptionMessageAbi {
    let ir_type = contract
        .types
        .iter()
        .find(|ty| ty.generated_name == expectation.type_name)
        .or_else(|| {
            contract
                .types
                .iter()
                .find(|ty| ty.qualified_name == expectation.type_name)
        });
    let type_fields = ir_type
        .map(|ty| {
            ty.fields
                .iter()
                .map(|field| (field.name.as_str(), field.ty.canonical_syntax()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    SelfDescriptionMessageAbi {
        type_name: expectation.type_name,
        size_bytes: expectation.size_bytes,
        align_bytes: expectation.align_bytes,
        empty: ir_type.is_some_and(|ty| ty.empty),
        fields: expectation
            .fields
            .into_iter()
            .map(|field| SelfDescriptionFieldAbi {
                ty: type_fields
                    .get(field.name.as_str())
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string()),
                name: field.name,
                offset_bytes: field.offset_bytes,
                size_bytes: field.size_bytes,
                align_bytes: field.align_bytes,
            })
            .collect(),
    }
}

fn self_description_message_frame(
    contract: &ContractIr,
    ty: &TypeIr,
) -> SelfDescriptionMessageFrame {
    let mut header_offset = 0usize;
    let mut fields = Vec::with_capacity(ty.fields.len());
    for field in &ty.fields {
        let header_size = frame_header_size_for_expr(contract, &field.ty);
        let tail_max = variable_tail_max_size(contract, &field.ty);
        fields.push(SelfDescriptionFrameField {
            name: field.name.clone(),
            ty: field.ty.canonical_syntax(),
            header_offset_bytes: header_offset,
            header_size_bytes: header_size,
            tail_max_bytes: tail_max,
        });
        header_offset += header_size;
    }
    SelfDescriptionMessageFrame {
        type_name: ty.name.clone(),
        encoding: "canonical_frame_v1".to_string(),
        header_size_bytes: frame_header_size_for_type(contract, ty),
        max_size_bytes: frame_max_size_for_type(contract, ty),
        variable: type_contains_variable_data(
            contract,
            &TypeExpr::Named {
                name: ty.name.clone(),
            },
        ),
        fields,
    }
}

fn channel_message_type(contract: &ContractIr, graph: &GraphIr, bind: &ChannelEdgeIr) -> String {
    let instances = graph
        .instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let source_instance = instances
        .get(bind.from.instance.name.as_str())
        .expect("validated bind source instance must exist");
    let component = component_by_name(contract, &source_instance.component.name);
    component
        .outputs
        .iter()
        .find(|port| port.name == bind.from.port)
        .map(|port| port.ty.canonical_syntax())
        .expect("validated bind source port must exist")
}

fn channel_name(channel: ChannelKind) -> &'static str {
    match channel {
        ChannelKind::Latest => "latest",
        ChannelKind::Fifo => "fifo",
    }
}

fn overflow_name(policy: IrOverflowPolicy) -> &'static str {
    match policy {
        IrOverflowPolicy::DropOldest => "drop_oldest",
        IrOverflowPolicy::DropNewest => "drop_newest",
        IrOverflowPolicy::Error => "error",
        IrOverflowPolicy::Block => "block",
    }
}

fn stale_name(policy: IrStalePolicy) -> &'static str {
    match policy {
        IrStalePolicy::Warn => "warn",
        IrStalePolicy::Drop => "drop",
        IrStalePolicy::HoldLast => "hold_last",
        IrStalePolicy::Error => "error",
    }
}

fn trigger_name(trigger: TriggerKind) -> &'static str {
    match trigger {
        TriggerKind::Periodic => "periodic",
        TriggerKind::OnMessage => "on_message",
        TriggerKind::Startup => "startup",
        TriggerKind::Shutdown => "shutdown",
    }
}

fn readiness_name(readiness: TaskReadiness) -> &'static str {
    match readiness {
        TaskReadiness::AnyReady => "any_ready",
        TaskReadiness::AllReady => "all_ready",
    }
}

fn self_description_params(
    component: &ComponentIr,
    instance: &InstanceIr,
) -> Vec<SelfDescriptionParam> {
    component
        .params
        .iter()
        .map(|param| SelfDescriptionParam {
            name: param.name.clone(),
            ty: param_type_name(param.ty).to_string(),
            update: param_update_name(param.update).to_string(),
            current: param_value_json(param_value_for_instance(instance, param)),
            min: param.min.as_ref().map(param_value_json),
            max: param.max.as_ref().map(param_value_json),
            choices: param.choices.iter().map(param_value_json).collect(),
        })
        .collect()
}

fn rust_byte_string_literal(value: &str) -> String {
    let mut hashes = String::from("#");
    while value.contains(&format!("\"{hashes}")) {
        hashes.push('#');
    }
    format!("br{hashes}\"{value}\"{hashes}")
}

fn hex_sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn cpp_raw_string_literal(value: &str) -> String {
    let mut hashes = String::new();
    while value.contains(&format!("){hashes}\"")) {
        hashes.push('#');
    }
    format!("R\"{hashes}({value}){hashes}\"")
}
