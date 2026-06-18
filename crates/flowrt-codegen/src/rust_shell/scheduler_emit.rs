use flowrt_ir::{ChannelKind, ContractIr, GraphIr, InstanceIr, PortIr, TaskIr, TriggerKind};

use crate::runtime_plan::{
    BindRuntimePlan, BoundaryRuntimePlan, BridgeRuntimePlan, ProcessRuntimePlan,
    SchedulerDataflowTaskPlan, SchedulerHiddenTaskKind, SchedulerHiddenTaskPlan, TaskEmissionPhase,
    scheduler_runtime_plan,
};
use crate::scheduler_tasks_for_order;

use super::step_emit::{
    RustStepEmission, emit_rust_app_step, emit_rust_apply_pending_params_for_order,
    emit_rust_on_message_revision_state, emit_rust_on_message_wake_checks,
};
use super::{operation_emit, service_emit, step_emit};

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

pub(super) struct RustSchedulerLoopEmission<'a> {
    pub(super) contract: &'a ContractIr,
    pub(super) graph: &'a GraphIr,
    pub(super) order: &'a [&'a InstanceIr],
    pub(super) binds: &'a [BindRuntimePlan],
    pub(super) bridges: &'a [BridgeRuntimePlan],
    pub(super) boundaries: &'a [BoundaryRuntimePlan],
    pub(super) process: Option<&'a ProcessRuntimePlan<'a>>,
    pub(super) fallback_step_function: &'a str,
    pub(super) external_tick: Option<&'a str>,
}

pub(super) fn emit_rust_scheduler_v2_loop(emission: RustSchedulerLoopEmission<'_>) -> String {
    let RustSchedulerLoopEmission {
        contract,
        graph,
        order,
        binds,
        bridges,
        boundaries,
        process,
        fallback_step_function,
        external_tick,
    } = emission;
    let _ = fallback_step_function;
    let scheduler_plan = scheduler_runtime_plan(contract, graph, order);
    let recoverable = crate::runtime_plan::recoverable_instances(contract, graph, order);
    let standby_failover = crate::runtime_plan::standby_failover_plans_for_order(graph, order);
    // 受控停机仅当图声明 on_faulted=stop 且存在可达终态故障的 isolate/restart 实例时启用；
    // 否则不 emit 任何图停机机制，fail_fast/degrade-only 图字节不变。
    let graph_stop = graph.health.on_faulted == flowrt_ir::GraphFaultReaction::Stop
        && recoverable.iter().any(|plan| {
            matches!(
                plan.policy,
                flowrt_ir::InstanceFailurePolicy::Isolate
                    | flowrt_ir::InstanceFailurePolicy::Restart
            )
        });
    let tasks = scheduler_plan
        .dataflow_tasks
        .iter()
        .map(|task| task.task)
        .collect::<Vec<_>>();
    let lane_ids = scheduler_plan
        .lanes
        .iter()
        .map(|lane| (lane.name.clone(), lane.id))
        .collect::<std::collections::BTreeMap<_, _>>();
    let service_tasks = scheduler_plan
        .hidden_tasks
        .iter()
        .filter(|task| task.kind == SchedulerHiddenTaskKind::Service)
        .collect::<Vec<_>>();
    let operation_tasks = scheduler_plan
        .hidden_tasks
        .iter()
        .filter(|task| task.kind == SchedulerHiddenTaskKind::Operation)
        .collect::<Vec<_>>();
    let mut output = String::new();
    let worker_threads = scheduler_plan.worker_threads;
    output.push_str(&format!(
        "        let mut scheduler = flowrt::DeterministicExecutor::new({worker_threads});\n",
    ));
    output.push_str(&format!(
        "        let worker_pool = flowrt::WorkerPool::new({worker_threads});\n"
    ));

    for lane in &scheduler_plan.lanes {
        let lane_name = &lane.name;
        let lane_id = lane.id;
        output.push_str(&format!(
            "        scheduler.add_lane(flowrt::LaneId({lane_id}), flowrt::LaneKind::Serial);\n        let _ = {lane_name:?};\n"
        ));
    }
    for task in &scheduler_plan.dataflow_tasks {
        let task_id = task.id;
        let lane_id = task.lane_id;
        let priority = task.priority;
        output.push_str(&format!(
            "        scheduler.add_task(flowrt::TaskSpec {{ id: flowrt::TaskId({task_id}), lane: flowrt::LaneId({lane_id}), priority: {priority} }});\n"
        ));
        if let Some(deadline_ms) = task.deadline_ms {
            output.push_str(&format!(
                "        scheduler.set_task_deadline_ms(flowrt::TaskId({task_id}), Some({deadline_ms}));\n"
            ));
        }
        if task.periodic_wake {
            output.push_str(&format!(
                "        scheduler.add_periodic(flowrt::PeriodicSpec {{ task: flowrt::TaskId({task_id}), period_ms: {} }});\n        scheduler.wake(flowrt::TaskId({task_id}));\n",
                task.period_ms.unwrap_or(1)
            ));
        }
    }
    output.push_str(&service_emit::emit_rust_service_scheduler_registration(
        &service_tasks,
    ));
    output.push_str(&operation_emit::emit_rust_operation_scheduler_registration(
        &operation_tasks,
    ));
    output.push_str(&emit_rust_on_message_revision_state(
        graph, &tasks, binds, bridges, boundaries,
    ));
    output.push_str(&format!(
        "        let scheduler_base_period_ms: u64 = {};\n",
        scheduler_plan.scheduler_base_period_ms
    ));
    let task_health_init = emit_rust_task_health_init(&scheduler_plan.dataflow_tasks);
    let clock_source = scheduler_clock_source(contract);
    let task_clock_source = rust_task_clock_source_expr(contract);
    let tick_base_init = if external_tick.is_some() {
        "__flowrt_external_tick_base".to_string()
    } else {
        "0".to_string()
    };
    let scheduler_now_init = external_tick
        .map(|grant| format!("{grant}.logical_time_ms"))
        .unwrap_or_else(|| "0".to_string());
    output.push_str(&format!(
        "        let mut tick_base: usize = {tick_base_init};\n        let mut scheduler_now_ms: u64 = {scheduler_now_init};\n        let mut health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();\n        const FAIRNESS_STARVATION_THRESHOLD: u64 = 10;\n",
    ));
    if external_tick.is_none() {
        output.push_str(&rust_scheduler_clock_init(contract));
        output.push_str(&rust_scheduler_replay_driver_init(contract, boundaries));
    }
    output.push_str(&format!("        let clock_source = {clock_source:?};\n"));
    output.push_str(&format!(
        "        let task_clock_source = {task_clock_source};\n        let task_completion_queue = flowrt::WorkerCompletionQueue::<Vec<FlowrtOutputCommit>>::new();\n        let scheduler_events_for_task_completion = scheduler_events.clone();\n        task_completion_queue.set_wake_callback(move || scheduler_events_for_task_completion.notify_data());\n        let mut pending_task_order: std::collections::VecDeque<flowrt::TaskId> = std::collections::VecDeque::new();\n        let mut pending_task_results: std::collections::BTreeMap<flowrt::TaskId, flowrt::TaskRunOutput<Vec<FlowrtOutputCommit>>> = std::collections::BTreeMap::new();\n        let mut pending_task_admissions: std::collections::BTreeMap<flowrt::TaskId, flowrt::TaskAdmission> = std::collections::BTreeMap::new();\n        let task_health_from_workers = std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeMap::<String, flowrt::IntrospectionTaskHealth>::new()));\n        let mut task_last_scheduled_time_ms: std::collections::BTreeMap<flowrt::TaskId, u64> = std::collections::BTreeMap::new();\n        let mut task_last_observed_time_ms: std::collections::BTreeMap<flowrt::TaskId, u64> = std::collections::BTreeMap::new();\n"
    ));
    output.push_str(&emit_rust_fault_state_decls(&recoverable, graph_stop));
    output.push_str(&emit_rust_failover_state_decls(&standby_failover));
    output.push_str(&emit_rust_injection_counter_decls(
        contract,
        &scheduler_plan.dataflow_tasks,
    ));
    output.push_str(
        "        while status == flowrt::Status::Ok\n            && !shutdown.is_requested()\n            && (run_ticks\n                .map(|limit| tick_base < limit)\n                .unwrap_or(true)\n                || !pending_task_order.is_empty())\n        {\n            let mut observed_data_generation: u64;\n",
    );
    if external_tick.is_none() {
        output.push_str(&rust_scheduler_data_time_update(contract, "            "));
    }
    output.push_str(
        "            let tick_time_ms = scheduler_now_ms;\n            scheduler.advance_to_ms(tick_time_ms);\n            scheduler.set_current_tick(tick_base as u64);\n",
    );
    output.push_str(&emit_rust_restart_driver(
        contract,
        order,
        &recoverable,
        graph_stop,
    ));
    output.push_str(&task_health_init);
    output.push_str(&emit_rust_apply_pending_params_for_order(contract, order));
    let has_service_tasks = !service_tasks.is_empty();
    let has_operation_tasks = !operation_tasks.is_empty();
    let woke_on_message_decl = if tasks.iter().any(|task| {
        matches!(
            task.trigger,
            TriggerKind::OnMessage | TriggerKind::OnSynchronized
        )
    }) || has_service_tasks
        || has_operation_tasks
    {
        "let mut woke_on_message = false;"
    } else {
        "let woke_on_message = false;"
    };
    output.push_str(&operation_emit::emit_rust_operation_tick_driver_state(
        &operation_tasks,
    ));
    output.push_str(&format!(
        "            introspection_state.record_tick_at(tick_time_ms, clock_source);\n            loop {{\n                observed_data_generation = scheduler_events.data_generation();\n                {woke_on_message_decl}\n"
    ));
    output.push_str(&crate::runtime_plan::indent_generated_block_levels(
        &emit_rust_on_message_wake_checks(graph, &tasks, binds, bridges, boundaries),
        1,
    ));
    // service wake checks
    let service_wake_checks =
        service_emit::emit_rust_service_wake_checks(contract, graph, &service_tasks);
    if !service_wake_checks.is_empty() {
        output.push_str(&crate::runtime_plan::indent_generated_block_levels(
            &service_wake_checks,
            1,
        ));
    }
    let operation_wake_checks =
        operation_emit::emit_rust_operation_wake_checks(contract, graph, &operation_tasks);
    if !operation_wake_checks.is_empty() {
        output.push_str(&crate::runtime_plan::indent_generated_block_levels(
            &operation_wake_checks,
            1,
        ));
    }
    output.push_str(
        "                for task_result in task_completion_queue.drain_completed() {\n                    pending_task_results.insert(task_result.task, task_result);\n                }\n                {\n                    let mut completed_health = task_health_from_workers.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                    health_map.append(&mut *completed_health);\n                }\n                let ready_batch = scheduler.take_ready_batch();\n                let submitted_task_count = ready_batch.len();\n                for admission in ready_batch.admissions().iter().copied() {\n                    let scheduled_delta_ms = task_last_scheduled_time_ms\n                        .insert(admission.task, admission.scheduled_time_ms)\n                        .map_or(0, |last| admission.scheduled_time_ms.saturating_sub(last));\n                    let observed_delta_ms = task_last_observed_time_ms\n                        .insert(admission.task, admission.observed_time_ms)\n                        .map_or(0, |last| admission.observed_time_ms.saturating_sub(last));\n                    let task_completion_queue_for_task = task_completion_queue.clone();\n                    let submitted = match admission.task {\n",
    );
    for task_plan in &scheduler_plan.dataflow_tasks {
        let task = task_plan.task;
        let task_id = task_plan.id;
        let lane_id = task_plan.lane_id;
        let function_name = match process {
            Some(process) => rust_process_task_step_function_name(process, task),
            None => rust_task_step_function_name(task),
        };
        output.push_str(&emit_rust_dataflow_submit_case(
            DataflowSubmitCaseEmission {
                contract,
                graph,
                binds,
                bridges,
                boundaries,
                function_name: &function_name,
                task,
                task_id,
                lane_id,
                task_name: &task_plan.timing_name,
                trigger: task_plan.trigger,
            },
        ));
    }
    output.push_str(&emit_rust_service_submit_cases(
        contract,
        graph,
        &service_tasks,
    ));
    output.push_str(&emit_rust_operation_submit_cases(
        contract,
        graph,
        &operation_tasks,
    ));
    output.push_str(&format!(
        "                        _ => {{\n                            let task_health_from_worker = task_health_from_workers.clone();\n                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {{\n{}                            let task_outcome = flowrt::TaskRunOutcome::error(Vec::new());\n{}                            }})\n                        }},\n                    }};\n",
        emit_rust_hidden_worker_closure_context("__flowrt_hidden"),
        emit_rust_scheduler_task_closure_tail(),
    ));
    let task_admission_health_update =
        emit_rust_task_admission_health_update(&scheduler_plan.dataflow_tasks);
    output.push_str(&format!(
        "                    match submitted {{\n                        Ok(()) => {{\n                            pending_task_order.push_back(admission.task);\n                            pending_task_admissions.insert(admission.task, admission);\n{task_admission_health_update}                        }}\n                        Err(_) => {{\n                            let _ = scheduler.complete_task(admission.task);\n                            status = flowrt::Status::Error;\n                            break;\n                        }}\n                    }}\n                }}\n                if status != flowrt::Status::Ok {{\n                    break;\n                }}\n                let mut committed_task_count = 0usize;\n                while let Some(task) = pending_task_order.front().copied() {{\n                    let Some(task_result) = pending_task_results.remove(&task) else {{\n                        break;\n                    }};\n                    pending_task_order.pop_front();\n                    let _ = scheduler.complete_task(task_result.task);\n                    committed_task_count += 1;\n{task_result_health_update}{task_error_handling}                    if let Some(commits) = task_result.outputs {{\n                        for commit in commits {{\n                            let commit_status = commit(app.as_ref(), &introspection_state, &scheduler_events, &mut health_map);\n                            if commit_status == flowrt::Status::Error {{\n                                status = flowrt::Status::Error;\n                                break;\n                            }}\n                            if commit_status == flowrt::Status::Retry {{\n                                status = flowrt::Status::Retry;\n                                break;\n                            }}\n                        }}\n                    }}\n                    if status != flowrt::Status::Ok {{\n                        break;\n                    }}\n                }}\n                if status != flowrt::Status::Ok {{\n                    break;\n                }}\n                if committed_task_count == 0 || (!woke_on_message && submitted_task_count == 0) {{\n                    break;\n                }}\n            }}\n            // 公平性检测：检查 lane 饥饿。\n{fairness_check}            // 将本轮健康快照写入 introspection。\n            for (_, health) in health_map.iter_mut() {{\n                introspection_state.record_task_health(health.clone());\n            }}\n            health_map.clear();\n{failover_boundary}{graph_stop_check}            if status == flowrt::Status::Ok {{\n                tick_base += 1;\n{advance_block}            }}\n        }}\n",
        fairness_check = emit_rust_fairness_check(&lane_ids),
        task_admission_health_update = task_admission_health_update,
        task_result_health_update = emit_rust_task_result_health_update(&scheduler_plan.dataflow_tasks),
        task_error_handling = emit_rust_task_error_handling(&recoverable, &standby_failover, graph_stop),
        failover_boundary = emit_rust_failover_boundary(&standby_failover),
        graph_stop_check = if graph_stop {
            "            if _graph_terminal_fault {\n                shutdown.request();\n            }\n"
        } else {
            ""
        },
        advance_block = if external_tick.is_some() {
            String::new()
        } else {
            rust_scheduler_advance_block(
                contract,
                &rust_next_periodic_deadline_expr(&scheduler_plan.dataflow_tasks),
            )
        },
    ));
    output
}

struct DataflowSubmitCaseEmission<'a> {
    contract: &'a ContractIr,
    graph: &'a GraphIr,
    binds: &'a [BindRuntimePlan],
    bridges: &'a [BridgeRuntimePlan],
    boundaries: &'a [BoundaryRuntimePlan],
    function_name: &'a str,
    task: &'a TaskIr,
    task_id: usize,
    lane_id: usize,
    task_name: &'a str,
    trigger: TriggerKind,
}

fn emit_rust_dataflow_submit_case(emission: DataflowSubmitCaseEmission<'_>) -> String {
    let DataflowSubmitCaseEmission {
        contract,
        graph,
        binds,
        bridges,
        boundaries,
        function_name,
        task,
        task_id,
        lane_id,
        task_name,
        trigger,
    } = emission;
    let trigger = crate::runtime_plan::runtime_trigger_name(trigger);
    let call_args = rust_collect_task_call_args_for_scheduler(contract, graph, task).join(", ");
    let capture_prelude =
        emit_rust_task_capture_prelude(contract, graph, binds, bridges, boundaries, task);
    let worker_ctx = emit_rust_worker_closure_context(task_name, trigger);
    let closure_tail = emit_rust_scheduler_task_closure_tail();
    // 真实任务调用块；非注入任务直接作为 task_outcome，字节与旧产物一致。
    let call_block = format!(
        "{{\n                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId({lane_id}));\n                                Self::{function_name}({call_args}, tick_time_ms as usize, &mut local_context, &introspection_state, &scheduler_events, &mut local_health_map)\n                            }}",
    );
    // test-only 故障注入门：命中调用序号时跳过用户回调、直接合成 error outcome（与真实回调返
    // Status::Error 的 TaskRunOutcome::error(empty) 字节等价），交既有 0.21.x 故障反应机器处理。
    let (inject_decl, task_outcome_expr) = match fault_injection_point_for(contract, task) {
        Some(point) => {
            let counter = format!("__inject_count_{task_id}");
            let flag = format!("__inject_fault_{task_id}");
            let hit = fault_injection_hit_expr(point, &counter);
            let decl = format!(
                "                            {counter} += 1;\n                            let {flag} = {hit};\n",
            );
            let expr = format!(
                "if {flag} {{\n                                flowrt::TaskRunOutcome::error(Vec::<FlowrtOutputCommit>::new())\n                            }} else {call_block}",
            );
            (decl, expr)
        }
        None => (String::new(), call_block),
    };
    format!(
        "                        flowrt::TaskId({task_id}) => {{\n{capture_prelude}                            let introspection_state = introspection_state.clone();\n                            let scheduler_events = scheduler_events.clone();\n                            let task_health_from_worker = task_health_from_workers.clone();\n{inject_decl}                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {{\n{worker_ctx}                            let task_outcome = {task_outcome_expr};\n{closure_tail}                            }})\n                        }},\n",
    )
}

fn rust_collect_task_call_args_for_scheduler(
    contract: &ContractIr,
    graph: &GraphIr,
    task: &TaskIr,
) -> Vec<String> {
    let instance = crate::instance_by_name(graph, &task.instance.name);
    let component = crate::component_by_name(contract, &instance.component.name);
    let mut args = Vec::new();
    args.push(step_emit::rust_task_component_capture_name(instance));

    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    for plan in crate::runtime_plan::client_service_plans(&service_plans, &instance.name) {
        args.push(step_emit::rust_task_service_client_capture_name(plan));
    }
    let operation_plans = crate::runtime_plan::operation_runtime_plans(contract, graph);
    for plan in crate::runtime_plan::client_operation_plans(&operation_plans, &instance.name) {
        args.push(step_emit::rust_task_operation_client_capture_name(plan));
    }
    if !component.params.is_empty() {
        args.push(step_emit::rust_task_params_capture_name(instance));
    }
    let effective_inputs = crate::runtime_plan::effective_task_inputs(graph, task);
    let task_inputs = effective_inputs
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    for input in &component.inputs {
        if task_inputs.contains(input.name.as_str()) {
            args.push(step_emit::rust_task_input_value_name(&input.name));
            args.push(step_emit::rust_task_input_stale_name(&input.name));
            args.push(step_emit::rust_task_input_revision_name(&input.name));
        }
    }
    if task.trigger == TriggerKind::OnSynchronized {
        args.push(step_emit::rust_synchronizer_field_name(task));
    }
    if let Some(plan) =
        crate::runtime_plan::standby_failover_plan_for_instance_in_graph(graph, &task.instance.name)
    {
        args.push(plan.active_field_name);
    }
    args
}

fn emit_rust_task_capture_prelude(
    contract: &ContractIr,
    graph: &GraphIr,
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
    task: &TaskIr,
) -> String {
    let instance = crate::instance_by_name(graph, &task.instance.name);
    let component = crate::component_by_name(contract, &instance.component.name);
    let mut output = String::new();
    output.push_str(&format!(
        "                            let {name} = self.{field}.clone();\n",
        name = step_emit::rust_task_component_capture_name(instance),
        field = instance.name,
    ));

    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    for plan in crate::runtime_plan::client_service_plans(&service_plans, &instance.name) {
        output.push_str(&format!(
            "                            let {name} = self.{field}.clone();\n",
            name = step_emit::rust_task_service_client_capture_name(plan),
            field = service_emit::client_field_name(plan),
        ));
    }
    let operation_plans = crate::runtime_plan::operation_runtime_plans(contract, graph);
    for plan in crate::runtime_plan::client_operation_plans(&operation_plans, &instance.name) {
        output.push_str(&format!(
            "                            let {name} = self.{field}.clone();\n",
            name = step_emit::rust_task_operation_client_capture_name(plan),
            field = operation_emit::operation_client_field_name(plan),
        ));
    }
    if !component.params.is_empty() {
        output.push_str(&format!(
            "                            let {name} = self.{field}_params.clone();\n",
            name = step_emit::rust_task_params_capture_name(instance),
            field = instance.name,
        ));
    }

    let effective_inputs = crate::runtime_plan::effective_task_inputs(graph, task);
    let task_inputs = effective_inputs
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    for input in &component.inputs {
        if !task_inputs.contains(input.name.as_str()) {
            continue;
        }
        output.push_str(&emit_rust_input_snapshot_capture(
            input, instance, binds, bridges, boundaries, task,
        ));
    }
    if task.trigger == TriggerKind::OnSynchronized {
        output.push_str(&format!(
            "                            let {name} = self.{name}.clone();\n",
            name = step_emit::rust_synchronizer_field_name(task),
        ));
    }
    if let Some(plan) =
        crate::runtime_plan::standby_failover_plan_for_instance_in_graph(graph, &task.instance.name)
    {
        let active = plan.active_field_name;
        output.push_str(&format!(
            "                            let {active} = {active}.clone();\n",
        ));
    }
    output
}

fn emit_rust_input_snapshot_capture(
    input: &PortIr,
    instance: &InstanceIr,
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
    task: &TaskIr,
) -> String {
    let value = step_emit::rust_task_input_value_name(&input.name);
    let stale = step_emit::rust_task_input_stale_name(&input.name);
    let revision = step_emit::rust_task_input_revision_name(&input.name);
    if let Some(bind) = binds
        .iter()
        .find(|bind| bind.target_instance == instance.name && bind.target_port == input.name)
    {
        return emit_rust_bind_snapshot_capture(input, bind, task, &value, &stale, &revision);
    }
    if let Some(bridge) = bridges.iter().find(|bridge| {
        bridge.direction == flowrt_ir::Ros2BridgeDirection::Ros2ToFlowrt
            && bridge.boundary_endpoint.is_none()
            && bridge.source_instance == instance.name
            && bridge.source_port == input.name
    }) {
        return emit_rust_bridge_snapshot_capture(input, bridge, task, &value, &stale, &revision);
    }
    if let Some(boundary) = boundaries.iter().find(|boundary| {
        boundary.direction == flowrt_ir::BoundaryDirection::Input
            && boundary.instance == instance.name
            && boundary.port == input.name
    }) {
        return emit_rust_boundary_snapshot_capture(input, boundary, &value, &stale, &revision);
    }
    let ty = crate::messages::rust_type(&input.ty);
    format!(
        "                            let ({value}, {stale}, {revision}) = (None::<{ty}>, false, 0u64);\n"
    )
}

fn emit_rust_bind_snapshot_capture(
    input: &PortIr,
    bind: &BindRuntimePlan,
    task: &TaskIr,
    value: &str,
    stale: &str,
    revision: &str,
) -> String {
    let guard = format!("__flowrt_{}_snapshot_guard", bind.field_name);
    let view = format!(
        "__flowrt_{}_snapshot_view",
        crate::snake_identifier(&input.name)
    );
    if matches!(crate::runtime_plan::bind_backend(bind), "iox2" | "zenoh") {
        if matches!(
            task.trigger,
            TriggerKind::OnMessage | TriggerKind::OnSynchronized
        ) {
            return format!(
                "                            let ({value}, {stale}, {revision}) = {{\n                                let {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                                let {view} = {guard}.cached_latest_at(tick_time_ms);\n                                ({view}.as_ref().cloned(), {view}.stale(), {guard}.revision())\n                            }};\n",
                field = bind.field_name,
            );
        }
        return format!(
            "                            let ({value}, {stale}, {revision}) = {{\n                                let mut {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                                let ({view}, __flowrt_snapshot_revision) = match {guard}.receive_latest_with_revision_at(tick_time_ms) {{\n                                    Ok(value) => value,\n                                    Err(_) => {{\n                                        status = flowrt::Status::Error;\n                                        break;\n                                    }}\n                                }};\n                                ({view}.as_ref().cloned(), {view}.stale(), __flowrt_snapshot_revision)\n                            }};\n",
            field = bind.field_name,
        );
    }

    match bind.channel {
        ChannelKind::Latest => format!(
            "                            let ({value}, {stale}, {revision}) = {{\n                                let {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                                let {view} = {guard}.view_at(tick_time_ms);\n                                ({view}.as_ref().cloned(), {view}.stale(), {guard}.revision())\n                            }};\n",
            field = bind.field_name,
        ),
        ChannelKind::Fifo => format!(
            "                            let ({value}, {stale}, {revision}) = {{\n                                let mut {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                                let __flowrt_fifo_read = {guard}.pop_at(tick_time_ms);\n                                let {view} = __flowrt_fifo_read.view();\n                                ({view}.as_ref().cloned(), {view}.stale(), {guard}.revision())\n                            }};\n",
            field = bind.field_name,
        ),
    }
}

fn emit_rust_bridge_snapshot_capture(
    input: &PortIr,
    bridge: &BridgeRuntimePlan,
    task: &TaskIr,
    value: &str,
    stale: &str,
    revision: &str,
) -> String {
    let guard = format!("__flowrt_{}_snapshot_guard", bridge.field_name);
    let view = format!(
        "__flowrt_{}_snapshot_view",
        crate::snake_identifier(&input.name)
    );
    if matches!(
        task.trigger,
        TriggerKind::OnMessage | TriggerKind::OnSynchronized
    ) {
        return format!(
            "                            let ({value}, {stale}, {revision}) = {{\n                                let {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                                let {view} = {guard}.cached_latest_at(tick_time_ms);\n                                ({view}.as_ref().cloned(), {view}.stale(), {guard}.revision())\n                            }};\n",
            field = bridge.field_name,
        );
    }
    format!(
        "                            let ({value}, {stale}, {revision}) = {{\n                                let mut {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                                let ({view}, __flowrt_snapshot_revision) = match {guard}.receive_latest_with_revision_at(tick_time_ms) {{\n                                    Ok(value) => value,\n                                    Err(_) => {{\n                                        status = flowrt::Status::Error;\n                                        break;\n                                    }}\n                                }};\n                                ({view}.as_ref().cloned(), {view}.stale(), __flowrt_snapshot_revision)\n                            }};\n",
        field = bridge.field_name,
    )
}

fn emit_rust_boundary_snapshot_capture(
    input: &PortIr,
    boundary: &BoundaryRuntimePlan,
    value: &str,
    stale: &str,
    revision: &str,
) -> String {
    let read = format!(
        "__flowrt_{}_boundary_read",
        crate::snake_identifier(&input.name)
    );
    let view = format!(
        "__flowrt_{}_snapshot_view",
        crate::snake_identifier(&input.name)
    );
    format!(
        "                            let ({value}, {stale}, {revision}) = {{\n                                let {read} = self.{field}.read_at(tick_time_ms);\n                                let {view} = {read}.view();\n                                ({view}.as_ref().cloned(), {view}.stale(), {read}.revision())\n                            }};\n",
        field = boundary.field_name,
    )
}

fn emit_rust_worker_closure_context(task_name: &str, trigger: &str) -> String {
    format!(
        "                            let task_name = {task_name:?};\n                            let task_trigger = {trigger:?};\n                            let mut local_context = flowrt::Context::with_timing(flowrt::TaskTiming {{\n                                step: tick_base as u64,\n                                task_name: task_name.to_string(),\n                                trigger: task_trigger.to_string(),\n                                clock_source: task_clock_source,\n                                scheduled_time_ms: admission.scheduled_time_ms,\n                                observed_time_ms: admission.observed_time_ms,\n                                scheduled_delta_ms,\n                                observed_delta_ms,\n                                period_ms: admission.period_ms,\n                                deadline_ms: admission.deadline_ms,\n                                lateness_ms: admission.lateness_ms,\n                                missed_periods: admission.missed_periods,\n                                deadline_missed: admission.deadline_ms.map_or(false, |deadline_ms| admission.lateness_ms > deadline_ms),\n                                overrun: admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms),\n                            }});\n                            let mut local_health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();\n",
    )
}

fn emit_rust_hidden_worker_closure_context(task_name: &str) -> String {
    format!(
        "                            let task_name = {task_name:?};\n                            let mut local_health_map: std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth> = std::collections::BTreeMap::new();\n",
    )
}

fn emit_rust_service_submit_cases(
    contract: &ContractIr,
    graph: &GraphIr,
    service_tasks: &[&SchedulerHiddenTaskPlan],
) -> String {
    let plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    if plans.is_empty() || service_tasks.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for task in service_tasks {
        let plan = plans
            .iter()
            .find(|plan| plan.index == task.source_index)
            .expect("scheduler service task must reference a service plan");
        let task_id = task.id;
        let lane_id = task.lane_id;
        let server_field = service_emit::server_field_name(plan);
        let server_var = format!("__flowrt_{}", crate::snake_identifier(&server_field));
        let service_name = crate::rust_string_literal(&plan.service_name);
        output.push_str(&format!(
            "                        flowrt::TaskId({task_id}) => {{\n                            let {server_var} = self.{server_field}.clone();\n                            let introspection_state = introspection_state.clone();\n                            let task_health_from_worker = task_health_from_workers.clone();\n                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {{\n{}                            let task_outcome = {{\n                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId({lane_id}));\n                                {server_var}.process_pending_requests();\n                                {{\n                                    let service_stats = {server_var}.stats();\n                                    introspection_state.record_service_health(flowrt::IntrospectionServiceStatus {{\n                                        name: {service_name}.to_string(),\n                                        ready: true,\n                                        in_flight: {server_var}.in_flight_count() as u64,\n                                        queued: {server_var}.pending_count() as u64,\n                                        total_requests: service_stats.requests,\n                                        timeout_count: service_stats.timeout,\n                                        busy_count: service_stats.busy,\n                                        unavailable_count: service_stats.unavailable,\n                                        late_drop_count: service_stats.late_dropped,\n                                    }});\n                                }}\n                                flowrt::TaskRunOutcome::new(flowrt::Status::Ok, Vec::new())\n                            }};\n{}                            }})\n                        }},\n",
            emit_rust_hidden_worker_closure_context(&task.name),
            emit_rust_scheduler_task_closure_tail(),
        ));
    }
    output
}

fn emit_rust_operation_submit_cases(
    contract: &ContractIr,
    graph: &GraphIr,
    operation_tasks: &[&SchedulerHiddenTaskPlan],
) -> String {
    let plans = crate::runtime_plan::operation_runtime_plans(contract, graph);
    if plans.is_empty() || operation_tasks.is_empty() {
        return String::new();
    }
    let mut output = String::new();
    for task in operation_tasks {
        let plan = plans
            .iter()
            .find(|plan| plan.index == task.source_index)
            .expect("scheduler operation task must reference an operation plan");
        let task_id = task.id;
        let lane_id = task.lane_id;
        let operation_name = crate::rust_string_literal(&plan.operation_name);
        let owner_name =
            crate::rust_string_literal(&format!("{}.{}", plan.client_instance, plan.client_port));
        let start_server = operation_emit::operation_start_server_field_name(plan);
        let cancel_server = operation_emit::operation_cancel_server_field_name(plan);
        let status_server = operation_emit::operation_status_server_field_name(plan);
        let start_var = format!("__flowrt_{}", crate::snake_identifier(&start_server));
        let cancel_var = format!("__flowrt_{}", crate::snake_identifier(&cancel_server));
        let status_var = format!("__flowrt_{}", crate::snake_identifier(&status_server));
        let control_field = format!("operation_control_{}", plan.index);
        let control_var = format!("__flowrt_{control_field}");
        output.push_str(&format!(
            "                        flowrt::TaskId({task_id}) => {{\n                            let {start_var} = self.{start_server}.clone();\n                            let {cancel_var} = self.{cancel_server}.clone();\n                            let {status_var} = self.{status_server}.clone();\n                            let {control_var} = self.{control_field}.clone();\n                            let introspection_state = introspection_state.clone();\n                            let task_health_from_worker = task_health_from_workers.clone();\n                            worker_pool.submit_collect(admission.task, &task_completion_queue_for_task, move || {{\n{}                            let task_outcome = {{\n                                let _flowrt_lane_guard = flowrt::enter_lane(flowrt::LaneId({lane_id}));\n                                let operation_cancel_control = {control_var}.clone();\n                                introspection_state.register_operation_cancel_handler({operation_name}, move |operation_id| {{\n                                    let mut control = operation_cancel_control.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                                    let snapshot = control.snapshot();\n                                    if flowrt_operation_id_string(snapshot.id) != operation_id {{\n                                        return Err(format!(\"stale operation invocation `{{}}`; current is `{{}}`\", operation_id, flowrt_operation_id_string(snapshot.id)));\n                                    }}\n                                    control.request_cancel(snapshot.id, snapshot.owner).map_err(|error| error.to_string())?;\n                                    Ok(flowrt_operation_status_from_snapshot({operation_name}, {owner_name}, control.snapshot()))\n                                }});\n                                {start_var}.process_pending_requests();\n                                {cancel_var}.process_pending_requests();\n                                {status_var}.process_pending_requests();\n                                let mut operation_control = {control_var}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                                let _ = operation_control.check_deadline(flowrt::monotonic_time_ms());\n                                let snapshot = operation_control.snapshot();\n                                let events = operation_control.drain_events();\n                                drop(operation_control);\n                                for event in events {{\n                                    let operation_id = flowrt_operation_id_string(event.id);\n                                    match event.kind {{\n                                        flowrt::OperationRuntimeEventKind::StateChanged => {{\n                                            if let Some(state) = event.state {{\n                                                introspection_state.record_operation_transition({operation_name}, &operation_id, state.as_str(), Some({owner_name}), if state.is_terminal() {{ None }} else {{ Some(snapshot.deadline_ms) }});\n                                            }}\n                                        }}\n                                        flowrt::OperationRuntimeEventKind::Progress => {{\n                                            introspection_state.record_operation_progress({operation_name}, &operation_id, event.sequence.unwrap_or(0));\n                                        }}\n                                        flowrt::OperationRuntimeEventKind::Result => {{\n                                            let result = event.state.map(flowrt::OperationState::as_str).unwrap_or(\"succeeded\");\n                                            introspection_state.record_operation_result({operation_name}, &operation_id, result, None);\n                                        }}\n                                        flowrt::OperationRuntimeEventKind::Error => {{\n                                            let result = event.state.map(flowrt::OperationState::as_str).unwrap_or(\"failed\");\n                                            introspection_state.record_operation_result({operation_name}, &operation_id, result, Some(\"handler error\"));\n                                        }}\n                                    }}\n                                }}\n                                introspection_state.record_operation_health(flowrt_operation_status_from_snapshot({operation_name}, {owner_name}, snapshot));\n                                flowrt::TaskRunOutcome::new(flowrt::Status::Ok, Vec::new())\n                            }};\n{}                            }})\n                        }},\n",
            emit_rust_hidden_worker_closure_context(&task.name),
            emit_rust_scheduler_task_closure_tail(),
        ));
    }
    output
}

fn scheduler_clock_source(contract: &ContractIr) -> &'static str {
    contract.artifact.clock_source.label()
}

fn rust_scheduler_uses_data_time(contract: &ContractIr) -> bool {
    !contract.artifact.clock_source.is_realtime()
}

fn rust_scheduler_clock_init(contract: &ContractIr) -> String {
    if rust_scheduler_uses_data_time(contract) {
        String::new()
    } else {
        "        let scheduler_started_at = std::time::Instant::now();\n        let scheduler_runtime_now_ms = || -> u64 {\n            scheduler_started_at\n                .elapsed()\n                .as_millis()\n                .min(u128::from(u64::MAX)) as u64\n        };\n"
            .to_string()
    }
}

fn rust_scheduler_data_time_update(contract: &ContractIr, indent: &str) -> String {
    if rust_scheduler_uses_data_time(contract) {
        // replay 由 advance block 的 time driver 推进 scheduler_now_ms，loop 顶部不再读 data_time。
        String::new()
    } else {
        format!(
            "{indent}scheduler_now_ms = scheduler_now_ms.max(scheduler_runtime_now_ms());\n{indent}let _ = scheduler_events.take_data_time_ms();\n"
        )
    }
}

fn rust_task_clock_source_expr(contract: &ContractIr) -> &'static str {
    if contract.artifact.clock_source.is_realtime() {
        "flowrt::ClockSource::Runtime"
    } else {
        "flowrt::ClockSource::Replay"
    }
}

/// 生成 scheduler 唤醒与逻辑时钟推进块。
///
/// realtime：按 wall-clock deadline 等待下一个 periodic deadline 或数据事件，Timer 到点把
/// `scheduler_now_ms` 推进到该 deadline。simulated_replay：逻辑时钟只由注入事件驱动，
/// 不计算 `Instant::now()` deadline、不被 wall-clock 节拍绑死，只等待下一个数据事件或关停；
/// 周期 task 在 `advance_to_ms` 时按 `missed_periods` 自动 catch-up，因此回放结果只取决于事件
/// 序列，与回放物理快慢无关 (G2)。逐周期回放步进留待 runtime 原生确定性回放驱动补齐。
fn rust_scheduler_wake_block(
    contract: &ContractIr,
    next_deadline_expr: &str,
    data_event_update: &str,
) -> String {
    if rust_scheduler_uses_data_time(contract) {
        format!(
            "                match scheduler_events.wait_until_after(observed_data_generation, None, &shutdown) {{\n                    flowrt::ScheduleEvent::Shutdown => break,\n                    flowrt::ScheduleEvent::Timer => {{}}\n                    flowrt::ScheduleEvent::Data => {{\n{data_event_update}                    }}\n                }}\n"
        )
    } else {
        format!(
            "                let next_periodic_deadline_ms = {next_deadline_expr};\n                let next_wake_deadline = next_periodic_deadline_ms.map(|deadline_ms| {{\n                    std::time::Instant::now()\n                        + std::time::Duration::from_millis(deadline_ms.saturating_sub(scheduler_now_ms))\n                }});\n                match scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, &shutdown) {{\n                    flowrt::ScheduleEvent::Shutdown => break,\n                    flowrt::ScheduleEvent::Timer => {{\n                        scheduler_now_ms = next_periodic_deadline_ms\n                            .unwrap_or_else(|| scheduler_now_ms.saturating_add(scheduler_base_period_ms));\n                    }}\n                    flowrt::ScheduleEvent::Data => {{\n{data_event_update}                    }}\n                }}\n"
        )
    }
}

/// 为 replay 时钟源生成运行时原生回放驱动初始化。
///
/// 设置 `FLOWRT_REPLAY_SOURCE` 时读取其指向的 MCAP，只装配目标在本图 input boundary 名集合内的
/// 外部激励事件，进入 runtime 原生确定性回放；加载失败 `eprintln` 并把 `status` 置 `Error`
/// （不 panic）。未设置环境变量时 driver 为 `None`，调度回退到外部 socket 注入（`flowrt replay` /
/// temporary island 交互式回放），不是错误。realtime 时钟源不生成本块。
fn rust_scheduler_replay_driver_init(
    contract: &ContractIr,
    boundaries: &[BoundaryRuntimePlan],
) -> String {
    if !rust_scheduler_uses_data_time(contract) {
        return String::new();
    }
    let names = boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Input)
        .map(|boundary| format!("{:?}", boundary.endpoint_name))
        .collect::<Vec<_>>()
        .join(", ");
    let template = r#"        let replay_boundary_inputs: std::collections::BTreeSet<String> = [__NAMES__].into_iter().map(::std::string::String::from).collect();
        let mut replay_time_driver = match std::env::var("FLOWRT_REPLAY_SOURCE") {
            Ok(replay_source) if !replay_source.is_empty() => {
                match flowrt::replay_driver_from_mcap(std::path::Path::new(&replay_source), &replay_boundary_inputs) {
                    Ok(replay_driver) => Some(replay_driver),
                    Err(error) => {
                        eprintln!("FlowRT: 无法加载 FLOWRT_REPLAY_SOURCE `{replay_source}`: {error}");
                        status = flowrt::Status::Error;
                        None
                    }
                }
            }
            _ => None,
        };
"#;
    template.replace("__NAMES__", &names)
}

/// 生成 scheduler 每个 tick 之后推进逻辑时钟的块。
///
/// realtime：保持既有行为——run_ticks 有界且无 pending 时按 base period 推进并 continue，否则
/// 按 wall-clock deadline 等待下一个 periodic deadline 或数据事件。simulated_replay 两种子模式由
/// `FLOWRT_REPLAY_SOURCE` 选择：设置时走 runtime 原生确定性回放（`ReplayDriver` 在「下一个事件
/// 时间」与「下一个 periodic 网格点」间逐步推进，命中事件经 `publish_boundary_input` 注入）；
/// 未设置时回退到外部 socket 注入（`flowrt replay` / temporary island 交互式回放），等待注入的
/// 边界事件并按其 `published_at_ms` 推进逻辑时钟。
fn rust_scheduler_advance_block(contract: &ContractIr, next_deadline_expr: &str) -> String {
    if rust_scheduler_uses_data_time(contract) {
        let template = r#"                if let Some(replay_driver) = replay_time_driver.as_mut() {
                    if shutdown.is_requested() {
                        break;
                    }
                    let next_periodic_deadline_ms = __NEXT_DEADLINE__;
                    match replay_driver.step(next_periodic_deadline_ms) {
                        flowrt::Step::Shutdown => break,
                        flowrt::Step::Timer => {
                            scheduler_now_ms = replay_driver.now_ms();
                        }
                        flowrt::Step::Data => {
                            scheduler_now_ms = replay_driver.now_ms();
                            for replay_event in replay_driver.take_pending_events() {
                                let _ = introspection_state.publish_boundary_input(
                                    &replay_event.target,
                                    replay_event.payload,
                                    Some(replay_event.time_ms),
                                );
                            }
                        }
                    }
                } else {
                    match scheduler_events.wait_until_after(observed_data_generation, None, &shutdown) {
                        flowrt::ScheduleEvent::Shutdown => break,
                        flowrt::ScheduleEvent::Timer => {}
                        flowrt::ScheduleEvent::Data => {
                            if let Some(data_time_ms) = scheduler_events.take_data_time_ms() {
                                scheduler_now_ms = scheduler_now_ms.max(data_time_ms);
                            }
                        }
                    }
                }
"#;
        template.replace("__NEXT_DEADLINE__", next_deadline_expr)
    } else {
        let mut block = String::from(
            "                if run_ticks.is_some() && pending_task_order.is_empty() {\n                    scheduler_now_ms = scheduler_now_ms.saturating_add(scheduler_base_period_ms);\n                    continue;\n                }\n",
        );
        block.push_str(&rust_scheduler_wake_block(
            contract,
            next_deadline_expr,
            &rust_scheduler_data_time_update(contract, "                        "),
        ));
        block
    }
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
            "        self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).set_schedule_waiter(scheduler_events.clone());\n",
            field = bind.field_name
        ));
    }
    for bridge in bridges {
        output.push_str(&format!(
            "        self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).set_schedule_waiter(scheduler_events.clone());\n",
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

fn emit_rust_task_result_health_update(tasks: &[SchedulerDataflowTaskPlan<'_>]) -> String {
    let mut output = String::new();
    output.push_str("                    match task_result.task {\n");
    for task in tasks {
        let task_id = task.id;
        let task_health = &task.timing_name;
        let lane = &task.lane;
        output.push_str(&format!(
            "                        flowrt::TaskId({task_id}) => {{\n                            let health = health_map.entry({task_health:?}.to_string()).or_default();\n                            health.name = {task_health:?}.to_string();\n                            health.lane = {lane:?}.to_string();\n                            health.inflight = false;\n                            if let Some(admission) = pending_task_admissions.remove(&task_result.task) {{\n                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);\n                                health.observed_time_ms = Some(admission.observed_time_ms);\n                                health.lateness_ms = Some(admission.lateness_ms);\n                                health.missed_periods = Some(admission.missed_periods);\n                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));\n                            }}\n                            health.run_count += 1;\n                            health.last_run_ms = Some(tick_time_ms);\n                            if task_result.status == flowrt::Status::Ok {{\n                                health.success_count += 1;\n                                health.consecutive_failures = 0;\n                                health.last_success_ms = Some(tick_time_ms);\n                            }} else if task_result.status == flowrt::Status::Error {{\n                                health.consecutive_failures += 1;\n                            }}\n                        }}\n"
        ));
    }
    output.push_str("                        _ => {}\n                    }\n");
    output
}

fn emit_rust_scheduler_task_closure_tail() -> String {
    "                            if let Some(health) = local_health_map.get_mut(task_name) {\n                                health.inflight = false;\n                                health.scheduled_time_ms = Some(admission.scheduled_time_ms);\n                                health.observed_time_ms = Some(admission.observed_time_ms);\n                                health.lateness_ms = Some(admission.lateness_ms);\n                                health.missed_periods = Some(admission.missed_periods);\n                                health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));\n                            }\n                            {\n                                let mut merged_health = task_health_from_worker.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n                                for (name, health) in local_health_map {\n                                    merged_health.insert(name, health);\n                                }\n                            }\n                            task_outcome\n".to_string()
}

fn emit_rust_task_admission_health_update(tasks: &[SchedulerDataflowTaskPlan<'_>]) -> String {
    let mut output = String::new();
    output.push_str("                            match admission.task {\n");
    for task in tasks {
        let task_id = task.id;
        let task_health = &task.timing_name;
        let lane = &task.lane;
        output.push_str(&format!(
            "                                flowrt::TaskId({task_id}) => {{\n                                    let health = health_map.entry({task_health:?}.to_string()).or_default();\n                                    health.name = {task_health:?}.to_string();\n                                    health.lane = {lane:?}.to_string();\n                                    health.inflight = true;\n                                    health.scheduled_time_ms = Some(admission.scheduled_time_ms);\n                                    health.observed_time_ms = Some(admission.observed_time_ms);\n                                    health.lateness_ms = Some(admission.lateness_ms);\n                                    health.missed_periods = Some(admission.missed_periods);\n                                    health.overrun = Some(admission.missed_periods > 0 || admission.period_ms.map_or(false, |period_ms| admission.lateness_ms > period_ms));\n                                }}\n"
        ));
    }
    output.push_str("                                _ => {}\n                            }\n");
    output
}

/// 为本轮 scheduler 预注册 task health 条目，确保未运行 task 也能记录公平性计数。
fn emit_rust_task_health_init(tasks: &[SchedulerDataflowTaskPlan<'_>]) -> String {
    let mut output = String::new();
    for task in tasks {
        let task_health = &task.timing_name;
        let lane = &task.lane;
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

fn rust_next_periodic_deadline_expr(tasks: &[SchedulerDataflowTaskPlan<'_>]) -> String {
    let deadlines = tasks
        .iter()
        .filter(|task| task.periodic_wake)
        .map(|task| format!("scheduler.next_deadline_ms(flowrt::TaskId({}))", task.id))
        .collect::<Vec<_>>();
    if deadlines.is_empty() {
        "None::<u64>".to_string()
    } else {
        format!("[{}].into_iter().flatten().min()", deadlines.join(", "))
    }
}

/// instance 重启的生命周期上下文变量名（native 用共享 `lifecycle_context`，io_boundary 用专属）。
fn rust_restart_context_name(component: &flowrt_ir::ComponentIr, name: &str) -> String {
    if component.kind == flowrt_ir::ComponentKind::IoBoundary {
        format!("{}_boundary_context", crate::snake_identifier(name))
    } else {
        "lifecycle_context".to_string()
    }
}

/// 退避表达式：`(initial << consecutive.min(31)).min(max)`，单位 clock-ms。
fn rust_backoff_expr(var: &str, initial_delay_ms: u64, max_delay_ms: u64) -> String {
    format!("({initial_delay_ms}u64 << {var}_fault_consecutive.min(31)).min({max_delay_ms}u64)")
}

/// 为 restart / degrade 策略 instance 生成故障状态局部变量声明（8 空格缩进，循环外）。
///
/// restart 用 `_next_restart_ms`/`_fault_consecutive`/`_terminal_faulted` 驱动退避重启；
/// degrade 仅用一个 `_degraded` bool 做边沿跟踪，避免每拍重复调用 `record_lifecycle_state`。
fn emit_rust_fault_state_decls(
    recoverable: &[crate::runtime_plan::RecoverableInstancePlan],
    graph_stop: bool,
) -> String {
    let mut output = String::new();
    if graph_stop {
        output.push_str("        let mut _graph_terminal_fault = false;\n");
    }
    for plan in recoverable {
        let var = crate::snake_identifier(&plan.name);
        match plan.policy {
            flowrt_ir::InstanceFailurePolicy::Restart => {
                output.push_str(&format!(
                    "        let mut {var}_next_restart_ms: Option<u64> = None;\n        let mut {var}_fault_consecutive: u32 = 0;\n        let mut {var}_terminal_faulted = false;\n",
                ));
            }
            flowrt_ir::InstanceFailurePolicy::Degrade => {
                output.push_str(&format!("        let mut {var}_degraded = false;\n"));
            }
            _ => {}
        }
    }
    output
}

fn emit_rust_failover_state_decls(plans: &[crate::runtime_plan::StandbyFailoverPlan]) -> String {
    let mut output = String::new();
    for plan in plans.iter().filter(|plan| !plan.standby.is_empty()) {
        output.push_str(&format!(
            "        let mut {active} = {primary}.to_string();\n        let mut {pending} = false;\n",
            active = plan.active_field_name,
            primary = crate::rust_string_literal(&plan.primary),
            pending = plan.pending_field_name,
        ));
    }
    output
}

fn emit_rust_failover_boundary(plans: &[crate::runtime_plan::StandbyFailoverPlan]) -> String {
    let mut output = String::new();
    for plan in plans.iter().filter(|plan| !plan.standby.is_empty()) {
        let active = &plan.active_field_name;
        let pending = &plan.pending_field_name;
        let old_active = format!(
            "flowrt_old_active_redundancy_{}",
            crate::snake_identifier(&plan.group)
        );
        let primary = crate::rust_string_literal(&plan.primary);
        let standby = crate::rust_string_literal(&plan.standby[0]);
        let group = crate::rust_string_literal(&plan.group);
        output.push_str(&format!(
            "            if {pending} {{\n                if {active} == {primary} {{\n                    let {old_active} = {active}.clone();\n                    {active} = {standby}.to_string();\n                    introspection_state.record_failover(flowrt::IntrospectionFailoverEvent {{\n                        event: \"failover\".to_string(),\n                        group: {group}.to_string(),\n                        old_active: {old_active},\n                        new_active: {standby}.to_string(),\n                        tick_id: tick_base as u64,\n                        reason: \"critical_fault\".to_string(),\n                    }});\n                }}\n                {pending} = false;\n            }}\n"
        ));
    }
    output
}

fn rust_failover_pending_mark(
    instance: &str,
    plans: &[crate::runtime_plan::StandbyFailoverPlan],
) -> String {
    plans
        .iter()
        .filter(|plan| plan.primary == instance && !plan.standby.is_empty())
        .map(|plan| {
            format!(
                "                            if {active} == {primary} {{\n                                {pending} = true;\n                            }}\n",
                active = plan.active_field_name,
                primary = crate::rust_string_literal(&plan.primary),
                pending = plan.pending_field_name,
            )
        })
        .collect()
}

/// 查找命中给定 task 的 test-only 故障注入点（按 EntityId 匹配）。
fn fault_injection_point_for<'a>(
    contract: &'a ContractIr,
    task: &TaskIr,
) -> Option<&'a flowrt_ir::FaultInjectionPointIr> {
    crate::runtime_plan::fault_injection_point_for(contract, task)
}

/// 注入命中布尔表达式：调用计数命中显式集合或达到 from_invocation 起点即触发。
fn fault_injection_hit_expr(point: &flowrt_ir::FaultInjectionPointIr, counter_var: &str) -> String {
    let mut clauses = Vec::new();
    if !point.invocations.is_empty() {
        let list = point
            .invocations
            .iter()
            .map(|n| format!("{n}u64"))
            .collect::<Vec<_>>()
            .join(", ");
        clauses.push(format!("[{list}].contains(&{counter_var})"));
    }
    if let Some(from) = point.from_invocation {
        clauses.push(format!("{counter_var} >= {from}u64"));
    }
    clauses.join(" || ")
}

/// 为每个故障注入目标 task 生成 per-task 调用计数器局部变量（8 空格缩进，循环外）。
///
/// 计数器在 scheduler 线程每次 admission 自增（pre-execution），与 fault 状态同处运行循环外，
/// 使 `(instance, task, 第 N 次调用)` 锚点在 simulated_replay 下逐次确定。
fn emit_rust_injection_counter_decls(
    contract: &ContractIr,
    dataflow_tasks: &[crate::runtime_plan::SchedulerDataflowTaskPlan<'_>],
) -> String {
    let mut output = String::new();
    for plan in dataflow_tasks {
        if fault_injection_point_for(contract, plan.task).is_some() {
            output.push_str(&format!(
                "        let mut __inject_count_{}: u64 = 0;\n",
                plan.id
            ));
        }
    }
    output
}

/// 在调度 tick 顶部生成 restart-due 驱动：到点重跑 on_init→on_start（同一对象），成功恢复
/// 调度并回 Running、清零退避；失败递增连续计数，达 max_restarts 进入终态 Faulted，否则按
/// 退避排下次。退避按 clock-ms 度量，回放下重启时机确定。
fn emit_rust_restart_driver(
    contract: &ContractIr,
    order: &[&InstanceIr],
    recoverable: &[crate::runtime_plan::RecoverableInstancePlan],
    graph_stop: bool,
) -> String {
    let mut output = String::new();
    let graph_terminal = if graph_stop {
        "                            _graph_terminal_fault = true;\n"
    } else {
        ""
    };
    for plan in recoverable {
        if plan.policy != flowrt_ir::InstanceFailurePolicy::Restart {
            continue;
        }
        let Some(restart) = plan.restart else {
            continue;
        };
        let instance = order
            .iter()
            .find(|instance| instance.name == plan.name)
            .expect("recoverable instance must be in order");
        let component = crate::component_by_name(contract, &instance.component.name);
        let ctx = rust_restart_context_name(component, &plan.name);
        let var = crate::snake_identifier(&plan.name);
        let lit = crate::rust_string_literal(&plan.name);
        let on_init = super::rust_component_method_call(
            component,
            &instance.name,
            &format!("on_init(&mut {ctx})"),
        );
        let on_start = super::rust_component_method_call(
            component,
            &instance.name,
            &format!("on_start(&mut {ctx})"),
        );
        let resume = plan
            .task_ids
            .iter()
            .map(|id| {
                format!("                        scheduler.resume_task(flowrt::TaskId({id}));\n")
            })
            .collect::<String>();
        let backoff = rust_backoff_expr(&var, restart.initial_delay_ms, restart.max_delay_ms);
        output.push_str(&format!(
            "            if let Some({var}_due_ms) = {var}_next_restart_ms {{\n                if scheduler_now_ms >= {var}_due_ms {{\n                    {var}_next_restart_ms = None;\n                    let mut {var}_restart_status = {on_init};\n                    if {var}_restart_status == flowrt::Status::Ok {{\n                        {var}_restart_status = {on_start};\n                    }}\n                    if {var}_restart_status == flowrt::Status::Ok {{\n                        {var}_fault_consecutive = 0;\n                        introspection_state.record_lifecycle_state({lit}, flowrt::LifecycleState::Running);\n{resume}                    }} else {{\n                        {var}_fault_consecutive += 1;\n                        introspection_state.record_lifecycle_state({lit}, flowrt::LifecycleState::Faulted);\n                        if {var}_fault_consecutive >= {max_restarts} {{\n                            {var}_terminal_faulted = true;\n{graph_terminal}                        }} else {{\n                            {var}_next_restart_ms = Some(scheduler_now_ms.saturating_add({backoff}));\n                        }}\n                    }}\n                }}\n            }}\n",
            max_restarts = restart.max_restarts,
        ));
    }
    output
}

/// 生成 commit drain 段对 task Error 的处理。
///
/// 无 recoverable instance 时返回既有行为（status=Error;break）；否则对属于 isolate/restart
/// instance 的 task 隔离续跑（不置 status、不 break），restart 还排下次重启；degrade instance
/// 的 task 仅记 `Degraded` 不挂起、不停图，并在该 task 后续返 Ok 时翻回 `Running`；其余 task 仍 fail_fast。
fn emit_rust_task_error_handling(
    recoverable: &[crate::runtime_plan::RecoverableInstancePlan],
    standby_failover: &[crate::runtime_plan::StandbyFailoverPlan],
    graph_stop: bool,
) -> String {
    if recoverable.is_empty() {
        return "                    if task_result.status == flowrt::Status::Error {\n                        status = flowrt::Status::Error;\n                        break;\n                    }\n".to_string();
    }
    let mut arms = String::new();
    let mut recovery_arms = String::new();
    for plan in recoverable {
        if plan.task_ids.is_empty() {
            continue;
        }
        let var = crate::snake_identifier(&plan.name);
        let lit = crate::rust_string_literal(&plan.name);
        let pattern = plan
            .task_ids
            .iter()
            .map(|id| format!("flowrt::TaskId({id})"))
            .collect::<Vec<_>>()
            .join(" | ");
        if plan.policy == flowrt_ir::InstanceFailurePolicy::Degrade {
            arms.push_str(&format!(
                "                        {pattern} => {{\n                            if !{var}_degraded {{\n                                introspection_state.record_lifecycle_state({lit}, flowrt::LifecycleState::Degraded);\n                                {var}_degraded = true;\n                            }}\n                        }}\n",
            ));
            recovery_arms.push_str(&format!(
                "                        {pattern} => {{\n                            if {var}_degraded {{\n                                introspection_state.record_lifecycle_state({lit}, flowrt::LifecycleState::Running);\n                                {var}_degraded = false;\n                            }}\n                        }}\n",
            ));
            continue;
        }
        let suspend = plan
            .task_ids
            .iter()
            .map(|id| {
                format!(
                    "                            scheduler.suspend_task(flowrt::TaskId({id}));\n"
                )
            })
            .collect::<String>();
        let restart_schedule = match (plan.policy, plan.restart) {
            (flowrt_ir::InstanceFailurePolicy::Restart, Some(restart)) => {
                let backoff =
                    rust_backoff_expr(&var, restart.initial_delay_ms, restart.max_delay_ms);
                format!(
                    "                            if !{var}_terminal_faulted {{\n                                {var}_next_restart_ms = Some(scheduler_now_ms.saturating_add({backoff}));\n                            }}\n",
                )
            }
            _ => String::new(),
        };
        // isolate 故障即时终态：受控停机下置图级终态标记（restart 的终态在 restart_driver 置位）。
        let graph_terminal =
            if graph_stop && plan.policy == flowrt_ir::InstanceFailurePolicy::Isolate {
                "                            _graph_terminal_fault = true;\n"
            } else {
                ""
            };
        let failover_pending = rust_failover_pending_mark(&plan.name, standby_failover);
        arms.push_str(&format!(
            "                        {pattern} => {{\n                            introspection_state.record_lifecycle_state({lit}, flowrt::LifecycleState::Faulted);\n{failover_pending}{suspend}{graph_terminal}{restart_schedule}                        }}\n",
        ));
    }
    let mut output = format!(
        "                    if task_result.status == flowrt::Status::Error {{\n                        match task_result.task {{\n{arms}                            _ => {{\n                                status = flowrt::Status::Error;\n                                break;\n                            }}\n                        }}\n                    }}\n",
    );
    if !recovery_arms.is_empty() {
        output.push_str(&format!(
            "                    if task_result.status == flowrt::Status::Ok {{\n                        match task_result.task {{\n{recovery_arms}                            _ => {{}}\n                        }}\n                    }}\n",
        ));
    }
    output
}
