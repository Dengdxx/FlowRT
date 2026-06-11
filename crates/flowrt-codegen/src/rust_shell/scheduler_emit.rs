use flowrt_ir::{ContractIr, GraphIr, InstanceIr, TaskIr, TriggerKind};

use crate::runtime_plan::{
    BindRuntimePlan, BoundaryRuntimePlan, BridgeRuntimePlan, ProcessRuntimePlan, TaskEmissionPhase,
};
use crate::{scheduler_tasks_for_order, selected_profile_worker_threads};

use super::step_emit::{
    RustStepEmission, emit_rust_app_step, emit_rust_apply_pending_params_for_order,
    emit_rust_on_message_revision_state, emit_rust_on_message_wake_checks, scheduler_lane_ids,
    task_health_name, task_lane_name,
};
use super::{operation_emit, service_emit};

pub(super) fn rust_task_step_function_name(task: &TaskIr) -> String {
    format!(
        "step_task_{}_{}",
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

pub(super) fn rust_process_task_step_function_name(
    process: &ProcessRuntimePlan<'_>,
    task: &TaskIr,
) -> String {
    format!(
        "step_process_{}_task_{}_{}",
        process.method_suffix,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

pub(super) fn emit_all_step_functions(
    emission: &RustStepEmission<'_>,
    graph: &GraphIr,
    order: &[&InstanceIr],
    output: &mut String,
) {
    output.push_str(&emit_rust_app_step(
        emission,
        order,
        "step",
        TaskEmissionPhase::Scheduler,
        None,
    ));
    output.push_str(&emit_rust_app_step(
        emission,
        order,
        "step_startup",
        TaskEmissionPhase::Startup,
        None,
    ));
    output.push_str(&emit_rust_app_step(
        emission,
        order,
        "step_shutdown",
        TaskEmissionPhase::Shutdown,
        None,
    ));
    for task in scheduler_tasks_for_order(graph, order) {
        output.push_str(&emit_rust_app_step(
            emission,
            order,
            &rust_task_step_function_name(task),
            TaskEmissionPhase::Scheduler,
            Some(task),
        ));
    }
}

pub(super) fn emit_process_step_functions(
    emission: &RustStepEmission<'_>,
    graph: &GraphIr,
    processes: &[ProcessRuntimePlan<'_>],
    output: &mut String,
) {
    for process in processes {
        output.push_str(&emit_rust_app_step(
            emission,
            &process.instances,
            &format!("step_process_{}", process.method_suffix),
            TaskEmissionPhase::Scheduler,
            None,
        ));
        output.push_str(&emit_rust_app_step(
            emission,
            &process.instances,
            &format!("step_process_{}_startup", process.method_suffix),
            TaskEmissionPhase::Startup,
            None,
        ));
        output.push_str(&emit_rust_app_step(
            emission,
            &process.instances,
            &format!("step_process_{}_shutdown", process.method_suffix),
            TaskEmissionPhase::Shutdown,
            None,
        ));
        for task in scheduler_tasks_for_order(graph, &process.instances) {
            output.push_str(&emit_rust_app_step(
                emission,
                &process.instances,
                &rust_process_task_step_function_name(process, task),
                TaskEmissionPhase::Scheduler,
                Some(task),
            ));
        }
    }
}

pub(super) fn emit_rust_scheduler_v2_loop(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
    process: Option<&ProcessRuntimePlan<'_>>,
    fallback_step_function: &str,
) -> String {
    let tasks = scheduler_tasks_for_order(graph, order);
    let mut output = String::new();
    output.push_str(&format!(
        "        let mut scheduler = flowrt::DeterministicExecutor::new({});\n",
        selected_profile_worker_threads(contract)
    ));

    let mut lane_ids = scheduler_lane_ids(&tasks);
    for (lane, lane_id) in &lane_ids {
        output.push_str(&format!(
            "        scheduler.add_lane(flowrt::LaneId({lane_id}), flowrt::LaneKind::Serial);\n        let _ = {lane:?};\n"
        ));
    }
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let lane_id = lane_ids[&task_lane_name(task)];
        let priority = task.priority.unwrap_or(0);
        output.push_str(&format!(
            "        scheduler.add_task(flowrt::TaskSpec {{ id: flowrt::TaskId({task_id}), lane: flowrt::LaneId({lane_id}), priority: {priority} }});\n"
        ));
        if task.trigger == TriggerKind::Periodic {
            output.push_str(&format!(
                "        scheduler.add_periodic(flowrt::PeriodicSpec {{ task: flowrt::TaskId({task_id}), period_ms: {} }});\n        scheduler.wake(flowrt::TaskId({task_id}));\n",
                task.period_ms.unwrap_or(1)
            ));
        }
    }
    // service task registration
    let next_task_id = tasks.len();
    let (service_lanes, service_tasks, service_task_end) =
        service_emit::emit_rust_service_scheduler_registration(
            contract,
            graph,
            next_task_id,
            &mut lane_ids,
        );
    output.push_str(&service_lanes);
    output.push_str(&service_tasks);
    let (operation_lanes, operation_tasks, operation_task_end) =
        operation_emit::emit_rust_operation_scheduler_registration(
            contract,
            graph,
            service_task_end,
            &mut lane_ids,
        );
    output.push_str(&operation_lanes);
    output.push_str(&operation_tasks);
    output.push_str(&emit_rust_on_message_revision_state(
        &tasks, binds, bridges, boundaries,
    ));
    output.push_str(&format!(
        "        let scheduler_base_period_ms: u64 = {};\n",
        scheduler_base_period_ms(&tasks)
    ));
    let task_health_init = emit_rust_task_health_init(&tasks);
    output.push_str(
        "        let mut tick_base: usize = 0;\n        let mut scheduler_now_ms: u64 = 0;\n        let mut health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();\n        const FAIRNESS_STARVATION_THRESHOLD: u64 = 10;\n        while status == flowrt::Status::Ok\n            && !shutdown.is_requested()\n            && run_ticks\n                .map(|limit| tick_base < limit)\n                .unwrap_or(true)\n        {\n            let mut observed_data_generation: u64;\n            let tick_time_ms = scheduler_now_ms;\n            scheduler.advance_to_ms(tick_time_ms);\n            scheduler.set_current_tick(tick_base as u64);\n",
    );
    output.push_str(&task_health_init);
    output.push_str(&emit_rust_apply_pending_params_for_order(contract, order));
    let has_service_tasks =
        !service_emit::emit_rust_service_wake_checks(contract, graph, next_task_id).is_empty();
    let has_operation_tasks =
        !operation_emit::emit_rust_operation_wake_checks(contract, graph, service_task_end)
            .is_empty();
    let woke_on_message_decl = if tasks
        .iter()
        .any(|task| task.trigger == TriggerKind::OnMessage)
        || has_service_tasks
        || has_operation_tasks
    {
        "let mut woke_on_message = false;"
    } else {
        "let woke_on_message = false;"
    };
    output.push_str(&format!(
        "            introspection_state.record_tick();\n            loop {{\n                observed_data_generation = scheduler_events.data_generation();\n                {woke_on_message_decl}\n"
    ));
    output.push_str(&crate::runtime_plan::indent_generated_block_levels(
        &emit_rust_on_message_wake_checks(&tasks, binds, bridges, boundaries),
        1,
    ));
    // service wake checks
    let service_wake_checks =
        service_emit::emit_rust_service_wake_checks(contract, graph, next_task_id);
    if !service_wake_checks.is_empty() {
        output.push_str(&crate::runtime_plan::indent_generated_block_levels(
            &service_wake_checks,
            1,
        ));
    }
    let operation_wake_checks =
        operation_emit::emit_rust_operation_wake_checks(contract, graph, service_task_end);
    if !operation_wake_checks.is_empty() {
        output.push_str(&crate::runtime_plan::indent_generated_block_levels(
            &operation_wake_checks,
            1,
        ));
    }
    output
        .push_str("                let task_statuses = scheduler.run_ready(|task| match task {\n");
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let lane_id = lane_ids[&task_lane_name(task)];
        let function_name = match process {
            Some(process) => rust_process_task_step_function_name(process, task),
            None => rust_task_step_function_name(task),
        };
        output.push_str(&format!(
            "                flowrt::TaskId({task_id}) => {{\n\
                 let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId({lane_id}));\n\
                 self.{function_name}(tick_time_ms as usize, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut health_map)\n\
             }},\n"
        ));
    }
    // service dispatch cases
    let (service_cases, _service_case_end) =
        service_emit::rust_service_dispatch_cases(contract, graph, next_task_id, &lane_ids);
    if !service_cases.is_empty() {
        output.push_str(&service_cases);
    }
    let (operation_cases, _operation_case_end) =
        operation_emit::rust_operation_dispatch_cases(contract, graph, service_task_end, &lane_ids);
    if !operation_cases.is_empty() {
        output.push_str(&operation_cases);
    }
    if tasks.is_empty() && service_cases.is_empty() && operation_cases.is_empty() {
        output.push_str(&format!(
            "                _ => self.{fallback_step_function}(tick_time_ms as usize, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut health_map),\n"
        ));
    } else {
        output.push_str("                _ => flowrt::Status::Error,\n");
    }
    output.push_str(&format!(
        "                }});\n                if !woke_on_message && task_statuses.is_empty() {{\n                    break;\n                }}\n                for task_status in task_statuses {{\n                    if task_status == flowrt::Status::Error {{\n                        status = flowrt::Status::Error;\n                        break;\n                    }}\n                }}\n                if status != flowrt::Status::Ok {{\n                    break;\n                }}\n            }}\n            // 公平性检测：检查 lane 饥饿。\n{fairness_check}            // 将本轮健康快照写入 introspection。\n            for (_, health) in health_map.iter_mut() {{\n                introspection_state.record_task_health(health.clone());\n            }}\n            health_map.clear();\n            if status == flowrt::Status::Ok {{\n                tick_base += 1;\n                if run_ticks.is_some() {{\n                    scheduler_now_ms = scheduler_now_ms.saturating_add(scheduler_base_period_ms);\n                    continue;\n                }}\n                let next_periodic_deadline_ms = {next_deadline_expr};\n                let next_wake_deadline = next_periodic_deadline_ms.map(|deadline_ms| {{\n                    std::time::Instant::now()\n                        + std::time::Duration::from_millis(deadline_ms.saturating_sub(scheduler_now_ms))\n                }});\n                match scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, &shutdown) {{\n                    flowrt::ScheduleEvent::Shutdown => break,\n                    flowrt::ScheduleEvent::Timer => {{\n                        scheduler_now_ms = next_periodic_deadline_ms\n                            .unwrap_or_else(|| scheduler_now_ms.saturating_add(scheduler_base_period_ms));\n                    }}\n                    flowrt::ScheduleEvent::Data => {{}}\n                }}\n            }}\n        }}\n",
        next_deadline_expr = rust_next_periodic_deadline_expr(&tasks),
        fairness_check = emit_rust_fairness_check(&lane_ids),
    ));
    let _ = operation_task_end;
    output
}

pub(super) fn emit_rust_scheduler_event_registration(
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
) -> String {
    let mut output = String::new();
    for bind in binds
        .iter()
        .filter(|bind| matches!(crate::runtime_plan::bind_backend(bind), "iox2" | "zenoh"))
    {
        output.push_str(&format!(
            "        self.{field}.set_schedule_waiter(scheduler_events.clone());\n",
            field = bind.field_name
        ));
    }
    for bridge in bridges {
        output.push_str(&format!(
            "        self.{field}.set_schedule_waiter(scheduler_events.clone());\n",
            field = bridge.field_name
        ));
    }
    for boundary in boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Input)
    {
        output.push_str(&format!(
            "        self.{field}.set_schedule_waiter(scheduler_events.clone());\n",
            field = boundary.field_name
        ));
    }
    output
}

fn scheduler_base_period_ms(tasks: &[&TaskIr]) -> u64 {
    tasks
        .iter()
        .filter(|task| task.trigger == TriggerKind::Periodic)
        .filter_map(|task| task.period_ms)
        .min()
        .unwrap_or(1)
}

/// 为本轮 scheduler 预注册 task health 条目，确保未运行 task 也能记录公平性计数。
fn emit_rust_task_health_init(tasks: &[&TaskIr]) -> String {
    let mut output = String::new();
    for task in tasks {
        let task_health = task_health_name(task);
        let lane = task_lane_name(task);
        output.push_str(&format!(
            "            {{\n                let __h = health_map.entry({task_health:?}.to_string()).or_default();\n                __h.name = {task_health:?}.to_string();\n                __h.lane = {lane:?}.to_string();\n            }}\n"
        ));
    }
    output
}

/// 生成 lane 饥饿检测代码。
///
/// 对每个已注册 lane 检查 `lane_starvation_ticks`，超过阈值时
/// 在 health_map 中为该 lane 的所有 task 记录 fairness_violations。
fn emit_rust_fairness_check(lane_ids: &std::collections::BTreeMap<String, usize>) -> String {
    let mut output = String::new();
    for (lane, lane_id) in lane_ids {
        output.push_str(&format!(
            "            if scheduler.lane_starvation_ticks(flowrt::LaneId({lane_id})) > FAIRNESS_STARVATION_THRESHOLD {{\n                for health in health_map.values_mut() {{\n                    if health.lane == {lane:?} {{\n                        health.fairness_violations += 1;\n                    }}\n                }}\n            }}\n"
        ));
    }
    output
}

fn rust_next_periodic_deadline_expr(tasks: &[&TaskIr]) -> String {
    let deadlines = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.trigger == TriggerKind::Periodic)
        .map(|(index, _)| format!("scheduler.next_deadline_ms(flowrt::TaskId({}))", index + 1))
        .collect::<Vec<_>>();
    if deadlines.is_empty() {
        "None::<u64>".to_string()
    } else {
        format!("[{}].into_iter().flatten().min()", deadlines.join(", "))
    }
}
