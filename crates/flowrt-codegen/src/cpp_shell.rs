use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    ChannelKind, ComponentIr, ContractIr, GraphIr, InstanceIr, LanguageKind,
    OverflowPolicy as IrOverflowPolicy, ParamIr, ParamType, ParamUpdatePolicy, ParamValue, PortIr,
    StalePolicy as IrStalePolicy,
};

use crate::messages::{cpp_type, iox2_frame_slot_type_for_expr};
use crate::runtime_plan::{
    BindRuntimePlan, ProcessRuntimePlan, TaskEmissionPhase, active_binds_for_instances,
    bind_runtime_plans, incoming_bind_index_map, indent_generated_block, nested_step_indent,
    on_message_trigger_guard, outgoing_bind_indices_map, process_runtime_plans,
    runtime_channel_message_type, runtime_channel_name, runtime_channel_probe_capacity,
    runtime_param_name, step_indent,
};
use crate::{
    component_by_name, float_literal, iox2_service_name, managed_header, param_json_literal,
    param_type_name, param_update_name, param_value_for_instance, pascal_case,
    selected_backend_name, task_for_instance, topo_order_instances_for_language, zenoh_key_expr,
};

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
    output.push_str("#include <cstdint>\n#include <string>\n\n");
    output.push_str("#include <flowrt/runtime.hpp>\n\n");
    output.push_str("#include \"flowrt_app/messages.hpp\"\n\n");
    output.push_str("namespace flowrt_app {\n\n");

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
            pascal_case(&component.name)
        ));
        output.push_str("public:\n");
        output.push_str(&format!(
            "    virtual ~{}Interface() = default;\n",
            pascal_case(&component.name)
        ));
        output.push_str(&cpp_lifecycle_method("on_init"));
        output.push_str(&cpp_lifecycle_method("on_start"));
        output.push_str(&cpp_lifecycle_method("on_stop"));
        output.push_str(&cpp_lifecycle_method("on_shutdown"));
        output.push_str(&cpp_params_update_signature(component));
        output.push_str(&cpp_tick_signature(component));
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
    let incoming_bind_index = incoming_bind_index_map(&bind_plans);
    let outgoing_bind_indices = outgoing_bind_indices_map(&bind_plans);
    let selected_backend = selected_backend_name(contract);

    let mut output = managed_header();
    output.push_str("#include \"flowrt_app/runtime_shell.hpp\"\n\n");
    output.push_str("#include \"flowrt_app/selfdesc.hpp\"\n\n");
    output.push_str("#include <cerrno>\n#include <chrono>\n#include <cstdint>\n#include <cstdlib>\n#include <optional>\n#include <span>\n#include <string>\n#include <string_view>\n#include <type_traits>\n#include <utility>\n#include <variant>\n#include <vector>\n\n");
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
        &selected_backend,
    ));
    let step_emission = CppStepEmission {
        contract,
        graph,
        binds: &bind_plans,
        incoming_bind_index: &incoming_bind_index,
        outgoing_bind_indices: &outgoing_bind_indices,
        selected_backend: &selected_backend,
    };
    output.push_str(&emit_cpp_app_step(
        &step_emission,
        &order,
        "step",
        TaskEmissionPhase::Scheduler,
    ));
    output.push_str(&emit_cpp_app_step(
        &step_emission,
        &order,
        "step_startup",
        TaskEmissionPhase::Startup,
    ));
    output.push_str(&emit_cpp_app_step(
        &step_emission,
        &order,
        "step_shutdown",
        TaskEmissionPhase::Shutdown,
    ));
    for process in &process_plans {
        output.push_str(&emit_cpp_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}", process.method_suffix),
            TaskEmissionPhase::Scheduler,
        ));
        output.push_str(&emit_cpp_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}_startup", process.method_suffix),
            TaskEmissionPhase::Startup,
        ));
        output.push_str(&emit_cpp_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}_shutdown", process.method_suffix),
            TaskEmissionPhase::Shutdown,
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
            package_name: &contract.package.name,
            process_name: &process.name,
        }));
    }
    let backend_factory = cpp_backend_factory(&selected_backend);
    output.push_str(&format!(
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
    let selected_backend = selected_backend_name(contract);

    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str(
        "#include <cstddef>\n#include <memory>\n#include <optional>\n#include <string_view>\n\n",
    );
    output.push_str("#include <flowrt/runtime.hpp>\n\n");
    output.push_str(
        "#include \"flowrt_app/components.hpp\"\n#include \"flowrt_app/messages.hpp\"\n\n",
    );
    output.push_str("namespace flowrt_app {\n\n");
    output.push_str(
        "/**\n * @brief Contract IR 驱动的 C++ inproc 应用 shell。\n *\n * `App` 持有用户组件实现和 FlowRT 管理的 channel 状态。用户代码通过 `flowrt_user::build_app()` 构造该对象，runtime shell 负责生命周期、调度和数据流转发。\n */\n",
    );
    output.push_str("class App {\npublic:\n");
    output.push_str(&emit_cpp_app_constructor_declaration(contract, &order));
    output.push_str(
        "    /**\n     * @brief 使用指定 backend 运行完整 C++ 应用图。\n     *\n     * @param backend 提供调度器和 capability 的 FlowRT backend。\n     * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。\n     * @return 应用执行状态。\n     */\n    flowrt::Status run(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks);\n\n    /**\n     * @brief 运行指定 RSDL process group。\n     *\n     * @param backend 提供调度器和 capability 的 FlowRT backend。\n     * @param process Contract IR 中声明的 process group 名称。\n     * @param run_ticks 可选的显式 tick 上限；为空表示无限运行。\n     * @return 应用执行状态。\n     */\n    flowrt::Status run_process(const flowrt::Backend& backend, std::string_view process, std::optional<std::size_t> run_ticks);\n\nprivate:\n    flowrt::Status step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);\n    flowrt::Status step_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);\n    flowrt::Status step_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);\n",
    );
    for process in &process_plans {
        output.push_str(&format!(
            "    flowrt::Status step_process_{}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);\n",
            process.method_suffix
        ));
        output.push_str(&format!(
            "    flowrt::Status step_process_{}_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);\n",
            process.method_suffix
        ));
        output.push_str(&format!(
            "    flowrt::Status step_process_{}_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);\n",
            process.method_suffix
        ));
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
            pascal_case(&component.name),
            instance.name
        ));
        if !component.params.is_empty() {
            output.push_str(&format!(
                "    {}Params {}_params_;\n",
                pascal_case(&component.name),
                instance.name
            ));
        }
    }
    for bind in &bind_plans {
        output.push_str(&format!(
            "    {} {}_;\n",
            cpp_runtime_channel_type(bind, &selected_backend),
            bind.field_name
        ));
        output.push_str(&format!(
            "    flowrt::IntrospectionChannelProbe {};\n",
            bind.probe_field_name
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
            pascal_case(&component.name),
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
    selected_backend: &str,
) -> String {
    let mut params = Vec::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        params.push(format!(
            "std::unique_ptr<{}Interface> {}",
            pascal_case(&component.name),
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
            cpp_runtime_channel_initializer(contract, graph, bind, selected_backend)
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
    output.push_str(" {}\n\n");
    output
}

struct CppStepEmission<'a> {
    contract: &'a ContractIr,
    graph: &'a GraphIr,
    binds: &'a [BindRuntimePlan],
    incoming_bind_index: &'a BTreeMap<(String, String), usize>,
    outgoing_bind_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    selected_backend: &'a str,
}

fn emit_cpp_app_step(
    emission: &CppStepEmission<'_>,
    order: &[&InstanceIr],
    function_name: &str,
    phase: TaskEmissionPhase,
) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "flowrt::Status App::{function_name}(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {{\n",
    ));
    if cpp_runtime_step_uses_tick_time(emission.binds, emission.selected_backend) {
        output.push_str(
            "    const auto tick_time_ms = static_cast<std::uint64_t>(tick);\n    (void)tick_time_ms;\n",
        );
    } else {
        output.push_str("    (void)tick;\n");
    }
    output.push_str("    (void)tick_context;\n");
    output.push_str("    (void)introspection_state;\n");

    for instance in order {
        let component = component_by_name(emission.contract, &instance.component.name);
        let Some(task) = task_for_instance(emission.graph, instance) else {
            continue;
        };
        if !phase.includes(task.trigger) {
            continue;
        }
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
                    output.push_str(&cpp_runtime_channel_read(
                        input,
                        bind,
                        &input_local,
                        emission.selected_backend,
                    ));
                    output.push_str(&cpp_runtime_stale_error_guard(&input_local, bind));
                } else {
                    output.push_str(&format!(
                        "    flowrt::Latest<{ty}> {local};\n",
                        ty = cpp_type(&input.ty),
                        local = input_local
                    ));
                }
            } else {
                output.push_str(&format!(
                    "    flowrt::Latest<{ty}> {local};\n",
                    ty = cpp_type(&input.ty),
                    local = input_local
                ));
            }
        }

        if let Some(guard) = &trigger_guard {
            output.push_str(&format!("    if ({guard}) {{\n"));
        }

        if !component.params.is_empty() && phase == TaskEmissionPhase::Scheduler {
            output.push_str(&cpp_apply_pending_params(
                instance,
                component,
                trigger_guard.is_some(),
            ));
        }

        if task.deadline_ms.is_some() {
            output.push_str(&format!(
                "{indent}const auto {instance}_deadline_started_at = std::chrono::steady_clock::now();\n",
                indent = step_indent(trigger_guard.is_some()),
                instance = instance.name
            ));
        }

        for port in &component.outputs {
            let output_local = cpp_step_local_name(&instance.name, &port.name);
            output.push_str(&format!(
                "{indent}flowrt::Output<{ty}> {local};\n",
                indent = step_indent(trigger_guard.is_some()),
                ty = cpp_type(&port.ty),
                local = output_local
            ));
        }

        let mut call_args = Vec::new();
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
            "{indent}if ({instance}_ && {instance}_->on_tick({args}) != flowrt::Status::Ok) {{\n{inner_indent}return flowrt::Status::Error;\n{indent}}}\n",
            indent = step_indent(trigger_guard.is_some()),
            inner_indent = nested_step_indent(trigger_guard.is_some()),
            instance = instance.name,
            args = call_args.join(", ")
        ));

        if let Some(deadline_ms) = task.deadline_ms {
            output.push_str(&format!(
                "{indent}if (std::chrono::steady_clock::now() - {instance}_deadline_started_at > std::chrono::milliseconds{{{deadline_ms}}}) {{\n{inner_indent}return flowrt::Status::Error;\n{indent}}}\n",
                indent = step_indent(trigger_guard.is_some()),
                inner_indent = nested_step_indent(trigger_guard.is_some()),
                instance = instance.name
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
            if outgoing.is_empty() {
                continue;
            }
            output.push_str(&format!(
                "{indent}if (const auto* value = {local}.as_ref()) {{\n",
                indent = step_indent(trigger_guard.is_some()),
                local = output_local
            ));
            for bind_index in outgoing {
                let bind = &emission.binds[bind_index];
                output.push_str(&indent_generated_block(
                    &cpp_runtime_channel_write(bind, emission.selected_backend),
                    trigger_guard.is_some(),
                ));
            }
            output.push_str(&format!("{}}}\n", step_indent(trigger_guard.is_some())));
        }

        if trigger_guard.is_some() {
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
    package_name: &'a str,
    process_name: &'a str,
}

fn emit_cpp_app_run_function(run: &CppRunEmission<'_>) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "flowrt::Status App::{}(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks) {{\n    flowrt::Context lifecycle_context;\n    auto status = flowrt::Status::Ok;\n",
        run.function_name
    ));
    output.push_str("    auto shutdown = flowrt::install_signal_shutdown_token();\n");
    output.push_str("    flowrt::IntrospectionState introspection_state;\n");
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
        "    if (status == flowrt::Status::Ok) {{\n        status = {}(0, lifecycle_context, introspection_state);\n    }}\n",
        run.startup_function_name
    ));
    output.push_str(&format!(
        "    {{\n        std::size_t tick_base = 0;\n        while (status == flowrt::Status::Ok && !shutdown.is_requested() && (!run_ticks.has_value() || tick_base < *run_ticks)) {{\n            status = backend.scheduler().run_ticks_until_shutdown(\n                1, shutdown, [this, &introspection_state, tick_base](std::size_t tick, flowrt::Context& tick_context) {{\n                    introspection_state.record_tick();\n                    return {}(tick_base + tick, tick_context, introspection_state);\n                }});\n            ++tick_base;\n        }}\n    }}\n",
        run.step_function_name
    ));
    output.push_str(&format!(
        "    if (status == flowrt::Status::Ok) {{\n        status = {}(0, lifecycle_context, introspection_state);\n    }}\n",
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

pub(crate) fn cpp_string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn cpp_runtime_channel_type(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let ty = cpp_type(&bind.source_type);
    if selected_backend == "iox2" {
        if bind.source_uses_variable_frame {
            return format!(
                "flowrt::iox2::Iox2FramePubSub<{ty}, {}>",
                iox2_frame_slot_type_for_expr(&bind.source_type)
            );
        }
        return format!("flowrt::iox2::Iox2PubSub<{ty}>");
    }
    if selected_backend == "zenoh" {
        return format!("flowrt::zenoh::ZenohPubSub<{ty}>");
    }

    match bind.channel {
        ChannelKind::Latest => format!("flowrt::LatestChannel<{ty}>"),
        ChannelKind::Fifo => format!("flowrt::FifoChannel<{ty}>"),
    }
}

fn cpp_runtime_channel_initializer(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
    selected_backend: &str,
) -> String {
    if selected_backend == "iox2" {
        if bind.source_uses_variable_frame {
            return format!(
                "flowrt::iox2::Iox2FramePubSub<{}, {}>::open_with_config({}, {})",
                cpp_type(&bind.source_type),
                iox2_frame_slot_type_for_expr(&bind.source_type),
                cpp_string_literal(&iox2_service_name(contract, graph, bind)),
                cpp_iox2_channel_config_expr(bind)
            );
        }
        return format!(
            "flowrt::iox2::Iox2PubSub<{}>::open_with_config({}, {})",
            cpp_type(&bind.source_type),
            cpp_string_literal(&iox2_service_name(contract, graph, bind)),
            cpp_iox2_channel_config_expr(bind)
        );
    }
    if selected_backend == "zenoh" {
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
    selected_backend: &str,
) -> String {
    if matches!(selected_backend, "iox2" | "zenoh") {
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

fn cpp_step_local_name(instance: &str, port: &str) -> String {
    format!("{instance}_{port}")
}

fn cpp_introspection_publish_record(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let helper = if bind.source_uses_variable_frame || selected_backend == "zenoh" {
        "record_introspection_publish_frame"
    } else {
        "record_introspection_publish_copy"
    };
    format!(
        "        {helper}(this->{probe}, *value, tick_time_ms);\n",
        probe = bind.probe_field_name
    )
}

fn cpp_runtime_channel_write(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let introspection_record = cpp_introspection_publish_record(bind, selected_backend);
    if matches!(selected_backend, "iox2" | "zenoh") {
        return format!(
            "        if (const auto status = status_from_push_result({field}_.publish_at(*value, tick_time_ms)); status != flowrt::Status::Ok) {{\n            return status;\n        }}\n{introspection_record}",
            field = bind.field_name
        );
    }

    match bind.channel {
        ChannelKind::Latest => format!(
            "        {field}_.publish_at(*value, tick_time_ms);\n{introspection_record}",
            field = bind.field_name
        ),
        ChannelKind::Fifo => format!(
            "        const auto {field}_result = {field}_.push_at(*value, tick_time_ms);\n        if (const auto status = status_from_push_result({field}_result); status != flowrt::Status::Ok) {{\n            return status;\n        }}\n        if (std::holds_alternative<flowrt::ChannelWriteOutcome>({field}_result)) {{\n            switch (std::get<flowrt::ChannelWriteOutcome>({field}_result)) {{\n                case flowrt::ChannelWriteOutcome::Accepted:\n                case flowrt::ChannelWriteOutcome::DroppedOldest:\n{introspection_record}                    break;\n                case flowrt::ChannelWriteOutcome::DroppedNewest:\n                case flowrt::ChannelWriteOutcome::Backpressured:\n                    break;\n            }}\n        }}\n",
            field = bind.field_name
        ),
    }
}

fn cpp_runtime_step_uses_tick_time(binds: &[BindRuntimePlan], selected_backend: &str) -> bool {
    (!binds.is_empty() && matches!(selected_backend, "iox2" | "zenoh"))
        || binds
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
        pascal_case(&component.name)
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
    let mut output = format!("{}Params{{", pascal_case(&component.name));
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
    let params_ty = format!("{}Params", pascal_case(&component.name));
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
            "{inner_indent}if ({instance}_ && {instance}_->on_params_update({instance}_params_, {next}, tick_context) != flowrt::Status::Ok) {{\n{deep_indent}return flowrt::Status::Error;\n{inner_indent}}}\n",
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
        "int main(int argc, char** argv) {\n    std::string_view process;\n    std::optional<std::size_t> run_ticks;\n    for (int index = 1; index < argc; ++index) {\n        const std::string_view arg(argv[index]);\n        if (arg == \"--process\") {\n            if (index + 1 >= argc) {\n                return 2;\n            }\n            process = argv[++index];\n        } else if (arg == \"--flowrt-run-ticks\") {\n            if (index + 1 >= argc) {\n                return 2;\n            }\n            const std::string_view raw(argv[++index]);\n            std::size_t ticks = 0;\n            const auto result = std::from_chars(raw.data(), raw.data() + raw.size(), ticks);\n            if (result.ec != std::errc{} || result.ptr != raw.data() + raw.size() || ticks == 0) {\n                return 2;\n            }\n            run_ticks = ticks;\n        } else {\n            return 2;\n        }\n    }\n\n    const auto status = process.empty() ? flowrt_app::run(run_ticks) : flowrt_app::run_process(process, run_ticks);\n    return status == flowrt::Status::Ok ? 0 : 1;\n}\n",
    );
    output
}

fn emit_cpp_introspection_helpers() -> String {
    r#"flowrt::IntrospectionChannelProbe register_introspection_channel(
    flowrt::IntrospectionState& state,
    std::string_view name,
    std::string_view message_type,
    std::size_t max_payload_len
) {
    try {
        state.register_channel_with_probe_capacity(
            std::string{name},
            std::string{message_type},
            std::optional<std::size_t>{max_payload_len});
        if (const auto probe = state.channel_probe(name); probe.has_value()) {
            return *probe;
        }
    } catch (...) {
    }
    return flowrt::IntrospectionChannelProbe{};
}

template <typename T>
void record_introspection_publish_copy(
    const flowrt::IntrospectionChannelProbe& probe,
    const T& value,
    std::uint64_t published_at_ms
) {
    probe.record_publish_event();
    if (!probe.enabled()) {
        return;
    }
    try {
        probe.try_record_bytes(
            std::span<const std::uint8_t>{reinterpret_cast<const std::uint8_t*>(&value), sizeof(T)},
            std::optional<std::uint64_t>{published_at_ms});
    } catch (...) {
    }
}

template <typename T>
void record_introspection_publish_frame(
    const flowrt::IntrospectionChannelProbe& probe,
    const T& value,
    std::uint64_t published_at_ms
) {
    probe.record_publish_event();
    if (!probe.enabled()) {
        return;
    }
    try {
        std::vector<std::uint8_t> payload(flowrt::detail::encoded_frame_size(value));
        flowrt::detail::encode_frame(value, payload);
        probe.try_record_bytes(payload, std::optional<std::uint64_t>{published_at_ms});
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
    const long long parsed = std::strtoll(owned.c_str(), &end, 10);
    if (errno != 0 || end == owned.c_str() || *end != '\0') {
        return false;
    }
    output = static_cast<T>(parsed);
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
            runtime_channel_probe_capacity(contract, bind),
            probe = bind.probe_field_name
        ));
    }
    output
}

fn cpp_callback_args(component: &ComponentIr) -> Vec<String> {
    let mut args = Vec::new();
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
            pascal_case(&component.name)
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

fn cpp_tick_signature(component: &ComponentIr) -> String {
    let args = cpp_callback_args(component);
    let doc = cpp_tick_doc(component);
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

fn cpp_tick_doc(component: &ComponentIr) -> String {
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
    for output_port in &component.outputs {
        output.push_str(&format!(
            "     * @param {} 输出端口写入句柄。\n",
            output_port.name
        ));
    }
    output.push_str("     * @return 本次回调的 FlowRT 执行状态。\n     */\n");
    output
}
