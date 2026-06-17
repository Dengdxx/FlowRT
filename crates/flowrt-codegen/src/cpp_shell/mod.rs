use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    ChannelKind, ComponentIr, ComponentKind, ContractIr, GraphFaultReaction, GraphIr,
    InstanceFailurePolicy, InstanceIr, IoBoundaryReadiness, LanguageKind,
    OperationConcurrencyPolicy, OperationPreemptPolicy, ParamIr, ParamType, ParamUpdatePolicy,
    ParamValue, PortIr, Ros2BridgeDirection, StalePolicy as IrStalePolicy,
};

mod app_constructor;
mod c_adapter_emit;
mod components;
mod helpers;
mod introspection_emit;
mod operation_emit;
mod params_emit;
mod run_emit;
mod service_emit;
mod step_emit;

pub(crate) use self::components::cpp_callback_args;

use self::app_constructor::*;
use self::c_adapter_emit::*;
use self::components::{cpp_component_interface_doc, cpp_lifecycle_method, cpp_tick_signature};
use self::helpers::*;
use self::introspection_emit::*;
use self::operation_emit::*;
use self::params_emit::*;
use self::run_emit::*;
use self::service_emit::*;
use self::step_emit::*;

use crate::messages::cpp_type;
use crate::resource_names::{
    resource_access_name, resource_failure_name, resource_health_name, resource_readiness_name,
    resource_satisfaction_status,
};
use crate::runtime_plan::{
    BindRuntimePlan, BoundaryRuntimePlan, BridgeRuntimePlan, ProcessRuntimePlan,
    SchedulerDataflowTaskPlan, SchedulerHiddenTaskKind, TaskEmissionPhase,
    active_binds_for_instances, active_boundaries_for_instances, bind_backend, bind_runtime_plans,
    boundary_runtime_plans, bridge_runtime_plans, incoming_bind_index_map,
    incoming_boundary_index_map, incoming_bridge_index_map, indent_generated_block,
    indent_generated_block_levels, nested_step_indent, on_message_trigger_guard,
    outgoing_bind_indices_map, outgoing_boundary_indices_map, outgoing_bridge_indices_map,
    process_runtime_plans, recoverable_instances, resolved_task_lane_name,
    runtime_channel_message_type, runtime_channel_name, runtime_channel_probe_capacity,
    runtime_param_name, scheduler_runtime_plan, step_indent,
};
use crate::{
    component_by_name, component_rust_name, float_literal, iox2_service_name, managed_header,
    param_json_literal, param_type_name, param_update_name, param_value_for_instance,
    ros2_bridge_key_expr, scheduler_tasks_for_order, selected_backend_name, tasks_for_instance,
    topo_order_instances_for_languages, zenoh_key_expr,
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
        (ParamType::F32, ParamValue::Integer(value)) => format!("{value}.0F"),
        (ParamType::F64, ParamValue::Float(value)) => float_literal(*value),
        (ParamType::F64, ParamValue::Integer(value)) => format!("{value}.0"),
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
        "#include <flowrt/runtime.hpp>\n#include <flowrt/inproc_service.hpp>\n#include <flowrt/operation.hpp>\n#include <flowrt/service.hpp>\n",
    );
    if contract.graphs.iter().any(|graph| {
        graph
            .tasks
            .iter()
            .any(|task| task.trigger == flowrt_ir::TriggerKind::OnSynchronized)
    }) {
        output.push_str("#include <flowrt/synchronizer.hpp>\n");
    }
    output.push('\n');
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
    output.push_str("#include <algorithm>\n#include <array>\n#include <atomic>\n#include <cerrno>\n#include <chrono>\n#include <cmath>\n#include <cstdint>\n#include <cstdio>\n#include <cstdlib>\n#include <cstring>\n");
    if contract_has_c_components(contract) {
        output.push_str("#include <iostream>\n");
    }
    output.push_str("#include <deque>\n#include <limits>\n#include <memory>\n#include <mutex>\n#include <optional>\n#include <set>\n#include <span>\n#include <string>\n#include <string_view>\n#include <thread>\n#include <type_traits>\n#include <utility>\n#include <variant>\n#include <vector>\n\n");
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
    for task in step_emit::cpp_on_synchronized_tasks(graph, &order) {
        output.push_str(&format!(
            "    {} {}_;\n",
            step_emit::cpp_synchronizer_field_type(),
            step_emit::cpp_synchronizer_field_name(task)
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

pub(crate) fn cpp_string_literal(value: &str) -> String {
    format!("{value:?}")
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
