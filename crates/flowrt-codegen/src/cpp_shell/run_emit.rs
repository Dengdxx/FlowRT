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
}

pub(super) fn emit_cpp_app_run_function(run: &CppRunEmission<'_>) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "flowrt::Status App::{}(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {{\n    flowrt::Context lifecycle_context;\n    auto status = flowrt::Status::Ok;\n",
        run.function_name
    ));
    output.push_str("    (void)backend;\n");
    output.push_str("    auto shutdown = flowrt::install_signal_shutdown_token();\n");
    output.push_str("    flowrt::IntrospectionState introspection_state;\n");
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
    output.push_str(&emit_cpp_boundary_input_registration(run.boundaries));
    output.push_str(&emit_cpp_boundary_output_probe_registration(run.boundaries));
    output.push_str(&format!(
        "    auto introspection_server = flowrt::spawn_status_server(\n        flowrt::IntrospectionIdentity{{\n            .self_description_hash = std::string{{flowrt_app::self_description_hash()}},\n            .package = {},\n            .process = {},\n            .runtime = \"cpp\",\n        }},\n        introspection_state);\n    (void)introspection_server;\n",
        cpp_string_literal(run.package_name),
        cpp_string_literal(run.process_name)
    ));
    for instance in run.order {
        output.push_str(&format!(
            "    bool {name}_initialized = false;\n    bool {name}_started = false;\n",
            name = instance.name
        ));
    }
    output.push_str(&emit_cpp_io_boundary_contexts(run.contract, run.order));
    for instance in run.order {
        let component = component_by_name(run.contract, &instance.component.name);
        let context_name = cpp_lifecycle_context_name(component, instance);
        output.push_str(&format!(
            "    if (status == flowrt::Status::Ok && {name}_) {{\n        status = {name}_->on_init({context});\n        {name}_initialized = status == flowrt::Status::Ok;\n    }}\n",
            name = instance.name,
            context = context_name,
        ));
    }
    for instance in run.order {
        let component = component_by_name(run.contract, &instance.component.name);
        let context_name = cpp_lifecycle_context_name(component, instance);
        output.push_str(&format!(
            "    if (status == flowrt::Status::Ok && {name}_initialized && {name}_) {{\n        status = {name}_->on_start({context});\n        {name}_started = status == flowrt::Status::Ok;\n    }}\n",
            name = instance.name,
            context = context_name,
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
    output.push_str(&emit_cpp_scheduler_v2_loop(run));
    output.push_str(&format!(
        "    if (status == flowrt::Status::Ok) {{\n        std::map<std::string, flowrt::IntrospectionTaskHealth> shutdown_health_map;\n        status = {}(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);\n    }}\n",
        run.shutdown_function_name
    ));
    for instance in run.order.iter().rev() {
        let component = component_by_name(run.contract, &instance.component.name);
        let context_name = cpp_lifecycle_context_name(component, instance);
        output.push_str(&format!(
            "    if ({name}_started && {name}_) {{\n        const auto stop_status = {name}_->on_stop({context});\n        if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok) {{\n            status = flowrt::Status::Error;\n        }}\n    }}\n",
            name = instance.name,
            context = context_name,
        ));
    }
    for instance in run.order.iter().rev() {
        let component = component_by_name(run.contract, &instance.component.name);
        let context_name = cpp_lifecycle_context_name(component, instance);
        output.push_str(&format!(
            "    if ({name}_initialized && {name}_) {{\n        const auto shutdown_status = {name}_->on_shutdown({context});\n        if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok) {{\n            status = flowrt::Status::Error;\n        }}\n    }}\n",
            name = instance.name,
            context = context_name,
        ));
    }
    output.push_str("    return status;\n}\n\n");
    output
}

pub(super) fn emit_cpp_scheduler_v2_loop(run: &CppRunEmission<'_>) -> String {
    let tasks = scheduler_tasks_for_order(run.graph, run.order);
    let mut output = String::new();
    let worker_threads = selected_profile_worker_threads(run.contract);
    output.push_str(&format!(
        "    flowrt::DeterministicExecutor scheduler{{{worker_threads}}};\n    flowrt::WorkerPool worker_pool{{{worker_threads}}};\n",
    ));

    let mut lane_names = tasks
        .iter()
        .map(|task| cpp_task_lane_name(task))
        .collect::<BTreeSet<_>>();
    let service_plans = crate::runtime_plan::service_runtime_plans(run.contract, run.graph);
    let operation_plans = crate::runtime_plan::operation_runtime_plans(run.contract, run.graph);
    for plan in &service_plans {
        if plan.backend.0 != "zenoh" {
            lane_names.insert(crate::runtime_plan::service_server_lane(plan));
        }
    }
    for plan in &operation_plans {
        if plan.backend.0 != "zenoh" {
            lane_names.insert(crate::runtime_plan::operation_server_lane(plan));
        }
    }
    for lane in &lane_names {
        let lane_expr = cpp_lane_id_expr(lane);
        output.push_str(&format!(
            "    scheduler.add_lane({lane_expr}, flowrt::LaneKind::Serial);\n    (void){};\n",
            cpp_string_literal(lane),
        ));
    }
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let lane_id = cpp_lane_id_expr(&cpp_task_lane_name(task));
        let priority = task.priority.unwrap_or(0);
        output.push_str(&format!(
            "    scheduler.add_task(flowrt::TaskSpec{{.id = flowrt::TaskId{{{task_id}}}, .lane = {lane_id}, .priority = {priority}}});\n"
        ));
        if let Some(deadline_ms) = task.deadline_ms {
            output.push_str(&format!(
                "    scheduler.set_task_deadline_ms(flowrt::TaskId{{{task_id}}}, std::uint64_t{{{deadline_ms}}});\n"
            ));
        }
        if task.trigger == flowrt_ir::TriggerKind::Periodic {
            output.push_str(&format!(
                "    scheduler.add_periodic(flowrt::PeriodicSpec{{.task = flowrt::TaskId{{{task_id}}}, .period = std::chrono::milliseconds{{{}}}}});\n    scheduler.wake(flowrt::TaskId{{{task_id}}});\n",
                task.period_ms.unwrap_or(1)
            ));
        }
    }
    // service task registration
    let mut next_task_id = tasks.len();
    let mut hidden_task_lane_names = BTreeMap::<usize, String>::new();
    for plan in &service_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        next_task_id += 1;
        let server_lane = crate::runtime_plan::service_server_lane(plan);
        let lane_id = cpp_lane_id_expr(&server_lane);
        hidden_task_lane_names.insert(next_task_id, server_lane);
        output.push_str(&format!(
            "    scheduler.add_task(flowrt::TaskSpec{{.id = flowrt::TaskId{{{next_task_id}}}, .lane = {lane_id}, .priority = 0}});\n"
        ));
    }
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        next_task_id += 1;
        let server_lane = crate::runtime_plan::operation_server_lane(plan);
        let lane_id = cpp_lane_id_expr(&server_lane);
        hidden_task_lane_names.insert(next_task_id, server_lane);
        output.push_str(&format!(
            "    scheduler.add_task(flowrt::TaskSpec{{.id = flowrt::TaskId{{{next_task_id}}}, .lane = {lane_id}, .priority = 0}});\n"
        ));
    }
    output.push_str(&emit_cpp_on_message_revision_state(
        &tasks,
        run.binds,
        run.bridges,
        run.boundaries,
    ));
    output.push_str(&format!(
        "    const auto scheduler_base_period_ms = std::uint64_t{{{}}};\n",
        cpp_scheduler_base_period_ms(&tasks)
    ));
    let task_health_init = emit_cpp_task_health_init(&tasks);
    let clock_source = cpp_scheduler_clock_source(run.contract);
    let task_clock_source = cpp_task_clock_source_expr(run.contract);
    output.push_str(
        "    std::size_t tick_base = 0;\n    std::uint64_t scheduler_now_ms = 0;\n    std::map<std::string, flowrt::IntrospectionTaskHealth> health_map;\n    constexpr std::uint64_t fairness_starvation_threshold = 10;\n",
    );
    output.push_str(&format!(
        "    const auto clock_source = std::string_view{{{}}};\n",
        cpp_string_literal(clock_source)
    ));
    output.push_str(&format!(
        "    const auto task_clock_source = {task_clock_source};\n    flowrt::WorkerCompletionQueue<std::vector<FlowrtOutputCommit>> task_completion_queue;\n    task_completion_queue.set_wake_callback([&scheduler_events]() {{ scheduler_events.notify_data(); }});\n    std::deque<flowrt::TaskId> pending_task_order;\n    std::map<flowrt::TaskId, flowrt::TaskRunOutput<std::vector<FlowrtOutputCommit>>> pending_task_results;\n    std::map<flowrt::TaskId, flowrt::TaskAdmission> pending_task_admissions;\n    std::mutex task_health_mutex;\n    std::map<std::string, flowrt::IntrospectionTaskHealth> task_health_from_workers;\n    std::map<flowrt::TaskId, std::uint64_t> task_last_scheduled_time_ms;\n    std::map<flowrt::TaskId, std::uint64_t> task_last_observed_time_ms;\n"
    ));
    output.push_str(
        "    while (status == flowrt::Status::Ok && !shutdown.is_requested() && ((!run_ticks.has_value() || tick_base < *run_ticks) || !pending_task_order.empty())) {\n        std::uint64_t observed_data_generation = scheduler_events.data_generation();\n        if (const auto data_time_ms = scheduler_events.take_data_time_ms()) {\n            scheduler_now_ms = std::max(scheduler_now_ms, *data_time_ms);\n        }\n        const auto tick_time_ms = scheduler_now_ms;\n        scheduler.advance_to(std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(tick_time_ms)});\n        scheduler.set_current_tick(static_cast<std::uint64_t>(tick_base));\n",
    );
    output.push_str(&task_health_init);
    output.push_str(&emit_cpp_apply_pending_params_for_order(
        run.contract,
        run.order,
    ));
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        output.push_str(&format!(
            "        bool flowrt_operation_tick_driven_{} = false;\n",
            plan.index
        ));
    }
    let has_inproc_service = service_plans.iter().any(|p| p.backend.0 != "zenoh");
    let has_inproc_operation = operation_plans.iter().any(|p| p.backend.0 != "zenoh");
    let woke_on_message_decl = if tasks
        .iter()
        .any(|task| task.trigger == flowrt_ir::TriggerKind::OnMessage)
        || has_inproc_service
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
        &emit_cpp_on_message_wake_checks(&tasks, run.binds, run.bridges, run.boundaries),
        1,
    ));
    // service wake checks
    let mut service_task_id = tasks.len();
    for plan in &service_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        service_task_id += 1;
        let server_field = cpp_service_server_field_name(plan);
        output.push_str(&format!(
            "            if ({server_field}_.has_value() && {server_field}_->pending_count() > 0) {{\n                scheduler.wake(flowrt::TaskId{{{service_task_id}}});\n                woke_on_message = true;\n            }}\n"
        ));
    }
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        service_task_id += 1;
        let start_server = cpp_operation_start_server_field_name(plan);
        let cancel_server = cpp_operation_cancel_server_field_name(plan);
        let status_server = cpp_operation_status_server_field_name(plan);
        let operation_index = plan.index;
        output.push_str(&format!(
            "            const auto flowrt_operation_snapshot_{operation_index} = this->operation_control_{operation_index}_ ? this->operation_control_{operation_index}_->snapshot() : flowrt::OperationStatusSnapshot{{}};\n            const bool flowrt_operation_active_{operation_index} = !flowrt::is_terminal(flowrt_operation_snapshot_{operation_index}.state) && flowrt_operation_snapshot_{operation_index}.state != flowrt::OperationState::Idle;\n            if (({start_server}_.has_value() && {start_server}_->pending_count() > 0) || ({cancel_server}_.has_value() && {cancel_server}_->pending_count() > 0) || ({status_server}_.has_value() && {status_server}_->pending_count() > 0) || (flowrt_operation_active_{operation_index} && !flowrt_operation_tick_driven_{operation_index})) {{\n                scheduler.wake(flowrt::TaskId{{{service_task_id}}});\n                if (flowrt_operation_active_{operation_index}) {{\n                    flowrt_operation_tick_driven_{operation_index} = true;\n                }}\n                woke_on_message = true;\n            }}\n"
        ));
    }
    output.push_str(
        "            for (auto task_result : task_completion_queue.drain_completed()) {\n                pending_task_results.insert_or_assign(task_result.task, std::move(task_result));\n            }\n            {\n                std::lock_guard<std::mutex> lock(task_health_mutex);\n                for (auto &[name, health] : task_health_from_workers) {\n                    health_map.insert_or_assign(name, std::move(health));\n                }\n                task_health_from_workers.clear();\n            }\n            auto ready_batch = scheduler.take_ready_batch();\n            const auto submitted_task_count = ready_batch.size();\n            for (const auto admission : ready_batch.admissions()) {\n                const auto scheduled_delta_ms = [&]() -> std::uint64_t {\n                    const auto [it, inserted] = task_last_scheduled_time_ms.insert_or_assign(admission.task, admission.scheduled_time_ms);\n                    return inserted || admission.scheduled_time_ms < it->second ? 0U : admission.scheduled_time_ms - it->second;\n                }();\n                const auto observed_delta_ms = [&]() -> std::uint64_t {\n                    const auto [it, inserted] = task_last_observed_time_ms.insert_or_assign(admission.task, admission.observed_time_ms);\n                    return inserted || admission.observed_time_ms < it->second ? 0U : admission.observed_time_ms - it->second;\n                }();\n                const auto submitted = worker_pool.submit_collect(admission.task, task_completion_queue, [this, &introspection_state, &scheduler_events, &task_health_mutex, &task_health_from_workers, admission, scheduled_delta_ms, observed_delta_ms, task_clock_source, tick_base, tick_time_ms]() {\n                    auto local_health_map = std::map<std::string, flowrt::IntrospectionTaskHealth>{};\n                    const auto [task_name, task_trigger] = [&]() -> std::pair<std::string_view, std::string_view> {\n                        switch (admission.task.value) {\n",
    );
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let task_name = cpp_task_timing_name(task);
        let trigger = cpp_trigger_name(task.trigger);
        output.push_str(&format!(
            "                            case {task_id}: return {{{}, {}}};\n",
            cpp_string_literal(&task_name),
            cpp_string_literal(trigger)
        ));
    }
    output.push_str(
        "                            default: return {\"__flowrt_hidden\", \"on_message\"};\n                        }\n                    }();\n                    auto local_context = flowrt::Context::with_timing(flowrt::TaskTiming{\n                        .step = static_cast<std::uint64_t>(tick_base),\n                        .task_name = std::string{task_name},\n                        .trigger = std::string{task_trigger},\n                        .clock_source = task_clock_source,\n                        .scheduled_time_ms = admission.scheduled_time_ms,\n                        .observed_time_ms = admission.observed_time_ms,\n                        .scheduled_delta_ms = scheduled_delta_ms,\n                        .observed_delta_ms = observed_delta_ms,\n                        .period_ms = admission.period_ms,\n                        .deadline_ms = admission.deadline_ms,\n                        .lateness_ms = admission.lateness_ms,\n                        .missed_periods = admission.missed_periods,\n                        .deadline_missed = admission.deadline_ms.has_value() && admission.lateness_ms > *admission.deadline_ms,\n                        .overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms),\n                    });\n                    auto merge_local_health = [&task_health_mutex, &task_health_from_workers, admission, task_name](std::map<std::string, flowrt::IntrospectionTaskHealth>&& local_health_map) {\n                        auto health_it = local_health_map.find(std::string{task_name});\n                        if (health_it != local_health_map.end()) {\n                            auto& health = health_it->second;\n                            health.inflight = false;\n                            health.scheduled_time_ms = admission.scheduled_time_ms;\n                            health.observed_time_ms = admission.observed_time_ms;\n                            health.lateness_ms = admission.lateness_ms;\n                            health.missed_periods = admission.missed_periods;\n                            health.overrun = admission.missed_periods > 0U || (admission.period_ms.has_value() && admission.lateness_ms > *admission.period_ms);\n                        }\n                        std::lock_guard<std::mutex> lock(task_health_mutex);\n                        for (auto &[name, health] : local_health_map) {\n                            task_health_from_workers.insert_or_assign(name, std::move(health));\n                        }\n                    };\n                    switch (admission.task.value) {\n",
    );
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let lane_id = cpp_lane_id_expr(&cpp_task_lane_name(task));
        let function_name = match run.process {
            Some(process) => cpp_process_task_step_function_name(process, task),
            None => cpp_task_step_function_name(task),
        };
        output.push_str(&format!(
            "                    case {task_id}: {{\n\
                         auto flowrt_lane_guard = flowrt::enter_lane({lane_id});\n\
                         (void)flowrt_lane_guard;\n\
                         auto task_outcome = {function_name}(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);\n\
                         merge_local_health(std::move(local_health_map));\n\
                         return task_outcome;\n\
                     }}\n"
        ));
    }
    // service dispatch cases
    service_task_id = tasks.len();
    for plan in &service_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        service_task_id += 1;
        let fn_name = cpp_service_step_fn_name(plan);
        let lane_id = cpp_lane_id_expr(&hidden_task_lane_names[&service_task_id]);
        output.push_str(&format!(
            "                    case {service_task_id}: {{\n\
                         auto flowrt_lane_guard = flowrt::enter_lane({lane_id});\n\
                         (void)flowrt_lane_guard;\n\
                         auto task_status = {fn_name}(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);\n\
                         merge_local_health(std::move(local_health_map));\n\
                         return FlowrtTaskOutcome{{.status = task_status, .outputs = std::vector<FlowrtOutputCommit>{{}}}};\n\
                     }}\n"
        ));
    }
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        service_task_id += 1;
        let fn_name = cpp_operation_step_fn_name(plan);
        let lane_id = cpp_lane_id_expr(&hidden_task_lane_names[&service_task_id]);
        output.push_str(&format!(
            "                    case {service_task_id}: {{\n\
                         auto flowrt_lane_guard = flowrt::enter_lane({lane_id});\n\
                         (void)flowrt_lane_guard;\n\
                         auto task_status = {fn_name}(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);\n\
                         merge_local_health(std::move(local_health_map));\n\
                         return FlowrtTaskOutcome{{.status = task_status, .outputs = std::vector<FlowrtOutputCommit>{{}}}};\n\
                     }}\n"
        ));
    }
    if tasks.is_empty()
        && service_plans.iter().all(|p| p.backend.0 == "zenoh")
        && operation_plans.iter().all(|p| p.backend.0 == "zenoh")
    {
        output.push_str(&format!(
            "                    default: {{\n                        auto task_status = {}(static_cast<std::size_t>(tick_time_ms), local_context, introspection_state, scheduler_events, local_health_map);\n                        merge_local_health(std::move(local_health_map));\n                        return FlowrtTaskOutcome{{.status = task_status, .outputs = std::vector<FlowrtOutputCommit>{{}}}};\n                    }}\n",
            run.step_function_name
        ));
    } else {
        output.push_str("                    default: return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{});\n");
    }
    let task_admission_health_update = emit_cpp_task_admission_health_update(&tasks);
    let task_result_health_update = emit_cpp_task_result_health_update(&tasks);
    let fairness_check = emit_cpp_fairness_check(&lane_names);
    output.push_str(&format!(
        "                }}\n                }});\n                if (submitted.accepted) {{\n                    pending_task_order.push_back(admission.task);\n                    pending_task_admissions.insert_or_assign(admission.task, admission);\n{task_admission_health_update}                }} else {{\n                    (void)scheduler.complete_task(admission.task);\n                    status = flowrt::Status::Error;\n                    break;\n                }}\n            }}\n            if (status != flowrt::Status::Ok) {{\n                break;\n            }}\n            std::size_t committed_task_count = 0;\n            while (!pending_task_order.empty()) {{\n                const auto task = pending_task_order.front();\n                const auto result_it = pending_task_results.find(task);\n                if (result_it == pending_task_results.end()) {{\n                    break;\n                }}\n                auto task_result = std::move(result_it->second);\n                pending_task_results.erase(result_it);\n                pending_task_order.pop_front();\n                (void)scheduler.complete_task(task_result.task);\n                ++committed_task_count;\n{task_result_health_update}                if (task_result.status == flowrt::Status::Error) {{\n                    status = flowrt::Status::Error;\n                    break;\n                }}\n                if (task_result.outputs.has_value()) {{\n                    for (auto& commit : *task_result.outputs) {{\n                        const auto commit_status = commit(*this, introspection_state, scheduler_events, health_map);\n                        if (commit_status == flowrt::Status::Error) {{\n                            status = flowrt::Status::Error;\n                            break;\n                        }}\n                        if (commit_status == flowrt::Status::Retry) {{\n                            status = flowrt::Status::Retry;\n                            break;\n                        }}\n                    }}\n                }}\n                if (status != flowrt::Status::Ok) {{\n                    break;\n                }}\n            }}\n            if (status != flowrt::Status::Ok) {{\n                break;\n            }}\n            if (committed_task_count == 0U || (!woke_on_message && submitted_task_count == 0U)) {{\n                break;\n            }}\n        }}\n        // 公平性检测：检查 lane 饥饿。\n{fairness_check}        // 将本轮健康快照写入 introspection。\n        for (auto &[name, health] : health_map) {{\n            introspection_state.record_task_health(std::move(health));\n        }}\n        health_map.clear();\n        if (status == flowrt::Status::Ok) {{\n            ++tick_base;\n            if (run_ticks.has_value() && pending_task_order.empty()) {{\n                scheduler_now_ms += scheduler_base_period_ms;\n                continue;\n            }}\n            const auto next_periodic_deadline_ms = {next_deadline_expr};\n            const auto next_wake_deadline = next_periodic_deadline_ms.has_value()\n                ? std::optional<std::chrono::steady_clock::time_point>{{\n                      std::chrono::steady_clock::now() +\n                      std::chrono::milliseconds{{static_cast<std::chrono::milliseconds::rep>(\n                          next_periodic_deadline_ms->value > scheduler_now_ms\n                              ? next_periodic_deadline_ms->value - scheduler_now_ms\n                              : 0U)}}}}\n                : std::nullopt;\n            switch (scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, shutdown)) {{\n                case flowrt::ScheduleEvent::Shutdown:\n                    status = flowrt::Status::Ok;\n                    break;\n                case flowrt::ScheduleEvent::Timer:\n                    scheduler_now_ms = next_periodic_deadline_ms.has_value()\n                                           ? next_periodic_deadline_ms->value\n                                           : scheduler_now_ms + scheduler_base_period_ms;\n                    break;\n                case flowrt::ScheduleEvent::Data:\n                    break;\n            }}\n            if (shutdown.is_requested()) {{\n                break;\n            }}\n        }}\n    }}\n",
        task_result_health_update = task_result_health_update,
        task_admission_health_update = task_admission_health_update,
            next_deadline_expr = cpp_next_periodic_deadline_expr(&tasks)
        )
        .replace(
            "case flowrt::ScheduleEvent::Data:\n                    break;",
            "case flowrt::ScheduleEvent::Data:\n                    if (const auto data_time_ms = scheduler_events.take_data_time_ms()) {\n                        scheduler_now_ms = std::max(scheduler_now_ms, *data_time_ms);\n                    }\n                    break;",
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
    if contract.artifact.temporary_overlay.is_some() {
        "simulated_replay"
    } else {
        "realtime"
    }
}

pub(super) fn cpp_task_clock_source_expr(contract: &ContractIr) -> &'static str {
    if contract.artifact.temporary_overlay.is_some() {
        "flowrt::ClockSource::Replay"
    } else {
        "flowrt::ClockSource::Runtime"
    }
}

pub(super) fn cpp_trigger_name(trigger: flowrt_ir::TriggerKind) -> &'static str {
    match trigger {
        flowrt_ir::TriggerKind::Periodic => "periodic",
        flowrt_ir::TriggerKind::OnMessage => "on_message",
        flowrt_ir::TriggerKind::Startup => "startup",
        flowrt_ir::TriggerKind::Shutdown => "shutdown",
    }
}

pub(super) fn cpp_task_timing_name(task: &flowrt_ir::TaskIr) -> String {
    format!("{}.{}", task.instance.name, task.name)
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

pub(super) fn cpp_scheduler_base_period_ms(tasks: &[&flowrt_ir::TaskIr]) -> u64 {
    tasks
        .iter()
        .filter(|task| task.trigger == flowrt_ir::TriggerKind::Periodic)
        .filter_map(|task| task.period_ms)
        .min()
        .unwrap_or(1)
}

/// 为本轮 scheduler 预注册 task health 条目，确保未运行 task 也能记录公平性计数。
pub(super) fn emit_cpp_task_health_init(tasks: &[&flowrt_ir::TaskIr]) -> String {
    let mut output = String::new();
    for task in tasks {
        let task_health = cpp_task_health_name(task);
        let lane = cpp_task_lane_name(task);
        output.push_str(&format!(
            "        {{\n            auto& health = health_map[\"{task_health}\"];\n            health.name = \"{task_health}\";\n            health.lane = \"{lane}\";\n        }}\n"
        ));
    }
    output
}

pub(super) fn emit_cpp_task_admission_health_update(tasks: &[&flowrt_ir::TaskIr]) -> String {
    let mut output = String::new();
    output.push_str("                    switch (admission.task.value) {\n");
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let task_health = cpp_task_health_name(task);
        let lane = cpp_task_lane_name(task);
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

pub(super) fn emit_cpp_task_result_health_update(tasks: &[&flowrt_ir::TaskIr]) -> String {
    let mut output = String::new();
    output.push_str("                switch (task_result.task.value) {\n");
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let task_health = cpp_task_health_name(task);
        let lane = cpp_task_lane_name(task);
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

pub(super) fn cpp_next_periodic_deadline_expr(tasks: &[&flowrt_ir::TaskIr]) -> String {
    let deadlines = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.trigger == flowrt_ir::TriggerKind::Periodic)
        .map(|(index, _)| format!("scheduler.next_deadline(flowrt::TaskId{{{}}})", index + 1))
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
