use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    BackendName, ChannelKind, ContractIr, GraphIr, InstanceIr, OverflowPolicy as IrOverflowPolicy,
    ParamIr, StalePolicy as IrStalePolicy, TaskIr, TriggerKind, TypeExpr,
};

use crate::{
    component_by_name, fixed_message_abi_expectations, frame_max_size_for_type, instance_by_name,
    port_by_name, rust_wire_size, snake_identifier, type_by_name, type_contains_variable_data,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskEmissionPhase {
    Scheduler,
    Startup,
    Shutdown,
}

impl TaskEmissionPhase {
    pub(crate) fn includes(self, trigger: TriggerKind) -> bool {
        match self {
            TaskEmissionPhase::Scheduler => {
                matches!(trigger, TriggerKind::Periodic | TriggerKind::OnMessage)
            }
            TaskEmissionPhase::Startup => trigger == TriggerKind::Startup,
            TaskEmissionPhase::Shutdown => trigger == TriggerKind::Shutdown,
        }
    }
}

pub(crate) fn on_message_trigger_guard<F>(task: &TaskIr, input_name: F) -> Option<String>
where
    F: Fn(&str) -> String,
{
    if task.trigger != TriggerKind::OnMessage || task.inputs.is_empty() {
        return None;
    }

    Some(
        task.inputs
            .iter()
            .map(|input| format!("{}.present()", input_name(input)))
            .collect::<Vec<_>>()
            .join(" || "),
    )
}

pub(crate) fn indent_generated_block(block: &str, nested: bool) -> String {
    if !nested {
        return block.to_string();
    }

    indent_generated_block_levels(block, 1)
}

pub(crate) fn indent_generated_block_levels(block: &str, levels: usize) -> String {
    if block.is_empty() {
        return String::new();
    }

    if levels == 0 {
        return block.to_string();
    }

    let prefix = "    ".repeat(levels);
    block
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

pub(crate) fn step_indent(nested: bool) -> &'static str {
    if nested { "        " } else { "    " }
}

pub(crate) fn nested_step_indent(nested: bool) -> &'static str {
    if nested { "            " } else { "        " }
}

pub(crate) fn rust_step_indent(nested: bool) -> &'static str {
    if nested { "            " } else { "        " }
}

pub(crate) fn rust_nested_step_indent(nested: bool) -> &'static str {
    if nested {
        "                "
    } else {
        "            "
    }
}

#[derive(Debug, Clone)]
pub(crate) struct BindRuntimePlan {
    pub(crate) index: usize,
    pub(crate) field_name: String,
    pub(crate) probe_field_name: String,
    pub(crate) channel: ChannelKind,
    pub(crate) backend: BackendName,
    pub(crate) overflow: IrOverflowPolicy,
    pub(crate) stale: IrStalePolicy,
    pub(crate) max_age_ms: Option<u64>,
    pub(crate) depth: Option<u32>,
    pub(crate) source_type: TypeExpr,
    pub(crate) source_uses_variable_frame: bool,
    pub(crate) source_instance: String,
    pub(crate) source_port: String,
    pub(crate) target_instance: String,
    pub(crate) target_port: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessRuntimePlan<'a> {
    pub(crate) name: String,
    pub(crate) method_suffix: String,
    pub(crate) instances: Vec<&'a InstanceIr>,
}

pub(crate) fn process_runtime_plans<'a>(order: &[&'a InstanceIr]) -> Vec<ProcessRuntimePlan<'a>> {
    let mut by_process = BTreeMap::<String, Vec<&'a InstanceIr>>::new();
    for &instance in order {
        by_process
            .entry(
                instance
                    .process
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
            )
            .or_default()
            .push(instance);
    }

    let mut used_suffixes = BTreeSet::new();
    by_process
        .into_iter()
        .enumerate()
        .map(|(index, (name, instances))| {
            let base = snake_identifier(&name);
            let mut suffix = base.clone();
            if !used_suffixes.insert(suffix.clone()) {
                suffix = format!("{}_{}", base, index);
                while !used_suffixes.insert(suffix.clone()) {
                    suffix.push('_');
                }
            }
            ProcessRuntimePlan {
                name,
                method_suffix: suffix,
                instances,
            }
        })
        .collect()
}

pub(crate) fn bind_runtime_plans(contract: &ContractIr, graph: &GraphIr) -> Vec<BindRuntimePlan> {
    graph
        .binds
        .iter()
        .enumerate()
        .map(|(index, bind)| {
            let source_instance = instance_by_name(graph, &bind.from.instance.name);
            let source_component = component_by_name(contract, &source_instance.component.name);
            let source_port = port_by_name(&source_component.outputs, &bind.from.port);
            BindRuntimePlan {
                index,
                field_name: format!("bind_{index}"),
                probe_field_name: format!("introspection_probe_bind_{index}"),
                channel: bind.channel,
                backend: bind.backend.clone(),
                overflow: bind.overflow,
                stale: bind.stale,
                max_age_ms: bind.max_age_ms,
                depth: bind.depth,
                source_type: source_port.ty.clone(),
                source_uses_variable_frame: type_contains_variable_data(contract, &source_port.ty),
                source_instance: source_instance.name.clone(),
                source_port: bind.from.port.clone(),
                target_instance: bind.to.instance.name.clone(),
                target_port: bind.to.port.clone(),
            }
        })
        .collect()
}

pub(crate) fn incoming_bind_index_map(
    plans: &[BindRuntimePlan],
) -> BTreeMap<(String, String), usize> {
    plans
        .iter()
        .map(|plan| {
            (
                (plan.target_instance.clone(), plan.target_port.clone()),
                plan.index,
            )
        })
        .collect()
}

pub(crate) fn outgoing_bind_indices_map(
    plans: &[BindRuntimePlan],
) -> BTreeMap<(String, String), Vec<usize>> {
    let mut map = BTreeMap::new();
    for plan in plans {
        map.entry((plan.source_instance.clone(), plan.source_port.clone()))
            .or_insert_with(Vec::new)
            .push(plan.index);
    }
    map
}

pub(crate) fn active_binds_for_instances<'a>(
    binds: &'a [BindRuntimePlan],
    order: &[&InstanceIr],
) -> Vec<&'a BindRuntimePlan> {
    let active_instances = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();
    binds
        .iter()
        .filter(|bind| {
            active_instances.contains(bind.source_instance.as_str())
                || active_instances.contains(bind.target_instance.as_str())
        })
        .collect()
}

pub(crate) fn bind_backend(bind: &BindRuntimePlan) -> &str {
    bind.backend.0.as_str()
}

pub(crate) fn contract_uses_backend(contract: &ContractIr, backend: &str) -> bool {
    contract
        .profiles
        .iter()
        .any(|profile| profile.backend.0 == backend)
        || contract
            .graphs
            .iter()
            .flat_map(|graph| &graph.binds)
            .any(|bind| bind.backend.0 == backend)
}

pub(crate) fn contract_backend_features(contract: &ContractIr) -> Vec<&'static str> {
    let mut features = Vec::new();
    if contract_uses_backend(contract, "iox2") {
        features.push("iox2");
    }
    if contract_uses_backend(contract, "zenoh") {
        features.push("zenoh");
    }
    features
}

pub(crate) fn runtime_channel_name(bind: &BindRuntimePlan) -> String {
    format!(
        "{}.{}_to_{}.{}",
        bind.source_instance, bind.source_port, bind.target_instance, bind.target_port
    )
}

pub(crate) fn runtime_channel_message_type(bind: &BindRuntimePlan) -> String {
    bind.source_type.canonical_syntax()
}

pub(crate) fn runtime_channel_probe_capacity(
    contract: &ContractIr,
    bind: &BindRuntimePlan,
) -> usize {
    match &bind.source_type {
        TypeExpr::Named { name } if bind.source_uses_variable_frame => {
            frame_max_size_for_type(contract, type_by_name(contract, name))
        }
        TypeExpr::Named { name } => fixed_message_abi_size(contract, name)
            .unwrap_or_else(|| rust_wire_size(contract, &bind.source_type)),
        other => rust_wire_size(contract, other),
    }
}

fn fixed_message_abi_size(contract: &ContractIr, type_name: &str) -> Option<usize> {
    fixed_message_abi_expectations(contract)
        .ok()?
        .into_iter()
        .find(|expectation| expectation.type_name == type_name)
        .map(|expectation| expectation.size_bytes)
}

pub(crate) fn runtime_param_name(instance: &InstanceIr, param: &ParamIr) -> String {
    format!("{}.{}", instance.name, param.name)
}
