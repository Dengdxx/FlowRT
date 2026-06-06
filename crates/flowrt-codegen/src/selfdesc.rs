use std::collections::BTreeMap;

use flowrt_conformance::MessageAbiExpectation;
use flowrt_ir::{
    ChannelEdgeIr, ChannelKind, ComponentIr, ContractIr, GraphIr, InstanceIr,
    OverflowPolicy as IrOverflowPolicy, StalePolicy as IrStalePolicy, TaskReadiness, TriggerKind,
    TypeExpr, TypeIr,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    Result, component_by_name, fixed_message_abi_expectations, frame_header_size_for_expr,
    frame_header_size_for_type, frame_max_size_for_type, language_name, managed_header,
    param_type_name, param_update_name, param_value_for_instance, param_value_json,
    type_contains_variable_data, variable_tail_max_size,
};

const SELF_DESCRIPTION_SCHEMA_VERSION: &str = "0.1";
const SELF_DESCRIPTION_SECTION: &str = ".flowrt.selfdesc";

#[derive(Debug, Serialize)]
struct SelfDescription<'a> {
    self_description_version: &'static str,
    ir_version: &'a str,
    schema_version: &'a str,
    source_hash: &'a str,
    package: SelfDescriptionPackage<'a>,
    profiles: Vec<SelfDescriptionProfile<'a>>,
    targets: Vec<SelfDescriptionTarget<'a>>,
    deployments: Vec<SelfDescriptionDeployment<'a>>,
    graphs: Vec<SelfDescriptionGraph<'a>>,
    message_abi: Vec<SelfDescriptionMessageAbi>,
    message_frames: Vec<SelfDescriptionMessageFrame>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionPackage<'a> {
    name: &'a str,
    version: Option<&'a str>,
    rsdl_version: &'a str,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionProfile<'a> {
    name: &'a str,
    backend: &'a str,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionTarget<'a> {
    name: &'a str,
    platform: Option<&'a str>,
    runtimes: Vec<&'static str>,
    backends: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionDeployment<'a> {
    graph: &'a str,
    profile: &'a str,
    target: &'a str,
    backend: &'a str,
    satisfied: bool,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionGraph<'a> {
    name: &'a str,
    scheduler: SelfDescriptionScheduler,
    instances: Vec<SelfDescriptionInstance<'a>>,
    tasks: Vec<SelfDescriptionTask<'a>>,
    channels: Vec<SelfDescriptionChannel>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionScheduler {
    worker_threads: u32,
    lanes: Vec<SelfDescriptionSchedulerLane>,
    tasks: Vec<SelfDescriptionSchedulerTask>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionSchedulerLane {
    name: String,
    kind: &'static str,
    instance: String,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionSchedulerTask {
    name: String,
    instance: String,
    lane: String,
    trigger: &'static str,
    readiness: &'static str,
    period_ms: Option<u64>,
    deadline_ms: Option<u64>,
    priority: Option<u32>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionInstance<'a> {
    name: &'a str,
    component: &'a str,
    process: &'a str,
    target: Option<&'a str>,
    runtime: &'static str,
    params: Vec<SelfDescriptionParam>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionParam {
    name: String,
    #[serde(rename = "type")]
    ty: &'static str,
    update: &'static str,
    current: serde_json::Value,
    min: Option<serde_json::Value>,
    max: Option<serde_json::Value>,
    choices: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionTask<'a> {
    name: &'a str,
    instance: &'a str,
    trigger: &'static str,
    readiness: &'static str,
    period_ms: Option<u64>,
    deadline_ms: Option<u64>,
    lane: String,
    priority: Option<u32>,
    inputs: &'a [String],
    outputs: &'a [String],
}

#[derive(Debug, Serialize)]
struct SelfDescriptionChannel {
    from: String,
    to: String,
    message_type: String,
    backend: String,
    service: Option<String>,
    key_expr: Option<String>,
    channel: &'static str,
    depth: Option<u32>,
    overflow: &'static str,
    stale_policy: &'static str,
    max_age_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionMessageAbi {
    type_name: String,
    size_bytes: usize,
    align_bytes: usize,
    fields: Vec<SelfDescriptionFieldAbi>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionFieldAbi {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    offset_bytes: usize,
    size_bytes: usize,
    align_bytes: usize,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionMessageFrame {
    type_name: String,
    encoding: &'static str,
    header_size_bytes: usize,
    max_size_bytes: Option<usize>,
    variable: bool,
    fields: Vec<SelfDescriptionFrameField>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionFrameField {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    header_offset_bytes: usize,
    header_size_bytes: usize,
    tail_max_bytes: Option<usize>,
}

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

fn self_description(contract: &ContractIr) -> Result<SelfDescription<'_>> {
    Ok(SelfDescription {
        self_description_version: SELF_DESCRIPTION_SCHEMA_VERSION,
        ir_version: &contract.ir_version,
        schema_version: &contract.schema_version,
        source_hash: &contract.source_hash,
        package: SelfDescriptionPackage {
            name: &contract.package.name,
            version: contract.package.version.as_deref(),
            rsdl_version: &contract.package.rsdl_version,
        },
        profiles: contract
            .profiles
            .iter()
            .map(|profile| SelfDescriptionProfile {
                name: &profile.name,
                backend: &profile.backend.0,
            })
            .collect(),
        targets: contract
            .targets
            .iter()
            .map(|target| SelfDescriptionTarget {
                name: &target.name,
                platform: target.platform.as_deref(),
                runtimes: target
                    .runtime
                    .iter()
                    .copied()
                    .map(language_name)
                    .collect::<Vec<_>>(),
                backends: target
                    .backends
                    .iter()
                    .map(|backend| backend.0.as_str())
                    .collect::<Vec<_>>(),
            })
            .collect(),
        deployments: contract
            .deployments
            .iter()
            .map(|deployment| SelfDescriptionDeployment {
                graph: &deployment.graph.name,
                profile: &deployment.profile.name,
                target: &deployment.target.name,
                backend: &deployment.backend.0,
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

fn self_description_graph<'a>(
    contract: &'a ContractIr,
    graph: &'a GraphIr,
) -> SelfDescriptionGraph<'a> {
    SelfDescriptionGraph {
        name: &graph.name,
        scheduler: self_description_scheduler(contract, graph),
        instances: graph
            .instances
            .iter()
            .map(|instance| {
                let component = component_by_name(contract, &instance.component.name);
                SelfDescriptionInstance {
                    name: &instance.name,
                    component: &instance.component.name,
                    process: instance.process.as_deref().unwrap_or("main"),
                    target: instance.target.as_ref().map(|target| target.name.as_str()),
                    runtime: language_name(component.language),
                    params: self_description_params(component, instance),
                }
            })
            .collect(),
        tasks: graph
            .tasks
            .iter()
            .map(|task| SelfDescriptionTask {
                name: &task.name,
                instance: &task.instance.name,
                trigger: trigger_name(task.trigger),
                readiness: readiness_name(task.readiness),
                period_ms: task.period_ms,
                deadline_ms: task.deadline_ms,
                lane: task_lane_name(task),
                priority: task.priority,
                inputs: &task.inputs,
                outputs: &task.outputs,
            })
            .collect(),
        channels: graph
            .binds
            .iter()
            .enumerate()
            .map(|(index, bind)| {
                let backend = bind.backend.0.as_str();
                SelfDescriptionChannel {
                    from: format!("{}.{}", bind.from.instance.name, bind.from.port),
                    to: format!("{}.{}", bind.to.instance.name, bind.to.port),
                    message_type: channel_message_type(contract, graph, bind),
                    backend: backend.to_string(),
                    service: (backend == "iox2")
                        .then(|| crate::iox2_service_name_for_edge(contract, graph, index, bind)),
                    key_expr: (backend == "zenoh")
                        .then(|| crate::zenoh_key_expr_for_edge(contract, graph, index, bind)),
                    channel: channel_name(bind.channel),
                    depth: bind.depth,
                    overflow: overflow_name(bind.overflow),
                    stale_policy: stale_name(bind.stale),
                    max_age_ms: bind.max_age_ms,
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
                kind: "serial",
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
                trigger: trigger_name(task.trigger),
                readiness: readiness_name(task.readiness),
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
        encoding: "canonical_frame_v1",
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
            ty: param_type_name(param.ty),
            update: param_update_name(param.update),
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
