use super::*;

pub(super) fn cpp_operation_registration_block(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    let start_name = cpp_string_literal(&cpp_operation_start_endpoint_name(plan));
    let cancel_name = cpp_string_literal(&cpp_operation_cancel_endpoint_name(plan));
    let status_name = cpp_string_literal(&cpp_operation_status_endpoint_name(plan));
    let feedback_name = cpp_string_literal(&cpp_operation_feedback_endpoint_name(plan));
    let result_name = cpp_string_literal(&cpp_operation_result_endpoint_name(plan));
    let operation_key_name = cpp_string_literal(&plan.operation_name);
    let client_field = cpp_operation_client_field_name(plan);
    let start_server = cpp_operation_start_server_field_name(plan);
    let cancel_server = cpp_operation_cancel_server_field_name(plan);
    let status_server = cpp_operation_status_server_field_name(plan);
    let handle_name = cpp_operation_client_handle_name(plan);
    let goal_ty = cpp_type(&plan.goal_type);
    let feedback_ty = cpp_type(&plan.feedback_type);
    let result_ty = cpp_type(&plan.result_type);
    let queue_depth = plan.queue_depth.max(1);
    let max_in_flight = plan.max_in_flight.max(1);
    let timeout_ms = plan.timeout_ms.max(1);
    let result_retention_ms = plan.result_retention_ms;
    let concurrency = cpp_operation_concurrency(plan.concurrency);
    let preempt = cpp_operation_preempt(plan.preempt);
    let operation_index = plan.index;
    let port = crate::snake_identifier(&plan.server_port);
    let server_instance = &plan.server_instance;
    let server_lane_id = cpp_lane_id_u64_expr(&crate::runtime_plan::operation_server_lane(plan));

    format!(
        r#"    {{
        (void){feedback_name};
        (void){result_name};
        const auto operation_policy_{operation_index} = flowrt::OperationPolicy::make(
            std::chrono::milliseconds{{{timeout_ms}}},
            {concurrency},
            {preempt},
            {queue_depth}U,
            {max_in_flight}U,
            std::chrono::milliseconds{{{result_retention_ms}}});
        this->operation_control_{operation_index}_ = std::make_shared<flowrt::OperationControl>(
            flowrt::fnv1a64({operation_key_name}),
            operation_policy_{operation_index}.value());
        flowrt::InprocServiceConfig config;
        config.queue_depth = {queue_depth};
        config.max_in_flight = {max_in_flight};
        config.default_timeout_ms = {timeout_ms};
        {start_server}_.emplace(
            {start_name},
            [this](const flowrt::OperationStartRequest<{goal_ty}>& request) -> flowrt::ServiceResult<flowrt::OperationStartAck> {{
                auto operation_worker_server = this->{server_instance}_;
                if (!operation_worker_server) {{
                    return flowrt::ServiceResult<flowrt::OperationStartAck>::err(flowrt::ServiceError::Unavailable);
                }}
                auto operation_control = this->operation_control_{operation_index}_;
                const auto started = operation_control->start_with_timeout(request.owner, flowrt::monotonic_time_ms(), request.timeout);
                if (!started.has_value()) {{
                    return flowrt_operation_control_error<flowrt::OperationStartAck>(started.error());
                }}
                const auto ack = started.value();
                const auto id = ack.id;
                auto goal_for_worker = request.goal;
                try {{
                    std::thread([operation_worker_server, operation_control, id, goal_for_worker = std::move(goal_for_worker)]() mutable {{
                        while (true) {{
                            const auto status = operation_control->status(id);
                            if (!status.has_value() || flowrt::is_terminal(status->state)) {{
                                return;
                            }}
                            if (operation_control->ready_to_run(id)) {{
                                break;
                            }}
                            std::this_thread::sleep_for(std::chrono::milliseconds{{1}});
                        }}
                        const auto cancel = operation_control->cancel_token_for(id);
                        if (!cancel.has_value()) {{
                            return;
                        }}
                        if (const auto error = operation_control->mark_running(id);
                            error != flowrt::OperationControlError::Ok) {{
                            return;
                        }}
                        auto progress = flowrt::OperationProgressPublisher<{feedback_ty}>{{id, [operation_control](flowrt::OperationId progress_id, std::uint64_t sequence, std::optional<std::vector<std::uint8_t>> payload) {{
                            operation_control->publish_progress_with_payload(progress_id, sequence, std::move(payload));
                        }}}};
                        flowrt::OperationState terminal_state = flowrt::OperationState::Failed;
                        std::optional<std::vector<std::uint8_t>> result_payload;
                        try {{
                            const auto result = operation_worker_server->on_{port}_operation(goal_for_worker, *cancel, progress);
                            switch (result.kind()) {{
                                case flowrt::OperationHandlerResult<{result_ty}>::Kind::Succeeded:
                                    if (result.value().has_value()) {{
                                        result_payload.emplace(flowrt::detail::encoded_frame_size(*result.value()));
                                        flowrt::detail::encode_frame(*result.value(), std::span<std::uint8_t>{{result_payload->data(), result_payload->size()}});
                                    }}
                                    terminal_state = flowrt::OperationState::Succeeded;
                                    break;
                                case flowrt::OperationHandlerResult<{result_ty}>::Kind::Failed:
                                    terminal_state = flowrt::OperationState::Failed;
                                    break;
                                case flowrt::OperationHandlerResult<{result_ty}>::Kind::Canceled:
                                    terminal_state = flowrt::OperationState::Cancelled;
                                    break;
                            }}
                        }} catch (...) {{
                            terminal_state = flowrt::OperationState::Failed;
                            result_payload = std::nullopt;
                        }}
                        (void)operation_control->complete_with_payload(id, terminal_state, std::move(result_payload));
                    }}).detach();
                }} catch (...) {{
                    (void)operation_control->complete(id, flowrt::OperationState::Failed);
                    return flowrt::ServiceResult<flowrt::OperationStartAck>::err(flowrt::ServiceError::HandlerError);
                }}
                return flowrt::ServiceResult<flowrt::OperationStartAck>::ok(ack);
            }},
            config);
        {cancel_server}_.emplace(
            {cancel_name},
            [this](const flowrt::OperationId& id) -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{
                const auto snapshot = this->operation_control_{operation_index}_->snapshot();
                if (const auto error = this->operation_control_{operation_index}_->request_cancel(id, snapshot.owner);
                    error != flowrt::OperationControlError::Ok) {{
                    return flowrt_operation_control_error<flowrt::OperationStatusSnapshot>(error);
                }}
                return flowrt::ServiceResult<flowrt::OperationStatusSnapshot>::ok(this->operation_control_{operation_index}_->snapshot());
            }},
            config);
        {status_server}_.emplace(
            {status_name},
            [this](const flowrt::OperationId& id) -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{
                const auto status = this->operation_control_{operation_index}_->status(id);
                if (!status.has_value()) {{
                    return flowrt_operation_control_error<flowrt::OperationStatusSnapshot>(status.error());
                }}
                return flowrt::ServiceResult<flowrt::OperationStatusSnapshot>::ok(status.value());
            }},
            config);
        {client_field}_ = {handle_name}(
            flowrt::InprocServiceClient<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>({start_name}, *{start_server}_, 0, {server_lane_id}),
            flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>({cancel_name}, *{cancel_server}_, 0, {server_lane_id}),
            flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>({status_name}, *{status_server}_, 0, {server_lane_id}));
    }}
"#,
    )
}

pub(super) fn cpp_operation_client_handle_classes(
    contract: &ContractIr,
    plans: &[crate::runtime_plan::OperationRuntimePlan],
) -> String {
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let mut emitted = std::collections::BTreeSet::new();
    for plan in plans {
        let handle_name = cpp_operation_client_handle_name(plan);
        if !emitted.insert(handle_name.clone()) {
            continue;
        }
        let goal_ty = cpp_type(&plan.goal_type);
        let default_timeout_ms = plan.timeout_ms.max(1);
        let owner_scope = cpp_string_literal(&plan.operation_name);
        let owner_name =
            cpp_string_literal(&format!("{}.{}", plan.client_instance, plan.client_port));
        if matches!(plan.backend.0.as_str(), "zenoh" | "iox2") {
            let backend = if plan.backend.0 == "iox2" {
                "iox2"
            } else {
                "zenoh"
            };
            let start_client_ty = if plan.backend.0 == "iox2" {
                cpp_operation_start_client_transport_type(contract, plan, &goal_ty)
            } else {
                format!(
                    "flowrt::zenoh::ZenohServiceClient<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>"
                )
            };
            let control_client_ty = if plan.backend.0 == "iox2" {
                "flowrt::iox2::Iox2ServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>".to_string()
            } else {
                "flowrt::zenoh::ZenohServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>".to_string()
            };
            if plan.backend.0 == "iox2" {
                output.push_str(&format!(
                    "/**\n * @brief `{client}.{port}` Operation client（{backend} backend）。\n *\n * `slot_` 在所属进程启动时由 runtime shell 经 `bind()` 填充内部 {backend} service clients；\n * 用户代码仍只看到 Operation start/cancel/status API。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() : slot_(std::make_shared<Slot>()) {{}}\n\n    void bind(\n        std::shared_ptr<{start_client_ty}> start_client,\n        std::shared_ptr<{control_client_ty}> cancel_client,\n        std::shared_ptr<{control_client_ty}> status_client) {{\n        if (slot_) {{\n            slot_->start_client = std::move(start_client);\n            slot_->cancel_client = std::move(cancel_client);\n            slot_->status_client = std::move(status_client);\n        }}\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStartAck> start(const {goal_ty}& goal, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->start_client) {{\n            return flowrt::OperationClientResult<flowrt::OperationStartAck>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        const auto owner = flowrt::OperationOwner{{.scope_key = flowrt::fnv1a64({owner_scope}), .owner_key = flowrt::fnv1a64({owner_name})}};\n        const auto request = flowrt::OperationStartRequest<{goal_ty}>{{.goal = goal, .owner = owner, .timeout = std::chrono::milliseconds{{static_cast<std::chrono::milliseconds::rep>(timeout_ms)}}}};\n        return flowrt::operation_client_result_from_service(slot_->start_client->call(request, timeout_ms));\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> cancel(flowrt::OperationId id, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->cancel_client) {{\n            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        return flowrt::operation_client_result_from_service(slot_->cancel_client->call(id, timeout_ms));\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> status(flowrt::OperationId id, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->status_client) {{\n            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        return flowrt::operation_client_result_from_service(slot_->status_client->call(id, timeout_ms));\n    }}\n\nprivate:\n    struct Slot {{\n        std::shared_ptr<{start_client_ty}> start_client;\n        std::shared_ptr<{control_client_ty}> cancel_client;\n        std::shared_ptr<{control_client_ty}> status_client;\n    }};\n    std::shared_ptr<Slot> slot_;\n}};\n\n",
                    client = plan.client_instance,
                    port = plan.client_port,
                ));
            } else {
                output.push_str(&format!(
                    "/**\n * @brief `{client}.{port}` Operation client（{backend} backend）。\n *\n * `slot_` 在所属进程启动时由 runtime shell 经 `bind()` 填充内部 {backend} service clients；\n * 用户代码仍只看到 Operation start/cancel/status API。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() : slot_(std::make_shared<Slot>()) {{}}\n\n    void bind(\n        {start_client_ty} start_client,\n        {control_client_ty} cancel_client,\n        {control_client_ty} status_client) {{\n        if (slot_) {{\n            slot_->start_client.emplace(std::move(start_client));\n            slot_->cancel_client.emplace(std::move(cancel_client));\n            slot_->status_client.emplace(std::move(status_client));\n        }}\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStartAck> start(const {goal_ty}& goal, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->start_client.has_value()) {{\n            return flowrt::OperationClientResult<flowrt::OperationStartAck>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        const auto owner = flowrt::OperationOwner{{.scope_key = flowrt::fnv1a64({owner_scope}), .owner_key = flowrt::fnv1a64({owner_name})}};\n        const auto request = flowrt::OperationStartRequest<{goal_ty}>{{.goal = goal, .owner = owner, .timeout = std::chrono::milliseconds{{static_cast<std::chrono::milliseconds::rep>(timeout_ms)}}}};\n        return flowrt::operation_client_result_from_service(slot_->start_client->call(request, timeout_ms));\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> cancel(flowrt::OperationId id, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->cancel_client.has_value()) {{\n            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        return flowrt::operation_client_result_from_service(slot_->cancel_client->call(id, timeout_ms));\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> status(flowrt::OperationId id, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->status_client.has_value()) {{\n            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        return flowrt::operation_client_result_from_service(slot_->status_client->call(id, timeout_ms));\n    }}\n\nprivate:\n    struct Slot {{\n        std::optional<{start_client_ty}> start_client;\n        std::optional<{control_client_ty}> cancel_client;\n        std::optional<{control_client_ty}> status_client;\n    }};\n    std::shared_ptr<Slot> slot_;\n}};\n\n",
                    client = plan.client_instance,
                    port = plan.client_port,
                ));
            }
        } else {
            output.push_str(&format!(
                "/**\n * @brief `{client}.{port}` Operation client typed wrapper。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() = default;\n\n    {handle_name}(\n        flowrt::InprocServiceClient<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck> start_client,\n        flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot> cancel_client,\n        flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot> status_client)\n        : start_client_(std::move(start_client)), cancel_client_(std::move(cancel_client)), status_client_(std::move(status_client)) {{}}\n\n    flowrt::OperationClientResult<flowrt::OperationStartAck> start(const {goal_ty}& goal, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!start_client_.has_value()) {{\n            return flowrt::OperationClientResult<flowrt::OperationStartAck>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        const auto owner = flowrt::OperationOwner{{.scope_key = flowrt::fnv1a64({owner_scope}), .owner_key = flowrt::fnv1a64({owner_name})}};\n        const auto request = flowrt::OperationStartRequest<{goal_ty}>{{.goal = goal, .owner = owner, .timeout = std::chrono::milliseconds{{static_cast<std::chrono::milliseconds::rep>(timeout_ms)}}}};\n        return flowrt::operation_client_result_from_service(start_client_->call(request, timeout_ms));\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> cancel(flowrt::OperationId id, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!cancel_client_.has_value()) {{\n            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        return flowrt::operation_client_result_from_service(cancel_client_->call(id, timeout_ms));\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> status(flowrt::OperationId id, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!status_client_.has_value()) {{\n            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        return flowrt::operation_client_result_from_service(status_client_->call(id, timeout_ms));\n    }}\n\nprivate:\n    std::optional<flowrt::InprocServiceClient<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>> start_client_;\n    std::optional<flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>> cancel_client_;\n    std::optional<flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>> status_client_;\n}};\n\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
        }
    }

    output
}

pub(super) fn cpp_operation_client_handle_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "OperationClient_{}_{}",
        crate::snake_identifier(&plan.client_component),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(super) fn cpp_operation_client_field_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "operation_client_{}_{}",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(super) fn cpp_operation_start_server_field_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "operation_start_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

pub(super) fn cpp_operation_cancel_server_field_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "operation_cancel_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

pub(super) fn cpp_operation_status_server_field_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "operation_status_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

pub(super) fn cpp_operation_step_fn_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "step_operation_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

pub(super) fn cpp_operation_handler_methods(
    component: &ComponentIr,
    graph: &GraphIr,
    plans: &[crate::runtime_plan::OperationRuntimePlan],
) -> String {
    let server_instances: std::collections::BTreeSet<&str> = graph
        .instances
        .iter()
        .filter(|instance| {
            instance.component.name == component.name
                || instance.component.name == component.generated_name
                || instance.component.name == component.qualified_name
        })
        .map(|instance| instance.name.as_str())
        .collect();
    let relevant_plans = plans
        .iter()
        .filter(|plan| server_instances.contains(plan.server_instance.as_str()))
        .collect::<Vec<_>>();
    if relevant_plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let mut emitted = std::collections::BTreeSet::new();
    for plan in relevant_plans {
        let method_name = format!(
            "on_{}_operation",
            crate::snake_identifier(&plan.server_port)
        );
        if !emitted.insert(method_name.clone()) {
            continue;
        }
        let goal_ty = cpp_type(&plan.goal_type);
        let feedback_ty = cpp_type(&plan.feedback_type);
        let result_ty = cpp_type(&plan.result_type);
        output.push_str(&format!(
            "    /**\n     * @brief 处理 `{}` Operation goal。\n     */\n",
            plan.server_port
        ));
        output.push_str(&format!(
            "    virtual flowrt::OperationHandlerResult<{result_ty}> {method_name}(\n        const {goal_ty}& goal,\n        flowrt::OperationCancelToken cancel,\n        flowrt::OperationProgressPublisher<{feedback_ty}>& progress) {{\n        (void)goal;\n        (void)cancel;\n        (void)progress;\n        return flowrt::OperationHandlerResult<{result_ty}>::failed();\n    }}\n\n",
        ));
    }

    output
}

pub(super) fn cpp_operation_start_endpoint_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "__flowrt_operation_{}_{}_start",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(super) fn cpp_operation_cancel_endpoint_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "__flowrt_operation_{}_{}_cancel",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(super) fn cpp_operation_status_endpoint_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "__flowrt_operation_{}_{}_status",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(super) fn cpp_operation_feedback_endpoint_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "__flowrt_operation_{}_{}_feedback",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(super) fn cpp_operation_result_endpoint_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "__flowrt_operation_{}_{}_result",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(super) fn cpp_operation_concurrency(policy: OperationConcurrencyPolicy) -> &'static str {
    match policy {
        OperationConcurrencyPolicy::Reject => "flowrt::OperationConcurrencyPolicy::Reject",
        OperationConcurrencyPolicy::Queue => "flowrt::OperationConcurrencyPolicy::Queue",
    }
}

pub(super) fn cpp_operation_preempt(policy: OperationPreemptPolicy) -> &'static str {
    match policy {
        OperationPreemptPolicy::Reject => "flowrt::OperationPreemptPolicy::Reject",
        OperationPreemptPolicy::CancelRunning => "flowrt::OperationPreemptPolicy::CancelRunning",
    }
}
