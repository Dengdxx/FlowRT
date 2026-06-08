use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    ChannelKind, ComponentIr, ContractIr, GraphIr, InstanceIr, LanguageKind,
    OperationConcurrencyPolicy, OperationPreemptPolicy, OverflowPolicy as IrOverflowPolicy,
    ParamIr, ParamType, ParamUpdatePolicy, ParamValue, PortIr, StalePolicy as IrStalePolicy,
};

use crate::messages::cpp_type;
use crate::runtime_plan::{
    BindRuntimePlan, BridgeRuntimePlan, ProcessRuntimePlan, TaskEmissionPhase,
    active_binds_for_instances, bind_backend, bind_runtime_plans, bridge_runtime_plans,
    incoming_bind_index_map, indent_generated_block, indent_generated_block_levels,
    nested_step_indent, on_message_trigger_guard, outgoing_bind_indices_map,
    outgoing_bridge_indices_map, process_runtime_plans, runtime_channel_message_type,
    runtime_channel_name, runtime_channel_probe_capacity, runtime_param_name, step_indent,
};
use crate::{
    component_by_name, component_rust_name, float_literal, iox2_service_name, managed_header,
    param_json_literal, param_type_name, param_update_name, param_value_for_instance,
    ros2_bridge_key_expr, scheduler_tasks_for_order, selected_backend_name,
    selected_profile_worker_threads, tasks_for_instance, topo_order_instances_for_language,
    zenoh_key_expr,
};

fn component_cpp_name(component: &ComponentIr) -> String {
    component_rust_name(component)
}

fn cpp_param_type(ty: ParamType) -> &'static str {
    match ty {
        ParamType::Bool => "bool",
        ParamType::U8 => "std::uint8_t",
        ParamType::U16 => "std::uint16_t",
        ParamType::U32 => "std::uint32_t",
        ParamType::U64 => "std::uint64_t",
        ParamType::I8 => "std::int8_t",
        ParamType::I16 => "std::int16_t",
        ParamType::I32 => "std::int32_t",
        ParamType::I64 => "std::int64_t",
        ParamType::F32 => "float",
        ParamType::F64 => "double",
        ParamType::String => "std::string",
        ParamType::Array | ParamType::Table => "std::string",
    }
}

fn cpp_param_literal(param: &ParamIr, value: &ParamValue) -> String {
    match (param.ty, value) {
        (ParamType::Bool, ParamValue::Bool(value)) => value.to_string(),
        (
            ParamType::U8
            | ParamType::U16
            | ParamType::U32
            | ParamType::U64
            | ParamType::I8
            | ParamType::I16
            | ParamType::I32
            | ParamType::I64,
            ParamValue::Integer(value),
        ) => format!("{}{{{value}}}", cpp_param_type(param.ty)),
        (ParamType::F32, ParamValue::Float(value)) => format!("{}F", float_literal(*value)),
        (ParamType::F32, ParamValue::Integer(value)) => format!("{}.0F", value),
        (ParamType::F64, ParamValue::Float(value)) => float_literal(*value),
        (ParamType::F64, ParamValue::Integer(value)) => format!("{}.0", value),
        (ParamType::String, ParamValue::String(value)) => cpp_string_literal(value),
        (ParamType::Array | ParamType::Table, _) => cpp_string_literal(&param_json_literal(value)),
        _ => cpp_string_literal(&param_json_literal(value)),
    }
}

pub(crate) fn emit_cpp_components(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str(
        "#include <cstdint>\n#include <map>\n#include <optional>\n#include <string>\n#include <utility>\n\n",
    );
    output.push_str(
        "#include <flowrt/runtime.hpp>\n#include <flowrt/inproc_service.hpp>\n#include <flowrt/operation.hpp>\n#include <flowrt/service.hpp>\n\n",
    );
    output.push_str("#include \"flowrt_app/messages.hpp\"\n\n");
    output.push_str("namespace flowrt_app {\n\n");

    // 预先计算 service plans
    let graph = contract.graphs.first();
    let service_plans = graph
        .map(|g| crate::runtime_plan::service_runtime_plans(contract, g))
        .unwrap_or_default();
    let operation_plans = graph
        .map(|g| crate::runtime_plan::operation_runtime_plans(contract, g))
        .unwrap_or_default();
    output.push_str(&cpp_service_client_handle_classes(&service_plans));
    output.push_str(&cpp_operation_client_handle_classes(&operation_plans));

    for component in contract
        .components
        .iter()
        .filter(|component| component.language == LanguageKind::Cpp)
    {
        if !component.params.is_empty() {
            output.push_str(&cpp_params_struct(component));
        }
        output.push_str(&cpp_component_interface_doc(component));
        output.push_str(&format!(
            "class {}Interface {{\n",
            component_cpp_name(component)
        ));
        output.push_str("public:\n");
        output.push_str(&format!(
            "    virtual ~{}Interface() = default;\n",
            component_cpp_name(component)
        ));
        output.push_str(&cpp_lifecycle_method("on_init"));
        output.push_str(&cpp_lifecycle_method("on_start"));
        output.push_str(&cpp_lifecycle_method("on_stop"));
        output.push_str(&cpp_lifecycle_method("on_shutdown"));
        output.push_str(&cpp_params_update_signature(component));
        // service handler 方法
        if let Some(g) = graph {
            output.push_str(&cpp_service_handler_methods(component, g, &service_plans));
            output.push_str(&cpp_operation_handler_methods(
                component,
                g,
                &operation_plans,
            ));
        }
        output.push_str(&cpp_tick_signature(
            component,
            &service_plans,
            &operation_plans,
        ));
        output.push_str("};\n\n");
    }

    output.push_str("}  // namespace flowrt_app\n");
    output
}

pub(crate) fn emit_cpp_runtime_shell(contract: &ContractIr) -> String {
    let graph = contract
        .graphs
        .first()
        .expect("normalized contract must contain at least one graph");
    let order = topo_order_instances_for_language(contract, graph, LanguageKind::Cpp);
    let process_plans = process_runtime_plans(&order);
    let bind_plans = bind_runtime_plans(contract, graph);
    let bridge_plans = bridge_runtime_plans(contract, graph);
    let incoming_bind_index = incoming_bind_index_map(&bind_plans);
    let outgoing_bind_indices = outgoing_bind_indices_map(&bind_plans);
    let outgoing_bridge_indices = outgoing_bridge_indices_map(&bridge_plans);
    let selected_backend = selected_backend_name(contract);

    let mut output = managed_header();
    output.push_str("#include \"flowrt_app/runtime_shell.hpp\"\n\n");
    output.push_str("#include \"flowrt_app/selfdesc.hpp\"\n\n");
    output.push_str("#include <algorithm>\n#include <cerrno>\n#include <chrono>\n#include <cstdint>\n#include <cstdlib>\n#include <limits>\n#include <optional>\n#include <span>\n#include <string>\n#include <string_view>\n#include <type_traits>\n#include <utility>\n#include <variant>\n#include <vector>\n\n");
    output.push_str("namespace {\n\n");
    output.push_str(
        "flowrt::Status status_from_push_result(const flowrt::ChannelPushResult& result) {\n    if (std::holds_alternative<flowrt::ChannelError>(result)) {\n        return flowrt::Status::Error;\n    }\n\n    switch (std::get<flowrt::ChannelWriteOutcome>(result)) {\n        case flowrt::ChannelWriteOutcome::Accepted:\n        case flowrt::ChannelWriteOutcome::DroppedOldest:\n        case flowrt::ChannelWriteOutcome::DroppedNewest:\n            return flowrt::Status::Ok;\n        case flowrt::ChannelWriteOutcome::Backpressured:\n            return flowrt::Status::Retry;\n    }\n\n    return flowrt::Status::Error;\n}\n\n",
    );
    output.push_str(&emit_cpp_introspection_helpers());
    output.push_str("}  // namespace\n\n");
    output.push_str("namespace flowrt_app {\n\n");
    output.push_str(&emit_cpp_app_constructor(
        contract,
        graph,
        &order,
        &bind_plans,
        &bridge_plans,
    ));
    let step_emission = CppStepEmission {
        contract,
        graph,
        binds: &bind_plans,
        bridges: &bridge_plans,
        incoming_bind_index: &incoming_bind_index,
        outgoing_bind_indices: &outgoing_bind_indices,
        outgoing_bridge_indices: &outgoing_bridge_indices,
    };
    output.push_str(&emit_cpp_app_step(
        &step_emission,
        &order,
        "step",
        TaskEmissionPhase::Scheduler,
        None,
    ));
    output.push_str(&emit_cpp_app_step(
        &step_emission,
        &order,
        "step_startup",
        TaskEmissionPhase::Startup,
        None,
    ));
    output.push_str(&emit_cpp_app_step(
        &step_emission,
        &order,
        "step_shutdown",
        TaskEmissionPhase::Shutdown,
        None,
    ));
    for task in scheduler_tasks_for_order(graph, &order) {
        output.push_str(&emit_cpp_app_step(
            &step_emission,
            &order,
            &cpp_task_step_function_name(task),
            TaskEmissionPhase::Scheduler,
            Some(task),
        ));
    }
    for process in &process_plans {
        output.push_str(&emit_cpp_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}", process.method_suffix),
            TaskEmissionPhase::Scheduler,
            None,
        ));
        output.push_str(&emit_cpp_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}_startup", process.method_suffix),
            TaskEmissionPhase::Startup,
            None,
        ));
        output.push_str(&emit_cpp_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}_shutdown", process.method_suffix),
            TaskEmissionPhase::Shutdown,
            None,
        ));
        for task in scheduler_tasks_for_order(graph, &process.instances) {
            output.push_str(&emit_cpp_app_step(
                &step_emission,
                &process.instances,
                &cpp_process_task_step_function_name(process, task),
                TaskEmissionPhase::Scheduler,
                Some(task),
            ));
        }
    }
    // service step functions
    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    for plan in &service_plans {
        let fn_name = cpp_service_step_fn_name(plan);
        let server_field = cpp_service_server_field_name(plan);
        let is_zenoh = plan.backend.0 == "zenoh";
        if is_zenoh {
            continue;
        }
        output.push_str(&format!(
            "flowrt::Status App::{fn_name}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {{\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    (void)scheduler_events;\n    (void)health_map;\n    if ({server_field}_.has_value()) {{\n        {server_field}_->process_pending();\n    }}\n    return flowrt::Status::Ok;\n}}\n\n"
        ));
    }
    // operation step functions
    let operation_plans = crate::runtime_plan::operation_runtime_plans(contract, graph);
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        output.push_str(&format!(
            "flowrt::Status App::{fn_name}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {{\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    (void)scheduler_events;\n    (void)health_map;\n    if ({start_server}_.has_value()) {{\n        {start_server}_->process_pending();\n    }}\n    if ({cancel_server}_.has_value()) {{\n        {cancel_server}_->process_pending();\n    }}\n    if ({status_server}_.has_value()) {{\n        {status_server}_->process_pending();\n    }}\n    return flowrt::Status::Ok;\n}}\n\n",
            fn_name = cpp_operation_step_fn_name(plan),
            start_server = cpp_operation_start_server_field_name(plan),
            cancel_server = cpp_operation_cancel_server_field_name(plan),
            status_server = cpp_operation_status_server_field_name(plan),
        ));
    }
    output.push_str(&emit_cpp_app_run_function(&CppRunEmission {
        contract,
        function_name: "run",
        step_function_name: "step",
        startup_function_name: "step_startup",
        shutdown_function_name: "step_shutdown",
        order: &order,
        binds: &bind_plans,
        graph,
        process: None,
        package_name: &contract.package.name,
        process_name: "main",
    }));
    output.push_str(&emit_cpp_app_run_process_dispatch(&process_plans));
    for process in &process_plans {
        let function_name = format!("run_process_{}", process.method_suffix);
        let step_function_name = format!("step_process_{}", process.method_suffix);
        let startup_function_name = format!("step_process_{}_startup", process.method_suffix);
        let shutdown_function_name = format!("step_process_{}_shutdown", process.method_suffix);
        output.push_str(&emit_cpp_app_run_function(&CppRunEmission {
            contract,
            function_name: &function_name,
            step_function_name: &step_function_name,
            startup_function_name: &startup_function_name,
            shutdown_function_name: &shutdown_function_name,
            order: &process.instances,
            binds: &bind_plans,
            graph,
            process: Some(process),
            package_name: &contract.package.name,
            process_name: &process.name,
        }));
    }
    let backend_factory = cpp_backend_factory(&selected_backend);
    output.push_str(
        &format!(
        "flowrt::Status run(std::optional<std::size_t> run_ticks) {{\n    auto backend = {backend_factory};\n    return flowrt_user::build_app().run(backend, run_ticks);\n}}\n\n"
    ));
    output.push_str(&format!(
        "flowrt::Status run_process(std::string_view process, std::optional<std::size_t> run_ticks) {{\n    auto backend = {backend_factory};\n    return flowrt_user::build_app().run_process(backend, process, run_ticks);\n}}\n\n"
    ));
    output.push_str("}  // namespace flowrt_app\n");
    output
}

pub(crate) fn emit_cpp_runtime_shell_header(contract: &ContractIr) -> String {
    let graph = contract
        .graphs
        .first()
        .expect("normalized contract must contain at least one graph");
    let order = topo_order_instances_for_language(contract, graph, LanguageKind::Cpp);
    let process_plans = process_runtime_plans(&order);
    let bind_plans = bind_runtime_plans(contract, graph);
    let bridge_plans = bridge_runtime_plans(contract, graph);

    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str(
        "#include <cstddef>\n#include <memory>\n#include <optional>\n#include <string_view>\n\n",
    );
    output.push_str("#include <flowrt/runtime.hpp>\n#include <flowrt/inproc_service.hpp>\n\n");
    output.push_str(
        "#include \"flowrt_app/components.hpp\"\n#include \"flowrt_app/messages.hpp\"\n\n",
    );
    output.push_str("namespace flowrt_app {\n\n");

    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    let operation_plans = crate::runtime_plan::operation_runtime_plans(contract, graph);
    output.push_str(
        "/**\n * @brief Contract IR 驱动的 C++ inproc 应用 shell。\n *\n * `App` 持有用户组件实现和 FlowRT 管理的 channel 状态。用户代码通过 `flowrt_user::build_app()` 构造该对象，runtime shell 负责生命周期、调度和数据流转发。\n */\n",
    );
    output.push_str("class App {\npublic:\n");
    output.push_str(&emit_cpp_app_constructor_declaration(contract, &order));
    output.push_str(
        "    /**\n     * @brief 使用指定 backend 运行完整 C++ 应用图。\n     *\n     * @param backend 提供调度器和 capability 的 FlowRT backend。\n     * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。\n     * @return 应用执行状态。\n     */\n    flowrt::Status run(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks);\n\n    /**\n     * @brief 运行指定 RSDL process group。\n     *\n     * @param backend 提供调度器和 capability 的 FlowRT backend。\n     * @param process Contract IR 中声明的 process group 名称。\n     * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。\n     * @return 应用执行状态。\n     */\n    flowrt::Status run_process(const flowrt::Backend& backend, std::string_view process, std::optional<std::size_t> run_ticks);\n\nprivate:\n    flowrt::Status step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n    flowrt::Status step_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n    flowrt::Status step_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n",
    );
    for task in scheduler_tasks_for_order(graph, &order) {
        output.push_str(&format!(
            "    flowrt::Status {}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n",
            cpp_task_step_function_name(task)
        ));
    }
    for process in &process_plans {
        output.push_str(&format!(
            "    flowrt::Status step_process_{}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n",
            process.method_suffix
        ));
        output.push_str(&format!(
            "    flowrt::Status step_process_{}_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n",
            process.method_suffix
        ));
        output.push_str(&format!(
            "    flowrt::Status step_process_{}_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n",
            process.method_suffix
        ));
        for task in scheduler_tasks_for_order(graph, &process.instances) {
            output.push_str(&format!(
                "    flowrt::Status {}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n",
                cpp_process_task_step_function_name(process, task)
            ));
        }
    }
    for process in &process_plans {
        output.push_str(&format!(
            "    flowrt::Status run_process_{}(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks);\n",
            process.method_suffix
        ));
    }
    output.push('\n');
    for instance in &order {
        let component = component_by_name(contract, &instance.component.name);
        output.push_str(&format!(
            "    std::unique_ptr<{}Interface> {}_;\n",
            component_cpp_name(component),
            instance.name
        ));
        if !component.params.is_empty() {
            output.push_str(&format!(
                "    {}Params {}_params_;\n",
                component_cpp_name(component),
                instance.name
            ));
        }
    }
    for bind in &bind_plans {
        output.push_str(&format!(
            "    {} {}_;\n",
            cpp_runtime_channel_type(bind),
            bind.field_name
        ));
        output.push_str(&format!(
            "    flowrt::IntrospectionChannelProbe {};\n",
            bind.probe_field_name
        ));
    }
    for bridge in &bridge_plans {
        output.push_str(&format!(
            "    {} {}_;\n",
            cpp_bridge_runtime_channel_type(bridge),
            bridge.field_name
        ));
    }
    // service client/server fields
    for plan in &service_plans {
        let client_field = cpp_service_client_field_name(plan);
        let handle_name = cpp_service_client_handle_name(plan);
        output.push_str(&format!("    {handle_name} {client_field}_;\n"));

        let server_field = cpp_service_server_field_name(plan);
        let req_ty = cpp_type(&plan.request_type);
        let resp_ty = cpp_type(&plan.response_type);
        let is_zenoh = plan.backend.0 == "zenoh";
        if !is_zenoh {
            output.push_str(&format!(
                "    std::optional<flowrt::InprocServiceServer<{req_ty}, {resp_ty}>> {server_field}_;\n"
            ));
        }
    }
    for plan in &operation_plans {
        let client_field = cpp_operation_client_field_name(plan);
        let handle_name = cpp_operation_client_handle_name(plan);
        output.push_str(&format!("    {handle_name} {client_field}_;\n"));
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let goal_ty = cpp_type(&plan.goal_type);
        output.push_str(&format!(
            "    std::optional<flowrt::InprocServiceServer<{goal_ty}, flowrt::OperationStartAck>> {}_;\n",
            cpp_operation_start_server_field_name(plan)
        ));
        output.push_str(&format!(
            "    std::optional<flowrt::InprocServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>> {}_;\n",
            cpp_operation_cancel_server_field_name(plan)
        ));
        output.push_str(&format!(
            "    std::optional<flowrt::InprocServiceServer<flowrt::OperationId, flowrt::OperationStatusSnapshot>> {}_;\n",
            cpp_operation_status_server_field_name(plan)
        ));
    }
    // service step function declarations
    for plan in &service_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let fn_name = cpp_service_step_fn_name(plan);
        output.push_str(&format!(
            "    flowrt::Status {fn_name}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n"
        ));
    }
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let fn_name = cpp_operation_step_fn_name(plan);
        output.push_str(&format!(
            "    flowrt::Status {fn_name}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n"
        ));
    }
    output.push_str("};\n\n");
    output.push_str(
        "/**\n * @brief 运行默认 C++ inproc 应用。\n *\n * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。\n * @return runtime shell 执行状态。\n */\nflowrt::Status run(std::optional<std::size_t> run_ticks);\n\n",
    );
    output.push_str(
        "/**\n * @brief 运行默认 C++ inproc 应用中的指定 process group。\n *\n * @param process process group 名称。\n * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。\n * @return runtime shell 执行状态。\n */\nflowrt::Status run_process(std::string_view process, std::optional<std::size_t> run_ticks);\n\n",
    );
    output.push_str("}  // namespace flowrt_app\n");
    output.push_str(
        "\nnamespace flowrt_user {\n\n/**\n * @brief 构造用户 C++ 组件实例并交给 FlowRT 管理 shell。\n *\n * 用户项目必须实现该函数。函数体应只装配用户组件对象，不写入 FlowRT 管理产物。\n *\n * @return 已注入用户组件实例的 FlowRT C++ 应用对象。\n */\nflowrt_app::App build_app();\n\n}  // namespace flowrt_user\n",
    );
    output
}

fn emit_cpp_app_constructor_declaration(contract: &ContractIr, order: &[&InstanceIr]) -> String {
    let mut params = Vec::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        params.push(format!(
            "std::unique_ptr<{}Interface> {}",
            component_cpp_name(component),
            instance.name
        ));
    }

    let mut output = String::new();
    output.push_str("    /**\n     * @brief 构造 C++ 应用 shell。\n     *\n");
    if params.is_empty() {
        output.push_str("     * 该 contract 没有需要注入的 C++ 组件实例。\n");
    } else {
        for instance in order {
            output.push_str(&format!(
                "     * @param {} 用户组件实例所有权；shell 在生命周期内独占持有该对象。\n",
                instance.name
            ));
        }
    }
    output.push_str("     */\n");
    if params.is_empty() {
        output.push_str("    App();\n\n");
    } else {
        output.push_str("    explicit App(\n");
        for (index, param) in params.iter().enumerate() {
            let suffix = if index + 1 == params.len() { "" } else { "," };
            output.push_str(&format!("        {param}{suffix}\n"));
        }
        output.push_str("    );\n\n");
    }
    output
}

fn emit_cpp_app_constructor(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
) -> String {
    let mut params = Vec::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        params.push(format!(
            "std::unique_ptr<{}Interface> {}",
            component_cpp_name(component),
            instance.name
        ));
    }

    let mut initializers = Vec::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        initializers.push(format!("{}_(std::move({}))", instance.name, instance.name));
        if !component.params.is_empty() {
            initializers.push(format!(
                "{}_params_({})",
                instance.name,
                cpp_params_initializer(component, instance)
            ));
        }
    }
    for bind in binds {
        initializers.push(format!(
            "{}_({})",
            bind.field_name,
            cpp_runtime_channel_initializer(contract, graph, bind)
        ));
    }
    for bridge in bridges {
        initializers.push(format!(
            "{}_({})",
            bridge.field_name,
            cpp_bridge_runtime_channel_initializer(contract, graph, bridge)
        ));
    }

    let mut output = String::new();
    if params.is_empty() {
        output.push_str("App::App()");
    } else {
        output.push_str("App::App(\n");
        for (index, param) in params.iter().enumerate() {
            let suffix = if index + 1 == params.len() { "" } else { "," };
            output.push_str(&format!("    {param}{suffix}\n"));
        }
        output.push(')');
    }
    if !initializers.is_empty() {
        output.push_str("\n    : ");
        for (index, initializer) in initializers.iter().enumerate() {
            if index == 0 {
                output.push_str(initializer);
            } else {
                output.push_str(&format!(",\n      {initializer}"));
            }
        }
    }
    output.push_str(" {\n");

    // service registration
    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    for plan in &service_plans {
        let is_zenoh = plan.backend.0 == "zenoh";
        if is_zenoh {
            continue;
        }
        let service_name_literal = cpp_string_literal(&plan.service_name);
        let client_field = cpp_service_client_field_name(plan);
        let server_field = cpp_service_server_field_name(plan);
        let req_ty = cpp_type(&plan.request_type);
        let resp_ty = cpp_type(&plan.response_type);
        let queue_depth = plan.queue_depth.max(1);
        let max_in_flight = plan.max_in_flight.max(1);
        let default_timeout = plan.timeout_ms.max(1);

        output.push_str(&format!(
            "    {{\n        flowrt::InprocServiceConfig config;\n        config.queue_depth = {queue_depth};\n        config.max_in_flight = {max_in_flight};\n        config.default_timeout_ms = {default_timeout};\n        {server_field}_.emplace(\n            {service_name_literal},\n            [this](const {req_ty}& request) -> flowrt::ServiceResult<{resp_ty}> {{\n                if (!this->{server_instance}_) {{\n                    return flowrt::ServiceResult<{resp_ty}>::err(flowrt::ServiceError::Unavailable);\n                }}\n                return this->{server_instance}_->on_{port}_request(request);\n            }},\n            config);\n        {client_field}_ = {cpp_handle_name}(\n            flowrt::InprocServiceClient<{req_ty}, {resp_ty}>(\n                {service_name_literal}, *{server_field}_));\n    }}\n",
            port = crate::snake_identifier(&plan.server_port),
            cpp_handle_name = cpp_service_client_handle_name(plan),
            server_instance = plan.server_instance,
        ));
    }
    let operation_plans = crate::runtime_plan::operation_runtime_plans(contract, graph);
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
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
        let concurrency = cpp_operation_concurrency(plan.concurrency);
        let preempt = cpp_operation_preempt(plan.preempt);
        let operation_index = plan.index;

        output.push_str(&format!(
            "    {{\n        (void){feedback_name};\n        (void){result_name};\n        auto operation_state_{operation_index} = std::make_shared<flowrt::OperationStatusSnapshot>(flowrt::OperationStatusSnapshot{{\n            .id = flowrt::OperationId{{.operation_key = flowrt::fnv1a64({operation_key_name}), .client_id = 0, .sequence = 0}},\n            .state = flowrt::OperationState::Accepted,\n            .cancel_requested = false,\n            .health = flowrt::OperationHealthSnapshot{{}},\n        }});\n        auto operation_sequence_{operation_index} = std::make_shared<std::uint64_t>(0);\n        flowrt::InprocServiceConfig config;\n        config.queue_depth = {queue_depth};\n        config.max_in_flight = {max_in_flight};\n        config.default_timeout_ms = {timeout_ms};\n        {start_server}_.emplace(\n            {start_name},\n            [this, operation_state_{operation_index}, operation_sequence_{operation_index}](const {goal_ty}& goal) -> flowrt::ServiceResult<flowrt::OperationStartAck> {{\n                if (!this->{server_instance}_) {{\n                    return flowrt::ServiceResult<flowrt::OperationStartAck>::err(flowrt::ServiceError::Unavailable);\n                }}\n                const auto sequence = (*operation_sequence_{operation_index})++;\n                const flowrt::OperationId id{{\n                    .operation_key = flowrt::fnv1a64({operation_key_name}),\n                    .client_id = 0,\n                    .sequence = sequence,\n                }};\n                const auto policy = flowrt::OperationPolicy::make(\n                    std::chrono::milliseconds{{{timeout_ms}}},\n                    {concurrency},\n                    {preempt},\n                    {queue_depth}U,\n                    {max_in_flight}U);\n                if (!policy.has_value()) {{\n                    return flowrt::ServiceResult<flowrt::OperationStartAck>::err(flowrt::ServiceError::HandlerError);\n                }}\n                flowrt::OperationLifecycle lifecycle{{id, *policy}};\n                if (lifecycle.transition(flowrt::OperationState::Running) != flowrt::OperationError::Ok) {{\n                    return flowrt::ServiceResult<flowrt::OperationStartAck>::err(flowrt::ServiceError::HandlerError);\n                }}\n                auto progress = flowrt::OperationProgressPublisher<{feedback_ty}>{{id}};\n                const auto result = this->{server_instance}_->on_{port}_operation(goal, lifecycle.cancel_token(), progress);\n                flowrt::OperationState terminal_state = flowrt::OperationState::Failed;\n                switch (result.kind()) {{\n                    case flowrt::OperationHandlerResult<{result_ty}>::Kind::Succeeded:\n                        terminal_state = flowrt::OperationState::Succeeded;\n                        break;\n                    case flowrt::OperationHandlerResult<{result_ty}>::Kind::Failed:\n                        terminal_state = flowrt::OperationState::Failed;\n                        break;\n                    case flowrt::OperationHandlerResult<{result_ty}>::Kind::Canceled:\n                        terminal_state = flowrt::OperationState::Canceled;\n                        break;\n                }}\n                (void)lifecycle.transition(terminal_state);\n                *operation_state_{operation_index} = lifecycle.snapshot();\n                return flowrt::ServiceResult<flowrt::OperationStartAck>::ok(flowrt::OperationStartAck::accepted_ack(id));\n            }},\n            config);\n        {cancel_server}_.emplace(\n            {cancel_name},\n            [operation_state_{operation_index}](const flowrt::OperationId& id) -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{\n                if (operation_state_{operation_index}->id == id) {{\n                    operation_state_{operation_index}->cancel_requested = true;\n                    if (!flowrt::is_terminal(operation_state_{operation_index}->state)) {{\n                        operation_state_{operation_index}->state = flowrt::OperationState::Canceling;\n                    }}\n                }}\n                return flowrt::ServiceResult<flowrt::OperationStatusSnapshot>::ok(*operation_state_{operation_index});\n            }},\n            config);\n        {status_server}_.emplace(\n            {status_name},\n            [operation_state_{operation_index}](const flowrt::OperationId& /*id*/) -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{\n                return flowrt::ServiceResult<flowrt::OperationStatusSnapshot>::ok(*operation_state_{operation_index});\n            }},\n            config);\n        {client_field}_ = {handle_name}(\n            flowrt::InprocServiceClient<{goal_ty}, flowrt::OperationStartAck>({start_name}, *{start_server}_),\n            flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>({cancel_name}, *{cancel_server}_),\n            flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>({status_name}, *{status_server}_));\n    }}\n",
            port = crate::snake_identifier(&plan.server_port),
            server_instance = plan.server_instance,
        ));
    }

    output.push_str("}\n\n");
    output
}

struct CppStepEmission<'a> {
    contract: &'a ContractIr,
    graph: &'a GraphIr,
    binds: &'a [BindRuntimePlan],
    bridges: &'a [BridgeRuntimePlan],
    incoming_bind_index: &'a BTreeMap<(String, String), usize>,
    outgoing_bind_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    outgoing_bridge_indices: &'a BTreeMap<(String, String), Vec<usize>>,
}

fn emit_cpp_app_step(
    emission: &CppStepEmission<'_>,
    order: &[&InstanceIr],
    function_name: &str,
    phase: TaskEmissionPhase,
    task_filter: Option<&flowrt_ir::TaskIr>,
) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "flowrt::Status App::{function_name}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {{\n",
    ));
    if cpp_runtime_step_uses_tick_time(emission.binds, emission.bridges) {
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
            let trigger_guard =
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
                            &cpp_runtime_channel_read(
                                input,
                                bind,
                                &input_local,
                                task.trigger == flowrt_ir::TriggerKind::OnMessage,
                            ),
                            true,
                        ));
                        // stale 健康计数在 error guard 之前记录，确保 Error policy 也能计数。
                        output.push_str(&indent_generated_block(
                            &cpp_runtime_stale_health_record(&input_local, &task_health),
                            true,
                        ));
                        output.push_str(&indent_generated_block(
                            &cpp_runtime_stale_error_guard(&input_local, bind),
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

            // 初始化 health_map 条目的 name 和 lane 字段。
            let lane_name = cpp_task_lane_name(task);
            let task_health = cpp_task_health_name(task);
            output.push_str(&format!(
                "        health_map[\"{task_health}\"].name = \"{task_health}\";\n        health_map[\"{task_health}\"].lane = \"{lane}\";\n",
                task_health = task_health,
                lane = lane_name,
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

            if task.deadline_ms.is_some() {
                let task_local = cpp_task_local_name(task);
                output.push_str(&format!(
                    "{body_indent}const auto {instance}_deadline_started_at = std::chrono::steady_clock::now();\n",
                    instance = task_local
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
            output.push_str(&format!(
                "{body_indent}if ({instance}_) {{\n{body_inner_indent}switch ({instance}_->on_tick({args})) {{\n{body_inner_indent}    case flowrt::Status::Ok:\n{body_inner_indent}        break;\n{body_inner_indent}    case flowrt::Status::Retry:\n{body_inner_indent}        return flowrt::Status::Retry;\n{body_inner_indent}    case flowrt::Status::Error:\n{body_inner_indent}        return flowrt::Status::Error;\n{body_inner_indent}}}\n{body_indent}}}\n",
                instance = instance.name,
                args = call_args.join(", ")
            ));

            if let Some(deadline_ms) = task.deadline_ms {
                let task_local = cpp_task_local_name(task);
                let task_health = cpp_task_health_name(task);
                output.push_str(&format!(
                    "{body_indent}const bool {instance}_deadline_exceeded = (std::chrono::steady_clock::now() - {instance}_deadline_started_at > std::chrono::milliseconds{{{deadline_ms}}});\n\
                     {body_indent}if ({instance}_deadline_exceeded) {{\n\
                     {body_inner_indent}health_map[\"{task_health}\"].deadline_missed += 1;\n\
                     {body_indent}}}\n",
                    instance = task_local,
                    task_health = task_health,
                ));
            }

            // 在 deadline_exceeded 守卫下发布输出：deadline miss 时不发布 late output。
            let has_deadline = task.deadline_ms.is_some();
            if has_deadline {
                let task_local = cpp_task_local_name(task);
                output.push_str(&format!(
                    "{body_indent}if (!{instance}_deadline_exceeded) {{\n",
                    instance = task_local
                ));
            }
            for port in &component.outputs {
                if !task_outputs.contains(port.name.as_str()) {
                    continue;
                }
                let output_local = cpp_step_local_name(&instance.name, &port.name);
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
                let publish_indent = if has_deadline {
                    format!("{body_indent}    ")
                } else {
                    body_indent.to_string()
                };
                output.push_str(&format!(
                    "{publish_indent}if (const auto* value = {local}.as_ref()) {{\n",
                    local = output_local
                ));
                for bind_index in outgoing {
                    let bind = &emission.binds[bind_index];
                    let task_health = cpp_task_health_name(task);
                    output.push_str(&indent_generated_block_levels(
                        &cpp_runtime_channel_write_with_health(bind, &task_health),
                        write_indent_levels + if has_deadline { 1 } else { 0 },
                    ));
                }
                for bridge_index in bridge_outgoing {
                    let bridge = &emission.bridges[bridge_index];
                    output.push_str(&indent_generated_block_levels(
                        &cpp_bridge_runtime_channel_write(bridge),
                        write_indent_levels + if has_deadline { 1 } else { 0 },
                    ));
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

    output.push_str("    return flowrt::Status::Ok;\n}\n\n");
    output
}

fn emit_cpp_app_run_process_dispatch(processes: &[ProcessRuntimePlan<'_>]) -> String {
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

struct CppRunEmission<'a> {
    contract: &'a ContractIr,
    function_name: &'a str,
    step_function_name: &'a str,
    startup_function_name: &'a str,
    shutdown_function_name: &'a str,
    order: &'a [&'a InstanceIr],
    binds: &'a [BindRuntimePlan],
    graph: &'a GraphIr,
    process: Option<&'a ProcessRuntimePlan<'a>>,
    package_name: &'a str,
    process_name: &'a str,
}

fn emit_cpp_app_run_function(run: &CppRunEmission<'_>) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "flowrt::Status App::{}(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {{\n    flowrt::Context lifecycle_context;\n    auto status = flowrt::Status::Ok;\n",
        run.function_name
    ));
    output.push_str("    (void)backend;\n");
    output.push_str("    auto shutdown = flowrt::install_signal_shutdown_token();\n");
    output.push_str("    flowrt::IntrospectionState introspection_state;\n");
    output.push_str("    flowrt::ScheduleWaiter scheduler_events;\n");
    output.push_str(&emit_cpp_scheduler_event_registration(run.binds));
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
    for instance in run.order {
        output.push_str(&format!(
            "    if (status == flowrt::Status::Ok && {name}_) {{\n        status = {name}_->on_init(lifecycle_context);\n        {name}_initialized = status == flowrt::Status::Ok;\n    }}\n",
            name = instance.name
        ));
    }
    for instance in run.order {
        output.push_str(&format!(
            "    if (status == flowrt::Status::Ok && {name}_initialized && {name}_) {{\n        status = {name}_->on_start(lifecycle_context);\n        {name}_started = status == flowrt::Status::Ok;\n    }}\n",
            name = instance.name
        ));
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
        output.push_str(&format!(
            "    if ({name}_started && {name}_) {{\n        const auto stop_status = {name}_->on_stop(lifecycle_context);\n        if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok) {{\n            status = flowrt::Status::Error;\n        }}\n    }}\n",
            name = instance.name
        ));
    }
    for instance in run.order.iter().rev() {
        output.push_str(&format!(
            "    if ({name}_initialized && {name}_) {{\n        const auto shutdown_status = {name}_->on_shutdown(lifecycle_context);\n        if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok) {{\n            status = flowrt::Status::Error;\n        }}\n    }}\n",
            name = instance.name
        ));
    }
    output.push_str("    return status;\n}\n\n");
    output
}

fn emit_cpp_scheduler_v2_loop(run: &CppRunEmission<'_>) -> String {
    let tasks = scheduler_tasks_for_order(run.graph, run.order);
    let mut output = String::new();
    output.push_str(&format!(
        "    flowrt::DeterministicExecutor scheduler{{{}}};\n",
        selected_profile_worker_threads(run.contract)
    ));

    let lane_ids = cpp_scheduler_lane_ids(&tasks);
    for (lane, lane_id) in &lane_ids {
        output.push_str(&format!(
            "    scheduler.add_lane(flowrt::LaneId{{{lane_id}}}, flowrt::LaneKind::Serial);\n    (void){};\n",
            cpp_string_literal(lane)
        ));
    }
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let lane_id = lane_ids[&cpp_task_lane_name(task)];
        let priority = task.priority.unwrap_or(0);
        output.push_str(&format!(
            "    scheduler.add_task(flowrt::TaskSpec{{.id = flowrt::TaskId{{{task_id}}}, .lane = flowrt::LaneId{{{lane_id}}}, .priority = {priority}}});\n"
        ));
        if task.trigger == flowrt_ir::TriggerKind::Periodic {
            output.push_str(&format!(
                "    scheduler.add_periodic(flowrt::PeriodicSpec{{.task = flowrt::TaskId{{{task_id}}}, .period = std::chrono::milliseconds{{{}}}}});\n    scheduler.wake(flowrt::TaskId{{{task_id}}});\n",
                task.period_ms.unwrap_or(1)
            ));
        }
    }
    // service task registration
    let service_plans = crate::runtime_plan::service_runtime_plans(run.contract, run.graph);
    let operation_plans = crate::runtime_plan::operation_runtime_plans(run.contract, run.graph);
    let mut next_task_id = tasks.len();
    let mut next_extra_lane_id = lane_ids.len() + 1;
    for plan in &service_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        next_task_id += 1;
        let _server_lane = crate::runtime_plan::service_server_lane(plan);
        let lane_id = next_extra_lane_id;
        next_extra_lane_id += 1;
        output.push_str(&format!(
            "    scheduler.add_lane(flowrt::LaneId{{{lane_id}}}, flowrt::LaneKind::Serial);\n    scheduler.add_task(flowrt::TaskSpec{{.id = flowrt::TaskId{{{next_task_id}}}, .lane = flowrt::LaneId{{{lane_id}}}, .priority = 0}});\n"
        ));
    }
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        next_task_id += 1;
        let lane_id = next_extra_lane_id;
        next_extra_lane_id += 1;
        output.push_str(&format!(
            "    scheduler.add_lane(flowrt::LaneId{{{lane_id}}}, flowrt::LaneKind::Serial);\n    scheduler.add_task(flowrt::TaskSpec{{.id = flowrt::TaskId{{{next_task_id}}}, .lane = flowrt::LaneId{{{lane_id}}}, .priority = 0}});\n"
        ));
    }
    output.push_str(&emit_cpp_on_message_revision_state(&tasks, run.binds));
    output.push_str(&format!(
        "    const auto scheduler_base_period_ms = std::uint64_t{{{}}};\n",
        cpp_scheduler_base_period_ms(&tasks)
    ));
    let task_health_init = emit_cpp_task_health_init(&tasks);
    output.push_str(
        "    std::size_t tick_base = 0;\n    std::uint64_t scheduler_now_ms = 0;\n    std::map<std::string, flowrt::IntrospectionTaskHealth> health_map;\n    constexpr std::uint64_t fairness_starvation_threshold = 10;\n    while (status == flowrt::Status::Ok && !shutdown.is_requested() && (!run_ticks.has_value() || tick_base < *run_ticks)) {\n        std::uint64_t observed_data_generation = scheduler_events.data_generation();\n        const auto tick_time_ms = scheduler_now_ms;\n        scheduler.advance_to(std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(tick_time_ms)});\n        scheduler.set_current_tick(static_cast<std::uint64_t>(tick_base));\n",
    );
    output.push_str(&task_health_init);
    output.push_str(&emit_cpp_apply_pending_params_for_order(
        run.contract,
        run.order,
    ));
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
        "        introspection_state.record_tick();\n        while (true) {{\n            observed_data_generation = scheduler_events.data_generation();\n            {woke_on_message_decl}\n"
    ));
    output.push_str(&indent_generated_block_levels(
        &emit_cpp_on_message_wake_checks(&tasks, run.binds),
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
        output.push_str(&format!(
            "            if (({start_server}_.has_value() && {start_server}_->pending_count() > 0) || ({cancel_server}_.has_value() && {cancel_server}_->pending_count() > 0) || ({status_server}_.has_value() && {status_server}_->pending_count() > 0)) {{\n                scheduler.wake(flowrt::TaskId{{{service_task_id}}});\n                woke_on_message = true;\n            }}\n"
        ));
    }
    output.push_str(
        "            const auto task_statuses = scheduler.run_ready([this, &lifecycle_context, &introspection_state, &scheduler_events, &health_map, tick_time_ms](flowrt::TaskId task) {\n                switch (task.value) {\n",
    );
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let function_name = match run.process {
            Some(process) => cpp_process_task_step_function_name(process, task),
            None => cpp_task_step_function_name(task),
        };
        output.push_str(&format!(
            "                    case {task_id}: return {function_name}(static_cast<std::size_t>(tick_time_ms), lifecycle_context, introspection_state, scheduler_events, health_map);\n"
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
        output.push_str(&format!(
            "                    case {service_task_id}: return {fn_name}(static_cast<std::size_t>(tick_time_ms), lifecycle_context, introspection_state, scheduler_events, health_map);\n"
        ));
    }
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        service_task_id += 1;
        let fn_name = cpp_operation_step_fn_name(plan);
        output.push_str(&format!(
            "                    case {service_task_id}: return {fn_name}(static_cast<std::size_t>(tick_time_ms), lifecycle_context, introspection_state, scheduler_events, health_map);\n"
        ));
    }
    if tasks.is_empty()
        && service_plans.iter().all(|p| p.backend.0 == "zenoh")
        && operation_plans.iter().all(|p| p.backend.0 == "zenoh")
    {
        output.push_str(&format!(
            "                    default: return {}(static_cast<std::size_t>(tick_time_ms), lifecycle_context, introspection_state, scheduler_events, health_map);\n",
            run.step_function_name
        ));
    } else {
        output.push_str("                    default: return flowrt::Status::Error;\n");
    }
    let fairness_check = emit_cpp_fairness_check(&lane_ids);
    output.push_str(&format!(
        "                }}\n            }});\n            if (!woke_on_message && task_statuses.empty()) {{\n                break;\n            }}\n            for (const auto task_status : task_statuses) {{\n                if (task_status == flowrt::Status::Error) {{\n                    status = flowrt::Status::Error;\n                    break;\n                }}\n            }}\n            if (status != flowrt::Status::Ok) {{\n                break;\n            }}\n        }}\n        // 公平性检测：检查 lane 饥饿。\n{fairness_check}        // 将本轮健康快照写入 introspection。\n        for (auto &[name, health] : health_map) {{\n            introspection_state.record_task_health(std::move(health));\n        }}\n        health_map.clear();\n        if (status == flowrt::Status::Ok) {{\n            ++tick_base;\n            if (run_ticks.has_value()) {{\n                scheduler_now_ms += scheduler_base_period_ms;\n                continue;\n            }}\n            const auto next_periodic_deadline_ms = {next_deadline_expr};\n            const auto next_wake_deadline = next_periodic_deadline_ms.has_value()\n                ? std::optional<std::chrono::steady_clock::time_point>{{\n                      std::chrono::steady_clock::now() +\n                      std::chrono::milliseconds{{static_cast<std::chrono::milliseconds::rep>(\n                          next_periodic_deadline_ms->value > scheduler_now_ms\n                              ? next_periodic_deadline_ms->value - scheduler_now_ms\n                              : 0U)}}}}\n                : std::nullopt;\n            switch (scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, shutdown)) {{\n                case flowrt::ScheduleEvent::Shutdown:\n                    status = flowrt::Status::Ok;\n                    break;\n                case flowrt::ScheduleEvent::Timer:\n                    scheduler_now_ms = next_periodic_deadline_ms.has_value()\n                                           ? next_periodic_deadline_ms->value\n                                           : scheduler_now_ms + scheduler_base_period_ms;\n                    break;\n                case flowrt::ScheduleEvent::Data:\n                    break;\n            }}\n            if (shutdown.is_requested()) {{\n                break;\n            }}\n        }}\n    }}\n",
            next_deadline_expr = cpp_next_periodic_deadline_expr(&tasks)
        )
        .replace(
            "next_periodic_deadline_ms->value",
            "static_cast<std::uint64_t>(next_periodic_deadline_ms->count())",
        ),
    );
    output
}

fn emit_cpp_scheduler_event_registration(binds: &[BindRuntimePlan]) -> String {
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
    output
}

fn cpp_task_lane_name(task: &flowrt_ir::TaskIr) -> String {
    task.lane
        .clone()
        .unwrap_or_else(|| format!("{}_serial", task.instance.name))
}

fn cpp_task_health_name(task: &flowrt_ir::TaskIr) -> String {
    format!("{}.{}", task.instance.name, task.name)
}

fn cpp_task_local_name(task: &flowrt_ir::TaskIr) -> String {
    format!(
        "{}_{}",
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

fn cpp_scheduler_base_period_ms(tasks: &[&flowrt_ir::TaskIr]) -> u64 {
    tasks
        .iter()
        .filter(|task| task.trigger == flowrt_ir::TriggerKind::Periodic)
        .filter_map(|task| task.period_ms)
        .min()
        .unwrap_or(1)
}

/// 为本轮 scheduler 预注册 task health 条目，确保未运行 task 也能记录公平性计数。
fn emit_cpp_task_health_init(tasks: &[&flowrt_ir::TaskIr]) -> String {
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

/// 生成 C++ lane 饥饿检测代码。
fn emit_cpp_fairness_check(lane_ids: &std::collections::BTreeMap<String, usize>) -> String {
    let mut output = String::new();
    for (lane, lane_id) in lane_ids {
        output.push_str(&format!(
            "        if (scheduler.lane_starvation_ticks(flowrt::LaneId{{{lane_id}}}) > fairness_starvation_threshold) {{\n            for (auto &[name, health] : health_map) {{\n                if (health.lane == \"{lane}\") {{\n                    health.fairness_violations += 1;\n                }}\n            }}\n        }}\n"
        ));
    }
    output
}

fn cpp_next_periodic_deadline_expr(tasks: &[&flowrt_ir::TaskIr]) -> String {
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

fn emit_cpp_apply_pending_params_for_order(contract: &ContractIr, order: &[&InstanceIr]) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if !component.params.is_empty() {
            output.push_str(&cpp_apply_pending_params(
                instance,
                component,
                false,
                "lifecycle_context",
            ));
        }
    }
    output
}

fn cpp_scheduler_lane_ids(tasks: &[&flowrt_ir::TaskIr]) -> BTreeMap<String, usize> {
    let mut lanes = BTreeMap::new();
    for task in tasks {
        let lane = cpp_task_lane_name(task);
        if !lanes.contains_key(&lane) {
            let next_id = lanes.len() + 1;
            lanes.insert(lane, next_id);
        }
    }
    lanes
}

fn cpp_task_step_function_name(task: &flowrt_ir::TaskIr) -> String {
    format!(
        "step_task_{}_{}",
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

fn cpp_process_task_step_function_name(
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

fn cpp_task_seen_revision_name(bind: &BindRuntimePlan, task: &flowrt_ir::TaskIr) -> String {
    format!(
        "{}_seen_revision_for_{}_{}",
        bind.field_name,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

fn cpp_input_binds_for_task<'a>(
    task: &flowrt_ir::TaskIr,
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

fn emit_cpp_on_message_revision_state(
    tasks: &[&flowrt_ir::TaskIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    for task in tasks
        .iter()
        .copied()
        .filter(|task| task.trigger == flowrt_ir::TriggerKind::OnMessage)
    {
        for bind in cpp_input_binds_for_task(task, binds) {
            output.push_str(&format!(
                "    std::uint64_t {seen} = 0;\n",
                seen = cpp_task_seen_revision_name(bind, task)
            ));
        }
    }
    output
}

fn emit_cpp_on_message_wake_checks(
    tasks: &[&flowrt_ir::TaskIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    for (index, task) in tasks.iter().enumerate() {
        if task.trigger != flowrt_ir::TriggerKind::OnMessage {
            continue;
        }
        let input_binds = cpp_input_binds_for_task(task, binds);
        if input_binds.is_empty() {
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
        let checks = input_binds
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
        output.push_str(&format!(
            "            scheduler.wake(flowrt::TaskId{{{}}});\n            woke_on_message = true;\n        }}\n",
            index + 1
        ));
    }
    output
}

pub(crate) fn cpp_string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn cpp_runtime_channel_type(bind: &BindRuntimePlan) -> String {
    let ty = cpp_type(&bind.source_type);
    if bind_backend(bind) == "iox2" {
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

fn cpp_bridge_runtime_channel_type(bridge: &BridgeRuntimePlan) -> String {
    format!(
        "flowrt::zenoh::ZenohPubSub<{}>",
        cpp_type(&bridge.source_type)
    )
}

fn cpp_runtime_channel_initializer(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
) -> String {
    if bind_backend(bind) == "iox2" {
        return format!(
            "flowrt::iox2::Iox2PubSub<{}>::open_with_config({}, {})",
            cpp_type(&bind.source_type),
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

fn cpp_bridge_runtime_channel_initializer(
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

fn cpp_zenoh_channel_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::zenoh::ZenohChannelConfig::latest().with_stale_config({})",
            cpp_runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => format!(
            "flowrt::zenoh::ZenohChannelConfig::fifo({}, {}).with_stale_config({})",
            bind.depth.unwrap_or(1),
            cpp_runtime_overflow_policy(bind.overflow),
            cpp_runtime_stale_config_expr(bind)
        ),
    }
}

fn cpp_iox2_channel_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config({})",
            cpp_runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => format!(
            "flowrt::iox2::Iox2ChannelConfig::fifo({}, {}).with_stale_config({})",
            bind.depth.unwrap_or(1),
            cpp_runtime_overflow_policy(bind.overflow),
            cpp_runtime_stale_config_expr(bind)
        ),
    }
}

fn cpp_runtime_latest_channel_initializer(bind: &BindRuntimePlan) -> String {
    let ty = cpp_type(&bind.source_type);
    if bind.max_age_ms.is_none() && bind.stale == IrStalePolicy::Warn {
        return String::new();
    }

    format!(
        "flowrt::LatestChannel<{ty}>::with_stale_config({})",
        cpp_runtime_stale_config_expr(bind)
    )
}

fn cpp_runtime_fifo_channel_initializer(bind: &BindRuntimePlan) -> String {
    let depth = bind.depth.unwrap_or(1);
    let overflow = cpp_runtime_overflow_policy(bind.overflow);
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

fn cpp_runtime_stale_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.max_age_ms {
        Some(max_age_ms) => format!(
            "flowrt::StaleConfig{{std::chrono::milliseconds{{{max_age_ms}}}, {}}}",
            cpp_runtime_stale_policy(bind.stale)
        ),
        None => format!(
            "flowrt::StaleConfig{{{}}}",
            cpp_runtime_stale_policy(bind.stale)
        ),
    }
}

fn cpp_runtime_channel_read(
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

fn cpp_runtime_stale_error_guard(local_name: &str, bind: &BindRuntimePlan) -> String {
    if bind.stale != IrStalePolicy::Error {
        return String::new();
    }

    format!(
        "    if ({local}.stale()) {{\n        return flowrt::Status::Error;\n    }}\n",
        local = local_name
    )
}

/// 生成 stale input 健康计数器记录代码（C++）。
fn cpp_runtime_stale_health_record(local_name: &str, task_health_name: &str) -> String {
    format!(
        "    if ({local}.stale()) {{\n        health_map[\"{task_health}\"].stale_input += 1;\n    }}\n",
        local = local_name,
        task_health = task_health_name,
    )
}

fn cpp_step_local_name(instance: &str, port: &str) -> String {
    format!("{instance}_{port}")
}

fn cpp_introspection_publish_record(bind: &BindRuntimePlan) -> String {
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
fn cpp_runtime_channel_write_with_health(bind: &BindRuntimePlan, task_health_name: &str) -> String {
    cpp_runtime_channel_write_inner(bind, Some(task_health_name))
}

fn cpp_runtime_channel_write_inner(
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

fn cpp_bridge_runtime_channel_write(bridge: &BridgeRuntimePlan) -> String {
    format!(
        "        if (const auto status = status_from_push_result({field}_.publish_at(*value, tick_time_ms)); status != flowrt::Status::Ok) {{\n            return status;\n        }}\n",
        field = bridge.field_name
    )
}

fn cpp_runtime_step_uses_tick_time(
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

fn cpp_backend_factory(selected_backend: &str) -> &'static str {
    match selected_backend {
        "inproc" => "flowrt::inproc_backend()",
        "iox2" => "flowrt::iox2_backend()",
        "zenoh" => "flowrt::zenoh_backend()",
        _ => unreachable!("validated contract selected backend must be known"),
    }
}

fn cpp_runtime_overflow_policy(policy: IrOverflowPolicy) -> &'static str {
    match policy {
        IrOverflowPolicy::DropOldest => "flowrt::OverflowPolicy::DropOldest",
        IrOverflowPolicy::DropNewest => "flowrt::OverflowPolicy::DropNewest",
        IrOverflowPolicy::Error => "flowrt::OverflowPolicy::Error",
        IrOverflowPolicy::Block => "flowrt::OverflowPolicy::Block",
    }
}

fn cpp_runtime_stale_policy(policy: IrStalePolicy) -> &'static str {
    match policy {
        IrStalePolicy::Warn => "flowrt::StalePolicy::Warn",
        IrStalePolicy::Drop => "flowrt::StalePolicy::Drop",
        IrStalePolicy::HoldLast => "flowrt::StalePolicy::HoldLast",
        IrStalePolicy::Error => "flowrt::StalePolicy::Error",
    }
}

fn cpp_params_struct(component: &ComponentIr) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "struct {}Params {{\n",
        component_cpp_name(component)
    ));
    for param in &component.params {
        output.push_str(&format!(
            "    {} {}{{}};\n",
            cpp_param_type(param.ty),
            param.name
        ));
    }
    output.push_str("};\n\n");
    output
}

fn cpp_params_initializer(component: &ComponentIr, instance: &InstanceIr) -> String {
    let mut output = format!("{}Params{{", component_cpp_name(component));
    for param in &component.params {
        let value = param_value_for_instance(instance, param);
        output.push_str(&format!(
            "\n        .{} = {},",
            param.name,
            cpp_param_literal(param, value)
        ));
    }
    output.push_str("\n    }");
    output
}

fn cpp_params_update_signature(component: &ComponentIr) -> String {
    if component.params.is_empty() {
        return String::new();
    }
    let params_ty = format!("{}Params", component_cpp_name(component));
    format!(
        "    /**\n     * @brief 参数 pending 值在 tick 边界通过校验后调用。\n     *\n     * @param old_params 当前已生效参数快照。\n     * @param new_params 即将生效的新参数快照。\n     * @param context runtime 上下文。\n     * @return 返回 `Ok` 后 shell 才会提交新参数。\n     */\n    virtual flowrt::Status on_params_update(\n        const {params_ty}& old_params,\n        const {params_ty}& new_params,\n        flowrt::Context& context) {{\n        (void)old_params;\n        (void)new_params;\n        (void)context;\n        return flowrt::ok();\n    }}\n"
    )
}

fn emit_cpp_introspection_param_registration(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        for param in &component.params {
            output.push_str(&format!(
                "    introspection_state.register_param(flowrt::IntrospectionParamSchema{{\n        .name = {},\n        .ty = {},\n        .update = {},\n        .current = {},\n        .min = {},\n        .max = {},\n        .choices = {},\n    }});\n",
                cpp_string_literal(&runtime_param_name(instance, param)),
                cpp_string_literal(param_type_name(param.ty)),
                cpp_string_literal(param_update_name(param.update)),
                cpp_string_literal(&param_json_literal(param_value_for_instance(instance, param))),
                cpp_optional_json_literal(param.min.as_ref()),
                cpp_optional_json_literal(param.max.as_ref()),
                cpp_json_fragment_vector_literal(&param.choices),
            ));
        }
    }
    output
}

fn cpp_apply_pending_params(
    instance: &InstanceIr,
    component: &ComponentIr,
    nested: bool,
    context_name: &str,
) -> String {
    let mut output = String::new();
    let indent = step_indent(nested);
    let inner_indent = nested_step_indent(nested);
    for param in &component.params {
        if param.update != ParamUpdatePolicy::OnTick {
            continue;
        }
        let runtime_name = runtime_param_name(instance, param);
        let pending = format!("{}_{}_pending", instance.name, param.name);
        let next = format!("{}_{}_next_params", instance.name, param.name);
        output.push_str(&format!(
            "{indent}if (const auto {pending} = introspection_state.take_pending_param({runtime_name}); {pending}.has_value()) {{\n",
            runtime_name = cpp_string_literal(&runtime_name)
        ));
        output.push_str(&format!(
            "{inner_indent}auto {next} = {instance}_params_;\n",
            instance = instance.name
        ));
        output.push_str(&format!(
            "{inner_indent}if (!decode_flowrt_param_value(*{pending}, {next}.{field})) {{\n{deep_indent}return flowrt::Status::Error;\n{inner_indent}}}\n",
            field = param.name,
            deep_indent = if nested { "                " } else { "            " }
        ));
        output.push_str(&format!(
            "{inner_indent}if ({instance}_ && {instance}_->on_params_update({instance}_params_, {next}, {context_name}) != flowrt::Status::Ok) {{\n{deep_indent}return flowrt::Status::Error;\n{inner_indent}}}\n",
            instance = instance.name,
            deep_indent = if nested { "                " } else { "            " }
        ));
        output.push_str(&format!(
            "{inner_indent}{instance}_params_ = std::move({next});\n{inner_indent}introspection_state.record_param_applied({runtime_name}, *{pending});\n",
            instance = instance.name,
            runtime_name = cpp_string_literal(&runtime_name)
        ));
        output.push_str(&format!("{indent}}}\n"));
    }
    output
}

fn cpp_optional_json_literal(value: Option<&ParamValue>) -> String {
    match value {
        Some(value) => format!(
            "std::optional<std::string>{{{}}}",
            cpp_string_literal(&param_json_literal(value))
        ),
        None => "std::nullopt".to_string(),
    }
}

fn cpp_json_fragment_vector_literal(values: &[ParamValue]) -> String {
    if values.is_empty() {
        return "{}".to_string();
    }
    let values = values
        .iter()
        .map(|value| cpp_string_literal(&param_json_literal(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("std::vector<std::string>{{{values}}}")
}

pub(crate) fn emit_cpp_main() -> String {
    let mut output = managed_header();
    output.push_str("#include \"flowrt_app/runtime_shell.hpp\"\n\n");
    output.push_str("#include <charconv>\n#include <cstddef>\n#include <optional>\n#include <string_view>\n#include <system_error>\n\n");
    output.push_str(
        "int main(int argc, char** argv) {\n    std::string_view process;\n    std::optional<std::size_t> run_ticks;\n    for (int index = 1; index < argc; ++index) {\n        const std::string_view arg(argv[index]);\n        if (arg == \"--process\") {\n            if (index + 1 >= argc) {\n                return 2;\n            }\n            process = argv[++index];\n        } else if (arg == \"--flowrt-run-ticks\" || arg == \"--flowrt-run-steps\") {\n            if (index + 1 >= argc) {\n                return 2;\n            }\n            const std::string_view raw(argv[++index]);\n            std::size_t ticks = 0;\n            const auto result = std::from_chars(raw.data(), raw.data() + raw.size(), ticks);\n            if (result.ec != std::errc{} || result.ptr != raw.data() + raw.size() || ticks == 0) {\n                return 2;\n            }\n            run_ticks = ticks;\n        } else {\n            return 2;\n        }\n    }\n\n    const auto status = process.empty() ? flowrt_app::run(run_ticks) : flowrt_app::run_process(process, run_ticks);\n    return status == flowrt::Status::Ok ? 0 : 1;\n}\n",
    );
    output
}

fn emit_cpp_introspection_helpers() -> String {
    r#"flowrt::IntrospectionChannelProbe register_introspection_channel(
    flowrt::IntrospectionState& state,
    std::string_view name,
    std::string_view message_type,
    std::optional<std::size_t> max_payload_len
) {
    try {
        state.register_channel_with_probe_capacity(
            std::string{name},
            std::string{message_type},
            max_payload_len);
        if (const auto probe = state.channel_probe(name); probe.has_value()) {
            return *probe;
        }
    } catch (...) {
    }
    return flowrt::IntrospectionChannelProbe{};
}

template <typename T>
void record_introspection_publish_copy(
    flowrt::IntrospectionState& state,
    std::string_view name,
    std::string_view message_type,
    const flowrt::IntrospectionChannelProbe& probe,
    const T& value,
    std::uint64_t published_at_ms
) {
    probe.record_publish_event();
    if (!probe.enabled() && !state.recorder_enabled_for_channel(name)) {
        return;
    }
    try {
        const auto payload = std::span<const std::uint8_t>{
            reinterpret_cast<const std::uint8_t*>(&value), sizeof(T)};
        state.try_record_channel_sample_bytes(
            name,
            message_type,
            payload,
            std::optional<std::uint64_t>{published_at_ms});
        if (probe.enabled()) {
            probe.try_record_bytes(payload, std::optional<std::uint64_t>{published_at_ms});
        }
    } catch (...) {
    }
}

template <typename T>
void record_introspection_publish_frame(
    flowrt::IntrospectionState& state,
    std::string_view name,
    std::string_view message_type,
    const flowrt::IntrospectionChannelProbe& probe,
    const T& value,
    std::uint64_t published_at_ms
) {
    probe.record_publish_event();
    if (!probe.enabled() && !state.recorder_enabled_for_channel(name)) {
        return;
    }
    try {
        std::vector<std::uint8_t> payload(flowrt::detail::encoded_frame_size(value));
        flowrt::detail::encode_frame(value, payload);
        state.try_record_channel_sample_bytes(
            name,
            message_type,
            payload,
            std::optional<std::uint64_t>{published_at_ms});
        if (probe.enabled()) {
            probe.try_record_bytes(payload, std::optional<std::uint64_t>{published_at_ms});
        }
    } catch (...) {
    }
}

inline bool decode_json_string_fragment(std::string_view value, std::string& output) {
    if (value.size() < 2 || value.front() != '"' || value.back() != '"') {
        return false;
    }
    output.clear();
    for (std::size_t index = 1; index + 1 < value.size(); ++index) {
        const char byte = value[index];
        if (byte != '\\') {
            output.push_back(byte);
            continue;
        }
        if (index + 1 >= value.size() - 1) {
            return false;
        }
        const char escape = value[++index];
        switch (escape) {
            case '"':
            case '\\':
            case '/':
                output.push_back(escape);
                break;
            case 'b':
                output.push_back('\b');
                break;
            case 'f':
                output.push_back('\f');
                break;
            case 'n':
                output.push_back('\n');
                break;
            case 'r':
                output.push_back('\r');
                break;
            case 't':
                output.push_back('\t');
                break;
            default:
                return false;
        }
    }
    return true;
}

inline bool decode_flowrt_param_value(std::string_view value, bool& output) {
    if (value == "true") {
        output = true;
        return true;
    }
    if (value == "false") {
        output = false;
        return true;
    }
    return false;
}

template <typename T>
bool decode_flowrt_param_value(std::string_view value, T& output)
    requires(std::is_integral_v<T> && !std::is_same_v<T, bool>)
{
    std::string owned{value};
    char* end = nullptr;
    errno = 0;
    if constexpr (std::is_signed_v<T>) {
        const long long parsed = std::strtoll(owned.c_str(), &end, 10);
        if (errno != 0 || end == owned.c_str() || *end != '\0') {
            return false;
        }
        if (parsed < static_cast<long long>(std::numeric_limits<T>::min()) ||
            parsed > static_cast<long long>(std::numeric_limits<T>::max())) {
            return false;
        }
        output = static_cast<T>(parsed);
    } else {
        if (!owned.empty() && owned.front() == '-') {
            return false;
        }
        const unsigned long long parsed = std::strtoull(owned.c_str(), &end, 10);
        if (errno != 0 || end == owned.c_str() || *end != '\0') {
            return false;
        }
        if (parsed > static_cast<unsigned long long>(std::numeric_limits<T>::max())) {
            return false;
        }
        output = static_cast<T>(parsed);
    }
    return true;
}

inline bool decode_flowrt_param_value(std::string_view value, float& output) {
    std::string owned{value};
    char* end = nullptr;
    errno = 0;
    const float parsed = std::strtof(owned.c_str(), &end);
    if (errno != 0 || end == owned.c_str() || *end != '\0') {
        return false;
    }
    output = parsed;
    return true;
}

inline bool decode_flowrt_param_value(std::string_view value, double& output) {
    std::string owned{value};
    char* end = nullptr;
    errno = 0;
    const double parsed = std::strtod(owned.c_str(), &end);
    if (errno != 0 || end == owned.c_str() || *end != '\0') {
        return false;
    }
    output = parsed;
    return true;
}

inline bool decode_flowrt_param_value(std::string_view value, std::string& output) {
    return decode_json_string_fragment(value, output);
}

"#
    .to_string()
}

fn emit_cpp_introspection_channel_registration(
    contract: &ContractIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    for bind in active_binds_for_instances(binds, order) {
        output.push_str(&format!(
            "    this->{probe} = register_introspection_channel(introspection_state, {}, {}, {});\n",
            cpp_string_literal(&runtime_channel_name(bind)),
            cpp_string_literal(&runtime_channel_message_type(bind)),
            cpp_optional_size_t_literal(runtime_channel_probe_capacity(contract, bind)),
            probe = bind.probe_field_name
        ));
    }
    output
}

fn cpp_optional_size_t_literal(value: Option<usize>) -> String {
    value.map_or_else(
        || "std::nullopt".to_string(),
        |value| format!("std::optional<std::size_t>{{{value}}}"),
    )
}

fn cpp_operation_client_handle_classes(
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
        if plan.backend.0 == "zenoh" {
            output.push_str(&format!(
                "/**\n * @brief `{client}.{port}` Operation client（zenoh backend，未实现）。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() = default;\n\n    flowrt::OperationClientResult<flowrt::OperationStartAck> start(const {goal_ty}& /*goal*/, std::uint64_t /*timeout_ms*/ = {default_timeout_ms}) {{\n        return flowrt::OperationClientResult<flowrt::OperationStartAck>::err(flowrt::OperationClientError::Backend);\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> cancel(flowrt::OperationId /*id*/, std::uint64_t /*timeout_ms*/ = {default_timeout_ms}) {{\n        return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Backend);\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> status(flowrt::OperationId /*id*/, std::uint64_t /*timeout_ms*/ = {default_timeout_ms}) {{\n        return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Backend);\n    }}\n}};\n\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
        } else {
            output.push_str(&format!(
                "/**\n * @brief `{client}.{port}` Operation client typed wrapper。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() = default;\n\n    {handle_name}(\n        flowrt::InprocServiceClient<{goal_ty}, flowrt::OperationStartAck> start_client,\n        flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot> cancel_client,\n        flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot> status_client)\n        : start_client_(std::move(start_client)), cancel_client_(std::move(cancel_client)), status_client_(std::move(status_client)) {{}}\n\n    flowrt::OperationClientResult<flowrt::OperationStartAck> start(const {goal_ty}& goal, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!start_client_.has_value()) {{\n            return flowrt::OperationClientResult<flowrt::OperationStartAck>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        return flowrt::operation_client_result_from_service(start_client_->call(goal, timeout_ms));\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> cancel(flowrt::OperationId id, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!cancel_client_.has_value()) {{\n            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        return flowrt::operation_client_result_from_service(cancel_client_->call(id, timeout_ms));\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> status(flowrt::OperationId id, std::uint64_t timeout_ms = {default_timeout_ms}) {{\n        if (!status_client_.has_value()) {{\n            return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Unavailable);\n        }}\n        return flowrt::operation_client_result_from_service(status_client_->call(id, timeout_ms));\n    }}\n\nprivate:\n    std::optional<flowrt::InprocServiceClient<{goal_ty}, flowrt::OperationStartAck>> start_client_;\n    std::optional<flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>> cancel_client_;\n    std::optional<flowrt::InprocServiceClient<flowrt::OperationId, flowrt::OperationStatusSnapshot>> status_client_;\n}};\n\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
        }
    }

    output
}

fn cpp_operation_client_handle_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "OperationClient_{}_{}",
        crate::snake_identifier(&plan.client_component),
        crate::snake_identifier(&plan.client_port)
    )
}

fn cpp_operation_client_field_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "operation_client_{}_{}",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn cpp_operation_start_server_field_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "operation_start_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

fn cpp_operation_cancel_server_field_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "operation_cancel_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

fn cpp_operation_status_server_field_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "operation_status_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

fn cpp_operation_step_fn_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "step_operation_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

fn cpp_operation_handler_methods(
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

fn cpp_operation_start_endpoint_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_start",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn cpp_operation_cancel_endpoint_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_cancel",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn cpp_operation_status_endpoint_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_status",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn cpp_operation_feedback_endpoint_name(
    plan: &crate::runtime_plan::OperationRuntimePlan,
) -> String {
    format!(
        "__flowrt_operation_{}_{}_feedback",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn cpp_operation_result_endpoint_name(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
    format!(
        "__flowrt_operation_{}_{}_result",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn cpp_operation_concurrency(policy: OperationConcurrencyPolicy) -> &'static str {
    match policy {
        OperationConcurrencyPolicy::Reject => "flowrt::OperationConcurrencyPolicy::Reject",
        OperationConcurrencyPolicy::Queue => "flowrt::OperationConcurrencyPolicy::Queue",
    }
}

fn cpp_operation_preempt(policy: OperationPreemptPolicy) -> &'static str {
    match policy {
        OperationPreemptPolicy::Reject => "flowrt::OperationPreemptPolicy::Reject",
        OperationPreemptPolicy::CancelRunning => "flowrt::OperationPreemptPolicy::CancelRunning",
    }
}

fn cpp_service_client_handle_classes(plans: &[crate::runtime_plan::ServiceRuntimePlan]) -> String {
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
        let default_timeout_ms = plan.timeout_ms.max(1);

        if is_zenoh {
            output.push_str(&format!(
                "/**\n * @brief `{client}.{port}` service client（zenoh backend，未实现）。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() = default;\n\n    flowrt::ServiceResult<{resp_ty}> call(const {req_ty}& /*request*/, std::uint64_t /*timeout_ms*/ = {default_timeout_ms}) {{\n        return flowrt::ServiceResult<{resp_ty}>::err(flowrt::ServiceError::Backend);\n    }}\n\n    flowrt::InprocServiceHandle<{resp_ty}> start_call(const {req_ty}& /*request*/, std::uint64_t /*timeout_ms*/ = {default_timeout_ms}) {{\n        return flowrt::InprocServiceHandle<{resp_ty}>::ready_error(flowrt::ServiceError::Backend);\n    }}\n}};\n\n",
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

fn cpp_service_client_handle_name(plan: &crate::runtime_plan::ServiceRuntimePlan) -> String {
    format!(
        "ServiceClient_{}_{}",
        crate::snake_identifier(&plan.client_component),
        crate::snake_identifier(&plan.client_port)
    )
}

fn cpp_service_client_field_name(plan: &crate::runtime_plan::ServiceRuntimePlan) -> String {
    format!(
        "service_client_{}_{}",
        crate::snake_identifier(&plan.client_instance),
        crate::snake_identifier(&plan.client_port)
    )
}

fn cpp_service_server_field_name(plan: &crate::runtime_plan::ServiceRuntimePlan) -> String {
    format!(
        "service_server_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

fn cpp_service_step_fn_name(plan: &crate::runtime_plan::ServiceRuntimePlan) -> String {
    format!(
        "step_service_{}_{}",
        crate::snake_identifier(&plan.server_instance),
        crate::snake_identifier(&plan.server_port)
    )
}

fn cpp_callback_args(
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

fn cpp_service_handler_methods(
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
             * @brief 处理 `{port}` service request。\n\
             *\n\
             * runtime shell 在 hidden service task 中调用该方法。用户业务逻辑\n\
             * 实现具体的 request -> response 转换。\n\
             *\n\
             * @param request 请求消息引用。\n\
             * @return 成功返回 `ServiceResult::ok(response)`，业务错误返回\n\
             *         `ServiceResult::err(error_code, message)`。\n\
             */\n",
            port = port_name,
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

fn cpp_component_interface_doc(component: &ComponentIr) -> String {
    format!(
        "/**\n * @brief `{}` 组件的 C++ 用户实现接口。\n *\n * 用户代码实现该接口并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。\n */\n",
        component.name
    )
}

fn cpp_lifecycle_method(name: &str) -> String {
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

fn cpp_tick_signature(
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

fn cpp_tick_doc(
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
