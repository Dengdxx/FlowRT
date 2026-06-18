use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    ChannelKind, ContractIr, GraphIr, InstanceIr, PortIr, Ros2BridgeDirection,
    StalePolicy as IrStalePolicy, TaskIr,
};

use crate::messages::rust_type;
use crate::runtime_plan::{
    BindRuntimePlan, BoundaryRuntimePlan, BridgeRuntimePlan, TaskEmissionPhase, bind_backend,
    indent_generated_block, indent_generated_block_levels, on_message_trigger_guard,
    resolved_task_lane_name,
};
use crate::{component_by_name, tasks_for_instance};

use super::{operation_emit, service_emit};

pub(super) struct RustStepEmission<'a> {
    pub contract: &'a ContractIr,
    pub graph: &'a GraphIr,
    pub binds: &'a [BindRuntimePlan],
    pub bridges: &'a [BridgeRuntimePlan],
    pub boundaries: &'a [BoundaryRuntimePlan],
    pub incoming_bind_index: &'a BTreeMap<(String, String), usize>,
    pub incoming_bridge_index: &'a BTreeMap<(String, String), usize>,
    pub incoming_boundary_index: &'a BTreeMap<(String, String), usize>,
    pub outgoing_bind_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    pub outgoing_bridge_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    pub outgoing_boundary_indices: &'a BTreeMap<(String, String), Vec<usize>>,
}

pub(super) fn rust_task_component_capture_name(instance: &InstanceIr) -> String {
    format!(
        "__flowrt_component_{}",
        crate::snake_identifier(&instance.name)
    )
}

pub(super) fn rust_task_params_capture_name(instance: &InstanceIr) -> String {
    format!(
        "__flowrt_params_{}",
        crate::snake_identifier(&instance.name)
    )
}

pub(super) fn rust_task_input_value_name(input: &str) -> String {
    format!("__flowrt_input_{}_value", crate::snake_identifier(input))
}

pub(super) fn rust_task_input_stale_name(input: &str) -> String {
    format!("__flowrt_input_{}_stale", crate::snake_identifier(input))
}

pub(super) fn rust_task_input_revision_name(input: &str) -> String {
    format!("__flowrt_input_{}_revision", crate::snake_identifier(input))
}

pub(super) fn rust_task_service_client_capture_name(
    plan: &crate::runtime_plan::ServiceRuntimePlan,
) -> String {
    format!(
        "__flowrt_{}",
        crate::snake_identifier(&service_emit::client_field_name(plan))
    )
}

pub(super) fn rust_task_operation_client_capture_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "__flowrt_{}",
        crate::snake_identifier(&operation_emit::operation_client_field_name(plan))
    )
}

fn rust_collect_task_parameters(emission: &RustStepEmission<'_>, task: &TaskIr) -> Vec<String> {
    let instance = crate::instance_by_name(emission.graph, &task.instance.name);
    let component = component_by_name(emission.contract, &instance.component.name);
    let mut params = Vec::new();
    params.push(format!(
        "{}: {}",
        rust_task_component_capture_name(instance),
        super::rust_component_storage_type(component)
    ));

    let service_plans =
        crate::runtime_plan::service_runtime_plans(emission.contract, emission.graph);
    for plan in crate::runtime_plan::client_service_plans(&service_plans, &instance.name) {
        params.push(format!(
            "{}: {}",
            rust_task_service_client_capture_name(plan),
            service_emit::client_handle_name(plan)
        ));
    }
    let operation_plans =
        crate::runtime_plan::operation_runtime_plans(emission.contract, emission.graph);
    for plan in crate::runtime_plan::client_operation_plans(&operation_plans, &instance.name) {
        params.push(format!(
            "{}: {}",
            rust_task_operation_client_capture_name(plan),
            operation_emit::operation_client_handle_name(plan)
        ));
    }
    if !component.params.is_empty() {
        params.push(format!(
            "{}: std::sync::Arc<std::sync::Mutex<{}Params>>",
            rust_task_params_capture_name(instance),
            crate::component_rust_name(component)
        ));
    }

    let effective_inputs = crate::runtime_plan::effective_task_inputs(emission.graph, task);
    let task_inputs = effective_inputs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for input in &component.inputs {
        if !task_inputs.contains(input.name.as_str()) {
            continue;
        }
        let ty = rust_type(&input.ty);
        params.push(format!(
            "{}: Option<{ty}>",
            rust_task_input_value_name(&input.name)
        ));
        params.push(format!("{}: bool", rust_task_input_stale_name(&input.name)));
        params.push(format!(
            "{}: u64",
            rust_task_input_revision_name(&input.name)
        ));
    }

    if task.trigger == flowrt_ir::TriggerKind::OnSynchronized {
        params.push(format!(
            "{}: {}",
            rust_synchronizer_field_name(task),
            rust_synchronizer_field_type()
        ));
    }

    if let Some(plan) = crate::runtime_plan::standby_failover_plan_for_instance_in_graph(
        emission.graph,
        &task.instance.name,
    ) {
        params.push(format!("{}: String", plan.active_field_name));
    }

    params
}

pub(super) fn emit_rust_app_step(
    emission: &RustStepEmission<'_>,
    order: &[&InstanceIr],
    function_name: &str,
    phase: TaskEmissionPhase,
    task_filter: Option<&TaskIr>,
) -> String {
    let mut output = String::new();
    let collect_outputs = phase == TaskEmissionPhase::Scheduler && task_filter.is_some();
    let return_type = if collect_outputs {
        "flowrt::TaskRunOutcome<Vec<FlowrtOutputCommit>>"
    } else {
        "flowrt::Status"
    };
    output.push_str("    #[allow(dead_code)]\n");
    output.push_str(&format!("    fn {function_name}(\n"));
    if collect_outputs {
        for param in rust_collect_task_parameters(emission, task_filter.expect("task exists")) {
            output.push_str(&format!("        {param},\n"));
        }
    } else {
        output.push_str("        &self,\n");
    }
    output.push_str(&format!(
        "        tick: usize,\n        _tick_context: &mut flowrt::Context,\n        introspection_state: &flowrt::IntrospectionState,\n        scheduler_events: &flowrt::ScheduleWaiter,\n        health_map: &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>,\n    ) -> {return_type} {{\n",
    ));
    output.push_str("        let _ = tick;\n");
    output.push_str("        let _ = introspection_state;\n");
    output.push_str("        let _ = scheduler_events;\n");
    output.push_str("        let _ = health_map;\n");
    if runtime_step_uses_tick_time(emission.binds, emission.bridges, emission.boundaries) {
        output.push_str("        let tick_time_ms = tick as u64;\n        let _ = tick_time_ms;\n");
    }
    if !collect_outputs {
        output.push_str(&emit_rust_ros2_boundary_input_pump(emission, order));
    }
    if collect_outputs {
        output.push_str(
            "        let mut __flowrt_output_commits: Vec<FlowrtOutputCommit> = Vec::new();\n",
        );
    }
    let standby_failover_plans =
        crate::runtime_plan::standby_failover_plans_for_order(emission.graph, order);

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
            let effective_inputs = crate::runtime_plan::effective_task_inputs(emission.graph, task);
            let task_inputs = effective_inputs
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let task_outputs = task
                .outputs
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let is_synchronized = task.trigger == flowrt_ir::TriggerKind::OnSynchronized;
            let non_consuming_read =
                task.trigger == flowrt_ir::TriggerKind::OnMessage || is_synchronized;
            let mut trigger_guard = on_message_trigger_guard(task, |input| input.to_string());

            for input in &component.inputs {
                if task_inputs.contains(input.name.as_str()) {
                    if let Some(bind_index) = emission
                        .incoming_bind_index
                        .get(&(instance.name.clone(), input.name.clone()))
                    {
                        let bind = &emission.binds[*bind_index];
                        let task_health = task_health_name(task);
                        if collect_outputs {
                            output.push_str(&indent_generated_block(
                                &rust_input_from_snapshot(input),
                                true,
                            ));
                        } else {
                            output.push_str(&indent_generated_block(
                                &super::backend_emit::runtime_channel_read(
                                    input,
                                    bind,
                                    non_consuming_read,
                                ),
                                true,
                            ));
                        }
                        output.push_str(&indent_generated_block(
                            &super::introspection_emit::rust_input_read_record_for_bind(
                                task, input, bind,
                            ),
                            true,
                        ));
                        // stale 健康计数在 error guard 之前记录，确保 Error policy 也能计数。
                        output.push_str(&indent_generated_block(
                            &runtime_stale_health_record(input, &task_health),
                            true,
                        ));
                        output.push_str(&indent_generated_block(
                            &adapt_rust_status_returns_for_collect(
                                &runtime_stale_error_guard(input, bind),
                                collect_outputs,
                            ),
                            true,
                        ));
                    } else if let Some(bridge_index) = emission
                        .incoming_bridge_index
                        .get(&(instance.name.clone(), input.name.clone()))
                    {
                        let bridge = &emission.bridges[*bridge_index];
                        if collect_outputs {
                            output.push_str(&indent_generated_block(
                                &rust_input_from_snapshot(input),
                                true,
                            ));
                        } else {
                            output.push_str(&indent_generated_block(
                                &super::backend_emit::bridge_runtime_channel_read(
                                    input,
                                    bridge,
                                    non_consuming_read,
                                ),
                                true,
                            ));
                        }
                        output.push_str(&indent_generated_block(
                            &super::introspection_emit::rust_input_read_record_for_bridge(
                                task, input, bridge,
                            ),
                            true,
                        ));
                    } else if let Some(boundary_index) = emission
                        .incoming_boundary_index
                        .get(&(instance.name.clone(), input.name.clone()))
                    {
                        let boundary = &emission.boundaries[*boundary_index];
                        if collect_outputs {
                            output.push_str(&indent_generated_block(
                                &rust_input_from_snapshot(input),
                                true,
                            ));
                        } else {
                            output.push_str(&indent_generated_block(
                                &rust_boundary_input_read(input, boundary),
                                true,
                            ));
                        }
                    } else {
                        output.push_str(&format!(
                            "            let {input} = flowrt::Latest::new(None, false);\n",
                            input = input.name
                        ));
                    }
                } else {
                    output.push_str(&format!(
                        "            let {input} = flowrt::Latest::new(None, false);\n",
                        input = input.name
                    ));
                }
            }

            // on_synchronized：把本 tick 各路 present 样本的 sample-time 推入同步器，
            // 仅在同步器发射出对齐集（poll 返回 Some）时执行回调；typed 样本由上面的
            // latest view 在发射点读取（latest-aligned -> 发射集即当前 latest）。
            if is_synchronized {
                let sync_handle = if collect_outputs {
                    rust_synchronizer_field_name(task)
                } else {
                    format!("self.{}", rust_synchronizer_field_name(task))
                };
                let lock_expr = format!(
                    "{sync_handle}.lock().unwrap_or_else(|poisoned| poisoned.into_inner())"
                );
                for (index, input_name) in effective_inputs.iter().enumerate() {
                    let Some(port) = component
                        .inputs
                        .iter()
                        .find(|port| port.name == *input_name)
                    else {
                        continue;
                    };
                    let Some((field, unit_to_ns)) =
                        crate::runtime_plan::boundary_sample_time_source(
                            emission.contract,
                            &port.ty,
                        )
                    else {
                        continue;
                    };
                    output.push_str(&format!(
                        "            if let Some(__flowrt_sync_sample) = {input}.as_ref() {{\n                {lock}.push({index}, (__flowrt_sync_sample.{field} as u64).saturating_mul({unit_to_ns}), ());\n            }}\n",
                        input = port.name,
                        lock = lock_expr,
                    ));
                }
                let ready_var = format!("{}_ready", rust_synchronizer_field_name(task));
                output.push_str(&format!(
                    "            let {ready_var} = {lock_expr}.poll().is_some();\n",
                ));
                trigger_guard = Some(ready_var);
            }

            // 初始化 health_map 条目的 name 和 lane 字段。
            let lane_name = task_lane_name(task);
            let task_health = task_health_name(task);
            output.push_str(&format!(
                "            {{\n                let __h = health_map.entry({task_health:?}.to_string()).or_default();\n                __h.name = {task_health:?}.to_string();\n                __h.lane = {lane_name:?}.to_string();\n            }}\n",
            ));

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

            output.push_str(&rust_lifecycle_fault_injection_guard(
                emission.contract,
                task,
                phase,
                body_indent,
                body_inner_indent,
            ));

            if task.deadline_ms.is_some() {
                output.push_str(&format!(
                    "{body_indent}let {name}_deadline_started_at = std::time::Instant::now();\n",
                    name = task_local_name(task)
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
                if collect_outputs {
                    call_args.push(format!("&{}", rust_task_service_client_capture_name(plan)));
                } else {
                    call_args.push(format!("&self.{}", service_emit::client_field_name(plan)));
                }
            }
            let operation_plans =
                crate::runtime_plan::operation_runtime_plans(emission.contract, emission.graph);
            for plan in
                crate::runtime_plan::client_operation_plans(&operation_plans, &instance.name)
            {
                if collect_outputs {
                    call_args.push(format!(
                        "&{}",
                        rust_task_operation_client_capture_name(plan)
                    ));
                } else {
                    call_args.push(format!(
                        "&self.{}",
                        operation_emit::operation_client_field_name(plan)
                    ));
                }
            }
            for input in &component.inputs {
                call_args.push(input.name.clone());
            }
            if !component.params.is_empty() {
                if collect_outputs {
                    call_args.push(format!(
                        "&{}.lock().unwrap_or_else(|poisoned| poisoned.into_inner())",
                        rust_task_params_capture_name(instance)
                    ));
                } else {
                    call_args.push(format!(
                        "&self.{}_params.lock().unwrap_or_else(|poisoned| poisoned.into_inner())",
                        instance.name
                    ));
                }
            }
            for port in &component.outputs {
                call_args.push(format!("&mut {}", port.name));
            }
            let method_call = format!("on_tick({})", call_args.join(", "));
            let on_tick_call = if collect_outputs {
                super::rust_component_method_call_for_receiver(
                    component,
                    &rust_task_component_capture_name(instance),
                    &method_call,
                )
            } else {
                super::rust_component_method_call(component, &instance.name, &method_call)
            };
            if collect_outputs {
                output.push_str(&format!(
                    "{body_indent}match {on_tick_call} {{\n{body_inner_indent}flowrt::Status::Ok => {{}}\n{body_inner_indent}flowrt::Status::Retry => return flowrt::TaskRunOutcome::retry(Vec::new()),\n{body_inner_indent}flowrt::Status::Error => return flowrt::TaskRunOutcome::error(Vec::new()),\n{body_indent}}}\n",
                ));
            } else {
                output.push_str(&format!(
                    "{body_indent}match {on_tick_call} {{\n{body_inner_indent}flowrt::Status::Ok => {{}}\n{body_inner_indent}flowrt::Status::Retry => return flowrt::Status::Retry,\n{body_inner_indent}flowrt::Status::Error => return flowrt::Status::Error,\n{body_indent}}}\n",
                ));
            }

            if let Some(deadline_ms) = task.deadline_ms {
                let task_local = task_local_name(task);
                let task_health = task_health_name(task);
                output.push_str(&format!(
                    "{body_indent}let {task_local}_deadline_exceeded = {task_local}_deadline_started_at.elapsed() > std::time::Duration::from_millis({deadline_ms});\n\
                     {body_indent}if {task_local}_deadline_exceeded {{\n\
                     {body_inner_indent}health_map.entry({task_health:?}.to_string()).or_default().deadline_missed += 1;\n\
                     {body_indent}}}\n",
                ));
            }

            // 在 deadline_exceeded 守卫下发布输出：deadline miss 时不发布 late output。
            let has_deadline = task.deadline_ms.is_some();
            if has_deadline {
                output.push_str(&format!(
                    "{body_indent}if !{name}_deadline_exceeded {{\n",
                    name = task_local_name(task)
                ));
            }
            for port in &component.outputs {
                if !task_outputs.contains(port.name.as_str()) {
                    continue;
                }
                let redundancy_plan = collect_outputs
                    .then(|| {
                        crate::runtime_plan::standby_failover_plan_for_instance(
                            &standby_failover_plans,
                            &instance.name,
                        )
                    })
                    .flatten();
                let logical_source = crate::runtime_plan::redundancy_logical_source_key(
                    redundancy_plan,
                    &instance.name,
                    &port.name,
                );
                let outgoing = emission
                    .outgoing_bind_indices
                    .get(&logical_source)
                    .cloned()
                    .unwrap_or_default();
                let bridge_outgoing = emission
                    .outgoing_bridge_indices
                    .get(&logical_source)
                    .cloned()
                    .unwrap_or_default();
                let boundary_outgoing = emission
                    .outgoing_boundary_indices
                    .get(&logical_source)
                    .cloned()
                    .unwrap_or_default();
                if outgoing.is_empty() && bridge_outgoing.is_empty() && boundary_outgoing.is_empty()
                {
                    continue;
                }
                let publish_indent = if has_deadline {
                    format!("{body_indent}    ")
                } else {
                    body_indent.to_string()
                };
                output.push_str(&format!(
                    "{publish_indent}if let Some(value) = {port}.as_ref().cloned() {{\n",
                    port = port.name
                ));
                let active_guard = redundancy_plan.map(|plan| {
                    format!(
                        "{} == {}",
                        plan.active_field_name,
                        crate::rust_string_literal(&instance.name)
                    )
                });
                if let Some(guard) = &active_guard {
                    output.push_str(&format!("{publish_indent}    if {guard} {{\n"));
                }
                for bind_index in outgoing {
                    let bind = &emission.binds[bind_index];
                    let task_health = task_health_name(task);
                    let write_code = if collect_outputs {
                        super::backend_emit::runtime_channel_commit_with_health(bind, &task_health)
                    } else {
                        super::backend_emit::runtime_channel_write_with_health(bind, &task_health)
                    };
                    output.push_str(&indent_generated_block_levels(
                        &write_code,
                        write_indent_levels
                            + if has_deadline { 1 } else { 0 }
                            + usize::from(active_guard.is_some()),
                    ));
                }
                for bridge_index in bridge_outgoing {
                    let bridge = &emission.bridges[bridge_index];
                    let write_code = if collect_outputs {
                        super::backend_emit::bridge_runtime_channel_commit(bridge)
                    } else {
                        super::backend_emit::bridge_runtime_channel_write(bridge)
                    };
                    output.push_str(&indent_generated_block_levels(
                        &write_code,
                        write_indent_levels
                            + if has_deadline { 1 } else { 0 }
                            + usize::from(active_guard.is_some()),
                    ));
                }
                for boundary_index in boundary_outgoing {
                    let boundary = &emission.boundaries[boundary_index];
                    let write_code = if collect_outputs {
                        rust_boundary_output_commit(boundary)
                    } else {
                        rust_boundary_output_write(boundary)
                    };
                    output.push_str(&indent_generated_block_levels(
                        &write_code,
                        write_indent_levels
                            + if has_deadline { 1 } else { 0 }
                            + usize::from(active_guard.is_some()),
                    ));
                }
                if active_guard.is_some() {
                    output.push_str(&format!("{publish_indent}    }}\n"));
                }
                output.push_str(&format!("{publish_indent}}}\n"));
            }
            if has_deadline {
                output.push_str(&format!("{body_indent}}}\n"));
            }

            if trigger_guard.is_some() {
                output.push_str("            }\n");
            }
            output.push_str("        }\n");
        }
    }

    if collect_outputs {
        output.push_str("        flowrt::TaskRunOutcome::ok(__flowrt_output_commits)\n    }\n");
    } else {
        output.push_str("        flowrt::Status::Ok\n    }\n");
    }
    output
}

fn rust_lifecycle_fault_injection_guard(
    contract: &ContractIr,
    task: &TaskIr,
    phase: TaskEmissionPhase,
    body_indent: &str,
    body_inner_indent: &str,
) -> String {
    let Some(point) = crate::runtime_plan::fault_injection_point_for(contract, task) else {
        return String::new();
    };
    let expected_kind = match phase {
        TaskEmissionPhase::Startup => flowrt_ir::FaultInjectionKind::StartupError,
        TaskEmissionPhase::Shutdown => flowrt_ir::FaultInjectionKind::ShutdownError,
        TaskEmissionPhase::Scheduler => return String::new(),
    };
    if point.kind != expected_kind {
        return String::new();
    }
    let hit = fault_injection_first_invocation_hit_expr(point);
    format!(
        "{body_indent}let __flowrt_inject_status_error = {hit};\n\
         {body_indent}if __flowrt_inject_status_error {{\n\
         {body_inner_indent}return flowrt::Status::Error;\n\
         {body_indent}}}\n"
    )
}

fn fault_injection_first_invocation_hit_expr(point: &flowrt_ir::FaultInjectionPointIr) -> String {
    let mut clauses = Vec::new();
    if !point.invocations.is_empty() {
        let list = point
            .invocations
            .iter()
            .map(|n| format!("{n}u64"))
            .collect::<Vec<_>>()
            .join(", ");
        clauses.push(format!("[{list}].contains(&1u64)"));
    }
    if let Some(from) = point.from_invocation {
        clauses.push(format!("1u64 >= {from}u64"));
    }
    clauses.join(" || ")
}

fn adapt_rust_status_returns_for_collect(code: &str, collect_outputs: bool) -> String {
    if !collect_outputs {
        return code.to_string();
    }
    code.replace(
        "return flowrt::Status::Error;",
        "return flowrt::TaskRunOutcome::error(Vec::new());",
    )
    .replace(
        "return flowrt::Status::Retry;",
        "return flowrt::TaskRunOutcome::retry(Vec::new());",
    )
}

pub(super) fn runtime_step_uses_tick_time(
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
) -> bool {
    if !bridges.is_empty() || !boundaries.is_empty() {
        return true;
    }
    binds
        .iter()
        .any(|bind| matches!(bind.channel, ChannelKind::Latest | ChannelKind::Fifo))
}

pub(super) fn emit_rust_on_message_revision_state(
    graph: &GraphIr,
    tasks: &[&TaskIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
) -> String {
    let mut output = String::new();
    for task in tasks.iter().copied().filter(|task| {
        matches!(
            task.trigger,
            flowrt_ir::TriggerKind::OnMessage | flowrt_ir::TriggerKind::OnSynchronized
        )
    }) {
        let inputs = crate::runtime_plan::effective_task_inputs(graph, task);
        for bind in input_binds_for_task(task, &inputs, binds) {
            output.push_str(&format!(
                "        let mut {seen}: u64 = 0;\n",
                seen = task_seen_revision_name(bind, task)
            ));
        }
        for bridge in input_bridges_for_task(task, &inputs, bridges) {
            output.push_str(&format!(
                "        let mut {seen}: u64 = 0;\n",
                seen = bridge_seen_revision_name(bridge, task)
            ));
        }
        for boundary in input_boundaries_for_task(task, &inputs, boundaries) {
            output.push_str(&format!(
                "        let mut {seen}: u64 = 0;\n",
                seen = boundary_seen_revision_name(boundary, task)
            ));
        }
    }
    output
}

pub(super) fn emit_rust_on_message_wake_checks(
    graph: &GraphIr,
    tasks: &[&TaskIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
) -> String {
    let mut output = String::new();
    for (index, task) in tasks.iter().enumerate() {
        if !matches!(
            task.trigger,
            flowrt_ir::TriggerKind::OnMessage | flowrt_ir::TriggerKind::OnSynchronized
        ) {
            continue;
        }
        let inputs = crate::runtime_plan::effective_task_inputs(graph, task);
        let input_binds = input_binds_for_task(task, &inputs, binds);
        let input_bridges = input_bridges_for_task(task, &inputs, bridges);
        let input_boundaries = input_boundaries_for_task(task, &inputs, boundaries);
        if input_binds.is_empty() && input_bridges.is_empty() && input_boundaries.is_empty() {
            continue;
        }
        for bind in &input_binds {
            if matches!(bind_backend(bind), "iox2" | "zenoh") {
                output.push_str(&format!(
                    "            let _ = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).receive_latest_at(tick_time_ms);\n",
                    field = bind.field_name
                ));
            }
        }
        for bridge in &input_bridges {
            output.push_str(&format!(
                "            let _ = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).receive_latest_at(tick_time_ms);\n",
                field = bridge.field_name
            ));
        }
        let mut checks = input_binds
            .iter()
            .map(|bind| {
                let revision_changed = format!(
                    "self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != {seen}",
                    field = bind.field_name,
                    seen = task_seen_revision_name(bind, task)
                );
                if bind.channel == ChannelKind::Fifo && bind_backend(bind) == "inproc" {
                    format!(
                        "({revision_changed} || !self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_empty())",
                        field = bind.field_name
                    )
                } else {
                    revision_changed
                }
            })
            .collect::<Vec<_>>();
        checks.extend(input_bridges.iter().map(|bridge| {
            format!(
                "self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != {seen}",
                field = bridge.field_name,
                seen = bridge_seen_revision_name(bridge, task)
            )
        }));
        checks.extend(input_boundaries.iter().map(|boundary| {
            format!(
                "self.{field}.revision() != {seen}",
                field = boundary.field_name,
                seen = boundary_seen_revision_name(boundary, task)
            )
        }));
        let joiner = match task.readiness {
            flowrt_ir::TaskReadiness::AnyReady => " || ",
            flowrt_ir::TaskReadiness::AllReady => " && ",
        };
        output.push_str(&format!("            if {} {{\n", checks.join(joiner)));
        for bind in &input_binds {
            output.push_str(&format!(
                "                {seen} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();\n",
                seen = task_seen_revision_name(bind, task),
                field = bind.field_name
            ));
        }
        for bridge in &input_bridges {
            output.push_str(&format!(
                "                {seen} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision();\n",
                seen = bridge_seen_revision_name(bridge, task),
                field = bridge.field_name
            ));
        }
        for boundary in &input_boundaries {
            output.push_str(&format!(
                "                {seen} = self.{field}.revision();\n",
                seen = boundary_seen_revision_name(boundary, task),
                field = boundary.field_name
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
    resolved_task_lane_name(task)
}

pub(super) fn task_health_name(task: &TaskIr) -> String {
    format!("{}.{}", task.instance.name, task.name)
}

fn task_local_name(task: &TaskIr) -> String {
    format!(
        "{}_{}",
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

/// on_synchronized task 的 App 同步器字段（与 worker 捕获本地名同名）。
pub(super) fn rust_synchronizer_field_name(task: &TaskIr) -> String {
    format!(
        "__flowrt_sync_{}_{}",
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

/// 同步器字段类型：runtime 提供的时间对齐原语，value 为 `()`（typed 样本由 latest
/// view 在发射点读取）。
pub(super) fn rust_synchronizer_field_type() -> &'static str {
    "std::sync::Arc<std::sync::Mutex<flowrt::synchronizer::Synchronizer<()>>>"
}

/// 同步器构造表达式：input 路数、buffer 容量、tolerance（ns）。
pub(super) fn rust_synchronizer_new_expr(graph: &GraphIr, task: &TaskIr) -> String {
    let group = crate::runtime_plan::sync_group_for_task(graph, task);
    let input_count = group.map(|group| group.inputs.len()).unwrap_or(0);
    let tolerance_ns = group
        .map(|group| group.tolerance_ms.saturating_mul(1_000_000))
        .unwrap_or(0);
    let capacity = SYNCHRONIZER_BUFFER_CAPACITY;
    format!(
        "std::sync::Arc::new(std::sync::Mutex::new(flowrt::synchronizer::Synchronizer::<()>::new({input_count}, {capacity}, {tolerance_ns})))",
    )
}

/// 同步器每路 buffer 默认容量。v1 固定值，足以吸收相近速率传感器的轻微抖动。
pub(super) const SYNCHRONIZER_BUFFER_CAPACITY: usize = 8;

/// 返回 `order` 内所有 on_synchronized task。
pub(super) fn on_synchronized_tasks<'a>(
    graph: &'a GraphIr,
    order: &[&'a InstanceIr],
) -> Vec<&'a TaskIr> {
    order
        .iter()
        .flat_map(|instance| crate::tasks_for_instance(graph, instance))
        .filter(|task| task.trigger == flowrt_ir::TriggerKind::OnSynchronized)
        .collect()
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

pub(super) fn bridge_seen_revision_name(bridge: &BridgeRuntimePlan, task: &TaskIr) -> String {
    format!(
        "{}_seen_revision_for_{}_{}",
        bridge.field_name,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

pub(super) fn boundary_seen_revision_name(boundary: &BoundaryRuntimePlan, task: &TaskIr) -> String {
    format!(
        "{}_seen_revision_for_{}_{}",
        boundary.field_name,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

pub(super) fn input_binds_for_task<'a>(
    task: &TaskIr,
    inputs: &[String],
    binds: &'a [BindRuntimePlan],
) -> Vec<&'a BindRuntimePlan> {
    inputs
        .iter()
        .filter_map(|input| {
            binds.iter().find(|bind| {
                bind.target_instance == task.instance.name && bind.target_port == *input
            })
        })
        .collect()
}

pub(super) fn input_bridges_for_task<'a>(
    task: &TaskIr,
    inputs: &[String],
    bridges: &'a [BridgeRuntimePlan],
) -> Vec<&'a BridgeRuntimePlan> {
    inputs
        .iter()
        .filter_map(|input| {
            bridges.iter().find(|bridge| {
                bridge.direction == Ros2BridgeDirection::Ros2ToFlowrt
                    && bridge.boundary_endpoint.is_none()
                    && bridge.source_instance == task.instance.name
                    && bridge.source_port == *input
            })
        })
        .collect()
}

pub(super) fn input_boundaries_for_task<'a>(
    task: &TaskIr,
    inputs: &[String],
    boundaries: &'a [BoundaryRuntimePlan],
) -> Vec<&'a BoundaryRuntimePlan> {
    inputs
        .iter()
        .filter_map(|input| {
            boundaries.iter().find(|boundary| {
                boundary.direction == flowrt_ir::BoundaryDirection::Input
                    && boundary.instance == task.instance.name
                    && boundary.port == *input
            })
        })
        .collect()
}

fn emit_rust_ros2_boundary_input_pump(
    emission: &RustStepEmission<'_>,
    order: &[&InstanceIr],
) -> String {
    let active_instances = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();
    let boundaries_by_name = emission
        .boundaries
        .iter()
        .map(|boundary| (boundary.endpoint_name.as_str(), boundary))
        .collect::<BTreeMap<_, _>>();
    let mut output = String::new();
    for bridge in emission.bridges.iter().filter(|bridge| {
        bridge.direction == Ros2BridgeDirection::Ros2ToFlowrt
            && active_instances.contains(bridge.source_instance.as_str())
    }) {
        let Some(endpoint_name) = bridge.boundary_endpoint.as_deref() else {
            continue;
        };
        let Some(boundary) = boundaries_by_name.get(endpoint_name).copied() else {
            continue;
        };
        output.push_str(&format!(
            "        let {bridge}_boundary_value = match self.{bridge}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).receive_latest_at(tick_time_ms) {{\n            Ok(value) => value.as_ref().cloned(),\n            Err(_) => return flowrt::Status::Error,\n        }};\n        if let Some(value) = {bridge}_boundary_value {{\n            self.{boundary}.inject_at(value, tick_time_ms);\n        }}\n",
            bridge = bridge.field_name,
            boundary = boundary.field_name,
        ));
    }
    output
}

fn rust_boundary_input_read(input: &PortIr, boundary: &BoundaryRuntimePlan) -> String {
    let revision = super::backend_emit::input_revision_local(input);
    format!(
        "        let {input}_read = self.{field}.read_at(tick_time_ms);\n        let {revision} = {input}_read.revision();\n        let {input} = {input}_read.view();\n",
        input = input.name,
        field = boundary.field_name,
        revision = revision,
    )
}

fn rust_input_from_snapshot(input: &PortIr) -> String {
    let revision = super::backend_emit::input_revision_local(input);
    format!(
        "        let {input} = flowrt::Latest::new({value}.as_ref(), {stale});\n        let {revision} = {snapshot_revision};\n",
        input = input.name,
        value = rust_task_input_value_name(&input.name),
        stale = rust_task_input_stale_name(&input.name),
        revision = revision,
        snapshot_revision = rust_task_input_revision_name(&input.name),
    )
}

fn rust_boundary_output_write(boundary: &BoundaryRuntimePlan) -> String {
    format!(
        "            self.{field}.publish_at(&value, tick_time_ms);\n",
        field = boundary.field_name,
    )
}

fn rust_boundary_output_commit(boundary: &BoundaryRuntimePlan) -> String {
    format!(
        "            let value = value.clone();\n            __flowrt_output_commits.push(Box::new(move |app, _introspection_state, _scheduler_events, _health_map| {{\n                app.{field}.publish_at(&value, tick_time_ms);\n                flowrt::Status::Ok\n            }}));\n",
        field = boundary.field_name,
    )
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

/// 生成 stale input 健康计数器记录代码。
///
/// 在 stale error guard 之后调用，记录所有 stale 检测到 health_map。
fn runtime_stale_health_record(input: &PortIr, task_health_name: &str) -> String {
    format!(
        "        if {input}.stale() {{\n            health_map.entry({task_health:?}.to_string()).or_default().stale_input += 1;\n        }}\n",
        input = input.name,
        task_health = task_health_name,
    )
}
