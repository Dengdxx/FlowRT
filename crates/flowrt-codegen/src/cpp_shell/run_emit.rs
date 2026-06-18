use super::*;

pub(super) fn emit_cpp_app_run_process_dispatch(processes: &[ProcessRuntimePlan<'_>]) -> String {
    let mut output = String::new();
    output.push_str(
        "flowrt::Status App::run_process(const flowrt::Backend& backend, std::string_view process, std::optional<std::size_t> run_ticks) {\n",
    );
    for process in processes {
        output.push_str(&format!(
            "    if (process == {}) {{\n        return run_process_{}(backend, run_ticks);\n    }}\n",
            cpp_string_literal(&process.name),
            process.method_suffix
        ));
    }
    output.push_str("    return flowrt::Status::Error;\n}\n\n");
    output
}

pub(super) struct CppRunEmission<'a> {
    pub(super) contract: &'a ContractIr,
    pub(super) function_name: &'a str,
    pub(super) step_function_name: &'a str,
    pub(super) startup_function_name: &'a str,
    pub(super) shutdown_function_name: &'a str,
    pub(super) order: &'a [&'a InstanceIr],
    pub(super) binds: &'a [BindRuntimePlan],
    pub(super) bridges: &'a [BridgeRuntimePlan],
    pub(super) boundaries: &'a [BoundaryRuntimePlan],
    pub(super) graph: &'a GraphIr,
    pub(super) process: Option<&'a ProcessRuntimePlan<'a>>,
    pub(super) package_name: &'a str,
    pub(super) process_name: &'a str,
    pub(super) mode: CppRunMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CppRunMode {
    SchedulerLoop,
    ExternalTick,
}

/// 为本进程内的反馈边 channel 播种初值（消息值初始化），与 Rust 侧一致。反馈边按单位延迟
/// 被拓扑剔除；启动期播种使首拍读到 present 初值而非空（latest 播 1 个，fifo 按 depth 播 N 个）。
/// init 省略时播零初值，给出时按源消息类型构造字面量。
fn emit_cpp_feedback_channel_seed(
    contract: &ContractIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
) -> String {
    let active = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut output = String::new();
    for bind in binds.iter().filter(|bind| bind.feedback) {
        if !active.contains(bind.source_instance.as_str()) {
            continue;
        }
        let value = crate::messages::cpp_feedback_seed_value(
            contract,
            &bind.source_type,
            bind.init.as_ref(),
        );
        let seed_count = match bind.channel {
            flowrt_ir::ChannelKind::Latest => 1,
            flowrt_ir::ChannelKind::Fifo => bind.deterministic_delay_ticks.unwrap_or(1).max(1),
        };
        if matches!(bind_backend(bind), "iox2" | "zenoh") {
            for _ in 0..seed_count {
                output.push_str(&format!(
                    "    {field}_.publish_at({value}, 0);\n",
                    field = bind.field_name,
                ));
            }
            continue;
        }
        match bind.channel {
            flowrt_ir::ChannelKind::Latest => {
                output.push_str(&format!(
                    "    {field}_.publish_at({value}, 0);\n",
                    field = bind.field_name,
                ));
            }
            flowrt_ir::ChannelKind::Fifo => {
                for _ in 0..seed_count {
                    output.push_str(&format!(
                        "    {field}_.push_at({value}, 0);\n",
                        field = bind.field_name,
                    ));
                }
            }
        }
    }
    output
}

pub(super) fn emit_cpp_app_run_function(run: &CppRunEmission<'_>) -> String {
    let mut output = String::new();
    match run.mode {
        CppRunMode::SchedulerLoop => output.push_str(&format!(
            "flowrt::Status App::{}(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {{\n",
            run.function_name
        )),
        CppRunMode::ExternalTick => output.push_str(&format!(
            "flowrt::Status App::{}(const flowrt::Backend& backend, flowrt::ExternalTick grant) {{\n",
            run.function_name
        )),
    }
    output.push_str(
        "    flowrt::Context lifecycle_context;\n    auto status = flowrt::Status::Ok;\n",
    );
    if run.mode == CppRunMode::ExternalTick {
        output.push_str(
            "    if (grant.tick_id > static_cast<std::uint64_t>(std::numeric_limits<std::size_t>::max() - std::size_t{1})) {\n        return flowrt::Status::Error;\n    }\n    const auto flowrt_external_tick_base = static_cast<std::size_t>(grant.tick_id);\n    const std::optional<std::size_t> run_ticks{flowrt_external_tick_base + std::size_t{1}};\n",
        );
    }
    output.push_str("    (void)backend;\n");
    output.push_str("    auto shutdown = flowrt::install_signal_shutdown_token();\n");
    output.push_str("    flowrt::IntrospectionState introspection_state;\n");
    output.push_str(&emit_cpp_graph_health_registration(run.graph));
    output.push_str("    flowrt::ScheduleWaiter scheduler_events;\n");
    output.push_str(&emit_cpp_scheduler_event_registration(
        run.binds,
        run.bridges,
        run.boundaries,
    ));
    output.push_str(
        "    introspection_state.set_self_description_json(std::string{flowrt_app::self_description_json()});\n",
    );
    output.push_str(&emit_cpp_introspection_channel_registration(
        run.contract,
        run.order,
        run.binds,
    ));
    output.push_str(&emit_cpp_introspection_param_registration(
        run.contract,
        run.order,
    ));
    output.push_str(&emit_cpp_resource_registration(
        run.graph,
        run.order,
        run.process_name,
    ));
    output.push_str(&emit_cpp_io_boundary_registration(run.contract, run.order));
    output.push_str(&emit_cpp_boundary_input_registration(
        run.contract,
        run.boundaries,
    ));
    output.push_str(&emit_cpp_boundary_output_probe_registration(run.boundaries));
    output.push_str(&format!(
        "    auto introspection_server = flowrt::spawn_status_server(\n        flowrt::IntrospectionIdentity{{\n            .self_description_hash = std::string{{flowrt_app::self_description_hash()}},\n            .package = {},\n            .process = {},\n            .runtime = \"cpp\",\n        }},\n        introspection_state);\n    (void)introspection_server;\n",
        cpp_string_literal(run.package_name),
        cpp_string_literal(run.process_name)
    ));
    output.push_str(&emit_cpp_feedback_channel_seed(
        run.contract,
        run.order,
        run.binds,
    ));
    for instance in run.order {
        output.push_str(&format!(
            "    bool {name}_initialized = false;\n    bool {name}_started = false;\n    introspection_state.record_lifecycle_state({lit}, flowrt::LifecycleState::Uninitialized);\n",
            name = instance.name,
            lit = cpp_string_literal(&instance.name),
        ));
    }
    output.push_str(&emit_cpp_io_boundary_contexts(run.contract, run.order));
    for instance in run.order {
        let component = component_by_name(run.contract, &instance.component.name);
        let context_name = cpp_lifecycle_context_name(component, instance);
        output.push_str(&format!(
            "    if (status == flowrt::Status::Ok && {name}_) {{\n        status = {name}_->on_init({context});\n        {name}_initialized = status == flowrt::Status::Ok;\n        introspection_state.record_lifecycle_state({lit}, {name}_initialized ? flowrt::LifecycleState::Initialized : flowrt::LifecycleState::Faulted);\n    }}\n",
            name = instance.name,
            context = context_name,
            lit = cpp_string_literal(&instance.name),
        ));
    }
    for instance in run.order {
        let component = component_by_name(run.contract, &instance.component.name);
        let context_name = cpp_lifecycle_context_name(component, instance);
        output.push_str(&format!(
            "    if (status == flowrt::Status::Ok && {name}_initialized && {name}_) {{\n        status = {name}_->on_start({context});\n        {name}_started = status == flowrt::Status::Ok;\n        introspection_state.record_lifecycle_state({lit}, {name}_started ? flowrt::LifecycleState::Running : flowrt::LifecycleState::Faulted);\n    }}\n",
            name = instance.name,
            context = context_name,
            lit = cpp_string_literal(&instance.name),
        ));
        if component
            .io_boundary
            .as_ref()
            .is_some_and(|policy| policy.readiness == IoBoundaryReadiness::ComponentStarted)
        {
            output.push_str(&format!(
                "    if ({name}_started) {{\n        if (auto* boundary = {context}.boundary(); boundary != nullptr) {{\n            boundary->mark_ready();\n        }}\n    }}\n",
                name = instance.name,
                context = context_name,
            ));
        }
    }
    output.push_str(&format!(
        "    if (status == flowrt::Status::Ok) {{\n        std::map<std::string, flowrt::IntrospectionTaskHealth> startup_health_map;\n        status = {}(0, lifecycle_context, introspection_state, scheduler_events, startup_health_map);\n    }}\n",
        run.startup_function_name
    ));
    output.push_str(&emit_cpp_zenoh_service_endpoints(
        run.contract,
        run.graph,
        run.order,
    ));
    output.push_str(&emit_cpp_scheduler_v2_loop(run));
    output.push_str(&format!(
        "    if (status == flowrt::Status::Ok) {{\n        std::map<std::string, flowrt::IntrospectionTaskHealth> shutdown_health_map;\n        status = {}(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);\n    }}\n",
        run.shutdown_function_name
    ));
    for instance in run.order.iter().rev() {
        let component = component_by_name(run.contract, &instance.component.name);
        let context_name = cpp_lifecycle_context_name(component, instance);
        output.push_str(&format!(
            "    if ({name}_started && {name}_) {{\n        const auto stop_status = {name}_->on_stop({context});\n        if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok) {{\n            status = flowrt::Status::Error;\n        }}\n        introspection_state.record_lifecycle_state({lit}, stop_status == flowrt::Status::Ok ? flowrt::LifecycleState::Stopped : flowrt::LifecycleState::Faulted);\n    }}\n",
            name = instance.name,
            context = context_name,
            lit = cpp_string_literal(&instance.name),
        ));
    }
    for instance in run.order.iter().rev() {
        let component = component_by_name(run.contract, &instance.component.name);
        let context_name = cpp_lifecycle_context_name(component, instance);
        output.push_str(&format!(
            "    if ({name}_initialized && {name}_) {{\n        const auto shutdown_status = {name}_->on_shutdown({context});\n        if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok) {{\n            status = flowrt::Status::Error;\n        }}\n        introspection_state.record_lifecycle_state({lit}, shutdown_status == flowrt::Status::Ok ? flowrt::LifecycleState::ShutDown : flowrt::LifecycleState::Faulted);\n    }}\n",
            name = instance.name,
            context = context_name,
            lit = cpp_string_literal(&instance.name),
        ));
    }
    output.push_str("    return status;\n}\n\n");
    output
}

fn emit_cpp_graph_health_registration(graph: &GraphIr) -> String {
    if graph.health.critical_instances.is_empty() {
        return String::new();
    }
    let critical = graph
        .health
        .critical_instances
        .iter()
        .map(|instance| cpp_string_literal(&instance.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!("    introspection_state.register_critical_instances({{{critical}}});\n")
}

/// 生成进程级 zenoh service 端点构造代码，注入 `App::run_process_*` 函数体（on_start 之后、
/// 调度循环之前）。在所属进程为 client 填充 transport client、为 server 打开 queryable
/// 并登记/标记 ready。server component 由 validator 保证 `parallel`，handler 在 queryable
/// 回调线程调用。无本进程 zenoh service 时返回空串，inproc 产物字节不漂移。
fn emit_cpp_zenoh_service_endpoints(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
) -> String {
    let plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    let active: std::collections::BTreeSet<&str> = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect();
    let zenoh_plans: Vec<&crate::runtime_plan::ServiceRuntimePlan> = plans
        .iter()
        .filter(|plan| plan.backend.0 == "zenoh")
        .filter(|plan| {
            active.contains(plan.client_instance.as_str())
                || active.contains(plan.server_instance.as_str())
        })
        .collect();
    if zenoh_plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    output.push_str("    if (status == flowrt::Status::Ok) {\n");
    output.push_str(
        "        auto zenoh_service_session = std::make_shared<::zenoh::Session>(flowrt::zenoh::open_zenoh_session_from_env());\n",
    );
    for plan in &zenoh_plans {
        let name = cpp_string_literal(&plan.service_name);
        let req_ty = cpp_type(&plan.request_type);
        let resp_ty = cpp_type(&plan.response_type);
        if active.contains(plan.client_instance.as_str()) {
            let client_field = cpp_service_client_field_name(plan);
            output.push_str(&format!(
                "        this->{client_field}_.bind(flowrt::zenoh::ZenohServiceClient<{req_ty}, {resp_ty}>::open({name}, zenoh_service_session));\n",
            ));
        }
        if active.contains(plan.server_instance.as_str()) {
            let server_field = cpp_service_server_field_name(plan);
            let server_instance = &plan.server_instance;
            let port = crate::snake_identifier(&plan.server_port);
            output.push_str(&format!(
                "        this->{server_field}_ = flowrt::zenoh::ZenohServiceServer<{req_ty}, {resp_ty}>::open(\n            {name}, zenoh_service_session,\n            [this](const {req_ty}& request) -> flowrt::ServiceResult<{resp_ty}> {{\n                if (!this->{server_instance}_) {{\n                    return flowrt::ServiceResult<{resp_ty}>::err(flowrt::ServiceError::Unavailable);\n                }}\n                return this->{server_instance}_->on_{port}_request(request);\n            }});\n        if (this->{server_field}_ && this->{server_field}_->ready()) {{\n            introspection_state.register_service({name});\n            introspection_state.mark_service_ready({name});\n        }} else {{\n            status = flowrt::Status::Error;\n        }}\n",
            ));
        }
    }
    output.push_str("    }\n");
    output
}

pub(super) fn emit_cpp_scheduler_v2_loop(run: &CppRunEmission<'_>) -> String {
    let scheduler_plan = scheduler_runtime_plan(run.contract, run.graph, run.order);
    let recoverable = recoverable_instances(run.contract, run.graph, run.order);
    let standby_failover =
        crate::runtime_plan::standby_failover_plans_for_order(run.graph, run.order);
    // 受控停机仅当图声明 on_faulted=stop 且存在 isolate/restart 实例时启用，镜像 Rust。
    let graph_stop = run.graph.health.on_faulted == GraphFaultReaction::Stop
        && recoverable.iter().any(|plan| {
            matches!(
                plan.policy,
                InstanceFailurePolicy::Isolate | InstanceFailurePolicy::Restart
            )
        });
    let tasks = scheduler_plan
        .dataflow_tasks
        .iter()
        .map(|task| task.task)
        .collect::<Vec<_>>();
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
    let external_tick = run.mode == CppRunMode::ExternalTick;
    let worker_threads = scheduler_plan.worker_threads;
    output.push_str(&format!(
        "    flowrt::DeterministicExecutor scheduler{{{worker_threads}}};\n    flowrt::WorkerPool worker_pool{{{worker_threads}}};\n",
    ));

    let lane_names = scheduler_plan
        .lanes
        .iter()
        .map(|lane| lane.name.clone())
        .collect::<BTreeSet<_>>();
    let service_plans = crate::runtime_plan::service_runtime_plans(run.contract, run.graph);
    let operation_plans = crate::runtime_plan::operation_runtime_plans(run.contract, run.graph);
    for lane in &lane_names {
        let lane_expr = cpp_lane_id_expr(lane);
        output.push_str(&format!(
            "    scheduler.add_lane({lane_expr}, flowrt::LaneKind::Serial);\n    (void){};\n",
            cpp_string_literal(lane),
        ));
    }
    for task in &scheduler_plan.dataflow_tasks {
        let task_id = task.id;
        let lane_id = cpp_lane_id_expr(&task.lane);
        let priority = task.priority;
        output.push_str(&format!(
            "    scheduler.add_task(flowrt::TaskSpec{{.id = flowrt::TaskId{{{task_id}}}, .lane = {lane_id}, .priority = {priority}}});\n"
        ));
        if let Some(deadline_ms) = task.deadline_ms {
            output.push_str(&format!(
                "    scheduler.set_task_deadline_ms(flowrt::TaskId{{{task_id}}}, std::uint64_t{{{deadline_ms}}});\n"
            ));
        }
        if task.periodic_wake {
            output.push_str(&format!(
                "    scheduler.add_periodic(flowrt::PeriodicSpec{{.task = flowrt::TaskId{{{task_id}}}, .period = std::chrono::milliseconds{{{}}}}});\n    scheduler.wake(flowrt::TaskId{{{task_id}}});\n",
                task.period_ms.unwrap_or(1)
            ));
        }
    }
    for task in &service_tasks {
        let task_id = task.id;
        let lane_id = cpp_lane_id_expr(&task.lane);
        let priority = task.priority;
        output.push_str(&format!(
            "    scheduler.add_task(flowrt::TaskSpec{{.id = flowrt::TaskId{{{task_id}}}, .lane = {lane_id}, .priority = {priority}}});\n"
        ));
    }
    for task in &operation_tasks {
        let task_id = task.id;
        let lane_id = cpp_lane_id_expr(&task.lane);
        let priority = task.priority;
        output.push_str(&format!(
            "    scheduler.add_task(flowrt::TaskSpec{{.id = flowrt::TaskId{{{task_id}}}, .lane = {lane_id}, .priority = {priority}}});\n"
        ));
    }
    output.push_str(&emit_cpp_on_message_revision_state(
        run.graph,
        &tasks,
        run.binds,
        run.bridges,
        run.boundaries,
    ));
    output.push_str(&format!(
        "    const auto scheduler_base_period_ms = std::uint64_t{{{}}};\n",
        scheduler_plan.scheduler_base_period_ms
    ));
    let task_health_init = emit_cpp_task_health_init(&scheduler_plan.dataflow_tasks);
    let clock_source = cpp_scheduler_clock_source(run.contract);
    let task_clock_source = cpp_task_clock_source_expr(run.contract);
    let tick_base_init = if external_tick {
        "flowrt_external_tick_base"
    } else {
        "0"
    };
    let scheduler_now_init = if external_tick {
        "grant.logical_time_ms"
    } else {
        "0"
    };
    output.push_str(&format!(
        "    std::size_t tick_base = {tick_base_init};\n    std::uint64_t scheduler_now_ms = {scheduler_now_init};\n    std::map<std::string, flowrt::IntrospectionTaskHealth> health_map;\n    constexpr std::uint64_t fairness_starvation_threshold = 10;\n",
    ));
    if !external_tick {
        output.push_str(&cpp_scheduler_clock_init(run.contract));
        output.push_str(&cpp_scheduler_replay_driver_init(
            run.contract,
            run.boundaries,
        ));
    }
    output.push_str(&format!(
        "    const auto clock_source = std::string_view{{{}}};\n",
        cpp_string_literal(clock_source)
    ));
    output.push_str(&format!(
        "    const auto task_clock_source = {task_clock_source};\n    flowrt::WorkerCompletionQueue<std::vector<FlowrtOutputCommit>> task_completion_queue;\n    task_completion_queue.set_wake_callback([&scheduler_events]() {{ scheduler_events.notify_data(); }});\n    std::deque<flowrt::TaskId> pending_task_order;\n    std::map<flowrt::TaskId, flowrt::TaskRunOutput<std::vector<FlowrtOutputCommit>>> pending_task_results;\n    std::map<flowrt::TaskId, flowrt::TaskAdmission> pending_task_admissions;\n    std::mutex task_health_mutex;\n    std::map<std::string, flowrt::IntrospectionTaskHealth> task_health_from_workers;\n    std::map<flowrt::TaskId, std::uint64_t> task_last_scheduled_time_ms;\n    std::map<flowrt::TaskId, std::uint64_t> task_last_observed_time_ms;\n"
    ));
    if external_tick {
        output.push_str("    (void)scheduler_base_period_ms;\n");
    }
    output.push_str(&emit_cpp_fault_state_decls(&recoverable, graph_stop));
    output.push_str(&emit_cpp_failover_state_decls(&standby_failover));
    output.push_str(&emit_cpp_injection_counter_decls(
        run.contract,
        &scheduler_plan.dataflow_tasks,
    ));
    output.push_str(
        "    while (status == flowrt::Status::Ok && !shutdown.is_requested() && ((!run_ticks.has_value() || tick_base < *run_ticks) || !pending_task_order.empty())) {\n        std::uint64_t observed_data_generation = scheduler_events.data_generation();\n",
    );
    if !external_tick {
        output.push_str(&cpp_scheduler_data_time_update(run.contract, "        "));
    }
    output.push_str(
        "        const auto tick_time_ms = scheduler_now_ms;\n        scheduler.advance_to(std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(tick_time_ms)});\n        scheduler.set_current_tick(static_cast<std::uint64_t>(tick_base));\n",
    );
    output.push_str(&emit_cpp_restart_driver(
        run.contract,
        run.order,
        &recoverable,
        graph_stop,
    ));
    output.push_str(&task_health_init);
    output.push_str(&emit_cpp_apply_pending_params_for_order(
        run.contract,
        run.order,
    ));
    for task in &operation_tasks {
        output.push_str(&format!(
            "        bool flowrt_operation_tick_driven_{} = false;\n",
            task.source_index
        ));
    }
    let has_inproc_service = !service_tasks.is_empty();
    let has_inproc_operation = !operation_tasks.is_empty();
    let woke_on_message_decl = if tasks.iter().any(|task| {
        matches!(
            task.trigger,
            flowrt_ir::TriggerKind::OnMessage | flowrt_ir::TriggerKind::OnSynchronized
        )
    }) || has_inproc_service
        || has_inproc_operation
    {
        "bool woke_on_message = false;"
    } else {
        "const bool woke_on_message = false;"
    };
    output.push_str(&format!(
        "        introspection_state.record_tick(tick_time_ms, clock_source);\n        while (true) {{\n            observed_data_generation = scheduler_events.data_generation();\n            {woke_on_message_decl}\n"
    ));
    output.push_str(&indent_generated_block_levels(
        &emit_cpp_on_message_wake_checks(run.graph, &tasks, run.binds, run.bridges, run.boundaries),
        1,
    ));
    // service wake checks
    for task in &service_tasks {
        let plan = service_plans
            .iter()
            .find(|plan| plan.index == task.source_index)
            .expect("scheduler service task must reference a service plan");
        let task_id = task.id;
        let server_field = cpp_service_server_field_name(plan);
        output.push_str(&format!(
            "            if ({server_field}_.has_value() && {server_field}_->pending_count() > 0) {{\n                scheduler.wake(flowrt::TaskId{{{task_id}}});\n                woke_on_message = true;\n            }}\n"
        ));
    }
    for task in &operation_tasks {
        let plan = operation_plans
            .iter()
            .find(|plan| plan.index == task.source_index)
            .expect("scheduler operation task must reference an operation plan");
        let task_id = task.id;
        let start_server = cpp_operation_start_server_field_name(plan);
        let cancel_server = cpp_operation_cancel_server_field_name(plan);
        let status_server = cpp_operation_status_server_field_name(plan);
        let operation_index = plan.index;
        output.push_str(&format!(
            "            const auto flowrt_operation_snapshot_{operation_index} = this->operation_control_{operation_index}_ ? this->operation_control_{operation_index}_->snapshot() : flowrt::OperationStatusSnapshot{{}};\n            const bool flowrt_operation_active_{operation_index} = !flowrt::is_terminal(flowrt_operation_snapshot_{operation_index}.state) && flowrt_operation_snapshot_{operation_index}.state != flowrt::OperationState::Idle;\n            if (({start_server}_.has_value() && {start_server}_->pending_count() > 0) || ({cancel_server}_.has_value() && {cancel_server}_->pending_count() > 0) || ({status_server}_.has_value() && {status_server}_->pending_count() > 0) || (flowrt_operation_active_{operation_index} && !flowrt_operation_tick_driven_{operation_index})) {{\n                scheduler.wake(flowrt::TaskId{{{task_id}}});\n                if (flowrt_operation_active_{operation_index}) {{\n                    flowrt_operation_tick_driven_{operation_index} = true;\n                }}\n                woke_on_message = true;\n            }}\n"
        ));
    }
    output.push_str(
        "            for (auto task_result : task_completion_queue.drain_completed()) {\n                pending_task_results.insert_or_assign(task_result.task, std::move(task_result));\n            }\n            {\n                std::lock_guard<std::mutex> lock(task_health_mutex);\n                for (auto &[name, health] : task_health_from_workers) {\n                    health_map.insert_or_assign(name, std::move(health));\n                }\n                task_health_from_workers.clear();\n            }\n            auto ready_batch = scheduler.take_ready_batch();\n            const auto submitted_task_count = ready_batch.size();\n            for (const auto admission : ready_batch.admissions()) {\n                const auto scheduled_delta_ms = [&]() -> std::uint64_t {\n                    const auto [it, inserted] = task_last_scheduled_time_ms.insert_or_assign(admission.task, admission.scheduled_time_ms);\n                    return inserted || admission.scheduled_time_ms < it->second ? 0U : admission.scheduled_time_ms - it->second;\n                }();\n                const auto observed_delta_ms = [&]() -> std::uint64_t {\n                    const auto [it, inserted] = task_last_observed_time_ms.insert_or_assign(admission.task, admission.observed_time_ms);\n                    return inserted || admission.observed_time_ms < it->second ? 0U : admission.observed_time_ms - it->second;\n                }();\n",
    );
    output.push_str(&emit_cpp_injection_decision(
        run.contract,
        &scheduler_plan.dataflow_tasks,
    ));
    output.push_str(
        "                const auto submitted = worker_pool.submit_collect(admission.task, task_completion_queue, [this, &introspection_state, &scheduler_events, &task_health_mutex, &task_health_from_workers, admission, scheduled_delta_ms, observed_delta_ms, task_clock_source, tick_base, tick_time_ms",
    );
    output.push_str(&cpp_failover_capture(&standby_failover));
    output.push_str(&cpp_injection_capture(
        run.contract,
        &scheduler_plan.dataflow_tasks,
    ));
    output.push_str(
        "]() {\n                    auto local_health_map = std::map<std::string, flowrt::IntrospectionTaskHealth>{};\n                    const auto [task_name, task_trigger] = [&]() -> std::pair<std::string_view, std::string_view> {\n                        switch (admission.task.value) {\n",
    );
    for task in &scheduler_plan.dataflow_tasks {
        let task_id = task.id;
        let task_name = &task.timing_name;
        let trigger = crate::runtime_plan::runtime_trigger_name(task.trigger);
        output.push_str(&format!(
            "                            case {task_id}: return {{{}, {}}};\n",
            cpp_string_literal(task_name),
            cpp_string_literal(trigger)
        ));
    }
    output.push_str(
        "                            default: return {\"__flowrt_hidden\", \"on_message\"};\n                        }\n                    }();\n                    auto local_context = flowrt::Context::with_timing(flowrt::TaskTiming{\n                        .step = static_cast<std::uint64_t>(tick_base),\n                        .task_name = std::string{task_name},\n                        .trigger = std::string{task_trigger},\n                        .clock_source = task_clock_source,\n                        .scheduled_time_ms = admission.scheduled_time_ms,\n                        .observed_time_ms = admission.observed_time_ms,\n                        .scheduled_delta_ms = scheduled_delta_ms,\n                        .observed_delta_ms = observed_delta_ms,\n                        .period_ms = admission.period_ms,\n                        .deadline_ms = admission.deadline_ms,\n                        .lateness_ms = admission.lateness_ms,\n                        .missed_periods = admission.missed_periods,\n                        .deadline_missed = admission.deadline_ms.has_value() && admission.lateness_ms > *admission.deadline_ms,\n                        .overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms),\n                    });\n                    auto merge_local_health = [&task_health_mutex, &task_health_from_workers, admission, task_name](std::map<std::string, flowrt::IntrospectionTaskHealth>&& local_health_map) {\n                        auto health_it = local_health_map.find(std::string{task_name});\n                        if (health_it != local_health_map.end()) {\n                            auto& health = health_it->second;\n                            health.inflight = false;\n                            health.scheduled_time_ms = admission.scheduled_time_ms;\n                            health.observed_time_ms = admission.observed_time_ms;\n                            health.lateness_ms = admission.lateness_ms;\n                            health.missed_periods = admission.missed_periods;\n                            health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);\n                        }\n                        std::lock_guard<std::mutex> lock(task_health_mutex);\n                        for (auto &[name, health] : local_health_map) {\n                            task_health_from_workers.insert_or_assign(name, std::move(health));\n                        }\n                    };\n                    switch (admission.task.value) {\n",
    );
    for task_plan in &scheduler_plan.dataflow_tasks {
        let task = task_plan.task;
        let task_id = task_plan.id;
        let lane_id = cpp_lane_id_expr(&task_plan.lane);
        let function_name = match run.process {
            Some(process) => cpp_process_task_step_function_name(process, task),
            None => cpp_task_step_function_name(task),
        };
        // test-only 故障注入门：命中时三元短路跳过用户回调，直接合成 error outcome（与回调返
        // Status::Error 等价），不影响非注入产物字节。
        let active_arg = crate::runtime_plan::standby_failover_plan_for_instance_in_graph(
            run.graph,
            &task.instance.name,
        )
        .map(|plan| format!("{}, ", plan.active_field_name))
        .unwrap_or_default();
        let call_expr = format!(
            "{function_name}(static_cast<std::size_t>(tick_time_ms), {active_arg}{injection_arg}local_context, introspection_state, scheduler_events, local_health_map)",
            injection_arg = cpp_task_injection_call_arg(run.contract, task)
                .map(|arg| format!("{arg}, "))
                .unwrap_or_default(),
        );
        let task_outcome_expr = if let Some(point) =
            crate::runtime_plan::scheduler_fault_injection_point_for(run.contract, task)
        {
            match point.kind {
                flowrt_ir::FaultInjectionKind::StatusError => format!(
                    "flowrt_inject_fault ? FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{{}}) : {call_expr}"
                ),
                flowrt_ir::FaultInjectionKind::Panic => format!(
                    "flowrt_inject_fault ? throw std::runtime_error({}) : {call_expr}",
                    cpp_string_literal(&cpp_fault_injection_panic_message(point))
                ),
                _ => call_expr,
            }
        } else {
            call_expr
        };
        output.push_str(&format!(
            "                    case {task_id}: {{\n\
                         auto flowrt_lane_guard = flowrt::enter_lane({lane_id});\n\
                         (void)flowrt_lane_guard;\n\
                         auto task_outcome = {task_outcome_expr};\n\
                         merge_local_health(std::move(local_health_map));\n\
                         return task_outcome;\n\
                     }}\n"
        ));
    }
    // service dispatch cases
    for task in &service_tasks {
        let plan = service_plans
            .iter()
            .find(|plan| plan.index == task.source_index)
            .expect("scheduler service task must reference a service plan");
        let task_id = task.id;
        let fn_name = cpp_service_step_fn_name(plan);
        let lane_id = cpp_lane_id_expr(&task.lane);
        output.push_str(&format!(
            "                    case {task_id}: {{\n\
                         auto flowrt_lane_guard = flowrt::enter_lane({lane_id});\n\
                         (void)flowrt_lane_guard;\n\
                         auto task_status = {fn_name}(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);\n\
                         merge_local_health(std::move(local_health_map));\n\
                         return FlowrtTaskOutcome{{.status = task_status, .outputs = std::vector<FlowrtOutputCommit>{{}}}};\n\
                     }}\n"
        ));
    }
    for task in &operation_tasks {
        let plan = operation_plans
            .iter()
            .find(|plan| plan.index == task.source_index)
            .expect("scheduler operation task must reference an operation plan");
        let task_id = task.id;
        let fn_name = cpp_operation_step_fn_name(plan);
        let lane_id = cpp_lane_id_expr(&task.lane);
        output.push_str(&format!(
            "                    case {task_id}: {{\n\
                         auto flowrt_lane_guard = flowrt::enter_lane({lane_id});\n\
                         (void)flowrt_lane_guard;\n\
                         auto task_status = {fn_name}(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);\n\
                         merge_local_health(std::move(local_health_map));\n\
                         return FlowrtTaskOutcome{{.status = task_status, .outputs = std::vector<FlowrtOutputCommit>{{}}}};\n\
                     }}\n"
        ));
    }
    if tasks.is_empty() && service_tasks.is_empty() && operation_tasks.is_empty() {
        output.push_str(&format!(
            "                    default: {{\n                        auto task_status = {}(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);\n                        merge_local_health(std::move(local_health_map));\n                        return FlowrtTaskOutcome{{.status = task_status, .outputs = std::vector<FlowrtOutputCommit>{{}}}};\n                    }}\n",
            run.step_function_name
        ));
    } else {
        output.push_str("                    default: return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});\n");
    }
    let task_admission_health_update =
        emit_cpp_task_admission_health_update(&scheduler_plan.dataflow_tasks);
    let task_result_health_update =
        emit_cpp_task_result_health_update(&scheduler_plan.dataflow_tasks);
    let fairness_check = emit_cpp_fairness_check(&lane_names);
    let data_event_case = format!(
        "case flowrt::ScheduleEvent::Data:\n{}                    break;",
        cpp_scheduler_data_time_update(run.contract, "                    ")
    );
    output.push_str(&format!(
        "                }}\n                }});\n                if (submitted.accepted) {{\n                    pending_task_order.push_back(admission.task);\n                    pending_task_admissions.insert_or_assign(admission.task, admission);\n{task_admission_health_update}                }} else {{\n                    (void)scheduler.complete_task(admission.task);\n                    status = flowrt::Status::Error;\n                    break;\n                }}\n            }}\n            if (status != flowrt::Status::Ok) {{\n                break;\n            }}\n            std::size_t committed_task_count = 0;\n            while (!pending_task_order.empty()) {{\n                const auto task = pending_task_order.front();\n                const auto result_it = pending_task_results.find(task);\n                if (result_it == pending_task_results.end()) {{\n                    break;\n                }}\n                auto task_result = std::move(result_it->second);\n                pending_task_results.erase(result_it);\n                pending_task_order.pop_front();\n                (void)scheduler.complete_task(task_result.task);\n                ++committed_task_count;\n{task_result_health_update}{task_error_handling}                if (task_result.outputs.has_value()) {{\n                    for (auto& commit : *task_result.outputs) {{\n                        const auto commit_status = commit(*this, introspection_state, scheduler_events, health_map);\n                        if (commit_status == flowrt::Status::Error) {{\n                            status = flowrt::Status::Error;\n                            break;\n                        }}\n                        if (commit_status == flowrt::Status::Retry) {{\n                            status = flowrt::Status::Retry;\n                            break;\n                        }}\n                    }}\n                }}\n                if (status != flowrt::Status::Ok) {{\n                    break;\n                }}\n            }}\n            if (status != flowrt::Status::Ok) {{\n                break;\n            }}\n            if (committed_task_count == 0U || (!woke_on_message && submitted_task_count == 0U)) {{\n                break;\n            }}\n        }}\n        // 公平性检测：检查 lane 饥饿。\n{fairness_check}        // 将本轮健康快照写入 introspection。\n        for (auto &[name, health] : health_map) {{\n            introspection_state.record_task_health(std::move(health));\n        }}\n        health_map.clear();\n{failover_boundary}{graph_stop_check}        if (status == flowrt::Status::Ok) {{\n            ++tick_base;\n{advance_block}        }}\n    }}\n",
        task_result_health_update = task_result_health_update,
        task_admission_health_update = task_admission_health_update,
        task_error_handling = emit_cpp_task_error_handling(&recoverable, &standby_failover, graph_stop),
        failover_boundary = emit_cpp_failover_boundary(&standby_failover),
            graph_stop_check = if graph_stop {
                "        if (_graph_terminal_fault) {\n            shutdown.request();\n        }\n"
            } else {
                ""
            },
            advance_block = if external_tick {
                String::new()
            } else {
                cpp_scheduler_advance_block(run.contract, &cpp_next_periodic_deadline_expr(&scheduler_plan.dataflow_tasks))
            }
        )
        .replace(
            "case flowrt::ScheduleEvent::Data:\n                    break;",
            &data_event_case,
        )
        .replace(
            "next_periodic_deadline_ms->value",
            "static_cast<std::uint64_t>(next_periodic_deadline_ms->count())",
        )
        .replace(
            r#"const auto scheduled_delta_ms = [&]() -> std::uint64_t {
                    const auto [it, inserted] = task_last_scheduled_time_ms.insert_or_assign(admission.task, admission.scheduled_time_ms);
                    return inserted || admission.scheduled_time_ms < it->second ? 0U : admission.scheduled_time_ms - it->second;
                }();
                const auto observed_delta_ms = [&]() -> std::uint64_t {
                    const auto [it, inserted] = task_last_observed_time_ms.insert_or_assign(admission.task, admission.observed_time_ms);
                    return inserted || admission.observed_time_ms < it->second ? 0U : admission.observed_time_ms - it->second;
                }();"#,
            r#"const auto scheduled_delta_ms = [&]() -> std::uint64_t {
                    const auto it = task_last_scheduled_time_ms.find(admission.task);
                    const auto delta = it == task_last_scheduled_time_ms.end() || admission.scheduled_time_ms < it->second ? 0U : admission.scheduled_time_ms - it->second;
                    task_last_scheduled_time_ms.insert_or_assign(admission.task, admission.scheduled_time_ms);
                    return delta;
                }();
                const auto observed_delta_ms = [&]() -> std::uint64_t {
                    const auto it = task_last_observed_time_ms.find(admission.task);
                    const auto delta = it == task_last_observed_time_ms.end() || admission.observed_time_ms < it->second ? 0U : admission.observed_time_ms - it->second;
                    task_last_observed_time_ms.insert_or_assign(admission.task, admission.observed_time_ms);
                    return delta;
                }();"#,
        ),
    );
    output
}

pub(super) fn cpp_scheduler_clock_source(contract: &ContractIr) -> &'static str {
    contract.artifact.clock_source.label()
}

fn cpp_scheduler_uses_data_time(contract: &ContractIr) -> bool {
    !contract.artifact.clock_source.is_realtime()
}

fn cpp_scheduler_clock_init(contract: &ContractIr) -> String {
    if cpp_scheduler_uses_data_time(contract) {
        String::new()
    } else {
        "    const auto scheduler_started_at = std::chrono::steady_clock::now();\n    const auto scheduler_runtime_now_ms = [&scheduler_started_at]() -> std::uint64_t {\n        const auto elapsed_ms = std::chrono::duration_cast<std::chrono::milliseconds>(\n                                    std::chrono::steady_clock::now() - scheduler_started_at)\n                                    .count();\n        return elapsed_ms <= 0 ? 0U : static_cast<std::uint64_t>(elapsed_ms);\n    };\n"
        .to_string()
    }
}

fn cpp_scheduler_data_time_update(contract: &ContractIr, indent: &str) -> String {
    if cpp_scheduler_uses_data_time(contract) {
        // replay 由 advance block 的回放驱动推进 scheduler_now_ms，loop 顶部不再读 data_time。
        String::new()
    } else {
        format!(
            "{indent}scheduler_now_ms = std::max(scheduler_now_ms, scheduler_runtime_now_ms());\n{indent}(void)scheduler_events.take_data_time_ms();\n"
        )
    }
}

/// 生成 C++ scheduler 唤醒与逻辑时钟推进块。
///
/// realtime：按 wall-clock(steady_clock) deadline 等待下一个 periodic deadline 或数据事件，
/// Timer 到点把 scheduler_now_ms 推进到该 deadline。simulated_replay：逻辑时钟只由注入事件
/// 驱动，不计算 steady_clock deadline、不被 wall-clock 节拍绑死，只等待下一个数据事件或关停；
/// 周期 task 在 advance_to 时按 missed_periods 自动 catch-up，因此回放结果只取决于事件序列，
/// 与回放物理快慢无关 (G2)。逐周期回放步进留待 runtime 原生确定性回放驱动补齐。
fn cpp_scheduler_wake_block(contract: &ContractIr, next_deadline_expr: &str) -> String {
    if cpp_scheduler_uses_data_time(contract) {
        let _ = next_deadline_expr;
        "            switch (scheduler_events.wait_until_after(observed_data_generation, std::nullopt, shutdown)) {\n                case flowrt::ScheduleEvent::Shutdown:\n                    status = flowrt::Status::Ok;\n                    break;\n                case flowrt::ScheduleEvent::Timer:\n                    break;\n                case flowrt::ScheduleEvent::Data:\n                    break;\n            }\n            if (shutdown.is_requested()) {\n                break;\n            }\n"
            .to_string()
    } else {
        format!(
            "            const auto next_periodic_deadline_ms = {next_deadline_expr};\n            const auto next_wake_deadline = next_periodic_deadline_ms.has_value()\n                ? std::optional<std::chrono::steady_clock::time_point>{{\n                      std::chrono::steady_clock::now() +\n                      std::chrono::milliseconds{{static_cast<std::chrono::milliseconds::rep>(\n                          next_periodic_deadline_ms->value > scheduler_now_ms\n                              ? next_periodic_deadline_ms->value - scheduler_now_ms\n                              : 0U)}}}}\n                : std::nullopt;\n            switch (scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, shutdown)) {{\n                case flowrt::ScheduleEvent::Shutdown:\n                    status = flowrt::Status::Ok;\n                    break;\n                case flowrt::ScheduleEvent::Timer:\n                    scheduler_now_ms = next_periodic_deadline_ms.has_value()\n                                           ? next_periodic_deadline_ms->value\n                                           : scheduler_now_ms + scheduler_base_period_ms;\n                    break;\n                case flowrt::ScheduleEvent::Data:\n                    break;\n            }}\n            if (shutdown.is_requested()) {{\n                break;\n            }}\n"
        )
    }
}

/// 为 replay 时钟源生成运行时原生回放驱动初始化。
///
/// 读取 `FLOWRT_REPLAY_SOURCE` 指向的 JSONL 回放时间线（C++ 无 MCAP 解析能力，`flowrt run`
/// 启动 C++ 生成 shell 前已把 MCAP 规范化为 JSONL），只装配目标在本图 input boundary 名集合内
/// 的外部激励事件。缺少环境变量或加载失败时打印诊断并把 `status` 置 `Error`（不抛异常），while
/// 循环因此不进入，run 返回 `Error`。realtime 时钟源不生成本块。镜像 Rust
/// rust_scheduler_replay_driver_init。
fn cpp_scheduler_replay_driver_init(
    contract: &ContractIr,
    boundaries: &[BoundaryRuntimePlan],
) -> String {
    if !cpp_scheduler_uses_data_time(contract) {
        return String::new();
    }
    let names = boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Input)
        .map(|boundary| cpp_string_literal(&boundary.endpoint_name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "    const std::set<std::string> replay_boundary_inputs = {{{names}}};\n    std::optional<flowrt::ReplayDriver> replay_time_driver;\n    {{\n        const char* replay_source = std::getenv(\"FLOWRT_REPLAY_SOURCE\");\n        if (replay_source != nullptr && replay_source[0] != '\\0') {{\n            auto replay_loaded = flowrt::replay_driver_from_timeline_file(replay_source, replay_boundary_inputs);\n            if (std::holds_alternative<flowrt::ReplayDriver>(replay_loaded)) {{\n                replay_time_driver = std::move(std::get<flowrt::ReplayDriver>(replay_loaded));\n            }} else {{\n                std::fprintf(stderr, \"FlowRT: 无法加载 FLOWRT_REPLAY_SOURCE `%s`: %s\\n\", replay_source, std::get<std::string>(replay_loaded).c_str());\n                status = flowrt::Status::Error;\n            }}\n        }}\n    }}\n"
    )
}

/// 生成 scheduler 每个 tick 之后推进逻辑时钟的块。
///
/// realtime：保持既有行为——run_ticks 有界且无 pending 时按 base period 推进并 continue，否则
/// 按 wall-clock deadline 等待下一个 periodic deadline 或数据事件。simulated_replay 两种子模式由
/// `FLOWRT_REPLAY_SOURCE` 选择：设置时走 runtime 原生确定性回放（ReplayDriver 逐步推进、命中事件
/// 经 publish_boundary_input 注入）；未设置时回退到外部 socket 注入（flowrt replay / temporary
/// island 交互式回放），按注入样本 data_time 推进逻辑时钟。镜像 Rust rust_scheduler_advance_block。
fn cpp_scheduler_advance_block(contract: &ContractIr, next_deadline_expr: &str) -> String {
    if cpp_scheduler_uses_data_time(contract) {
        format!(
            "            if (replay_time_driver.has_value()) {{\n                auto& replay_driver = *replay_time_driver;\n                const auto next_periodic_deadline_chrono = {next_deadline_expr};\n                const auto replay_next_periodic_deadline_ms = next_periodic_deadline_chrono.has_value()\n                    ? std::optional<std::uint64_t>{{static_cast<std::uint64_t>(next_periodic_deadline_chrono->count())}}\n                    : std::nullopt;\n                const auto replay_step = replay_driver.next_step(replay_next_periodic_deadline_ms, shutdown);\n                if (replay_step == flowrt::Step::Shutdown) {{\n                    break;\n                }}\n                scheduler_now_ms = replay_driver.now_ms();\n                if (replay_step == flowrt::Step::Data) {{\n                    for (const auto& replay_event : replay_driver.take_pending_events()) {{\n                        (void)introspection_state.publish_boundary_input(replay_event.target, std::span<const std::uint8_t>{{replay_event.payload.data(), replay_event.payload.size()}}, std::optional<std::uint64_t>{{replay_event.time_ms}});\n                    }}\n                }}\n            }} else {{\n                switch (scheduler_events.wait_until_after(observed_data_generation, std::nullopt, shutdown)) {{\n                    case flowrt::ScheduleEvent::Shutdown:\n                        status = flowrt::Status::Ok;\n                        break;\n                    case flowrt::ScheduleEvent::Timer:\n                        break;\n                    case flowrt::ScheduleEvent::Data:\n                        if (const auto data_time_ms = scheduler_events.take_data_time_ms()) {{\n                            scheduler_now_ms = std::max(scheduler_now_ms, *data_time_ms);\n                        }}\n                        break;\n                }}\n                if (shutdown.is_requested()) {{\n                    break;\n                }}\n            }}\n"
        )
    } else {
        let mut block = String::from(
            "            if (run_ticks.has_value() && pending_task_order.empty()) {\n                scheduler_now_ms += scheduler_base_period_ms;\n                continue;\n            }\n",
        );
        block.push_str(&cpp_scheduler_wake_block(contract, next_deadline_expr));
        block
    }
}

pub(super) fn cpp_task_clock_source_expr(contract: &ContractIr) -> &'static str {
    if contract.artifact.clock_source.is_realtime() {
        "flowrt::ClockSource::Runtime"
    } else {
        "flowrt::ClockSource::Replay"
    }
}

pub(super) fn emit_cpp_scheduler_event_registration(
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
) -> String {
    let mut output = String::new();
    for bind in binds
        .iter()
        .filter(|bind| matches!(bind_backend(bind), "iox2" | "zenoh"))
    {
        output.push_str(&format!(
            "    {field}_.set_schedule_waiter(scheduler_events);\n",
            field = bind.field_name
        ));
    }
    for bridge in bridges {
        output.push_str(&format!(
            "    {field}_.set_schedule_waiter(scheduler_events);\n",
            field = bridge.field_name
        ));
    }
    for boundary in boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Input)
    {
        output.push_str(&format!(
            "    {field}_.set_schedule_waiter(scheduler_events);\n",
            field = boundary.field_name
        ));
    }
    output
}

pub(super) fn cpp_task_lane_name(task: &flowrt_ir::TaskIr) -> String {
    resolved_task_lane_name(task)
}

pub(super) fn cpp_task_health_name(task: &flowrt_ir::TaskIr) -> String {
    format!("{}.{}", task.instance.name, task.name)
}

pub(super) fn cpp_task_local_name(task: &flowrt_ir::TaskIr) -> String {
    format!(
        "{}_{}",
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

/// 为本轮 scheduler 预注册 task health 条目，确保未运行 task 也能记录公平性计数。
pub(super) fn emit_cpp_task_health_init(tasks: &[SchedulerDataflowTaskPlan<'_>]) -> String {
    let mut output = String::new();
    for task in tasks {
        let task_health = &task.timing_name;
        let lane = &task.lane;
        output.push_str(&format!(
            "        {{\n            auto& health = health_map[\"{task_health}\"];\n            health.name = \"{task_health}\";\n            health.lane = \"{lane}\";\n        }}\n"
        ));
    }
    output
}

pub(super) fn emit_cpp_task_admission_health_update(
    tasks: &[SchedulerDataflowTaskPlan<'_>],
) -> String {
    let mut output = String::new();
    output.push_str("                    switch (admission.task.value) {\n");
    for task in tasks {
        let task_id = task.id;
        let task_health = &task.timing_name;
        let lane = &task.lane;
        output.push_str(&format!(
            "                        case {task_id}: {{\n\
                             auto& health = health_map[\"{task_health}\"];\n\
                             health.name = \"{task_health}\";\n\
                             health.lane = \"{lane}\";\n\
                             health.inflight = true;\n\
                             health.scheduled_time_ms = admission.scheduled_time_ms;\n\
                             health.observed_time_ms = admission.observed_time_ms;\n\
                             health.lateness_ms = admission.lateness_ms;\n\
                             health.missed_periods = admission.missed_periods;\n\
                             health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);\n\
                             break;\n\
                         }}\n"
        ));
    }
    output.push_str(
        "                        default:\n                            break;\n                    }\n",
    );
    output
}

pub(super) fn emit_cpp_task_result_health_update(
    tasks: &[SchedulerDataflowTaskPlan<'_>],
) -> String {
    let mut output = String::new();
    output.push_str("                switch (task_result.task.value) {\n");
    for task in tasks {
        let task_id = task.id;
        let task_health = &task.timing_name;
        let lane = &task.lane;
        output.push_str(&format!(
            "                    case {task_id}: {{\n\
                         auto& health = health_map[\"{task_health}\"];\n\
                         health.name = \"{task_health}\";\n\
                         health.lane = \"{lane}\";\n\
                         health.inflight = false;\n\
                         if (const auto admission_it = pending_task_admissions.find(task_result.task); admission_it != pending_task_admissions.end()) {{\n\
                             const auto& admission = admission_it->second;\n\
                             health.scheduled_time_ms = admission.scheduled_time_ms;\n\
                             health.observed_time_ms = admission.observed_time_ms;\n\
                             health.lateness_ms = admission.lateness_ms;\n\
                             health.missed_periods = admission.missed_periods;\n\
                             health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);\n\
                             pending_task_admissions.erase(admission_it);\n\
                         }}\n\
                         health.run_count += 1;\n\
                         health.last_run_ms = tick_time_ms;\n\
                         if (task_result.status == flowrt::Status::Ok) {{\n\
                             health.success_count += 1;\n\
                             health.consecutive_failures = 0;\n\
                             health.last_success_ms = tick_time_ms;\n\
                         }} else if (task_result.status == flowrt::Status::Error) {{\n\
                             health.consecutive_failures += 1;\n\
                         }}\n\
                         break;\n\
                     }}\n"
        ));
    }
    output.push_str(
        "                    default:\n                        break;\n                }\n",
    );
    output
}

/// 生成 C++ lane 饥饿检测代码。
pub(super) fn emit_cpp_fairness_check(lane_names: &BTreeSet<String>) -> String {
    let mut output = String::new();
    for lane in lane_names {
        let lane_id = cpp_lane_id_expr(lane);
        output.push_str(&format!(
            "        if (scheduler.lane_starvation_ticks({lane_id}) > fairness_starvation_threshold) {{\n            for (auto &[name, health] : health_map) {{\n                if (health.lane == \"{lane}\") {{\n                    health.fairness_violations += 1;\n                }}\n            }}\n        }}\n"
        ));
    }
    output
}

pub(super) fn cpp_next_periodic_deadline_expr(tasks: &[SchedulerDataflowTaskPlan<'_>]) -> String {
    let deadlines = tasks
        .iter()
        .filter(|task| task.periodic_wake)
        .map(|task| format!("scheduler.next_deadline(flowrt::TaskId{{{}}})", task.id))
        .collect::<Vec<_>>();
    if deadlines.is_empty() {
        "std::optional<std::chrono::milliseconds>{}".to_string()
    } else {
        let mut output = "std::optional<std::chrono::milliseconds>{std::min({".to_string();
        output.push_str(&deadlines.join(", "));
        output.push_str("})}");
        output
    }
}

pub(super) fn cpp_lane_id_expr(lane_name: &str) -> String {
    format!(
        "flowrt::LaneId{{flowrt::fnv1a64({})}}",
        cpp_string_literal(lane_name)
    )
}

pub(super) fn cpp_lane_id_u64_expr(lane_name: &str) -> String {
    format!("flowrt::fnv1a64({})", cpp_string_literal(lane_name))
}

/// C++ 退避表达式：min(initial << min(consecutive,31), max)，clock-ms。
fn cpp_backoff_expr(var: &str, initial_delay_ms: u64, max_delay_ms: u64) -> String {
    format!(
        "std::min<std::uint64_t>({initial_delay_ms}ULL << std::min<std::uint32_t>({var}_fault_consecutive, 31U), {max_delay_ms}ULL)"
    )
}

/// 为 restart / degrade 策略 instance 生成 C++ 故障状态局部变量（4 空格缩进，循环外）。
///
/// restart 用退避三件套；degrade 仅用一个 `_degraded` bool 做边沿跟踪，镜像 Rust。
fn emit_cpp_fault_state_decls(
    recoverable: &[crate::runtime_plan::RecoverableInstancePlan],
    graph_stop: bool,
) -> String {
    let mut output = String::new();
    if graph_stop {
        output.push_str("    bool _graph_terminal_fault = false;\n");
    }
    for plan in recoverable {
        let var = crate::snake_identifier(&plan.name);
        match plan.policy {
            InstanceFailurePolicy::Restart => {
                output.push_str(&format!(
                    "    std::optional<std::uint64_t> {var}_next_restart_ms;\n    std::uint32_t {var}_fault_consecutive = 0;\n    bool {var}_terminal_faulted = false;\n",
                ));
            }
            InstanceFailurePolicy::Degrade => {
                output.push_str(&format!("    bool {var}_degraded = false;\n"));
            }
            _ => {}
        }
    }
    output
}

fn emit_cpp_failover_state_decls(plans: &[crate::runtime_plan::StandbyFailoverPlan]) -> String {
    let mut output = String::new();
    for plan in plans.iter().filter(|plan| !plan.standby.is_empty()) {
        output.push_str(&format!(
            "    std::string {active} = {primary};\n    bool {pending} = false;\n",
            active = plan.active_field_name,
            primary = cpp_string_literal(&plan.primary),
            pending = plan.pending_field_name,
        ));
    }
    output
}

fn cpp_failover_capture(plans: &[crate::runtime_plan::StandbyFailoverPlan]) -> String {
    plans
        .iter()
        .filter(|plan| !plan.standby.is_empty())
        .map(|plan| format!(", {}", plan.active_field_name))
        .collect()
}

fn emit_cpp_failover_boundary(plans: &[crate::runtime_plan::StandbyFailoverPlan]) -> String {
    let mut output = String::new();
    for plan in plans.iter().filter(|plan| !plan.standby.is_empty()) {
        let active = &plan.active_field_name;
        let pending = &plan.pending_field_name;
        let old_active = format!(
            "flowrt_old_active_redundancy_{}",
            crate::snake_identifier(&plan.group)
        );
        let primary = cpp_string_literal(&plan.primary);
        let standby = cpp_string_literal(&plan.standby[0]);
        let group = cpp_string_literal(&plan.group);
        output.push_str(&format!(
            "        if ({pending}) {{\n            if ({active} == {primary}) {{\n                auto {old_active} = {active};\n                {active} = {standby};\n                introspection_state.record_failover(flowrt::IntrospectionFailoverEvent{{\n                    .event = \"failover\",\n                    .group = {group},\n                    .old_active = std::move({old_active}),\n                    .new_active = {standby},\n                    .tick_id = static_cast<std::uint64_t>(tick_base),\n                    .reason = \"critical_fault\",\n                }});\n            }}\n            {pending} = false;\n        }}\n"
        ));
    }
    output
}

fn cpp_failover_pending_mark(
    instance: &str,
    plans: &[crate::runtime_plan::StandbyFailoverPlan],
) -> String {
    plans
        .iter()
        .filter(|plan| plan.primary == instance && !plan.standby.is_empty())
        .map(|plan| {
            format!(
                "                            if ({active} == {primary}) {{\n                                {pending} = true;\n                            }}\n",
                active = plan.active_field_name,
                primary = cpp_string_literal(&plan.primary),
                pending = plan.pending_field_name,
            )
        })
        .collect()
}

/// C++ 注入命中布尔表达式，镜像 Rust：调用计数命中显式集合或达到 from_invocation 起点即触发。
fn cpp_fault_injection_hit_expr(
    point: &flowrt_ir::FaultInjectionPointIr,
    counter_var: &str,
) -> String {
    let mut clauses = Vec::new();
    for invocation in &point.invocations {
        clauses.push(format!("{counter_var} == {invocation}ULL"));
    }
    if let Some(from) = point.from_invocation {
        clauses.push(format!("{counter_var} >= {from}ULL"));
    }
    clauses.join(" || ")
}

/// 为每个故障注入目标 task 生成 per-task 调用计数器局部变量（4 空格缩进，run 函数体，循环外）。
fn emit_cpp_injection_counter_decls(
    contract: &ContractIr,
    dataflow_tasks: &[crate::runtime_plan::SchedulerDataflowTaskPlan<'_>],
) -> String {
    let mut output = String::new();
    for plan in dataflow_tasks {
        if crate::runtime_plan::scheduler_fault_injection_point_for(contract, plan.task).is_some() {
            output.push_str(&format!(
                "    std::uint64_t __inject_count_{} = 0;\n",
                plan.id
            ));
        }
    }
    output
}

/// scheduler 线程在 submit 前对本次 admission 计算注入决策（自增对应计数器并置 bool）。
/// 无注入任务时返回空串，使非注入产物字节不漂移。
fn emit_cpp_injection_decision(
    contract: &ContractIr,
    dataflow_tasks: &[crate::runtime_plan::SchedulerDataflowTaskPlan<'_>],
) -> String {
    let mut arms = String::new();
    let mut needs_fault = false;
    let mut needs_deadline_miss = false;
    let mut needs_backend_drop = false;
    for plan in dataflow_tasks {
        if let Some(point) =
            crate::runtime_plan::scheduler_fault_injection_point_for(contract, plan.task)
        {
            let counter = format!("__inject_count_{}", plan.id);
            let hit = cpp_fault_injection_hit_expr(point, &counter);
            match point.kind {
                flowrt_ir::FaultInjectionKind::DeadlineMiss => {
                    needs_deadline_miss = true;
                    arms.push_str(&format!(
                        "                    case {}: {{ ++{counter}; const bool __flowrt_inject_deadline_miss_{} = {hit}; __flowrt_inject_deadline_miss = __flowrt_inject_deadline_miss_{}; break; }}\n",
                        plan.id, plan.id, plan.id
                    ));
                }
                flowrt_ir::FaultInjectionKind::BackendDrop => {
                    needs_backend_drop = true;
                    arms.push_str(&format!(
                        "                    case {}: {{ ++{counter}; const bool __flowrt_inject_backend_drop_{} = {hit}; __flowrt_inject_backend_drop = __flowrt_inject_backend_drop_{}; break; }}\n",
                        plan.id, plan.id, plan.id
                    ));
                }
                _ => {
                    needs_fault = true;
                    arms.push_str(&format!(
                        "                    case {}: {{ ++{counter}; flowrt_inject_fault = {hit}; break; }}\n",
                        plan.id
                    ));
                }
            }
        }
    }
    if arms.is_empty() {
        return String::new();
    }
    if needs_fault && !needs_deadline_miss && !needs_backend_drop {
        return format!(
            "                bool flowrt_inject_fault = false;\n                switch (admission.task.value) {{\n{arms}                    default: break;\n                }}\n"
        );
    }
    let mut decls = String::new();
    if needs_fault {
        decls.push_str("                bool flowrt_inject_fault = false;\n");
    }
    if needs_deadline_miss {
        decls.push_str("                bool __flowrt_inject_deadline_miss = false;\n");
    }
    if needs_backend_drop {
        decls.push_str("                bool __flowrt_inject_backend_drop = false;\n");
    }
    format!(
        "{decls}                switch (admission.task.value) {{\n{arms}                    default: break;\n                }}\n"
    )
}

/// 注入产物的 worker lambda 需按值捕获注入决策 bool；无注入任务时为空，捕获列表字节不漂移。
fn cpp_injection_capture(
    contract: &ContractIr,
    dataflow_tasks: &[crate::runtime_plan::SchedulerDataflowTaskPlan<'_>],
) -> String {
    let mut captures = Vec::new();
    if dataflow_tasks.iter().any(|plan| {
        crate::runtime_plan::scheduler_fault_injection_point_for(contract, plan.task).is_some_and(
            |point| {
                matches!(
                    point.kind,
                    flowrt_ir::FaultInjectionKind::StatusError
                        | flowrt_ir::FaultInjectionKind::Panic
                )
            },
        )
    }) {
        captures.push("flowrt_inject_fault");
    }
    if dataflow_tasks.iter().any(|plan| {
        crate::runtime_plan::scheduler_fault_injection_point_for(contract, plan.task)
            .is_some_and(|point| point.kind == flowrt_ir::FaultInjectionKind::DeadlineMiss)
    }) {
        captures.push("__flowrt_inject_deadline_miss");
    }
    if dataflow_tasks.iter().any(|plan| {
        crate::runtime_plan::scheduler_fault_injection_point_for(contract, plan.task)
            .is_some_and(|point| point.kind == flowrt_ir::FaultInjectionKind::BackendDrop)
    }) {
        captures.push("__flowrt_inject_backend_drop");
    }
    if captures.is_empty() {
        String::new()
    } else {
        format!(", {}", captures.join(", "))
    }
}

fn cpp_task_injection_call_arg(
    contract: &ContractIr,
    task: &flowrt_ir::TaskIr,
) -> Option<&'static str> {
    match crate::runtime_plan::scheduler_fault_injection_point_for(contract, task)?.kind {
        flowrt_ir::FaultInjectionKind::DeadlineMiss => Some("__flowrt_inject_deadline_miss"),
        flowrt_ir::FaultInjectionKind::BackendDrop => Some("__flowrt_inject_backend_drop"),
        _ => None,
    }
}

fn cpp_fault_injection_panic_message(point: &flowrt_ir::FaultInjectionPointIr) -> String {
    let reason = point.reason.trim();
    if reason.is_empty() {
        "FlowRT fault injection panic".to_string()
    } else {
        format!("FlowRT fault injection panic: {reason}")
    }
}

/// C++ restart-due 驱动（8 空格缩进，循环内 tick 顶部），镜像 Rust 行为。
fn emit_cpp_restart_driver(
    contract: &ContractIr,
    order: &[&InstanceIr],
    recoverable: &[crate::runtime_plan::RecoverableInstancePlan],
    graph_stop: bool,
) -> String {
    let mut output = String::new();
    let graph_terminal = if graph_stop {
        "                    _graph_terminal_fault = true;\n"
    } else {
        ""
    };
    for plan in recoverable {
        if plan.policy != InstanceFailurePolicy::Restart {
            continue;
        }
        let Some(restart) = plan.restart else {
            continue;
        };
        let instance = order
            .iter()
            .find(|instance| instance.name == plan.name)
            .expect("recoverable instance must be in order");
        let component = component_by_name(contract, &instance.component.name);
        let ctx = cpp_lifecycle_context_name(component, instance);
        let var = crate::snake_identifier(&plan.name);
        let lit = cpp_string_literal(&plan.name);
        let member = &instance.name;
        let resume = plan
            .task_ids
            .iter()
            .map(|id| {
                format!("                    scheduler.resume_task(flowrt::TaskId{{{id}}});\n")
            })
            .collect::<String>();
        let backoff = cpp_backoff_expr(&var, restart.initial_delay_ms, restart.max_delay_ms);
        output.push_str(&format!(
            "        if ({var}_next_restart_ms.has_value() && scheduler_now_ms >= *{var}_next_restart_ms) {{\n            {var}_next_restart_ms.reset();\n            introspection_state.record_instance_restart({lit});\n            auto {var}_restart_status = {member}_ ? {member}_->on_init({ctx}) : flowrt::Status::Error;\n            if ({var}_restart_status == flowrt::Status::Ok) {{\n                {var}_restart_status = {member}_->on_start({ctx});\n            }}\n            if ({var}_restart_status == flowrt::Status::Ok) {{\n                {var}_fault_consecutive = 0;\n                introspection_state.record_lifecycle_state({lit}, flowrt::LifecycleState::Running);\n{resume}            }} else {{\n                {var}_fault_consecutive += 1;\n                introspection_state.record_lifecycle_transition({lit}, flowrt::LifecycleState::Faulted, static_cast<std::uint64_t>(tick_base), \"restart_failed\");\n                if ({var}_fault_consecutive >= {max_restarts}U) {{\n                    {var}_terminal_faulted = true;\n{graph_terminal}                }} else {{\n                    {var}_next_restart_ms = scheduler_now_ms + {backoff};\n                }}\n            }}\n        }}\n",
            max_restarts = restart.max_restarts,
        ));
    }
    output
}

/// C++ commit drain 对 task Error 的处理（16 空格缩进）。
///
/// 无 recoverable 时返回既有 status=Error;break；否则按 task id switch：isolate/restart instance
/// 隔离续跑（依赖后续 `if (status != Ok) break;` 不触发），restart 还排下次重启；degrade instance
/// 仅记 `Degraded` 不挂起、不停图，并在该 task 后续返 Ok 时翻回 `Running`；其余 fail_fast。
fn emit_cpp_task_error_handling(
    recoverable: &[crate::runtime_plan::RecoverableInstancePlan],
    standby_failover: &[crate::runtime_plan::StandbyFailoverPlan],
    graph_stop: bool,
) -> String {
    if recoverable.is_empty() {
        return "                if (task_result.status == flowrt::Status::Error) {\n                    status = flowrt::Status::Error;\n                    break;\n                }\n".to_string();
    }
    let mut arms = String::new();
    let mut recovery_arms = String::new();
    for plan in recoverable {
        if plan.task_ids.is_empty() {
            continue;
        }
        let var = crate::snake_identifier(&plan.name);
        let lit = cpp_string_literal(&plan.name);
        let labels = plan
            .task_ids
            .iter()
            .map(|id| format!("                        case {id}:\n"))
            .collect::<String>();
        if plan.policy == InstanceFailurePolicy::Degrade {
            arms.push_str(&format!(
                "{labels}                        {{\n                            if (!{var}_degraded) {{\n                                introspection_state.record_lifecycle_state({lit}, flowrt::LifecycleState::Degraded);\n                                {var}_degraded = true;\n                            }}\n                            break;\n                        }}\n",
            ));
            recovery_arms.push_str(&format!(
                "{labels}                        {{\n                            if ({var}_degraded) {{\n                                introspection_state.record_lifecycle_state({lit}, flowrt::LifecycleState::Running);\n                                {var}_degraded = false;\n                            }}\n                            break;\n                        }}\n",
            ));
            continue;
        }
        let suspend = plan
            .task_ids
            .iter()
            .map(|id| {
                format!(
                    "                            scheduler.suspend_task(flowrt::TaskId{{{id}}});\n"
                )
            })
            .collect::<String>();
        let restart_schedule = match (plan.policy, plan.restart) {
            (InstanceFailurePolicy::Restart, Some(restart)) => {
                let backoff =
                    cpp_backoff_expr(&var, restart.initial_delay_ms, restart.max_delay_ms);
                format!(
                    "                            if (!{var}_terminal_faulted) {{\n                                {var}_next_restart_ms = scheduler_now_ms + {backoff};\n                            }}\n",
                )
            }
            _ => String::new(),
        };
        // isolate 故障即时终态：受控停机下置图级终态标记（restart 终态在 restart_driver 置位）。
        let graph_terminal = if graph_stop && plan.policy == InstanceFailurePolicy::Isolate {
            "                            _graph_terminal_fault = true;\n"
        } else {
            ""
        };
        let failover_pending = cpp_failover_pending_mark(&plan.name, standby_failover);
        arms.push_str(&format!(
            "{labels}                        {{\n                            introspection_state.record_lifecycle_transition({lit}, flowrt::LifecycleState::Faulted, static_cast<std::uint64_t>(tick_base), \"task_error\");\n{failover_pending}{suspend}{graph_terminal}{restart_schedule}                            break;\n                        }}\n",
        ));
    }
    let mut output = format!(
        "                if (task_result.status == flowrt::Status::Error) {{\n                    switch (task_result.task.value) {{\n{arms}                        default:\n                            status = flowrt::Status::Error;\n                            break;\n                    }}\n                }}\n",
    );
    if !recovery_arms.is_empty() {
        output.push_str(&format!(
            "                if (task_result.status == flowrt::Status::Ok) {{\n                    switch (task_result.task.value) {{\n{recovery_arms}                        default:\n                            break;\n                    }}\n                }}\n",
        ));
    }
    output
}
