use super::*;

pub(super) fn cpp_service_client_handle_classes(
    contract: &ContractIr,
    plans: &[crate::runtime_plan::ServiceRuntimePlan],
) -> String {
    if plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    let mut emitted = std::collections::BTreeSet::new();
    for plan in plans {
        let handle_name = cpp_service_client_handle_name(plan);
        if !emitted.insert(handle_name.clone()) {
            continue;
        }
        let req_ty = cpp_type(&plan.request_type);
        let resp_ty = cpp_type(&plan.response_type);
        let is_zenoh = plan.backend.0 == "zenoh";
        let is_iox2 = plan.backend.0 == "iox2";
        let default_timeout_ms = plan.timeout_ms.max(1);

        if is_iox2 {
            let client_ty = cpp_service_client_transport_type(contract, plan, &req_ty, &resp_ty);
            output.push_str(&format!(
                "/**\n * @brief `{client}.{port}` service client（iox2 backend）。\n *\n * `slot_` 在所属进程启动时由 runtime shell 经 `bind()` 填充 `Iox2ServiceClient`；\n * 其它进程不填充，调用返回 `ServiceError::Unavailable`。handle 经 shared_ptr 共享 slot，可拷贝传入回调。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() : slot_(std::make_shared<Slot>()) {{}}\n\n    /** @brief 由所属进程 runtime shell 填充 transport client。 */\n    void bind(std::shared_ptr<{client_ty}> client) {{\n        if (slot_) {{\n            slot_->client = std::move(client);\n        }}\n    }}\n\n    flowrt::ServiceResult<{resp_ty}> call(const {req_ty}& request, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->client) {{\n            return flowrt::ServiceResult<{resp_ty}>::err(flowrt::ServiceError::Unavailable);\n        }}\n        return slot_->client->call(request, timeout_ms);\n    }}\n\n    flowrt::InprocServiceHandle<{resp_ty}> start_call(const {req_ty}& request, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->client) {{\n            return flowrt::InprocServiceHandle<{resp_ty}>::ready_error(flowrt::ServiceError::Unavailable);\n        }}\n        return flowrt::InprocServiceHandle<{resp_ty}>::ready(slot_->client->call(request, timeout_ms));\n    }}\n\nprivate:\n    struct Slot {{\n        std::shared_ptr<{client_ty}> client;\n    }};\n    std::shared_ptr<Slot> slot_;\n}};\n\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
        } else if is_zenoh {
            let backend = if is_iox2 { "iox2" } else { "zenoh" };
            let client_ty = format!("flowrt::zenoh::ZenohServiceClient<{req_ty}, {resp_ty}>");
            let client_label = "ZenohServiceClient";
            output.push_str(&format!(
                "/**\n * @brief `{client}.{port}` service client（{backend} backend）。\n *\n * `slot_` 在所属进程启动时由 runtime shell 经 `bind()` 填充 `{client_label}`；\n * 其它进程不填充，调用返回 `ServiceError::Unavailable`。handle 经 shared_ptr 共享 slot，可拷贝传入回调。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() : slot_(std::make_shared<Slot>()) {{}}\n\n    /** @brief 由所属进程 runtime shell 填充 transport client。 */\n    void bind({client_ty} client) {{\n        if (slot_) {{\n            slot_->client.emplace(std::move(client));\n        }}\n    }}\n\n    flowrt::ServiceResult<{resp_ty}> call(const {req_ty}& request, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->client.has_value()) {{\n            return flowrt::ServiceResult<{resp_ty}>::err(flowrt::ServiceError::Unavailable);\n        }}\n        return slot_->client->call(request, timeout_ms);\n    }}\n\n    flowrt::InprocServiceHandle<{resp_ty}> start_call(const {req_ty}& request, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!slot_ || !slot_->client.has_value()) {{\n            return flowrt::InprocServiceHandle<{resp_ty}>::ready_error(flowrt::ServiceError::Unavailable);\n        }}\n        return flowrt::InprocServiceHandle<{resp_ty}>::ready(slot_->client->call(request, timeout_ms));\n    }}\n\nprivate:\n    struct Slot {{\n        std::optional<{client_ty}> client;\n    }};\n    std::shared_ptr<Slot> slot_;\n}};\n\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
        } else {
            output.push_str(&format!(
                "/**\n * @brief `{client}.{port}` service client typed wrapper。\n *\n * 封装 FlowRT service client，提供同步 `call()` 和非阻塞 `start_call()`。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() = default;\n\n    explicit {handle_name}(flowrt::InprocServiceClient<{req_ty}, {resp_ty}> client)\n        : client_(std::move(client)) {{}}\n\n    /**\n     * @brief 发起同步阻塞 service 调用。\n     *\n     * @param request 请求消息。\n     * @param timeout_ms 超时毫秒。0 会返回 Timeout。\n     * @return 成功返回响应值，超时或错误返回 ServiceError。\n     */\n    flowrt::ServiceResult<{resp_ty}> call(const {req_ty}& request, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!client_.has_value()) {{\n            return flowrt::ServiceResult<{resp_ty}>::err(flowrt::ServiceError::Unavailable);\n        }}\n        return client_->call(request, timeout_ms);\n    }}\n\n    /**\n     * @brief 发起非阻塞 service 调用。\n     *\n     * @param request 请求消息。\n     * @param timeout_ms 超时毫秒。0 会返回 Timeout。\n     * @return 非阻塞 handle，支持 `poll()` 和 `complete()`。\n     */\n    flowrt::InprocServiceHandle<{resp_ty}> start_call(const {req_ty}& request, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!client_.has_value()) {{\n            return flowrt::InprocServiceHandle<{resp_ty}>::ready_error(flowrt::ServiceError::Unavailable);\n        }}\n        return client_->start_call(request, timeout_ms);\n    }}\n\nprivate:\n    std::optional<flowrt::InprocServiceClient<{req_ty}, {resp_ty}>> client_;\n}};\n\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
        }
    }
    output
}

pub(super) fn cpp_service_client_handle_name(
    plan: &crate::runtime_plan::ServiceRuntimePlan,
) -> String {
    format!(
        "ServiceClient_{}_{}",
        crate::snake_identifier(&plan.client_component),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(super) fn cpp_service_client_field_name(
    plan: &crate::runtime_plan::ServiceRuntimePlan,
) -> String {
    format!(
        "service_client_{}_{}",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

pub(super) fn cpp_service_server_field_name(
    plan: &crate::runtime_plan::ServiceRuntimePlan,
) -> String {
    format!(
        "service_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

pub(super) fn cpp_service_step_fn_name(plan: &crate::runtime_plan::ServiceRuntimePlan) -> String {
    format!(
        "step_service_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

pub(super) fn cpp_service_handler_methods(
    component: &ComponentIr,
    graph: &GraphIr,
    plans: &[crate::runtime_plan::ServiceRuntimePlan],
) -> String {
    // 找出该 component 的所有实例作为 server 的 plans
    let server_instances: std::collections::BTreeSet<&str> = graph
        .instances
        .iter()
        .filter(|i| {
            i.component.name == component.name
                || i.component.name == component.generated_name
                || i.component.name == component.qualified_name
        })
        .map(|i| i.name.as_str())
        .collect();

    let relevant_plans: Vec<&crate::runtime_plan::ServiceRuntimePlan> = plans
        .iter()
        .filter(|p| server_instances.contains(p.server_instance.as_str()))
        .collect();

    if relevant_plans.is_empty() {
        return String::new();
    }

    let mut output = String::new();
    for plan in relevant_plans {
        let method_name = format!("on_{}_request", crate::snake_identifier(&plan.server_port));
        let req_ty = cpp_type(&plan.request_type);
        let resp_ty = cpp_type(&plan.response_type);
        let port_name = &plan.server_port;

        output.push_str(&format!(
            "    /**\n\
             * @brief 处理 `{port_name}` service request。\n\
             *\n\
             * runtime shell 在 hidden service task 中调用该方法。用户业务逻辑\n\
             * 实现具体的 request -> response 转换。\n\
             *\n\
             * @param request 请求消息引用。\n\
             * @return 成功返回 `ServiceResult::ok(response)`，业务错误返回\n\
             *         `ServiceResult::err(error_code, message)`。\n\
             */\n",
        ));
        output.push_str(&format!(
            "    virtual flowrt::ServiceResult<{resp_ty}> {method_name}(const {req_ty}& request) {{\n\
                 (void)request;\n\
                 return flowrt::ServiceResult<{resp_ty}>::err(flowrt::ServiceError::HandlerError);\n\
             }}\n\n",
        ));
    }

    output
}
