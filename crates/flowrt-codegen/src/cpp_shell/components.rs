use super::*;

pub(super) fn cpp_component_interface_doc(component: &ComponentIr) -> String {
    format!(
        "/**\n * @brief `{}` 组件的 C++ 用户实现接口。\n *\n * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。\n */\n",
        component.name
    )
}

pub(super) fn cpp_lifecycle_method(name: &str) -> String {
    let brief = match name {
        "on_init" => "组件初始化钩子。",
        "on_start" => "组件启动钩子。",
        "on_stop" => "组件停止钩子。",
        "on_shutdown" => "组件关闭钩子。",
        _ => "组件生命周期钩子。",
    };
    format!(
        "    /**\n     * @brief {brief}\n     *\n     * @param context runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。\n     * @return 本次生命周期步骤的 FlowRT 执行状态。\n     */\n    virtual flowrt::Status {name}(flowrt::Context& context) {{\n        (void)context;\n        return flowrt::ok();\n    }}\n"
    )
}

pub(super) fn cpp_tick_signature(
    component: &ComponentIr,
    service_plans: &[crate::runtime_plan::ServiceRuntimePlan],
    operation_plans: &[crate::runtime_plan::OperationRuntimePlan],
) -> String {
    let args = cpp_callback_args(component, service_plans, operation_plans);
    let doc = cpp_tick_doc(component, service_plans, operation_plans);
    if args.is_empty() {
        format!("{doc}    virtual flowrt::Status on_tick() = 0;\n")
    } else {
        let joined = args
            .iter()
            .map(|arg| format!("        {arg}"))
            .collect::<Vec<_>>()
            .join(",\n");
        format!("{doc}    virtual flowrt::Status on_tick(\n{joined}) = 0;\n")
    }
}

pub(super) fn cpp_tick_doc(
    component: &ComponentIr,
    service_plans: &[crate::runtime_plan::ServiceRuntimePlan],
    operation_plans: &[crate::runtime_plan::OperationRuntimePlan],
) -> String {
    let mut output = format!(
        "    /**\n     * @brief 执行一次 `{}` 组件调度回调。\n     *\n     * runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入视图内部指针到回调之外。\n",
        component.name
    );
    if !component.inputs.is_empty() || !component.outputs.is_empty() {
        output.push_str("     *\n");
    }
    for input in &component.inputs {
        output.push_str(&format!(
            "     * @param {} latest snapshot 输入视图。\n",
            input.name
        ));
    }
    let mut emitted_service_args = std::collections::BTreeSet::new();
    for plan in service_plans.iter().filter(|plan| {
        plan.client_component == component.name
            || plan.client_component == component.generated_name
            || plan.client_component == component.qualified_name
    }) {
        let arg_name = crate::snake_identifier(&plan.client_port);
        if emitted_service_args.insert(arg_name.clone()) {
            output.push_str(&format!(
                "     * @param {arg_name} typed service client handle。\n"
            ));
        }
    }
    let mut emitted_operation_args = std::collections::BTreeSet::new();
    for plan in operation_plans.iter().filter(|plan| {
        plan.client_component == component.name
            || plan.client_component == component.generated_name
            || plan.client_component == component.qualified_name
    }) {
        let arg_name = crate::snake_identifier(&plan.client_port);
        if emitted_operation_args.insert(arg_name.clone()) {
            output.push_str(&format!(
                "     * @param {arg_name} typed Operation client handle。\n"
            ));
        }
    }
    for output_port in &component.outputs {
        output.push_str(&format!(
            "     * @param {} 输出端口写入句柄。\n",
            output_port.name
        ));
    }
    output.push_str("     * @return 本次回调的 FlowRT 执行状态。\n     */\n");
    output
}

pub(crate) fn cpp_callback_args(
    component: &ComponentIr,
    service_plans: &[crate::runtime_plan::ServiceRuntimePlan],
    operation_plans: &[crate::runtime_plan::OperationRuntimePlan],
) -> Vec<String> {
    let mut args = Vec::new();
    let mut emitted_service_args = std::collections::BTreeSet::new();
    for plan in service_plans.iter().filter(|plan| {
        plan.client_component == component.name
            || plan.client_component == component.generated_name
            || plan.client_component == component.qualified_name
    }) {
        let arg_name = crate::snake_identifier(&plan.client_port);
        if emitted_service_args.insert(arg_name.clone()) {
            args.push(format!(
                "{}& {arg_name}",
                cpp_service_client_handle_name(plan)
            ));
        }
    }
    let mut emitted_operation_args = std::collections::BTreeSet::new();
    for plan in operation_plans.iter().filter(|plan| {
        plan.client_component == component.name
            || plan.client_component == component.generated_name
            || plan.client_component == component.qualified_name
    }) {
        let arg_name = crate::snake_identifier(&plan.client_port);
        if emitted_operation_args.insert(arg_name.clone()) {
            args.push(format!(
                "{}& {arg_name}",
                cpp_operation_client_handle_name(plan)
            ));
        }
    }
    for input in &component.inputs {
        args.push(format!(
            "const flowrt::Latest<{}>& {}",
            cpp_type(&input.ty),
            input.name
        ));
    }
    if !component.params.is_empty() {
        args.push(format!(
            "const {}Params& params",
            component_cpp_name(component)
        ));
    }
    for output in &component.outputs {
        args.push(format!(
            "flowrt::Output<{}>& {}",
            cpp_type(&output.ty),
            output.name
        ));
    }
    args
}
