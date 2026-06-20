use super::*;

pub(super) struct CppStepEmission<'a> {
    pub(super) contract: &'a ContractIr,
    pub(super) graph: &'a GraphIr,
    pub(super) binds: &'a [BindRuntimePlan],
    pub(super) bridges: &'a [BridgeRuntimePlan],
    pub(super) boundaries: &'a [BoundaryRuntimePlan],
    pub(super) incoming_bind_index: &'a BTreeMap<(String, String), usize>,
    pub(super) incoming_bridge_index: &'a BTreeMap<(String, String), usize>,
    pub(super) incoming_boundary_index: &'a BTreeMap<(String, String), usize>,
    pub(super) outgoing_bind_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    pub(super) outgoing_bridge_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    pub(super) outgoing_boundary_indices: &'a BTreeMap<(String, String), Vec<usize>>,
}

pub(super) fn emit_cpp_app_step(
    emission: &CppStepEmission<'_>,
    order: &[&InstanceIr],
    function_name: &str,
    phase: TaskEmissionPhase,
    task_filter: Option<&flowrt_ir::TaskIr>,
) -> String {
    let mut output = String::new();
    let collect_outputs = phase == TaskEmissionPhase::Scheduler && task_filter.is_some();
    let return_type = if collect_outputs {
        "FlowrtTaskOutcome"
    } else {
        "flowrt::Status"
    };
    let task_active_param = task_filter
        .filter(|_| collect_outputs)
        .and_then(|task| cpp_redundancy_active_param_decl(emission.graph, task));
    let task_injection_param = task_filter
        .filter(|_| collect_outputs)
        .and_then(|task| cpp_fault_injection_task_parameter(emission.contract, task));
    output.push_str(&format!(
        "{return_type} App::{function_name}(std::size_t tick{task_active_param}{task_injection_param}, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {{\n",
        task_active_param = task_active_param
            .map(|param| format!(", {param}"))
            .unwrap_or_default(),
        task_injection_param = task_injection_param
            .map(|param| format!(", {param}"))
            .unwrap_or_default(),
    ));
    if cpp_runtime_step_uses_tick_time(emission.binds, emission.bridges, emission.boundaries)
        || order.iter().any(|instance| {
            component_by_name(emission.contract, &instance.component.name).language
                == LanguageKind::C
        })
    {
        output.push_str(
            "    const auto tick_time_ms = static_cast<std::uint64_t>(tick);\n    (void)tick_time_ms;\n",
        );
    } else {
        output.push_str("    (void)tick;\n");
    }
    output.push_str("    (void)tick_context;\n");
    output.push_str("    (void)introspection_state;\n");
    output.push_str("    (void)scheduler_events;\n");
    output.push_str("    (void)health_map;\n");
    if collect_outputs {
        if let Some(task) = task_filter {
            output.push_str(&cpp_backend_drop_fault_injection_guard(emission, task));
        }
    }
    output.push_str(&adapt_cpp_status_returns_for_collect(
        &emit_cpp_ros2_boundary_input_pump(emission, order),
        collect_outputs,
    ));
    if collect_outputs {
        output.push_str("    std::vector<FlowrtOutputCommit> flowrt_output_commits;\n");
    }
    let standby_failover_plans =
        crate::runtime_plan::standby_failover_plans_for_order(emission.graph, order);

    for instance in order {
        let component = component_by_name(emission.contract, &instance.component.name);
        if task_filter.is_none()
            && !component.params.is_empty()
            && phase == TaskEmissionPhase::Scheduler
        {
            output.push_str(&cpp_apply_pending_params(
                instance,
                component,
                false,
                "tick_context",
            ));
        }
        for task in tasks_for_instance(emission.graph, instance) {
            if !phase.includes(task.trigger) {
                continue;
            }
            if task_filter.is_some_and(|filter| filter.id != task.id) {
                continue;
            }
            output.push_str("    {\n");
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
            let mut trigger_guard =
                on_message_trigger_guard(task, |input| cpp_step_local_name(&instance.name, input));

            for input in &component.inputs {
                let input_local = cpp_step_local_name(&instance.name, &input.name);
                if task_inputs.contains(input.name.as_str()) {
                    if let Some(bind_index) = emission
                        .incoming_bind_index
                        .get(&(instance.name.clone(), input.name.clone()))
                    {
                        let bind = &emission.binds[*bind_index];
                        let task_health = cpp_task_health_name(task);
                        output.push_str(&indent_generated_block(
                            &adapt_cpp_status_returns_for_collect(
                                &cpp_runtime_channel_read(
                                    input,
                                    bind,
                                    &input_local,
                                    non_consuming_read,
                                ),
                                collect_outputs,
                            ),
                            true,
                        ));
                        // stale 健康计数在 error guard 之前记录，确保 Error policy 也能计数。
                        output.push_str(&indent_generated_block(
                            &cpp_runtime_stale_health_record(&input_local, &task_health),
                            true,
                        ));
                        output.push_str(&indent_generated_block(
                            &adapt_cpp_status_returns_for_collect(
                                &cpp_runtime_stale_error_guard(&input_local, bind),
                                collect_outputs,
                            ),
                            true,
                        ));
                    } else if let Some(bridge_index) = emission
                        .incoming_bridge_index
                        .get(&(instance.name.clone(), input.name.clone()))
                    {
                        let bridge = &emission.bridges[*bridge_index];
                        output.push_str(&indent_generated_block(
                            &adapt_cpp_status_returns_for_collect(
                                &cpp_bridge_runtime_channel_read(
                                    input,
                                    bridge,
                                    &input_local,
                                    non_consuming_read,
                                ),
                                collect_outputs,
                            ),
                            true,
                        ));
                    } else if let Some(boundary_index) = emission
                        .incoming_boundary_index
                        .get(&(instance.name.clone(), input.name.clone()))
                    {
                        let boundary = &emission.boundaries[*boundary_index];
                        output.push_str(&indent_generated_block(
                            &cpp_boundary_input_read(boundary, &input_local),
                            true,
                        ));
                    } else {
                        output.push_str(&format!(
                            "        flowrt::Latest<{ty}> {local};\n",
                            ty = cpp_type(&input.ty),
                            local = input_local
                        ));
                    }
                } else {
                    output.push_str(&format!(
                        "        flowrt::Latest<{ty}> {local};\n",
                        ty = cpp_type(&input.ty),
                        local = input_local
                    ));
                }
            }

            // on_synchronized：把本 tick 各路 present 样本的 sample-time 推入同步器成员，
            // 仅在同步器发射出对齐集（poll 有值）时执行回调；typed 样本由上面的 latest
            // view 在发射点读取（latest-aligned -> 发射集即当前 latest）。
            if is_synchronized {
                let sync_field = cpp_synchronizer_field_name(task);
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
                    let local = cpp_step_local_name(&instance.name, &port.name);
                    output.push_str(&format!(
                        "        if (const auto* __flowrt_sync_sample = {local}.get()) {{\n            {sync_field}_.push({index}, static_cast<std::uint64_t>(__flowrt_sync_sample->{field}) * {unit_to_ns}ULL, 0);\n        }}\n",
                    ));
                }
                let ready_var = format!("{sync_field}_ready");
                output.push_str(&format!(
                    "        const bool {ready_var} = {sync_field}_.poll().has_value();\n",
                ));
                trigger_guard = Some(ready_var);
            }

            // 初始化 health_map 条目的 name 和 lane 字段。
            let lane_name = cpp_task_lane_name(task);
            let task_health = cpp_task_health_name(task);
            output.push_str(&format!(
                "        health_map[\"{task_health}\"].name = \"{task_health}\";\n        health_map[\"{task_health}\"].lane = \"{lane_name}\";\n",
            ));

            if let Some(guard) = &trigger_guard {
                output.push_str(&format!("        if ({guard}) {{\n"));
            }
            let body_indent = if trigger_guard.is_some() {
                "            "
            } else {
                "        "
            };
            let body_inner_indent = if trigger_guard.is_some() {
                "                "
            } else {
                "            "
            };
            let write_indent_levels = if trigger_guard.is_some() { 2 } else { 1 };

            output.push_str(&cpp_lifecycle_fault_injection_guard(
                emission.contract,
                task,
                phase,
                body_indent,
                body_inner_indent,
            ));

            if task.deadline_ms.is_some() {
                let task_local = cpp_task_local_name(task);
                output.push_str(&format!(
                    "{body_indent}const auto {task_local}_deadline_started_at = std::chrono::steady_clock::now();\n"
                ));
            }

            for port in &component.outputs {
                let output_local = cpp_step_local_name(&instance.name, &port.name);
                output.push_str(&format!(
                    "{body_indent}flowrt::Output<{ty}> {local};\n",
                    ty = cpp_type(&port.ty),
                    local = output_local
                ));
            }

            let mut call_args = Vec::new();
            let service_plans =
                crate::runtime_plan::service_runtime_plans(emission.contract, emission.graph);
            for plan in crate::runtime_plan::client_service_plans(&service_plans, &instance.name) {
                call_args.push(format!("{}_", cpp_service_client_field_name(plan)));
            }
            let operation_plans =
                crate::runtime_plan::operation_runtime_plans(emission.contract, emission.graph);
            for plan in
                crate::runtime_plan::client_operation_plans(&operation_plans, &instance.name)
            {
                call_args.push(format!("{}_", cpp_operation_client_field_name(plan)));
            }
            for input in &component.inputs {
                call_args.push(cpp_step_local_name(&instance.name, &input.name));
            }
            if !component.params.is_empty() {
                call_args.push(format!("{}_params_", instance.name));
            }
            for port in &component.outputs {
                call_args.push(cpp_step_local_name(&instance.name, &port.name));
            }
            if component.language == LanguageKind::C {
                output.push_str(&emit_c_adapter_task_step_call(
                    emission,
                    instance,
                    component,
                    task,
                    collect_outputs,
                    body_indent,
                    body_inner_indent,
                ));
            } else if collect_outputs {
                output.push_str(&format!(
                    "{body_indent}if ({instance}_) {{\n{body_inner_indent}switch ({instance}_->on_tick({args})) {{\n{body_inner_indent}    case flowrt::Status::Ok:\n{body_inner_indent}        break;\n{body_inner_indent}    case flowrt::Status::Retry:\n{body_inner_indent}        return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{{}});\n{body_inner_indent}    case flowrt::Status::Error:\n{body_inner_indent}        return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{{}});\n{body_inner_indent}}}\n{body_indent}}}\n",
                    instance = instance.name,
                    args = call_args.join(", ")
                ));
            } else {
                output.push_str(&format!(
                    "{body_indent}if ({instance}_) {{\n{body_inner_indent}switch ({instance}_->on_tick({args})) {{\n{body_inner_indent}    case flowrt::Status::Ok:\n{body_inner_indent}        break;\n{body_inner_indent}    case flowrt::Status::Retry:\n{body_inner_indent}        return flowrt::Status::Retry;\n{body_inner_indent}    case flowrt::Status::Error:\n{body_inner_indent}        return flowrt::Status::Error;\n{body_inner_indent}}}\n{body_indent}}}\n",
                    instance = instance.name,
                    args = call_args.join(", ")
                ));
            }

            if let Some(deadline_ms) = task.deadline_ms {
                let task_local = cpp_task_local_name(task);
                let task_health = cpp_task_health_name(task);
                let deadline_expr =
                    cpp_deadline_exceeded_expr(emission.contract, task, &task_local, deadline_ms);
                output.push_str(&format!(
                    "{body_indent}const bool {task_local}_deadline_exceeded = {deadline_expr};\n\
                     {body_indent}if ({task_local}_deadline_exceeded) {{\n\
                     {body_inner_indent}health_map[\"{task_health}\"].deadline_missed += 1;\n\
                     {body_indent}}}\n",
                ));
            }

            // 在 deadline_exceeded 守卫下发布输出：deadline miss 时不发布 late output。
            let has_deadline = task.deadline_ms.is_some();
            if has_deadline {
                let task_local = cpp_task_local_name(task);
                output.push_str(&format!(
                    "{body_indent}if (!{task_local}_deadline_exceeded) {{\n"
                ));
            }
            for port in &component.outputs {
                if !task_outputs.contains(port.name.as_str()) {
                    continue;
                }
                let output_local = cpp_step_local_name(&instance.name, &port.name);
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
                let mut commit_index = 0usize;
                output.push_str(&format!(
                    "{publish_indent}if (const auto* value = {output_local}.as_ref()) {{\n"
                ));
                let active_guard = redundancy_plan.map(|plan| {
                    format!(
                        "{} == {}",
                        plan.active_field_name,
                        cpp_string_literal(&instance.name)
                    )
                });
                if let Some(guard) = &active_guard {
                    output.push_str(&format!("{publish_indent}    if ({guard}) {{\n"));
                }
                for bind_index in outgoing {
                    let bind = &emission.binds[bind_index];
                    let task_health = cpp_task_health_name(task);
                    let write_code = if collect_outputs {
                        let payload = format!("flowrt_payload_{commit_index}");
                        commit_index += 1;
                        cpp_runtime_channel_commit_with_health(bind, &task_health, &payload)
                    } else {
                        cpp_runtime_channel_write_with_health(bind, &task_health)
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
                        let payload = format!("flowrt_payload_{commit_index}");
                        commit_index += 1;
                        cpp_bridge_runtime_channel_commit(bridge, &payload)
                    } else {
                        cpp_bridge_runtime_channel_write(bridge)
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
                        let payload = format!("flowrt_payload_{commit_index}");
                        commit_index += 1;
                        cpp_boundary_output_commit(boundary, &payload)
                    } else {
                        cpp_boundary_output_write(boundary)
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
                output.push_str("        }\n");
            }
            output.push_str("    }\n");
        }
    }

    if collect_outputs {
        output
            .push_str("    return FlowrtTaskOutcome::ok(std::move(flowrt_output_commits));\n}\n\n");
    } else {
        output.push_str("    return flowrt::Status::Ok;\n}\n\n");
    }
    output
}

fn cpp_fault_injection_task_parameter(
    contract: &ContractIr,
    task: &flowrt_ir::TaskIr,
) -> Option<&'static str> {
    match crate::runtime_plan::scheduler_fault_injection_point_for(contract, task)?.kind {
        flowrt_ir::FaultInjectionKind::DeadlineMiss => Some("bool __flowrt_inject_deadline_miss"),
        flowrt_ir::FaultInjectionKind::BackendDrop => Some("bool __flowrt_inject_backend_drop"),
        _ => None,
    }
}

fn cpp_deadline_exceeded_expr(
    contract: &ContractIr,
    task: &flowrt_ir::TaskIr,
    task_local: &str,
    deadline_ms: u64,
) -> String {
    let elapsed_expr = format!(
        "(std::chrono::steady_clock::now() - {task_local}_deadline_started_at > std::chrono::milliseconds{{{deadline_ms}}})"
    );
    match crate::runtime_plan::scheduler_fault_injection_point_for(contract, task) {
        Some(point) if point.kind == flowrt_ir::FaultInjectionKind::DeadlineMiss => {
            format!("__flowrt_inject_deadline_miss || {elapsed_expr}")
        }
        _ => elapsed_expr,
    }
}

fn cpp_backend_drop_fault_injection_guard(
    emission: &CppStepEmission<'_>,
    task: &flowrt_ir::TaskIr,
) -> String {
    let Some(point) =
        crate::runtime_plan::scheduler_fault_injection_point_for(emission.contract, task)
    else {
        return String::new();
    };
    if point.kind != flowrt_ir::FaultInjectionKind::BackendDrop {
        return String::new();
    }

    let output_ports = task
        .outputs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut route_updates = String::new();
    for bind in emission.binds.iter().filter(|bind| {
        bind.source_instance == task.instance.name
            && output_ports.contains(bind.source_port.as_str())
            && bind_backend(bind) != "inproc"
    }) {
        let route = cpp_string_literal(&runtime_channel_name(bind));
        route_updates.push_str(&format!(
            "        introspection_state.record_route_backend_health({route}, flowrt::BackendHealthSnapshot::fault_injection_backend_drop());\n\
             introspection_state.record_route_drop({route});\n",
        ));
    }
    if route_updates.is_empty() {
        return String::new();
    }

    format!(
        "    if (__flowrt_inject_backend_drop) {{\n\
{route_updates}        return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{{}});\n\
    }}\n"
    )
}

fn cpp_lifecycle_fault_injection_guard(
    contract: &ContractIr,
    task: &flowrt_ir::TaskIr,
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
    let hit = cpp_fault_injection_first_invocation_hit_expr(point);
    format!(
        "{body_indent}const bool flowrt_inject_status_error = {hit};\n\
         {body_indent}if (flowrt_inject_status_error) {{\n\
         {body_inner_indent}return flowrt::Status::Error;\n\
         {body_indent}}}\n"
    )
}

fn cpp_fault_injection_first_invocation_hit_expr(
    point: &flowrt_ir::FaultInjectionPointIr,
) -> String {
    let mut clauses = Vec::new();
    for invocation in &point.invocations {
        clauses.push(format!("1ULL == {invocation}ULL"));
    }
    if let Some(from) = point.from_invocation {
        clauses.push(format!("1ULL >= {from}ULL"));
    }
    clauses.join(" || ")
}

pub(super) fn adapt_cpp_status_returns_for_collect(code: &str, collect_outputs: bool) -> String {
    if !collect_outputs {
        return code.to_string();
    }
    code.replace(
        "return flowrt::Status::Error;",
        "return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});",
    )
    .replace(
        "return flowrt::Status::Retry;",
        "return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{});",
    )
}

pub(super) fn cpp_task_step_function_name(task: &flowrt_ir::TaskIr) -> String {
    format!(
        "step_task_{}_{}",
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

pub(super) fn cpp_process_task_step_function_name(
    process: &ProcessRuntimePlan<'_>,
    task: &flowrt_ir::TaskIr,
) -> String {
    format!(
        "step_process_{}_task_{}_{}",
        process.method_suffix,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

pub(super) fn cpp_redundancy_active_param_decl(
    graph: &GraphIr,
    task: &flowrt_ir::TaskIr,
) -> Option<String> {
    crate::runtime_plan::standby_failover_plan_for_instance_in_graph(graph, &task.instance.name)
        .map(|plan| format!("std::string {}", plan.active_field_name))
}

pub(super) fn cpp_task_seen_revision_name(
    bind: &BindRuntimePlan,
    task: &flowrt_ir::TaskIr,
) -> String {
    format!(
        "{}_seen_revision_for_{}_{}",
        bind.field_name,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

pub(super) fn cpp_bridge_seen_revision_name(
    bridge: &BridgeRuntimePlan,
    task: &flowrt_ir::TaskIr,
) -> String {
    format!(
        "{}_seen_revision_for_{}_{}",
        bridge.field_name,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

pub(super) fn cpp_boundary_seen_revision_name(
    boundary: &BoundaryRuntimePlan,
    task: &flowrt_ir::TaskIr,
) -> String {
    format!(
        "{}_seen_revision_for_{}_{}",
        boundary.field_name,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

/// on_synchronized task 的 App 同步器成员字段基名（使用处追加 `_`）。
pub(super) fn cpp_synchronizer_field_name(task: &flowrt_ir::TaskIr) -> String {
    format!(
        "__flowrt_sync_{}_{}",
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

/// 同步器成员类型：value 用 `int`（typed 样本由 latest view 在发射点读取，value 无意义）。
pub(super) fn cpp_synchronizer_field_type() -> &'static str {
    "flowrt::Synchronizer<int>"
}

/// 同步器构造实参 `(input 路数, buffer 容量, tolerance_ns)`。
pub(super) fn cpp_synchronizer_ctor_args(graph: &GraphIr, task: &flowrt_ir::TaskIr) -> String {
    let group = crate::runtime_plan::sync_group_for_task(graph, task);
    let input_count = group.map(|group| group.inputs.len()).unwrap_or(0);
    let tolerance_ns = group
        .map(|group| group.tolerance_ms.saturating_mul(1_000_000))
        .unwrap_or(0);
    format!("{input_count}, 8, {tolerance_ns}")
}

/// 返回 `order` 内所有 on_synchronized task。
pub(super) fn cpp_on_synchronized_tasks<'a>(
    graph: &'a GraphIr,
    order: &[&'a InstanceIr],
) -> Vec<&'a flowrt_ir::TaskIr> {
    order
        .iter()
        .flat_map(|instance| tasks_for_instance(graph, instance))
        .filter(|task| task.trigger == flowrt_ir::TriggerKind::OnSynchronized)
        .collect()
}

pub(super) fn cpp_input_binds_for_task<'a>(
    task: &flowrt_ir::TaskIr,
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

pub(super) fn cpp_input_bridges_for_task<'a>(
    task: &flowrt_ir::TaskIr,
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

pub(super) fn cpp_input_boundaries_for_task<'a>(
    task: &flowrt_ir::TaskIr,
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

pub(super) fn emit_cpp_ros2_boundary_input_pump(
    emission: &CppStepEmission<'_>,
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
            "    auto {bridge}_boundary_latest_result = {bridge}_.receive_latest_at(tick_time_ms);\n    if (std::holds_alternative<flowrt::ChannelError>({bridge}_boundary_latest_result)) {{\n        return flowrt::Status::Error;\n    }}\n    const auto {bridge}_boundary_latest = std::get<flowrt::Latest<{ty}>>({bridge}_boundary_latest_result);\n    if (const auto* value = {bridge}_boundary_latest.get()) {{\n        {boundary}_.inject_at(*value, tick_time_ms);\n    }}\n",
            bridge = bridge.field_name,
            boundary = boundary.field_name,
            ty = cpp_type(&bridge.source_type),
        ));
    }
    output
}

pub(super) fn emit_cpp_on_message_revision_state(
    graph: &GraphIr,
    tasks: &[&flowrt_ir::TaskIr],
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
        for bind in cpp_input_binds_for_task(task, &inputs, binds) {
            output.push_str(&format!(
                "    std::uint64_t {seen} = 0;\n",
                seen = cpp_task_seen_revision_name(bind, task)
            ));
        }
        for bridge in cpp_input_bridges_for_task(task, &inputs, bridges) {
            output.push_str(&format!(
                "    std::uint64_t {seen} = 0;\n",
                seen = cpp_bridge_seen_revision_name(bridge, task)
            ));
        }
        for boundary in cpp_input_boundaries_for_task(task, &inputs, boundaries) {
            output.push_str(&format!(
                "    std::uint64_t {seen} = 0;\n",
                seen = cpp_boundary_seen_revision_name(boundary, task)
            ));
        }
    }
    output
}

pub(super) fn emit_cpp_on_message_wake_checks(
    graph: &GraphIr,
    tasks: &[&flowrt_ir::TaskIr],
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
        let input_binds = cpp_input_binds_for_task(task, &inputs, binds);
        let input_bridges = cpp_input_bridges_for_task(task, &inputs, bridges);
        let input_boundaries = cpp_input_boundaries_for_task(task, &inputs, boundaries);
        if input_binds.is_empty() && input_bridges.is_empty() && input_boundaries.is_empty() {
            continue;
        }
        for bind in &input_binds {
            if matches!(bind_backend(bind), "iox2" | "zenoh") {
                output.push_str(&format!(
                    "        (void){field}_.receive_latest_at(tick_time_ms);\n",
                    field = bind.field_name
                ));
            }
        }
        for bridge in &input_bridges {
            output.push_str(&format!(
                "        (void){field}_.receive_latest_at(tick_time_ms);\n",
                field = bridge.field_name
            ));
        }
        let mut checks = input_binds
            .iter()
            .map(|bind| {
                let revision_changed = format!(
                    "{field}_.revision() != {seen}",
                    field = bind.field_name,
                    seen = cpp_task_seen_revision_name(bind, task)
                );
                if bind.channel == ChannelKind::Fifo && bind_backend(bind) == "inproc" {
                    format!(
                        "({revision_changed} || !{field}_.empty())",
                        field = bind.field_name
                    )
                } else {
                    revision_changed
                }
            })
            .collect::<Vec<_>>();
        checks.extend(input_bridges.iter().map(|bridge| {
            format!(
                "{field}_.revision() != {seen}",
                field = bridge.field_name,
                seen = cpp_bridge_seen_revision_name(bridge, task)
            )
        }));
        checks.extend(input_boundaries.iter().map(|boundary| {
            format!(
                "{field}_.revision() != {seen}",
                field = boundary.field_name,
                seen = cpp_boundary_seen_revision_name(boundary, task)
            )
        }));
        let joiner = match task.readiness {
            flowrt_ir::TaskReadiness::AnyReady => " || ",
            flowrt_ir::TaskReadiness::AllReady => " && ",
        };
        output.push_str(&format!("        if ({}) {{\n", checks.join(joiner)));
        for bind in &input_binds {
            output.push_str(&format!(
                "            {seen} = {field}_.revision();\n",
                seen = cpp_task_seen_revision_name(bind, task),
                field = bind.field_name
            ));
        }
        for bridge in &input_bridges {
            output.push_str(&format!(
                "            {seen} = {field}_.revision();\n",
                seen = cpp_bridge_seen_revision_name(bridge, task),
                field = bridge.field_name
            ));
        }
        for boundary in &input_boundaries {
            output.push_str(&format!(
                "            {seen} = {field}_.revision();\n",
                seen = cpp_boundary_seen_revision_name(boundary, task),
                field = boundary.field_name
            ));
        }
        output.push_str(&format!(
            "            scheduler.wake(flowrt::TaskId{{{}}});\n            woke_on_message = true;\n        }}\n",
            index + 1
        ));
    }
    output
}

pub(super) fn cpp_runtime_channel_type(contract: &ContractIr, bind: &BindRuntimePlan) -> String {
    let ty = cpp_type(&bind.source_type);
    if bind_backend(bind) == "iox2" {
        if let Some(cap) = cpp_iox2_frame_channel_cap(contract, bind) {
            return format!("flowrt::iox2::Iox2FramePubSub<{ty}, {cap}>");
        }
        return format!("flowrt::iox2::Iox2PubSub<{ty}>");
    }
    if bind_backend(bind) == "zenoh" {
        return format!("flowrt::zenoh::ZenohPubSub<{ty}>");
    }

    match bind.channel {
        ChannelKind::Latest => format!("flowrt::LatestChannel<{ty}>"),
        ChannelKind::Fifo => format!("flowrt::FifoChannel<{ty}>"),
    }
}

pub(super) fn cpp_bridge_runtime_channel_type(bridge: &BridgeRuntimePlan) -> String {
    format!(
        "flowrt::zenoh::ZenohPubSub<{}>",
        cpp_type(&bridge.source_type)
    )
}

pub(super) fn cpp_runtime_channel_initializer(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
) -> String {
    if bind_backend(bind) == "iox2" {
        let channel = if let Some(cap) = cpp_iox2_frame_channel_cap(contract, bind) {
            format!(
                "flowrt::iox2::Iox2FramePubSub<{}, {cap}>",
                cpp_type(&bind.source_type)
            )
        } else {
            format!("flowrt::iox2::Iox2PubSub<{}>", cpp_type(&bind.source_type))
        };
        return format!(
            "{channel}::open_with_config({}, {})",
            cpp_string_literal(&iox2_service_name(contract, graph, bind)),
            cpp_iox2_channel_config_expr(bind)
        );
    }
    if bind_backend(bind) == "zenoh" {
        return format!(
            "flowrt::zenoh::ZenohPubSub<{}>::open_with_config({}, {})",
            cpp_type(&bind.source_type),
            cpp_string_literal(&zenoh_key_expr(contract, graph, bind)),
            cpp_zenoh_channel_config_expr(bind)
        );
    }

    match bind.channel {
        ChannelKind::Latest => cpp_runtime_latest_channel_initializer(bind),
        ChannelKind::Fifo => cpp_runtime_fifo_channel_initializer(bind),
    }
}

pub(super) fn cpp_bridge_runtime_channel_initializer(
    contract: &ContractIr,
    graph: &GraphIr,
    bridge: &BridgeRuntimePlan,
) -> String {
    format!(
        "flowrt::zenoh::ZenohPubSub<{}>::open_with_config({}, flowrt::zenoh::ZenohChannelConfig::latest())",
        cpp_type(&bridge.source_type),
        cpp_string_literal(&ros2_bridge_key_expr(contract, graph, bridge))
    )
}

pub(super) fn cpp_zenoh_channel_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::zenoh::ZenohChannelConfig::latest().with_stale_config({})",
            cpp_runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => format!(
            "flowrt::zenoh::ZenohChannelConfig::fifo({}, {}).with_stale_config({})",
            bind.depth.unwrap_or(1),
            crate::runtime_plan::runtime_overflow_policy_path(bind.overflow),
            cpp_runtime_stale_config_expr(bind)
        ),
    }
}

pub(super) fn cpp_iox2_channel_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config({})",
            cpp_runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => format!(
            "flowrt::iox2::Iox2ChannelConfig::fifo({}, {}).with_stale_config({})",
            bind.depth.unwrap_or(1),
            crate::runtime_plan::runtime_overflow_policy_path(bind.overflow),
            cpp_runtime_stale_config_expr(bind)
        ),
    }
}

pub(super) fn cpp_runtime_latest_channel_initializer(bind: &BindRuntimePlan) -> String {
    let ty = cpp_type(&bind.source_type);
    if bind.max_age_ms.is_none() && bind.stale == IrStalePolicy::Warn {
        return String::new();
    }

    format!(
        "flowrt::LatestChannel<{ty}>::with_stale_config({})",
        cpp_runtime_stale_config_expr(bind)
    )
}

pub(super) fn cpp_runtime_fifo_channel_initializer(bind: &BindRuntimePlan) -> String {
    let depth = bind.depth.unwrap_or(1);
    let overflow = crate::runtime_plan::runtime_overflow_policy_path(bind.overflow);
    if bind.max_age_ms.is_none() && bind.stale == IrStalePolicy::Warn {
        return format!("{depth}, {overflow}");
    }

    format!(
        "flowrt::FifoChannel<{}>::with_stale_config({}, {}, {})",
        cpp_type(&bind.source_type),
        depth,
        overflow,
        cpp_runtime_stale_config_expr(bind)
    )
}

pub(super) fn cpp_runtime_stale_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.max_age_ms {
        Some(max_age_ms) => format!(
            "flowrt::StaleConfig{{std::chrono::milliseconds{{{max_age_ms}}}, {}}}",
            crate::runtime_plan::runtime_stale_policy_path(bind.stale)
        ),
        None => format!(
            "flowrt::StaleConfig{{{}}}",
            crate::runtime_plan::runtime_stale_policy_path(bind.stale)
        ),
    }
}

pub(super) fn cpp_runtime_channel_read(
    input: &PortIr,
    bind: &BindRuntimePlan,
    local_name: &str,
    use_cached_transport: bool,
) -> String {
    if matches!(bind_backend(bind), "iox2" | "zenoh") {
        if use_cached_transport {
            return format!(
                "    const auto {local} = {field}_.cached_latest_at(tick_time_ms);\n",
                local = local_name,
                field = bind.field_name
            );
        }
        return format!(
            "    auto {local}_result = {field}_.receive_latest_at(tick_time_ms);\n    if (std::holds_alternative<flowrt::ChannelError>({local}_result)) {{\n        return flowrt::Status::Error;\n    }}\n    const auto {local} = std::get<flowrt::Latest<{ty}>>({local}_result);\n",
            local = local_name,
            field = bind.field_name,
            ty = cpp_type(&input.ty)
        );
    }

    match bind.channel {
        ChannelKind::Latest => format!(
            "    const auto {local} = {field}_.view_at(tick_time_ms);\n",
            local = local_name,
            field = bind.field_name
        ),
        ChannelKind::Fifo => format!(
            "    auto {local}_read = {field}_.pop_at(tick_time_ms);\n    const auto {local} = {local}_read.view();\n",
            local = local_name,
            field = bind.field_name
        ),
    }
}

pub(super) fn cpp_runtime_stale_error_guard(local_name: &str, bind: &BindRuntimePlan) -> String {
    if bind.stale != IrStalePolicy::Error {
        return String::new();
    }

    format!("    if ({local_name}.stale()) {{\n        return flowrt::Status::Error;\n    }}\n")
}

/// 生成 stale input 健康计数器记录代码（C++）。
pub(super) fn cpp_runtime_stale_health_record(local_name: &str, task_health_name: &str) -> String {
    format!(
        "    if ({local_name}.stale()) {{\n        health_map[\"{task_health_name}\"].stale_input += 1;\n    }}\n",
    )
}

pub(super) fn cpp_step_local_name(instance: &str, port: &str) -> String {
    format!("{instance}_{port}")
}

pub(super) fn cpp_introspection_publish_record(bind: &BindRuntimePlan) -> String {
    let helper = if bind.source_uses_variable_frame || bind_backend(bind) == "zenoh" {
        "record_introspection_publish_frame"
    } else {
        "record_introspection_publish_copy"
    };
    format!(
        "        {helper}(introspection_state, {channel}, {message_type}, this->{probe}, *value, tick_time_ms);\n",
        channel = cpp_string_literal(&runtime_channel_name(bind)),
        message_type = cpp_string_literal(&runtime_channel_message_type(bind)),
        probe = bind.probe_field_name
    )
}

/// 生成 channel 写入代码（C++），带健康计数器记录。
pub(super) fn cpp_runtime_channel_write_with_health(
    bind: &BindRuntimePlan,
    task_health_name: &str,
) -> String {
    cpp_runtime_channel_write_inner(bind, Some(task_health_name))
}

pub(super) fn cpp_runtime_channel_commit_with_health(
    bind: &BindRuntimePlan,
    task_health_name: &str,
    payload_name: &str,
) -> String {
    let body = cpp_runtime_channel_write_inner(bind, Some(task_health_name))
        .replace(
            &format!("{}_.", bind.field_name),
            &format!("app.{}_.", bind.field_name),
        )
        .replace("this->", "app.");
    let health_arg = if body.contains("health_map") {
        "health_map"
    } else {
        "/*health_map*/"
    };
    format!(
        "        auto {payload_name} = *value;\n        flowrt_output_commits.emplace_back([{payload_name} = std::move({payload_name}), tick_time_ms](App& app, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& {health_arg}) mutable {{\n            const auto* value = &{payload_name};\n{body}            return flowrt::Status::Ok;\n        }});\n"
    )
}

pub(super) fn cpp_runtime_channel_write_inner(
    bind: &BindRuntimePlan,
    task_health_name: Option<&str>,
) -> String {
    let introspection_record = cpp_introspection_publish_record(bind);
    if matches!(bind_backend(bind), "iox2" | "zenoh") {
        return format!(
            "        if (const auto status = status_from_push_result({field}_.publish_at(*value, tick_time_ms)); status != flowrt::Status::Ok) {{\n            return status;\n        }}\n        scheduler_events.notify_data();\n{introspection_record}",
            field = bind.field_name
        );
    }

    match bind.channel {
        ChannelKind::Latest => format!(
            "        {field}_.publish_at(*value, tick_time_ms);\n        scheduler_events.notify_data();\n{introspection_record}",
            field = bind.field_name
        ),
        ChannelKind::Fifo => {
            if let Some(task_health) = task_health_name {
                format!(
                    "        const auto {field}_result = {field}_.push_at(*value, tick_time_ms);\n        if (const auto status = status_from_push_result({field}_result); status != flowrt::Status::Ok) {{\n            if (std::holds_alternative<flowrt::ChannelWriteOutcome>({field}_result)) {{\n                if (std::get<flowrt::ChannelWriteOutcome>({field}_result) == flowrt::ChannelWriteOutcome::Backpressured) {{\n                    health_map[\"{task_health}\"].backpressure += 1;\n                }}\n            }} else {{\n                health_map[\"{task_health}\"].overflow += 1;\n            }}\n            return status;\n        }}\n        if (std::holds_alternative<flowrt::ChannelWriteOutcome>({field}_result)) {{\n            switch (std::get<flowrt::ChannelWriteOutcome>({field}_result)) {{\n                case flowrt::ChannelWriteOutcome::Accepted:\n                case flowrt::ChannelWriteOutcome::DroppedOldest:\n                    scheduler_events.notify_data();\n{introspection_record}                    break;\n                case flowrt::ChannelWriteOutcome::DroppedNewest:\n                case flowrt::ChannelWriteOutcome::Backpressured:\n                    break;\n            }}\n        }}\n",
                    field = bind.field_name,
                    task_health = task_health,
                )
            } else {
                format!(
                    "        const auto {field}_result = {field}_.push_at(*value, tick_time_ms);\n        if (const auto status = status_from_push_result({field}_result); status != flowrt::Status::Ok) {{\n            return status;\n        }}\n        if (std::holds_alternative<flowrt::ChannelWriteOutcome>({field}_result)) {{\n            switch (std::get<flowrt::ChannelWriteOutcome>({field}_result)) {{\n                case flowrt::ChannelWriteOutcome::Accepted:\n                case flowrt::ChannelWriteOutcome::DroppedOldest:\n                    scheduler_events.notify_data();\n{introspection_record}                    break;\n                case flowrt::ChannelWriteOutcome::DroppedNewest:\n                case flowrt::ChannelWriteOutcome::Backpressured:\n                    break;\n            }}\n        }}\n",
                    field = bind.field_name,
                )
            }
        }
    }
}

pub(super) fn cpp_bridge_runtime_channel_write(bridge: &BridgeRuntimePlan) -> String {
    format!(
        "        if (const auto status = status_from_push_result({field}_.publish_at(*value, tick_time_ms)); status != flowrt::Status::Ok) {{\n            return status;\n        }}\n",
        field = bridge.field_name
    )
}

pub(super) fn cpp_bridge_runtime_channel_commit(
    bridge: &BridgeRuntimePlan,
    payload_name: &str,
) -> String {
    format!(
        "        auto {payload_name} = *value;\n        flowrt_output_commits.emplace_back([{payload_name} = std::move({payload_name}), tick_time_ms](App& app, flowrt::IntrospectionState& /*introspection_state*/, flowrt::ScheduleWaiter& /*scheduler_events*/, std::map<std::string, flowrt::IntrospectionTaskHealth>& /*health_map*/) mutable {{\n            const auto* value = &{payload_name};\n            if (const auto status = status_from_push_result(app.{field}_.publish_at(*value, tick_time_ms)); status != flowrt::Status::Ok) {{\n                return status;\n            }}\n            return flowrt::Status::Ok;\n        }});\n",
        field = bridge.field_name
    )
}

pub(super) fn cpp_bridge_runtime_channel_read(
    input: &PortIr,
    bridge: &BridgeRuntimePlan,
    local_name: &str,
    use_cached_transport: bool,
) -> String {
    if use_cached_transport {
        return format!(
            "    const auto {local} = {field}_.cached_latest_at(tick_time_ms);\n",
            local = local_name,
            field = bridge.field_name
        );
    }
    format!(
        "    auto {local}_result = {field}_.receive_latest_at(tick_time_ms);\n    if (std::holds_alternative<flowrt::ChannelError>({local}_result)) {{\n        return flowrt::Status::Error;\n    }}\n    const auto {local} = std::get<flowrt::Latest<{ty}>>({local}_result);\n",
        local = local_name,
        field = bridge.field_name,
        ty = cpp_type(&input.ty)
    )
}

pub(super) fn cpp_runtime_step_uses_tick_time(
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

pub(super) fn cpp_boundary_input_read(boundary: &BoundaryRuntimePlan, local_name: &str) -> String {
    format!(
        "    const auto {local}_read = {field}_.read_at(tick_time_ms);\n    const auto {local} = {local}_read.view();\n",
        local = local_name,
        field = boundary.field_name,
    )
}

pub(super) fn cpp_boundary_output_write(boundary: &BoundaryRuntimePlan) -> String {
    format!(
        "        {field}_.publish_at(*value, tick_time_ms);\n",
        field = boundary.field_name,
    )
}

pub(super) fn cpp_boundary_output_commit(
    boundary: &BoundaryRuntimePlan,
    payload_name: &str,
) -> String {
    format!(
        "        auto {payload_name} = *value;\n        flowrt_output_commits.emplace_back([{payload_name} = std::move({payload_name}), tick_time_ms](App& app, flowrt::IntrospectionState& /*introspection_state*/, flowrt::ScheduleWaiter& /*scheduler_events*/, std::map<std::string, flowrt::IntrospectionTaskHealth>& /*health_map*/) mutable {{\n            const auto* value = &{payload_name};\n            app.{field}_.publish_at(*value, tick_time_ms);\n            return flowrt::Status::Ok;\n        }});\n",
        field = boundary.field_name,
    )
}
