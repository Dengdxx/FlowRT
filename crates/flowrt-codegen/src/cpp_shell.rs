use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    ChannelKind, ComponentIr, ComponentKind, ContractIr, GraphIr, InstanceIr, IoBoundaryReadiness,
    LanguageKind, OperationConcurrencyPolicy, OperationPreemptPolicy,
    OverflowPolicy as IrOverflowPolicy, ParamIr, ParamType, ParamUpdatePolicy, ParamValue, PortIr,
    Ros2BridgeDirection, StalePolicy as IrStalePolicy,
};

use crate::messages::cpp_type;
use crate::resource_names::{
    resource_access_name, resource_failure_name, resource_health_name, resource_readiness_name,
    resource_satisfaction_status,
};
use crate::runtime_plan::{
    BindRuntimePlan, BoundaryRuntimePlan, BridgeRuntimePlan, ProcessRuntimePlan, TaskEmissionPhase,
    active_binds_for_instances, active_boundaries_for_instances, bind_backend, bind_runtime_plans,
    boundary_runtime_plans, bridge_runtime_plans, incoming_bind_index_map,
    incoming_boundary_index_map, incoming_bridge_index_map, indent_generated_block,
    indent_generated_block_levels, nested_step_indent, on_message_trigger_guard,
    outgoing_bind_indices_map, outgoing_boundary_indices_map, outgoing_bridge_indices_map,
    process_runtime_plans, resolved_task_lane_name, runtime_channel_message_type,
    runtime_channel_name, runtime_channel_probe_capacity, runtime_param_name, step_indent,
};
use crate::{
    component_by_name, component_rust_name, float_literal, iox2_service_name, managed_header,
    param_json_literal, param_type_name, param_update_name, param_value_for_instance,
    ros2_bridge_key_expr, scheduler_tasks_for_order, selected_backend_name,
    selected_profile_worker_threads, tasks_for_instance, topo_order_instances_for_languages,
    zenoh_key_expr,
};

pub(crate) fn component_cpp_name(component: &ComponentIr) -> String {
    component_rust_name(component)
}

fn component_uses_cpp_shell(component: &ComponentIr) -> bool {
    matches!(component.language, LanguageKind::C | LanguageKind::Cpp)
}

fn contract_has_c_components(contract: &ContractIr) -> bool {
    contract
        .components
        .iter()
        .any(|component| component.language == LanguageKind::C)
}

fn contract_has_cpp_components(contract: &ContractIr) -> bool {
    contract
        .components
        .iter()
        .any(|component| component.language == LanguageKind::Cpp)
}

fn topo_order_instances_for_cpp_shell<'a>(
    contract: &ContractIr,
    graph: &'a GraphIr,
) -> Vec<&'a InstanceIr> {
    topo_order_instances_for_languages(contract, graph, &[LanguageKind::C, LanguageKind::Cpp])
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
        .filter(|component| component_uses_cpp_shell(component))
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

pub(crate) fn emit_c_component_header(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str("#include <flowrt/abi.h>\n\n");
    output.push_str("#ifdef __cplusplus\nextern \"C\" {\n#endif\n\n");

    let graph = contract
        .graphs
        .first()
        .expect("normalized contract must contain at least one graph");
    for instance in topo_order_instances_for_cpp_shell(contract, graph) {
        let component = component_by_name(contract, &instance.component.name);
        if component.language != LanguageKind::C {
            continue;
        }
        output.push_str(&format!(
            "const flowrt_c_component_callback_table_t* {}(void);\n",
            c_callback_factory_symbol(instance)
        ));
    }

    output.push_str("\n#ifdef __cplusplus\n}\n#endif\n");
    output
}

pub(crate) fn emit_cpp_runtime_shell(contract: &ContractIr) -> String {
    let graph = contract
        .graphs
        .first()
        .expect("normalized contract must contain at least one graph");
    let order = topo_order_instances_for_cpp_shell(contract, graph);
    let process_plans = process_runtime_plans(&order);
    let bind_plans = bind_runtime_plans(contract, graph);
    let bridge_plans = bridge_runtime_plans(contract, graph);
    let boundary_plans = boundary_runtime_plans(graph);
    let incoming_bind_index = incoming_bind_index_map(&bind_plans);
    let incoming_bridge_index = incoming_bridge_index_map(&bridge_plans);
    let incoming_boundary_index = incoming_boundary_index_map(&boundary_plans);
    let outgoing_bind_indices = outgoing_bind_indices_map(&bind_plans);
    let outgoing_bridge_indices = outgoing_bridge_indices_map(&bridge_plans);
    let outgoing_boundary_indices = outgoing_boundary_indices_map(&boundary_plans);
    let selected_backend = selected_backend_name(contract);

    let mut output = managed_header();
    output.push_str("#include \"flowrt_app/runtime_shell.hpp\"\n\n");
    if contract_has_c_components(contract) {
        output.push_str("#include \"flowrt_app/c_components.h\"\n\n");
    }
    output.push_str("#include \"flowrt_app/selfdesc.hpp\"\n\n");
    output.push_str("#include <algorithm>\n#include <array>\n#include <atomic>\n#include <cerrno>\n#include <chrono>\n#include <cmath>\n#include <cstdint>\n#include <cstdlib>\n#include <cstring>\n");
    if contract_has_c_components(contract) {
        output.push_str("#include <iostream>\n");
    }
    output.push_str("#include <deque>\n#include <limits>\n#include <memory>\n#include <mutex>\n#include <optional>\n#include <span>\n#include <string>\n#include <string_view>\n#include <thread>\n#include <type_traits>\n#include <utility>\n#include <variant>\n#include <vector>\n\n");
    output.push_str("namespace {\n\n");
    output.push_str(
        "flowrt::Status status_from_push_result(const flowrt::ChannelPushResult& result) {\n    if (std::holds_alternative<flowrt::ChannelError>(result)) {\n        return flowrt::Status::Error;\n    }\n\n    switch (std::get<flowrt::ChannelWriteOutcome>(result)) {\n        case flowrt::ChannelWriteOutcome::Accepted:\n        case flowrt::ChannelWriteOutcome::DroppedOldest:\n        case flowrt::ChannelWriteOutcome::DroppedNewest:\n            return flowrt::Status::Ok;\n        case flowrt::ChannelWriteOutcome::Backpressured:\n            return flowrt::Status::Retry;\n    }\n\n    return flowrt::Status::Error;\n}\n\n",
    );
    output.push_str(
        "std::string flowrt_operation_id_string(flowrt::OperationId id) {\n    return std::to_string(id.operation_key) + \":\" + std::to_string(id.client_id) + \":\" + std::to_string(id.sequence);\n}\n\nflowrt::IntrospectionOperationStatus flowrt_operation_status_from_snapshot(std::string_view name, std::string_view owner, const flowrt::OperationStatusSnapshot& snapshot) {\n    const bool active = !flowrt::is_terminal(snapshot.state) && snapshot.state != flowrt::OperationState::Idle;\n    flowrt::IntrospectionOperationStatus status;\n    status.name = std::string{name};\n    status.ready = true;\n    status.running = active ? 1U : 0U;\n    status.queued = 0U;\n    if (active) {\n        status.current_operation_ids.push_back(flowrt_operation_id_string(snapshot.id));\n    }\n    status.total_started = snapshot.health.started;\n    status.succeeded_count = snapshot.health.succeeded;\n    status.failed_count = snapshot.health.failed;\n    status.canceled_count = snapshot.health.canceled;\n    status.timeout_count = snapshot.health.timeout;\n    status.preempted_count = snapshot.health.preempted;\n    status.current_state = std::string{flowrt::to_string(snapshot.state)};\n    status.current_owner = snapshot.owner.owner_key == 0U ? std::nullopt : std::optional<std::string>{std::string{owner}};\n    status.current_deadline_ms = active ? std::optional<std::uint64_t>{snapshot.deadline_ms} : std::nullopt;\n    status.last_event = \"flowrt.operation.state_changed\";\n    status.last_error = std::nullopt;\n    status.last_transition_ms = flowrt::monotonic_time_ms();\n    return status;\n}\n\n",
    );
    output.push_str(
        "template <typename T>\nflowrt::ServiceResult<T> flowrt_operation_control_error(flowrt::OperationControlError error) {\n    switch (error) {\n        case flowrt::OperationControlError::Busy:\n        case flowrt::OperationControlError::OwnerConflict:\n            return flowrt::ServiceResult<T>::err_with_message(flowrt::ServiceError::Busy, std::string{flowrt::to_string(error)});\n        case flowrt::OperationControlError::StaleInvocation:\n        case flowrt::OperationControlError::AlreadyTerminal:\n            return flowrt::ServiceResult<T>::err_with_message(flowrt::ServiceError::Rejected, std::string{flowrt::to_string(error)});\n        case flowrt::OperationControlError::InvalidTransition:\n        case flowrt::OperationControlError::InvalidPolicy:\n        case flowrt::OperationControlError::Ok:\n            return flowrt::ServiceResult<T>::err_with_message(flowrt::ServiceError::HandlerError, std::string{flowrt::to_string(error)});\n    }\n    return flowrt::ServiceResult<T>::err(flowrt::ServiceError::HandlerError);\n}\n\n",
    );
    if contract_has_c_components(contract) {
        output.push_str(&emit_c_adapter_helpers(contract, graph, &order));
    }
    output.push_str(&emit_cpp_introspection_helpers());
    output.push_str(&emit_cpp_param_constraint_helpers(&order, contract));
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
        boundaries: &boundary_plans,
        incoming_bind_index: &incoming_bind_index,
        incoming_bridge_index: &incoming_bridge_index,
        incoming_boundary_index: &incoming_boundary_index,
        outgoing_bind_indices: &outgoing_bind_indices,
        outgoing_bridge_indices: &outgoing_bridge_indices,
        outgoing_boundary_indices: &outgoing_boundary_indices,
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
        let operation_name = cpp_string_literal(&plan.operation_name);
        let owner_name =
            cpp_string_literal(&format!("{}.{}", plan.client_instance, plan.client_port));
        let operation_index = plan.index;
        output.push_str(&format!(
            "flowrt::Status App::{fn_name}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {{\n    (void)tick;\n    (void)tick_context;\n    (void)scheduler_events;\n    (void)health_map;\n    introspection_state.register_operation_cancel_handler({operation_name}, [this](std::string_view operation_id) -> std::variant<flowrt::IntrospectionOperationStatus, std::string> {{\n        const auto snapshot = this->operation_control_{operation_index}_->snapshot();\n        if (flowrt_operation_id_string(snapshot.id) != operation_id) {{\n            return std::string{{\"stale operation invocation `\"}} + std::string{{operation_id}} + \"`; current is `\" + flowrt_operation_id_string(snapshot.id) + \"`\";\n        }}\n        if (const auto error = this->operation_control_{operation_index}_->request_cancel(snapshot.id, snapshot.owner); error != flowrt::OperationControlError::Ok) {{\n            return std::string{{flowrt::to_string(error)}};\n        }}\n        return flowrt_operation_status_from_snapshot({operation_name}, {owner_name}, this->operation_control_{operation_index}_->snapshot());\n    }});\n    if ({start_server}_.has_value()) {{\n        {start_server}_->process_pending();\n    }}\n    if ({cancel_server}_.has_value()) {{\n        {cancel_server}_->process_pending();\n    }}\n    if ({status_server}_.has_value()) {{\n        {status_server}_->process_pending();\n    }}\n    if (this->operation_control_{operation_index}_) {{\n        (void)this->operation_control_{operation_index}_->check_deadline(flowrt::monotonic_time_ms());\n        const auto snapshot = this->operation_control_{operation_index}_->snapshot();\n        const auto events = this->operation_control_{operation_index}_->drain_events();\n        for (const auto& event : events) {{\n            const auto operation_id = flowrt_operation_id_string(event.id);\n            switch (event.kind) {{\n                case flowrt::OperationRuntimeEventKind::StateChanged:\n                    if (event.state.has_value()) {{\n                        introspection_state.record_operation_transition(\n                            {operation_name},\n                            operation_id,\n                            flowrt::to_string(*event.state),\n                            std::optional<std::string_view>{{{owner_name}}},\n                            flowrt::is_terminal(*event.state) ? std::nullopt : std::optional<std::uint64_t>{{snapshot.deadline_ms}});\n                    }}\n                    break;\n                case flowrt::OperationRuntimeEventKind::Progress:\n                    introspection_state.record_operation_progress({operation_name}, operation_id, event.sequence.value_or(0U));\n                    break;\n                case flowrt::OperationRuntimeEventKind::Result: {{\n                    const auto result = event.state.has_value() ? flowrt::to_string(*event.state) : std::string_view{{\"succeeded\"}};\n                    introspection_state.record_operation_result({operation_name}, operation_id, result, std::nullopt);\n                    break;\n                }}\n                case flowrt::OperationRuntimeEventKind::Error: {{\n                    const auto result = event.state.has_value() ? flowrt::to_string(*event.state) : std::string_view{{\"failed\"}};\n                    introspection_state.record_operation_result({operation_name}, operation_id, result, std::optional<std::string_view>{{\"handler error\"}});\n                    break;\n                }}\n            }}\n        }}\n        introspection_state.record_operation_health(flowrt_operation_status_from_snapshot({operation_name}, {owner_name}, snapshot));\n    }}\n    return flowrt::Status::Ok;\n}}\n\n",
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
        bridges: &bridge_plans,
        boundaries: &boundary_plans,
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
            bridges: &bridge_plans,
            boundaries: &boundary_plans,
            graph,
            process: Some(process),
            package_name: &contract.package.name,
            process_name: &process.name,
        }));
    }
    let backend_factory = cpp_backend_factory(&selected_backend);
    let app_expr = if contract_has_cpp_components(contract) {
        "flowrt_user::build_app()"
    } else {
        "flowrt_app::App()"
    };
    output.push_str(
        &format!(
        "flowrt::Status run(std::optional<std::size_t> run_ticks) {{\n    auto backend = {backend_factory};\n    return {app_expr}.run(backend, run_ticks);\n}}\n\n"
    ));
    output.push_str(&format!(
        "flowrt::Status run_process(std::string_view process, std::optional<std::size_t> run_ticks) {{\n    auto backend = {backend_factory};\n    return {app_expr}.run_process(backend, process, run_ticks);\n}}\n\n"
    ));
    output.push_str("}  // namespace flowrt_app\n");
    output
}

pub(crate) fn emit_cpp_runtime_shell_header(contract: &ContractIr) -> String {
    let graph = contract
        .graphs
        .first()
        .expect("normalized contract must contain at least one graph");
    let order = topo_order_instances_for_cpp_shell(contract, graph);
    let process_plans = process_runtime_plans(&order);
    let bind_plans = bind_runtime_plans(contract, graph);
    let bridge_plans = bridge_runtime_plans(contract, graph);
    let boundary_plans = boundary_runtime_plans(graph);

    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str(
        "#include <cstddef>\n#include <functional>\n#include <map>\n#include <memory>\n#include <optional>\n#include <string_view>\n#include <vector>\n\n",
    );
    output.push_str("#include <flowrt/runtime.hpp>\n#include <flowrt/inproc_service.hpp>\n\n");
    output.push_str(
        "#include \"flowrt_app/components.hpp\"\n#include \"flowrt_app/messages.hpp\"\n\n",
    );
    output.push_str("namespace flowrt_app {\n\n");

    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    let operation_plans = crate::runtime_plan::operation_runtime_plans(contract, graph);
    output.push_str(
        "class App;\nusing FlowrtOutputCommit = std::function<flowrt::Status(App&, flowrt::IntrospectionState&, flowrt::ScheduleWaiter&, std::map<std::string, flowrt::IntrospectionTaskHealth>&)>;\nusing FlowrtTaskOutcome = flowrt::TaskRunOutcome<std::vector<FlowrtOutputCommit>>;\n\n",
    );
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
            "    FlowrtTaskOutcome {}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n",
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
                "    FlowrtTaskOutcome {}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);\n",
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
        let pointer_type = if cpp_instance_needs_operation_shared_ptr(instance, &operation_plans) {
            "std::shared_ptr"
        } else {
            "std::unique_ptr"
        };
        output.push_str(&format!(
            "    {pointer_type}<{}Interface> {}_;\n",
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
    for boundary in active_boundaries_for_instances(&boundary_plans, &order) {
        let ty = cpp_type(&boundary.ty);
        let field_ty = match boundary.direction {
            flowrt_ir::BoundaryDirection::Input => format!("flowrt::BoundaryInput<{ty}>"),
            flowrt_ir::BoundaryDirection::Output => format!("flowrt::BoundaryOutput<{ty}>"),
        };
        output.push_str(&format!("    {} {}_;\n", field_ty, boundary.field_name));
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
            "    std::optional<flowrt::InprocServiceServer<flowrt::OperationStartRequest<{goal_ty}>, flowrt::OperationStartAck>> {}_;\n",
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
        output.push_str(&format!(
            "    std::shared_ptr<flowrt::OperationControl> operation_control_{}_; \n",
            plan.index
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
    if contract_has_cpp_components(contract) {
        output.push_str(
            "\nnamespace flowrt_user {\n\n/**\n * @brief 构造用户 C++ 组件实例并交给 FlowRT 管理 shell。\n *\n * 用户项目必须实现该函数。函数体应只装配用户组件对象，不写入 FlowRT 管理产物。\n *\n * @return 已注入用户组件实例的 FlowRT C++ 应用对象。\n */\nflowrt_app::App build_app();\n\n}  // namespace flowrt_user\n",
        );
    }
    output
}

fn emit_cpp_app_constructor_declaration(contract: &ContractIr, order: &[&InstanceIr]) -> String {
    let mut params = Vec::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.language != LanguageKind::Cpp {
            continue;
        }
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
            let component = component_by_name(contract, &instance.component.name);
            if component.language != LanguageKind::Cpp {
                continue;
            }
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

fn cpp_instance_needs_operation_shared_ptr(
    instance: &InstanceIr,
    operation_plans: &[crate::runtime_plan::OperationRuntimePlan],
) -> bool {
    operation_plans
        .iter()
        .any(|plan| plan.backend.0 != "zenoh" && plan.server_instance == instance.name)
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
        if component.language != LanguageKind::Cpp {
            continue;
        }
        params.push(format!(
            "std::unique_ptr<{}Interface> {}",
            component_cpp_name(component),
            instance.name
        ));
    }

    let mut initializers = Vec::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.language == LanguageKind::C {
            initializers.push(format!(
                "{}_(make_c_{}_adapter())",
                instance.name,
                crate::snake_identifier(&instance.name)
            ));
        } else {
            initializers.push(format!("{}_(std::move({}))", instance.name, instance.name));
        }
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
        let server_lane = crate::runtime_plan::service_server_lane(plan);
        let server_lane_id = cpp_lane_id_u64_expr(&server_lane);

        output.push_str(&format!(
            "    {{\n        flowrt::InprocServiceConfig config;\n        config.queue_depth = {queue_depth};\n        config.max_in_flight = {max_in_flight};\n        config.default_timeout_ms = {default_timeout};\n        {server_field}_.emplace(\n            {service_name_literal},\n            [this](const {req_ty}& request) -> flowrt::ServiceResult<{resp_ty}> {{\n                if (!this->{server_instance}_) {{\n                    return flowrt::ServiceResult<{resp_ty}>::err(flowrt::ServiceError::Unavailable);\n                }}\n                return this->{server_instance}_->on_{port}_request(request);\n            }},\n            config);\n        {client_field}_ = {cpp_handle_name}(\n            flowrt::InprocServiceClient<{req_ty}, {resp_ty}>(\n                {service_name_literal}, *{server_field}_, 0, {server_lane_id}));\n    }}\n",
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
        output.push_str(&cpp_operation_registration_block(plan));
    }

    output.push_str("}\n\n");
    output
}

fn cpp_operation_registration_block(plan: &crate::runtime_plan::OperationRuntimePlan) -> String {
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
            {max_in_flight}U);
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
                const auto cancel = operation_control->cancel_token().value();
                if (const auto error = operation_control->mark_running(id);
                    error != flowrt::OperationControlError::Ok) {{
                    return flowrt_operation_control_error<flowrt::OperationStartAck>(error);
                }}
                auto goal_for_worker = request.goal;
                try {{
                    std::thread([operation_worker_server, operation_control, id, cancel, goal_for_worker = std::move(goal_for_worker)]() mutable {{
                        auto progress = flowrt::OperationProgressPublisher<{feedback_ty}>{{id, [operation_control](flowrt::OperationId progress_id, std::uint64_t sequence) {{
                            operation_control->publish_progress(progress_id, sequence);
                        }}}};
                        flowrt::OperationState terminal_state = flowrt::OperationState::Failed;
                        try {{
                            const auto result = operation_worker_server->on_{port}_operation(goal_for_worker, cancel, progress);
                            switch (result.kind()) {{
                                case flowrt::OperationHandlerResult<{result_ty}>::Kind::Succeeded:
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
                        }}
                        (void)operation_control->complete(id, terminal_state);
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
            [this](const flowrt::OperationId& /*id*/) -> flowrt::ServiceResult<flowrt::OperationStatusSnapshot> {{
                return flowrt::ServiceResult<flowrt::OperationStatusSnapshot>::ok(this->operation_control_{operation_index}_->snapshot());
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

fn emit_c_adapter_helpers(contract: &ContractIr, graph: &GraphIr, order: &[&InstanceIr]) -> String {
    let mut output = String::new();
    output.push_str(
        r#"using namespace flowrt_app;

flowrt_string_view_t c_string_view(std::string_view value) {
    return flowrt_string_view_t{.data = value.data(), .len = value.size()};
}

flowrt_bytes_view_t c_bytes_view(const void* data, std::size_t size) {
    return flowrt_bytes_view_t{.data = reinterpret_cast<const std::uint8_t*>(data), .len = size};
}

flowrt_c_clock_source_t c_clock_source(flowrt::ClockSource source) {
    switch (source) {
        case flowrt::ClockSource::Runtime:
            return FLOWRT_C_CLOCK_SOURCE_RUNTIME;
        case flowrt::ClockSource::Replay:
            return FLOWRT_C_CLOCK_SOURCE_REPLAY;
    }
    return FLOWRT_C_CLOCK_SOURCE_RUNTIME;
}

flowrt_c_task_timing_t make_c_task_timing(const flowrt::TaskTiming& timing) {
    return flowrt_c_task_timing_t{
        .step = timing.step,
        .task_name = c_string_view(timing.task_name),
        .trigger = c_string_view(timing.trigger),
        .clock_source = c_clock_source(timing.clock_source),
        .reserved0 = 0U,
        .scheduled_time_ms = timing.scheduled_time_ms,
        .observed_time_ms = timing.observed_time_ms,
        .scheduled_delta_ms = timing.scheduled_delta_ms,
        .observed_delta_ms = timing.observed_delta_ms,
        .period_ms = timing.period_ms.value_or(0U),
        .deadline_ms = timing.deadline_ms.value_or(0U),
        .lateness_ms = timing.lateness_ms,
        .missed_periods = timing.missed_periods,
        .has_period_ms = timing.period_ms.has_value() ? std::uint8_t{1} : std::uint8_t{0},
        .has_deadline_ms = timing.deadline_ms.has_value() ? std::uint8_t{1} : std::uint8_t{0},
        .deadline_missed = timing.deadline_missed ? std::uint8_t{1} : std::uint8_t{0},
        .overrun = timing.overrun ? std::uint8_t{1} : std::uint8_t{0},
        .reserved = {},
    };
}

flowrt::Status status_from_c(flowrt_status_t status) {
    switch (status) {
        case FLOWRT_STATUS_OK:
            return flowrt::Status::Ok;
        case FLOWRT_STATUS_RETRY:
            return flowrt::Status::Retry;
        case FLOWRT_STATUS_ERROR:
            return flowrt::Status::Error;
        default:
            return flowrt::Status::Error;
    }
}

const char* callback_table_validation_error(const flowrt_c_component_callback_table_t* callbacks,
                                            bool needs_periodic,
                                            bool needs_on_message,
                                            bool needs_startup,
                                            bool needs_shutdown) {
    if (callbacks == nullptr) {
        return "FlowRT C component callback table is null";
    }
    if (callbacks->size < sizeof(flowrt_c_component_callback_table_t)) {
        return "FlowRT C component callback table size is too small";
    }
    if (callbacks->version_major != FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR) {
        return "FlowRT C component callback table major version mismatch";
    }
    if (callbacks->version_minor != FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR) {
        return "FlowRT C component callback table minor version mismatch";
    }
    constexpr std::uint64_t kRequiredFeatures =
        FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 |
        FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1;
    constexpr std::uint64_t kKnownFeatures = kRequiredFeatures;
    if ((callbacks->feature_flags & FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0) !=
        FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0) {
        return "FlowRT C component callback table missing v0 feature bit";
    }
    if ((callbacks->feature_flags & FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1) !=
        FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1) {
        return "FlowRT C component callback table missing task timing feature bit";
    }
    if ((callbacks->feature_flags & ~kKnownFeatures) != 0U) {
        return "FlowRT C component callback table has unknown feature bit";
    }
    if (needs_periodic && callbacks->run_periodic == nullptr) {
        return "FlowRT C component callback table missing run_periodic";
    }
    if (needs_on_message && callbacks->run_on_message == nullptr) {
        return "FlowRT C component callback table missing run_on_message";
    }
    if (needs_startup && callbacks->run_startup == nullptr) {
        return "FlowRT C component callback table missing run_startup";
    }
    if (needs_shutdown && callbacks->run_shutdown == nullptr) {
        return "FlowRT C component callback table missing run_shutdown";
    }
    return nullptr;
}

flowrt_c_component_context_t make_c_component_context(std::string_view component_name,
                                                      std::string_view instance_name,
                                                      std::string_view task_name,
                                                      std::string_view lane_name,
                                                      const flowrt::Context& runtime_context,
                                                      std::uint64_t step,
                                                      std::uint64_t tick_time_ms,
                                                      std::uint64_t deadline_ms,
                                                      bool has_deadline_ms) {
    const auto* timing = runtime_context.timing();
    return flowrt_c_component_context_t{
        .component_name = c_string_view(component_name),
        .instance_name = c_string_view(instance_name),
        .task_name = c_string_view(task_name),
        .lane_name = c_string_view(lane_name),
        .step = timing != nullptr ? timing->step : step,
        .tick_time_ms = timing != nullptr ? timing->observed_time_ms : tick_time_ms,
        .deadline_ms =
            timing != nullptr ? timing->deadline_ms.value_or(0U) : deadline_ms,
        .has_deadline_ms =
            timing != nullptr ? (timing->deadline_ms.has_value() ? std::uint8_t{1} : std::uint8_t{0})
                              : (has_deadline_ms ? std::uint8_t{1} : std::uint8_t{0}),
        .has_timing = timing != nullptr ? std::uint8_t{1} : std::uint8_t{0},
        .reserved = {},
        .timing = timing != nullptr ? make_c_task_timing(*timing) : flowrt_c_task_timing_t{},
    };
}

"#,
    );

    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.language != LanguageKind::C {
            continue;
        }
        output.push_str(&emit_c_adapter_class(graph, instance, component));
    }

    output
}

fn emit_c_adapter_class(graph: &GraphIr, instance: &InstanceIr, component: &ComponentIr) -> String {
    let class_name = c_adapter_class_name(instance);
    let factory_name = c_adapter_factory_name(instance);
    let symbol = c_callback_factory_symbol(instance);
    let needs_periodic = c_bool_literal(
        tasks_for_instance(graph, instance)
            .any(|task| task.trigger == flowrt_ir::TriggerKind::Periodic),
    );
    let needs_on_message = c_bool_literal(
        tasks_for_instance(graph, instance)
            .any(|task| task.trigger == flowrt_ir::TriggerKind::OnMessage),
    );
    let needs_startup = c_bool_literal(
        tasks_for_instance(graph, instance)
            .any(|task| task.trigger == flowrt_ir::TriggerKind::Startup),
    );
    let needs_shutdown = c_bool_literal(
        tasks_for_instance(graph, instance)
            .any(|task| task.trigger == flowrt_ir::TriggerKind::Shutdown),
    );

    let mut output = String::new();
    output.push_str(&format!(
        "class {class_name} final : public flowrt_app::{component}Interface {{\npublic:\n    explicit {class_name}(const flowrt_c_component_callback_table_t* callbacks) : callbacks_(callbacks) {{}}\n\n",
        component = component_cpp_name(component),
    ));
    output.push_str(&emit_c_adapter_lifecycle_method(
        "on_init",
        &component.name,
        &instance.name,
    ));
    output.push_str(&emit_c_adapter_lifecycle_method(
        "on_start",
        &component.name,
        &instance.name,
    ));
    output.push_str(&emit_c_adapter_lifecycle_method(
        "on_stop",
        &component.name,
        &instance.name,
    ));
    output.push_str(&emit_c_adapter_lifecycle_method(
        "on_shutdown",
        &component.name,
        &instance.name,
    ));
    output.push_str(&emit_c_adapter_on_tick_override(component));
    for task in tasks_for_instance(graph, instance) {
        output.push_str(&emit_c_adapter_task_method(component, instance, task));
    }
    output.push_str(&format!(
        "private:\n    bool callbacks_valid(std::string_view instance_name) const {{\n        const char* error = callback_table_validation_error(callbacks_, {needs_periodic}, {needs_on_message}, {needs_startup}, {needs_shutdown});\n        if (error == nullptr) {{\n            return true;\n        }}\n        std::cerr << \"FlowRT C component callback table invalid for instance `\" << instance_name << \"`: \" << error << '\\n';\n        return false;\n    }}\n\n    flowrt::Status call_lifecycle(flowrt_c_lifecycle_callback_t callback, std::string_view hook_name, std::string_view component_name, std::string_view instance_name, const flowrt::Context& runtime_context) {{\n        if (!callbacks_valid(instance_name)) {{\n            return flowrt::Status::Error;\n        }}\n        if (callback == nullptr) {{\n            return flowrt::Status::Ok;\n        }}\n        const auto context = make_c_component_context(component_name, instance_name, hook_name, std::string_view{{}}, runtime_context, 0U, 0U, 0U, false);\n        return status_from_c(callback(callbacks_->user_data, &context));\n    }}\n\n    const flowrt_c_component_callback_table_t* callbacks_ = nullptr;\n}};\n\nstd::unique_ptr<flowrt_app::{component}Interface> {factory_name}() {{\n    return std::make_unique<{class_name}>({symbol}());\n}}\n\n",
        component = component_cpp_name(component),
    ));
    output
}

fn emit_c_adapter_lifecycle_method(
    name: &str,
    component_name: &str,
    instance_name: &str,
) -> String {
    format!(
        "    flowrt::Status {name}(flowrt::Context& context) override {{\n        return call_lifecycle(callbacks_ != nullptr ? callbacks_->{name} : nullptr, {hook}, {component}, {instance}, context);\n    }}\n\n",
        hook = cpp_string_literal(name),
        component = cpp_string_literal(component_name),
        instance = cpp_string_literal(instance_name),
    )
}

fn emit_c_adapter_on_tick_override(component: &ComponentIr) -> String {
    let args = cpp_callback_args(component, &[], &[]);
    if args.is_empty() {
        return "    flowrt::Status on_tick() override {\n        return flowrt::Status::Error;\n    }\n\n"
            .to_string();
    }
    let joined = args
        .iter()
        .map(|arg| format!("        {arg}"))
        .collect::<Vec<_>>()
        .join(",\n");
    format!(
        "    flowrt::Status on_tick(\n{joined}) override {{\n        return flowrt::Status::Error;\n    }}\n\n"
    )
}

fn emit_c_adapter_task_method(
    component: &ComponentIr,
    instance: &InstanceIr,
    task: &flowrt_ir::TaskIr,
) -> String {
    let mut args = vec![
        "std::uint64_t step".to_string(),
        "std::uint64_t tick_time_ms".to_string(),
        "flowrt::Context& tick_context".to_string(),
    ];
    for input in &component.inputs {
        args.push(format!("std::uint64_t {}_revision", input.name));
        args.push(format!(
            "const flowrt::Latest<{}>& {}",
            cpp_type(&input.ty),
            input.name
        ));
    }
    for output in &component.outputs {
        args.push(format!(
            "flowrt::Output<{}>& {}",
            cpp_type(&output.ty),
            output.name
        ));
    }
    let joined = args
        .iter()
        .map(|arg| format!("        {arg}"))
        .collect::<Vec<_>>()
        .join(",\n");
    let callback_field = c_task_callback_field(task);
    let mut output = format!(
        "    flowrt::Status {method}(\n{joined}) {{\n        if (!callbacks_valid({instance_name}) || callbacks_->{callback_field} == nullptr) {{\n            return flowrt::Status::Error;\n        }}\n        const auto context = make_c_component_context({component_name}, {instance_name}, {task_name}, {lane_name}, tick_context, step, tick_time_ms, {deadline}, {has_deadline});\n",
        method = c_adapter_task_method_name(task),
        component_name = cpp_string_literal(&component.name),
        instance_name = cpp_string_literal(&instance.name),
        task_name = cpp_string_literal(&task.name),
        lane_name = cpp_string_literal(&cpp_task_lane_name(task)),
        deadline = task.deadline_ms.unwrap_or(0),
        has_deadline = c_bool_literal(task.deadline_ms.is_some()),
    );

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

    for input in component
        .inputs
        .iter()
        .filter(|input| task_inputs.contains(input.name.as_str()))
    {
        let input_name = crate::snake_identifier(&input.name);
        output.push_str(&format!(
            "        const auto* {input_name}_payload = {port}.get();\n        flowrt_c_input_view_t {instance}_{input_name}_input{{\n            .name = c_string_view({port_name}),\n            .type_name = c_string_view({type_name}),\n            .schema_hash = flowrt::fnv1a64({type_name}),\n            .size_bytes = sizeof({ty}),\n            .payload = {input_name}_payload != nullptr ? c_bytes_view({input_name}_payload, sizeof({ty})) : flowrt_bytes_view_t{{}},\n            .source_time_ms = tick_time_ms,\n            .revision = {port}_revision,\n            .present = {port}.present() ? std::uint8_t{{1}} : std::uint8_t{{0}},\n            .stale = {port}.stale() ? std::uint8_t{{1}} : std::uint8_t{{0}},\n            .reserved = {{}},\n        }};\n",
            instance = crate::snake_identifier(&instance.name),
            port = input.name,
            port_name = cpp_string_literal(&input.name),
            type_name = cpp_string_literal(&input.ty.canonical_syntax()),
            ty = cpp_type(&input.ty),
        ));
    }

    for output_port in component
        .outputs
        .iter()
        .filter(|output_port| task_outputs.contains(output_port.name.as_str()))
    {
        let output_name = crate::snake_identifier(&output_port.name);
        output.push_str(&format!(
            "        std::array<std::uint8_t, sizeof({ty})> {instance}_{output_name}_storage{{}};\n        flowrt_c_output_slot_t {instance}_{output_name}_output{{\n            .name = c_string_view({port_name}),\n            .type_name = c_string_view({type_name}),\n            .schema_hash = flowrt::fnv1a64({type_name}),\n            .size_bytes = sizeof({ty}),\n            .data = {instance}_{output_name}_storage.data(),\n            .capacity = {instance}_{output_name}_storage.size(),\n            .written_len = 0U,\n            .status = FLOWRT_C_OUTPUT_UNWRITTEN,\n            .reserved = {{}},\n        }};\n",
            instance = crate::snake_identifier(&instance.name),
            port_name = cpp_string_literal(&output_port.name),
            type_name = cpp_string_literal(&output_port.ty.canonical_syntax()),
            ty = cpp_type(&output_port.ty),
        ));
    }

    output.push_str(&emit_c_input_array(instance, component, &task_inputs));
    output.push_str(&emit_c_output_array(instance, component, &task_outputs));
    output.push_str(&format!(
        "        const auto callback_status = status_from_c(callbacks_->{callback_field}(callbacks_->user_data, &context, &inputs, &outputs));\n        if (callback_status != flowrt::Status::Ok) {{\n            return callback_status;\n        }}\n"
    ));
    for (index, output_port) in component
        .outputs
        .iter()
        .filter(|output_port| task_outputs.contains(output_port.name.as_str()))
        .enumerate()
    {
        let output_name = crate::snake_identifier(&output_port.name);
        output.push_str(&format!(
            "        {instance}_{output_name}_output = output_slots[{index}];\n",
            instance = crate::snake_identifier(&instance.name),
        ));
    }
    for output_port in component
        .outputs
        .iter()
        .filter(|output_port| task_outputs.contains(output_port.name.as_str()))
    {
        let output_name = crate::snake_identifier(&output_port.name);
        output.push_str(&format!(
            "        if ({instance}_{output_name}_output.status == FLOWRT_C_OUTPUT_WRITTEN && {instance}_{output_name}_output.written_len == sizeof({ty})) {{\n            {ty} {instance}_{output_name}_value{{}};\n            std::memcpy(&{instance}_{output_name}_value, {instance}_{output_name}_storage.data(), sizeof({ty}));\n            {port}.write({instance}_{output_name}_value);\n        }} else if ({instance}_{output_name}_output.status == FLOWRT_C_OUTPUT_UNWRITTEN) {{\n        }} else {{\n            std::cerr << \"FlowRT C component output invalid for instance `{instance_raw}`, port `{port}`: status=\"\n                      << {instance}_{output_name}_output.status << \" written_len=\"\n                      << {instance}_{output_name}_output.written_len << \" expected=\" << sizeof({ty}) << '\\n';\n            return flowrt::Status::Error;\n        }}\n",
            instance = crate::snake_identifier(&instance.name),
            instance_raw = instance.name,
            port = output_port.name,
            ty = cpp_type(&output_port.ty),
        ));
    }
    output.push_str("        return flowrt::Status::Ok;\n    }\n\n");

    output
}

fn emit_c_input_array(
    instance: &InstanceIr,
    component: &ComponentIr,
    task_inputs: &BTreeSet<&str>,
) -> String {
    let inputs = component
        .inputs
        .iter()
        .filter(|input| task_inputs.contains(input.name.as_str()))
        .map(|input| {
            format!(
                "{}_{}_input",
                crate::snake_identifier(&instance.name),
                crate::snake_identifier(&input.name)
            )
        })
        .collect::<Vec<_>>();
    if inputs.is_empty() {
        return "        std::array<flowrt_c_input_view_t, 0> input_views{};\n        flowrt_c_input_array_view_t inputs{.data = nullptr, .len = 0U};\n"
            .to_string();
    }
    let body = inputs
        .iter()
        .map(|input| format!("            {input},"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "        std::array<flowrt_c_input_view_t, {len}> input_views{{{{\n{body}\n        }}}};\n        flowrt_c_input_array_view_t inputs{{.data = input_views.data(), .len = input_views.size()}};\n",
        len = inputs.len(),
    )
}

fn emit_c_output_array(
    instance: &InstanceIr,
    component: &ComponentIr,
    task_outputs: &BTreeSet<&str>,
) -> String {
    let outputs = component
        .outputs
        .iter()
        .filter(|output| task_outputs.contains(output.name.as_str()))
        .map(|output| {
            format!(
                "{}_{}_output",
                crate::snake_identifier(&instance.name),
                crate::snake_identifier(&output.name)
            )
        })
        .collect::<Vec<_>>();
    if outputs.is_empty() {
        return "        std::array<flowrt_c_output_slot_t, 0> output_slots{};\n        flowrt_c_output_array_view_t outputs{.data = nullptr, .len = 0U};\n"
            .to_string();
    }
    let body = outputs
        .iter()
        .map(|output| format!("            {output},"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "        std::array<flowrt_c_output_slot_t, {len}> output_slots{{{{\n{body}\n        }}}};\n        flowrt_c_output_array_view_t outputs{{.data = output_slots.data(), .len = output_slots.size()}};\n",
        len = outputs.len(),
    )
}

fn emit_c_adapter_task_step_call(
    emission: &CppStepEmission<'_>,
    instance: &InstanceIr,
    component: &ComponentIr,
    task: &flowrt_ir::TaskIr,
    collect_outputs: bool,
    body_indent: &str,
    body_inner_indent: &str,
) -> String {
    let mut args = vec![
        "static_cast<std::uint64_t>(tick)".to_string(),
        "tick_time_ms".to_string(),
        "tick_context".to_string(),
    ];
    for input in &component.inputs {
        args.push(c_input_revision_expr(emission, instance, input));
        args.push(cpp_step_local_name(&instance.name, &input.name));
    }
    for output in &component.outputs {
        args.push(cpp_step_local_name(&instance.name, &output.name));
    }
    let call = format!(
        "static_cast<{}*>({}_.get())->{}({})",
        c_adapter_class_name(instance),
        instance.name,
        c_adapter_task_method_name(task),
        args.join(", ")
    );

    if collect_outputs {
        format!(
            "{body_indent}if ({instance}_) {{\n{body_inner_indent}switch ({call}) {{\n{body_inner_indent}    case flowrt::Status::Ok:\n{body_inner_indent}        break;\n{body_inner_indent}    case flowrt::Status::Retry:\n{body_inner_indent}        return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{{}});\n{body_inner_indent}    case flowrt::Status::Error:\n{body_inner_indent}        return FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{{}});\n{body_inner_indent}}}\n{body_indent}}}\n",
            instance = instance.name,
        )
    } else {
        format!(
            "{body_indent}if ({instance}_) {{\n{body_inner_indent}switch ({call}) {{\n{body_inner_indent}    case flowrt::Status::Ok:\n{body_inner_indent}        break;\n{body_inner_indent}    case flowrt::Status::Retry:\n{body_inner_indent}        return flowrt::Status::Retry;\n{body_inner_indent}    case flowrt::Status::Error:\n{body_inner_indent}        return flowrt::Status::Error;\n{body_inner_indent}}}\n{body_indent}}}\n",
            instance = instance.name,
        )
    }
}

fn c_input_revision_expr(
    emission: &CppStepEmission<'_>,
    instance: &InstanceIr,
    input: &PortIr,
) -> String {
    if let Some(bind_index) = emission
        .incoming_bind_index
        .get(&(instance.name.clone(), input.name.clone()))
    {
        return format!("{}_.revision()", emission.binds[*bind_index].field_name);
    }
    if let Some(bridge_index) = emission
        .incoming_bridge_index
        .get(&(instance.name.clone(), input.name.clone()))
    {
        return format!("{}_.revision()", emission.bridges[*bridge_index].field_name);
    }
    if let Some(boundary_index) = emission
        .incoming_boundary_index
        .get(&(instance.name.clone(), input.name.clone()))
    {
        return format!(
            "{}_.revision()",
            emission.boundaries[*boundary_index].field_name
        );
    }
    "0U".to_string()
}

fn c_task_callback_field(task: &flowrt_ir::TaskIr) -> &'static str {
    match task.trigger {
        flowrt_ir::TriggerKind::Periodic => "run_periodic",
        flowrt_ir::TriggerKind::OnMessage => "run_on_message",
        flowrt_ir::TriggerKind::Startup => "run_startup",
        flowrt_ir::TriggerKind::Shutdown => "run_shutdown",
    }
}

fn c_adapter_class_name(instance: &InstanceIr) -> String {
    format!(
        "C{}Adapter",
        crate::pascal_case(&crate::snake_identifier(&instance.name))
    )
}

fn c_adapter_factory_name(instance: &InstanceIr) -> String {
    format!("make_c_{}_adapter", crate::snake_identifier(&instance.name))
}

fn c_adapter_task_method_name(task: &flowrt_ir::TaskIr) -> String {
    format!("run_{}", cpp_task_local_name(task))
}

fn c_callback_factory_symbol(instance: &InstanceIr) -> String {
    format!(
        "flowrt_app_{}_callbacks",
        crate::snake_identifier(&instance.name)
    )
}

fn c_bool_literal(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

struct CppStepEmission<'a> {
    contract: &'a ContractIr,
    graph: &'a GraphIr,
    binds: &'a [BindRuntimePlan],
    bridges: &'a [BridgeRuntimePlan],
    boundaries: &'a [BoundaryRuntimePlan],
    incoming_bind_index: &'a BTreeMap<(String, String), usize>,
    incoming_bridge_index: &'a BTreeMap<(String, String), usize>,
    incoming_boundary_index: &'a BTreeMap<(String, String), usize>,
    outgoing_bind_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    outgoing_bridge_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    outgoing_boundary_indices: &'a BTreeMap<(String, String), Vec<usize>>,
}

fn emit_cpp_app_step(
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
    output.push_str(&format!(
        "{return_type} App::{function_name}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map) {{\n",
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
    output.push_str(&adapt_cpp_status_returns_for_collect(
        &emit_cpp_ros2_boundary_input_pump(emission, order),
        collect_outputs,
    ));
    if collect_outputs {
        output.push_str("    std::vector<FlowrtOutputCommit> flowrt_output_commits;\n");
    }

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
                            &adapt_cpp_status_returns_for_collect(
                                &cpp_runtime_channel_read(
                                    input,
                                    bind,
                                    &input_local,
                                    task.trigger == flowrt_ir::TriggerKind::OnMessage,
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
                                    task.trigger == flowrt_ir::TriggerKind::OnMessage,
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
                let boundary_outgoing = emission
                    .outgoing_boundary_indices
                    .get(&(instance.name.clone(), port.name.clone()))
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
                    "{publish_indent}if (const auto* value = {local}.as_ref()) {{\n",
                    local = output_local
                ));
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
                        write_indent_levels + if has_deadline { 1 } else { 0 },
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
                        write_indent_levels + if has_deadline { 1 } else { 0 },
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

    if collect_outputs {
        output
            .push_str("    return FlowrtTaskOutcome::ok(std::move(flowrt_output_commits));\n}\n\n");
    } else {
        output.push_str("    return flowrt::Status::Ok;\n}\n\n");
    }
    output
}

fn adapt_cpp_status_returns_for_collect(code: &str, collect_outputs: bool) -> String {
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
    bridges: &'a [BridgeRuntimePlan],
    boundaries: &'a [BoundaryRuntimePlan],
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

fn emit_cpp_resource_registration(
    graph: &GraphIr,
    order: &[&InstanceIr],
    process_name: &str,
) -> String {
    let instance_names = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut output = String::new();
    for satisfaction in &graph.resource_satisfactions {
        if !instance_names.contains(satisfaction.instance.name.as_str()) {
            continue;
        }
        let provider = satisfaction.provider.as_ref();
        output.push_str(&format!(
            "    introspection_state.register_resource(flowrt::IntrospectionResourceStatus{{\n        .name = {},\n        .capability = {},\n        .access = std::optional<std::string>{{{}}},\n        .state = \"unknown\",\n        .required = {},\n        .readiness = std::optional<std::string>{{{}}},\n        .health = std::optional<std::string>{{{}}},\n        .on_failure = std::optional<std::string>{{{}}},\n        .contract_status = std::optional<std::string>{{{}}},\n        .satisfied = std::optional<bool>{{{}}},\n        .provider = {},\n        .provider_scope = std::nullopt,\n        .provider_readiness_source = std::nullopt,\n        .provider_health_source = std::nullopt,\n        .diagnostic = {},\n        .suggestion = std::nullopt,\n        .source = std::optional<std::string>{{\"contract\"}},\n        .owner_process = std::optional<std::string>{{{}}},\n        .last_error = std::nullopt,\n        .updated_unix_ms = std::nullopt,\n    }});\n",
            cpp_string_literal(&format!(
                "{}.{}",
                satisfaction.instance.name, satisfaction.resource
            )),
            cpp_string_literal(&satisfaction.capability.0),
            cpp_string_literal(resource_access_name(satisfaction.access)),
            satisfaction.required,
            cpp_string_literal(resource_readiness_name(satisfaction.readiness)),
            cpp_string_literal(resource_health_name(satisfaction.health)),
            cpp_string_literal(resource_failure_name(satisfaction.on_failure)),
            cpp_string_literal(resource_satisfaction_status(satisfaction)),
            satisfaction.satisfied,
            provider.map_or_else(
                || "std::nullopt".to_string(),
                |provider| format!(
                    "std::optional<std::string>{{{}}}",
                    cpp_string_literal(&provider.name)
                )
            ),
            satisfaction.diagnostic.as_ref().map_or_else(
                || "std::nullopt".to_string(),
                |diagnostic| format!(
                    "std::optional<std::string>{{{}}}",
                    cpp_string_literal(diagnostic)
                )
            ),
            cpp_string_literal(process_name),
        ));
    }
    output
}

fn emit_cpp_io_boundary_registration(contract: &ContractIr, order: &[&InstanceIr]) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.kind != ComponentKind::IoBoundary {
            continue;
        }
        output.push_str(&format!(
            "    introspection_state.register_io_boundary({}, {}, {});\n",
            cpp_string_literal(&instance.name),
            cpp_string_literal(&component.name),
            cpp_boundary_resources_literal(component)
        ));
    }
    output
}

fn emit_cpp_boundary_input_registration(boundaries: &[BoundaryRuntimePlan]) -> String {
    let mut output = String::new();
    for boundary in boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Input)
    {
        output.push_str(&format!(
            "    introspection_state.register_boundary_input({}, {}, {}_);\n",
            cpp_string_literal(&boundary.endpoint_name),
            cpp_string_literal(&boundary.ty.canonical_syntax()),
            boundary.field_name,
        ));
    }
    output
}

fn emit_cpp_boundary_output_probe_registration(boundaries: &[BoundaryRuntimePlan]) -> String {
    let mut output = String::new();
    for boundary in boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Output)
    {
        let ty = cpp_type(&boundary.ty);
        output.push_str(&format!(
            "    introspection_state.register_channel({}, {});\n    auto {field}_probe = {field}_.register_sink(\n        [&introspection_state](const {ty}& value, std::optional<std::uint64_t> published_at_ms) {{\n            try {{\n                std::vector<std::uint8_t> payload(flowrt::detail::encoded_frame_size(value));\n                flowrt::detail::encode_frame(value, std::span<std::uint8_t>{{payload}});\n                introspection_state.record_channel_publish_bytes({}, {}, std::move(payload), published_at_ms);\n            }} catch (...) {{\n            }}\n        }});\n    (void){field}_probe;\n",
            cpp_string_literal(&boundary.endpoint_name),
            cpp_string_literal(&boundary.ty.canonical_syntax()),
            cpp_string_literal(&boundary.endpoint_name),
            cpp_string_literal(&boundary.ty.canonical_syntax()),
            field = boundary.field_name,
        ));
    }
    output
}

fn emit_cpp_io_boundary_contexts(contract: &ContractIr, order: &[&InstanceIr]) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.kind != ComponentKind::IoBoundary {
            continue;
        }
        output.push_str(&format!(
            "    auto {context} = flowrt::Context::for_boundary(flowrt::BoundaryContext{{{}, {}, {}, [&introspection_state](flowrt::BoundaryStatus status) {{\n        introspection_state.record_io_boundary_health(std::move(status));\n    }}, [&introspection_state](std::string_view name, const flowrt::FrameDescriptor& descriptor, flowrt::FrameLeaseStatus status, bool payload_recording) {{\n        const auto record = introspection_state.record_frame_descriptor_event(name, descriptor, status, payload_recording);\n        return flowrt::BoundaryRecordOutcome{{.recorded = record.recorded, .dropped = record.dropped}};\n    }}}});\n",
            cpp_string_literal(&instance.name),
            cpp_string_literal(&component.name),
            cpp_boundary_resources_literal(component),
            context = cpp_lifecycle_context_name(component, instance),
        ));
    }
    output
}

fn cpp_boundary_resources_literal(component: &ComponentIr) -> String {
    let mut output = String::from("std::vector<flowrt::BoundaryResourceStatus>{");
    for resource in &component.resources {
        output.push_str(&format!(
            "flowrt::BoundaryResourceStatus{{.name = {}, .kind = {}}},",
            cpp_string_literal(&resource.name),
            cpp_string_literal(&resource.capability.0)
        ));
    }
    output.push('}');
    output
}

fn cpp_lifecycle_context_name(component: &ComponentIr, instance: &InstanceIr) -> String {
    if component.kind == ComponentKind::IoBoundary {
        format!(
            "{}_boundary_context",
            crate::snake_identifier(&instance.name)
        )
    } else {
        "lifecycle_context".to_string()
    }
}

fn emit_cpp_scheduler_v2_loop(run: &CppRunEmission<'_>) -> String {
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

fn cpp_scheduler_clock_source(contract: &ContractIr) -> &'static str {
    if contract.artifact.temporary_overlay.is_some() {
        "simulated_replay"
    } else {
        "realtime"
    }
}

fn cpp_task_clock_source_expr(contract: &ContractIr) -> &'static str {
    if contract.artifact.temporary_overlay.is_some() {
        "flowrt::ClockSource::Replay"
    } else {
        "flowrt::ClockSource::Runtime"
    }
}

fn cpp_trigger_name(trigger: flowrt_ir::TriggerKind) -> &'static str {
    match trigger {
        flowrt_ir::TriggerKind::Periodic => "periodic",
        flowrt_ir::TriggerKind::OnMessage => "on_message",
        flowrt_ir::TriggerKind::Startup => "startup",
        flowrt_ir::TriggerKind::Shutdown => "shutdown",
    }
}

fn cpp_task_timing_name(task: &flowrt_ir::TaskIr) -> String {
    format!("{}.{}", task.instance.name, task.name)
}

fn emit_cpp_scheduler_event_registration(
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

fn cpp_task_lane_name(task: &flowrt_ir::TaskIr) -> String {
    resolved_task_lane_name(task)
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

fn emit_cpp_task_admission_health_update(tasks: &[&flowrt_ir::TaskIr]) -> String {
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

fn emit_cpp_task_result_health_update(tasks: &[&flowrt_ir::TaskIr]) -> String {
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
fn emit_cpp_fairness_check(lane_names: &BTreeSet<String>) -> String {
    let mut output = String::new();
    for lane in lane_names {
        let lane_id = cpp_lane_id_expr(lane);
        output.push_str(&format!(
            "        if (scheduler.lane_starvation_ticks({lane_id}) > fairness_starvation_threshold) {{\n            for (auto &[name, health] : health_map) {{\n                if (health.lane == \"{lane}\") {{\n                    health.fairness_violations += 1;\n                }}\n            }}\n        }}\n"
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

fn cpp_lane_id_expr(lane_name: &str) -> String {
    format!(
        "flowrt::LaneId{{flowrt::fnv1a64({})}}",
        cpp_string_literal(lane_name)
    )
}

fn cpp_lane_id_u64_expr(lane_name: &str) -> String {
    format!("flowrt::fnv1a64({})", cpp_string_literal(lane_name))
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

fn cpp_bridge_seen_revision_name(bridge: &BridgeRuntimePlan, task: &flowrt_ir::TaskIr) -> String {
    format!(
        "{}_seen_revision_for_{}_{}",
        bridge.field_name,
        crate::snake_identifier(&task.instance.name),
        crate::snake_identifier(&task.name)
    )
}

fn cpp_boundary_seen_revision_name(
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

fn cpp_input_bridges_for_task<'a>(
    task: &flowrt_ir::TaskIr,
    bridges: &'a [BridgeRuntimePlan],
) -> Vec<&'a BridgeRuntimePlan> {
    task.inputs
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

fn cpp_input_boundaries_for_task<'a>(
    task: &flowrt_ir::TaskIr,
    boundaries: &'a [BoundaryRuntimePlan],
) -> Vec<&'a BoundaryRuntimePlan> {
    task.inputs
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

fn emit_cpp_ros2_boundary_input_pump(
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

fn emit_cpp_on_message_revision_state(
    tasks: &[&flowrt_ir::TaskIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
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
        for bridge in cpp_input_bridges_for_task(task, bridges) {
            output.push_str(&format!(
                "    std::uint64_t {seen} = 0;\n",
                seen = cpp_bridge_seen_revision_name(bridge, task)
            ));
        }
        for boundary in cpp_input_boundaries_for_task(task, boundaries) {
            output.push_str(&format!(
                "    std::uint64_t {seen} = 0;\n",
                seen = cpp_boundary_seen_revision_name(boundary, task)
            ));
        }
    }
    output
}

fn emit_cpp_on_message_wake_checks(
    tasks: &[&flowrt_ir::TaskIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
) -> String {
    let mut output = String::new();
    for (index, task) in tasks.iter().enumerate() {
        if task.trigger != flowrt_ir::TriggerKind::OnMessage {
            continue;
        }
        let input_binds = cpp_input_binds_for_task(task, binds);
        let input_bridges = cpp_input_bridges_for_task(task, bridges);
        let input_boundaries = cpp_input_boundaries_for_task(task, boundaries);
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

fn cpp_runtime_channel_commit_with_health(
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

fn cpp_bridge_runtime_channel_commit(bridge: &BridgeRuntimePlan, payload_name: &str) -> String {
    format!(
        "        auto {payload_name} = *value;\n        flowrt_output_commits.emplace_back([{payload_name} = std::move({payload_name}), tick_time_ms](App& app, flowrt::IntrospectionState& /*introspection_state*/, flowrt::ScheduleWaiter& /*scheduler_events*/, std::map<std::string, flowrt::IntrospectionTaskHealth>& /*health_map*/) mutable {{\n            const auto* value = &{payload_name};\n            if (const auto status = status_from_push_result(app.{field}_.publish_at(*value, tick_time_ms)); status != flowrt::Status::Ok) {{\n                return status;\n            }}\n            return flowrt::Status::Ok;\n        }});\n",
        field = bridge.field_name
    )
}

fn cpp_bridge_runtime_channel_read(
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

fn cpp_runtime_step_uses_tick_time(
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

fn cpp_boundary_input_read(boundary: &BoundaryRuntimePlan, local_name: &str) -> String {
    format!(
        "    const auto {local}_read = {field}_.read_at(tick_time_ms);\n    const auto {local} = {local}_read.view();\n",
        local = local_name,
        field = boundary.field_name,
    )
}

fn cpp_boundary_output_write(boundary: &BoundaryRuntimePlan) -> String {
    format!(
        "        {field}_.publish_at(*value, tick_time_ms);\n",
        field = boundary.field_name,
    )
}

fn cpp_boundary_output_commit(boundary: &BoundaryRuntimePlan, payload_name: &str) -> String {
    format!(
        "        auto {payload_name} = *value;\n        flowrt_output_commits.emplace_back([{payload_name} = std::move({payload_name}), tick_time_ms](App& app, flowrt::IntrospectionState& /*introspection_state*/, flowrt::ScheduleWaiter& /*scheduler_events*/, std::map<std::string, flowrt::IntrospectionTaskHealth>& /*health_map*/) mutable {{\n            const auto* value = &{payload_name};\n            app.{field}_.publish_at(*value, tick_time_ms);\n            return flowrt::Status::Ok;\n        }});\n",
        field = boundary.field_name,
    )
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

fn emit_cpp_param_constraint_helpers(order: &[&InstanceIr], contract: &ContractIr) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        for param in &component.params {
            if param.update != ParamUpdatePolicy::OnTick || !param_has_constraints(param) {
                continue;
            }
            let helper = cpp_param_constraint_helper_name(instance, param);
            let ty = cpp_param_type(param.ty);
            let checks = cpp_param_constraint_checks(param);
            output.push_str(&format!(
                "bool {helper}(const {ty}& value) {{\n    return {checks};\n}}\n\n"
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
            "{indent}if (const auto {pending} = introspection_state.peek_pending_param({runtime_name}); {pending}.has_value()) {{\n",
            runtime_name = cpp_string_literal(&runtime_name)
        ));
        output.push_str(&format!(
            "{inner_indent}auto {next} = {instance}_params_;\n",
            instance = instance.name
        ));
        output.push_str(&format!(
            "{inner_indent}if (!decode_flowrt_param_value(*{pending}, {next}.{field})) {{\n{deep_indent}introspection_state.record_param_rejected({runtime_name}, *{pending}, \"decode_failed\");\n",
            field = param.name,
            runtime_name = cpp_string_literal(&runtime_name),
            deep_indent = if nested { "                " } else { "            " }
        ));
        if param_has_constraints(param) {
            output.push_str(&format!(
                "{inner_indent}}} else if (!{}({next}.{field})) {{\n{deep_indent}introspection_state.record_param_rejected({runtime_name}, *{pending}, \"constraint_failed\");\n",
                cpp_param_constraint_helper_name(instance, param),
                field = param.name,
                runtime_name = cpp_string_literal(&runtime_name),
                deep_indent = if nested { "                " } else { "            " }
            ));
        }
        output.push_str(&format!(
            "{inner_indent}}} else if ({instance}_ && {instance}_->on_params_update({instance}_params_, {next}, {context_name}) != flowrt::Status::Ok) {{\n{deep_indent}introspection_state.record_param_rejected({runtime_name}, *{pending}, \"callback_rejected\");\n",
            instance = instance.name,
            runtime_name = cpp_string_literal(&runtime_name),
            deep_indent = if nested { "                " } else { "            " }
        ));
        output.push_str(&format!(
            "{inner_indent}}} else {{\n{deep_indent}{instance}_params_ = std::move({next});\n{deep_indent}introspection_state.record_param_applied({runtime_name}, *{pending});\n{inner_indent}}}\n",
            instance = instance.name,
            runtime_name = cpp_string_literal(&runtime_name),
            deep_indent = if nested { "                " } else { "            " }
        ));
        output.push_str(&format!("{indent}}}\n"));
    }
    output
}

fn param_has_constraints(param: &ParamIr) -> bool {
    param.min.is_some() || param.max.is_some() || !param.choices.is_empty()
}

fn cpp_param_constraint_helper_name(instance: &InstanceIr, param: &ParamIr) -> String {
    format!(
        "flowrt_validate_pending_param_{}_{}",
        crate::snake_identifier(&instance.name),
        crate::snake_identifier(&param.name)
    )
}

fn cpp_param_constraint_checks(param: &ParamIr) -> String {
    let mut checks = Vec::new();
    if param_type_supports_range(param.ty)
        && let Some(min) = &param.min
    {
        if let Some(literal) = cpp_param_constraint_literal(param, min) {
            checks.push(format!("value >= {literal}"));
        }
    }
    if param_type_supports_range(param.ty)
        && let Some(max) = &param.max
    {
        if let Some(literal) = cpp_param_constraint_literal(param, max) {
            checks.push(format!("value <= {literal}"));
        }
    }
    let choices = param
        .choices
        .iter()
        .filter_map(|choice| cpp_param_constraint_literal(param, choice))
        .map(|literal| format!("value == {literal}"))
        .collect::<Vec<_>>();
    if !choices.is_empty() {
        checks.push(format!("({})", choices.join(" || ")));
    }
    if checks.is_empty() {
        "true".to_string()
    } else {
        checks.join(" && ")
    }
}

fn param_type_supports_range(ty: ParamType) -> bool {
    matches!(
        ty,
        ParamType::U8
            | ParamType::U16
            | ParamType::U32
            | ParamType::U64
            | ParamType::I8
            | ParamType::I16
            | ParamType::I32
            | ParamType::I64
            | ParamType::F32
            | ParamType::F64
            | ParamType::String
    )
}

fn cpp_param_constraint_literal(param: &ParamIr, value: &ParamValue) -> Option<String> {
    match (param.ty, value) {
        (
            ParamType::Bool
            | ParamType::U8
            | ParamType::U16
            | ParamType::U32
            | ParamType::U64
            | ParamType::I8
            | ParamType::I16
            | ParamType::I32
            | ParamType::I64
            | ParamType::F32
            | ParamType::F64
            | ParamType::String,
            _,
        ) => Some(cpp_param_literal(param, value)),
        (ParamType::Array | ParamType::Table, _) => Some(cpp_param_literal(param, value)),
    }
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
    if (errno != 0 || end == owned.c_str() || *end != '\0' || !std::isfinite(parsed)) {
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
    if (errno != 0 || end == owned.c_str() || *end != '\0' || !std::isfinite(parsed)) {
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
        let owner_scope = cpp_string_literal(&plan.operation_name);
        let owner_name =
            cpp_string_literal(&format!("{}.{}", plan.client_instance, plan.client_port));
        if plan.backend.0 == "zenoh" {
            output.push_str(&format!(
                "/**\n * @brief `{client}.{port}` Operation client（zenoh backend，未实现）。\n */\nclass {handle_name} {{\npublic:\n    {handle_name}() = default;\n\n    flowrt::OperationClientResult<flowrt::OperationStartAck> start(const {goal_ty}& /*goal*/, std::uint64_t /*timeout_ms*/ = {default_timeout_ms}) {{\n        return flowrt::OperationClientResult<flowrt::OperationStartAck>::err(flowrt::OperationClientError::Backend);\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> cancel(flowrt::OperationId /*id*/, std::uint64_t /*timeout_ms*/ = {default_timeout_ms}) {{\n        return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Backend);\n    }}\n\n    flowrt::OperationClientResult<flowrt::OperationStatusSnapshot> status(flowrt::OperationId /*id*/, std::uint64_t /*timeout_ms*/ = {default_timeout_ms}) {{\n        return flowrt::OperationClientResult<flowrt::OperationStatusSnapshot>::err(flowrt::OperationClientError::Backend);\n    }}\n}};\n\n",
                client = plan.client_instance,
                port = plan.client_port,
            ));
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
