use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    ChannelKind, ContractIr, GraphIr, InstanceIr, PortIr, StalePolicy as IrStalePolicy, TaskIr,
};

use crate::messages::rust_type;
use crate::runtime_plan::{
    BindRuntimePlan, BridgeRuntimePlan, TaskEmissionPhase, bind_backend, indent_generated_block,
    indent_generated_block_levels, on_message_trigger_guard,
};
use crate::{component_by_name, tasks_for_instance};

use super::service_emit;

pub(super) struct RustStepEmission<'a> {
    pub contract: &'a ContractIr,
    pub graph: &'a GraphIr,
    pub binds: &'a [BindRuntimePlan],
    pub bridges: &'a [BridgeRuntimePlan],
    pub incoming_bind_index: &'a BTreeMap<(String, String), usize>,
    pub outgoing_bind_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    pub outgoing_bridge_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    /// 需要 Arc<Mutex<...>> 存储的 service server 实例名集合。
    pub service_server_instances: &'a std::collections::BTreeSet<String>,
}

pub(super) fn emit_rust_app_step(
    emission: &RustStepEmission<'_>,
    order: &[&InstanceIr],
    function_name: &str,
    phase: TaskEmissionPhase,
    task_filter: Option<&TaskIr>,
) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "    #[allow(dead_code)]\n    fn {function_name}(\n        &mut self,\n        tick: usize,\n        _tick_context: &mut flowrt::Context,\n        introspection_state: &flowrt::IntrospectionState,\n        scheduler_events: &flowrt::ScheduleWaiter,\n    ) -> flowrt::Status {{\n",
    ));
    output.push_str("        let _ = tick;\n");
    output.push_str("        let _ = introspection_state;\n");
    output.push_str("        let _ = scheduler_events;\n");
    if runtime_step_uses_tick_time(emission.binds, emission.bridges) {
        output.push_str("        let tick_time_ms = tick as u64;\n        let _ = tick_time_ms;\n");
    }

    for instance in order {
        let component = component_by_name(emission.contract, &instance.component.name);
        if task_filter.is_none()
            && !component.params.is_empty()
            && phase == TaskEmissionPhase::Scheduler
        {
            output.push_str(&super::params_emit::rust_apply_pending_params(
                instance,
                component,
                false,
                "_tick_context",
            ));
        }
        for task in tasks_for_instance(emission.graph, instance) {
            if !phase.includes(task.trigger) {
                continue;
            }
            if task_filter.is_some_and(|filter| filter.id != task.id) {
                continue;
            }
            output.push_str("        {\n");
            let task_inputs = task
                .inputs
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let task_outputs = task
                .outputs
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let trigger_guard = on_message_trigger_guard(task, |input| input.to_string());

            for input in &component.inputs {
                if task_inputs.contains(input.name.as_str()) {
                    let bind_index = emission
                        .incoming_bind_index
                        .get(&(instance.name.clone(), input.name.clone()))
                        .expect("validated graph must provide a bind for each task input");
                    let bind = &emission.binds[*bind_index];
                    output.push_str(&indent_generated_block(
                        &super::backend_emit::runtime_channel_read(
                            input,
                            bind,
                            task.trigger == flowrt_ir::TriggerKind::OnMessage,
                        ),
                        true,
                    ));
                    output.push_str(&indent_generated_block(
                        &runtime_stale_error_guard(input, bind),
                        true,
                    ));
                } else {
                    output.push_str(&format!(
                        "            let {input} = flowrt::Latest::new(None, false);\n",
                        input = input.name
                    ));
                }
            }

            if let Some(guard) = &trigger_guard {
                output.push_str(&format!("            if {guard} {{\n"));
            }
            let body_indent = if trigger_guard.is_some() {
                "                "
            } else {
                "            "
            };
            let body_inner_indent = if trigger_guard.is_some() {
                "                    "
            } else {
                "                "
            };
            let write_indent_levels = if trigger_guard.is_some() { 2 } else { 1 };

            if task.deadline_ms.is_some() {
                output.push_str(&format!(
                    "{body_indent}let {name}_deadline_started_at = std::time::Instant::now();\n",
                    name = instance.name
                ));
            }

            for port in &component.outputs {
                output.push_str(&format!(
                    "{body_indent}let mut {port} = flowrt::Output::<{ty}>::new();\n",
                    port = port.name,
                    ty = rust_type(&port.ty)
                ));
            }

            let mut call_args = Vec::new();
            let service_plans =
                crate::runtime_plan::service_runtime_plans(emission.contract, emission.graph);
            for plan in crate::runtime_plan::client_service_plans(&service_plans, &instance.name) {
                call_args.push(format!("&self.{}", service_emit::client_field_name(plan)));
            }
            for input in &component.inputs {
                call_args.push(input.name.clone());
            }
            if !component.params.is_empty() {
                call_args.push(format!("&self.{}_params", instance.name));
            }
            for port in &component.outputs {
                call_args.push(format!("&mut {}", port.name));
            }
            let on_tick_call = if emission.service_server_instances.contains(&instance.name) {
                format!(
                    "self.{name}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick({args})",
                    name = instance.name,
                    args = call_args.join(", ")
                )
            } else {
                format!(
                    "self.{name}.on_tick({args})",
                    name = instance.name,
                    args = call_args.join(", ")
                )
            };
            output.push_str(&format!(
                "{body_indent}match {on_tick_call} {{\n{body_inner_indent}flowrt::Status::Ok => {{}}\n{body_inner_indent}flowrt::Status::Retry => return flowrt::Status::Retry,\n{body_inner_indent}flowrt::Status::Error => return flowrt::Status::Error,\n{body_indent}}}\n",
            ));

            if let Some(deadline_ms) = task.deadline_ms {
                output.push_str(&format!(
                    "{body_indent}if {name}_deadline_started_at.elapsed() > std::time::Duration::from_millis({deadline_ms}) {{\n{body_inner_indent}return flowrt::Status::Error;\n{body_indent}}}\n",
                    name = instance.name
                ));
            }

            for port in &component.outputs {
                if !task_outputs.contains(port.name.as_str()) {
                    continue;
                }
                let outgoing = emission
                    .outgoing_bind_indices
                    .get(&(instance.name.clone(), port.name.clone()))
                    .cloned()
                    .unwrap_or_default();
                let bridge_outgoing = emission
                    .outgoing_bridge_indices
                    .get(&(instance.name.clone(), port.name.clone()))
                    .cloned()
                    .unwrap_or_default();
                if outgoing.is_empty() && bridge_outgoing.is_empty() {
                    continue;
                }
                output.push_str(&format!(
                    "{body_indent}if let Some(value) = {port}.as_ref().cloned() {{\n",
                    port = port.name
                ));
                for bind_index in outgoing {
                    let bind = &emission.binds[bind_index];
                    output.push_str(&indent_generated_block_levels(
                        &super::backend_emit::runtime_channel_write(bind),
                        write_indent_levels,
                    ));
                }
                for bridge_index in bridge_outgoing {
                    let bridge = &emission.bridges[bridge_index];
                    output.push_str(&indent_generated_block_levels(
                        &super::backend_emit::bridge_runtime_channel_write(bridge),
                        write_indent_levels,
                    ));
                }
                output.push_str(&format!("{body_indent}}}\n"));
            }

            if trigger_guard.is_some() {
                output.push_str("            }\n");
            }
            output.push_str("        }\n");
        }
    }

    output.push_str("        flowrt::Status::Ok\n    }\n");
    output
}

pub(super) fn runtime_step_uses_tick_time(
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
) -> bool {
    if !bridges.is_empty() {
        return true;
    }
    binds
        .iter()
        .any(|bind| matches!(bind.channel, ChannelKind::Latest | ChannelKind::Fifo))
}

pub(super) fn emit_rust_on_message_revision_state(
    tasks: &[&TaskIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    for task in tasks
        .iter()
        .copied()
        .filter(|task| task.trigger == flowrt_ir::TriggerKind::OnMessage)
    {
        for bind in input_binds_for_task(task, binds) {
            output.push_str(&format!(
                "        let mut {seen}: u64 = 0;\n",
                seen = task_seen_revision_name(bind, task)
            ));
        }
    }
    output
}

pub(super) fn emit_rust_on_message_wake_checks(
    tasks: &[&TaskIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    for (index, task) in tasks.iter().enumerate() {
        if task.trigger != flowrt_ir::TriggerKind::OnMessage {
            continue;
        }
        let input_binds = input_binds_for_task(task, binds);
        if input_binds.is_empty() {
            continue;
        }
        for bind in &input_binds {
            if matches!(bind_backend(bind), "iox2" | "zenoh") {
                output.push_str(&format!(
                    "            let _ = self.{field}.receive_latest_at(tick_time_ms);\n",
                    field = bind.field_name
                ));
            }
        }
        let checks = input_binds
            .iter()
            .map(|bind| {
                let revision_changed = format!(
                    "self.{field}.revision() != {seen}",
                    field = bind.field_name,
                    seen = task_seen_revision_name(bind, task)
                );
                if bind.channel == ChannelKind::Fifo && bind_backend(bind) == "inproc" {
                    format!(
                        "({revision_changed} || !self.{field}.is_empty())",
                        field = bind.field_name
                    )
                } else {
                    revision_changed
                }
            })
            .collect::<Vec<_>>();
        let joiner = match task.readiness {
            flowrt_ir::TaskReadiness::AnyReady => " || ",
            flowrt_ir::TaskReadiness::AllReady => " && ",
        };
        output.push_str(&format!("            if {} {{\n", checks.join(joiner)));
        for bind in &input_binds {
            output.push_str(&format!(
                "                {seen} = self.{field}.revision();\n",
                seen = task_seen_revision_name(bind, task),
                field = bind.field_name
            ));
        }
        output.push_str(&format!(
            "                scheduler.wake(flowrt::TaskId({}));\n                woke_on_message = true;\n            }}\n",
            index + 1
        ));
    }
    output
}

pub(super) fn emit_rust_apply_pending_params_for_order(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if !component.params.is_empty() {
            output.push_str(&super::params_emit::rust_apply_pending_params(
                instance,
                component,
                false,
                "&mut lifecycle_context",
            ));
        }
    }
    output
}

pub(super) fn task_lane_name(task: &TaskIr) -> String {
    task.lane
        .clone()
        .unwrap_or_else(|| format!("{}_serial", task.instance.name))
}

pub(super) fn scheduler_lane_ids(tasks: &[&TaskIr]) -> std::collections::BTreeMap<String, usize> {
    let mut lanes = std::collections::BTreeMap::new();
    for task in tasks {
        let lane = task_lane_name(task);
        if !lanes.contains_key(&lane) {
            let next_id = lanes.len() + 1;
            lanes.insert(lane, next_id);
        }
    }
    lanes
}

pub(super) fn task_seen_revision_name(bind: &BindRuntimePlan, task: &TaskIr) -> String {
    format!(
        "{}_seen_revision_for_{}_{}",
        bind.field_name,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

pub(super) fn input_binds_for_task<'a>(
    task: &TaskIr,
    binds: &'a [BindRuntimePlan],
) -> Vec<&'a BindRuntimePlan> {
    task.inputs
        .iter()
        .filter_map(|input| {
            binds.iter().find(|bind| {
                bind.target_instance == task.instance.name && bind.target_port == *input
            })
        })
        .collect()
}

fn runtime_stale_error_guard(input: &PortIr, bind: &BindRuntimePlan) -> String {
    if bind.stale != IrStalePolicy::Error {
        return String::new();
    }

    format!(
        "        if {input}.stale() {{\n            return flowrt::Status::Error;\n        }}\n",
        input = input.name
    )
}
