use std::collections::BTreeMap;

use flowrt_conformance::MessageAbiExpectation;
use flowrt_ir::{
    ChannelEdgeIr, ChannelKind, ComponentIr, ContractIr, GraphIr, InstanceIr,
    OverflowPolicy as IrOverflowPolicy, StalePolicy as IrStalePolicy, TaskReadiness, TriggerKind,
    TypeExpr, TypeIr,
};
use flowrt_selfdesc::{
    SELF_DESCRIPTION_SCHEMA_VERSION, SELF_DESCRIPTION_SECTION, SelfDescription,
    SelfDescriptionChannel, SelfDescriptionDeployment, SelfDescriptionFieldAbi,
    SelfDescriptionFrameField, SelfDescriptionGraph, SelfDescriptionInstance,
    SelfDescriptionMessageAbi, SelfDescriptionMessageFrame, SelfDescriptionPackage,
    SelfDescriptionParam, SelfDescriptionProfile, SelfDescriptionScheduler,
    SelfDescriptionSchedulerLane, SelfDescriptionSchedulerTask, SelfDescriptionTarget,
    SelfDescriptionTask,
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
                backend: profile.backend.0.clone(),
            })
            .collect(),
        targets: contract
            .targets
            .iter()
            .map(|target| SelfDescriptionTarget {
                name: target.name.clone(),
                platform: target.platform.clone(),
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
        scheduler: self_description_scheduler(contract, graph),
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
                }
            })
            .collect(),
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
                period_ms: task.period_ms,
                deadline_ms: task.deadline_ms,
                priority: task.priority,
            })
            .collect(),
    }
}

fn task_lane_name(task: &flowrt_ir::TaskIr) -> String {
    task.lane
        .clone()
        .unwrap_or_else(|| format!("{}_serial", task.instance.name))
}

fn self_description_message_abi(
    contract: &ContractIr,
    expectation: MessageAbiExpectation,
) -> SelfDescriptionMessageAbi {
    let type_fields = contract
        .types
        .iter()
        .find(|ty| ty.name == expectation.type_name)
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
