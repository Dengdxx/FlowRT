//! FlowRT 管理应用产物的生成入口。
//!
//! 本 crate 只从 Contract IR 生成 glue：消息类型、组件接口、runtime shell、启动配置和构建文件。
//! 生成内容必须位于用户项目可见的 `flowrt/` 目录下，并且不得承载用户业务逻辑。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use flowrt_ir::{
    ChannelEdgeIr, ChannelKind, ComponentIr, ContractIr, FieldIr, GraphIr, InstanceIr,
    LanguageKind, OverflowPolicy as IrOverflowPolicy, ParamIr, ParamType, ParamUpdatePolicy,
    ParamValue, PortIr, StalePolicy as IrStalePolicy, TaskIr, TriggerKind, TypeExpr, TypeIr,
};
use flowrt_validate::validate_contract;

mod build_files;
mod launch_manifest;
mod messages;
mod selfdesc;
mod supervisor;

use build_files::{emit_cargo_manifest, emit_cmake};
use launch_manifest::emit_launch_manifest;
use messages::{
    cpp_type, emit_cpp_message_abi_tests, emit_cpp_messages, emit_rust_message_abi_tests,
    emit_rust_messages, fixed_message_abi_expectations, frame_header_size_for_expr,
    frame_header_size_for_type, frame_max_size_for_type, iox2_frame_slot_type_for_expr, rust_type,
    rust_wire_size, type_contains_variable_data, variable_tail_max_size,
};
use selfdesc::{
    emit_cpp_selfdesc_header, emit_cpp_selfdesc_source, emit_rust_selfdesc, emit_self_description,
};
use supervisor::{emit_rust_supervisor, emit_rust_supervisor_main};

/// artifact emission 返回的结果类型。
pub type Result<T> = std::result::Result<T, CodegenError>;

/// 生成 FlowRT 管理产物时产生的错误。
#[derive(Debug, thiserror::Error)]
pub enum CodegenError {
    #[error("Contract IR v0.1 must contain exactly one graph; found {count}")]
    GraphCount { count: usize },

    #[error("failed to serialize launch manifest: {0}")]
    LaunchJson(#[from] serde_json::Error),

    #[error("failed to derive message ABI expectations: {0}")]
    MessageAbi(#[from] flowrt_conformance::AbiError),

    #[error("contract validation failed: {0}")]
    Validation(#[from] flowrt_validate::ValidationReport),
}

/// 一个要写入应用 `flowrt/` 目录下的 FlowRT 管理文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub relative_path: PathBuf,
    pub content: String,
}

/// 从一个 Contract IR 文档生成的文件集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactBundle {
    pub artifacts: Vec<Artifact>,
}

/// codegen 输出语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodegenLanguage {
    Cpp,
    Rust,
}

/// 一个计划生成的输出族。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenUnit {
    pub language: CodegenLanguage,
    pub artifact_group: &'static str,
}

/// 从 Contract IR 推导出的保守 codegen plan。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenPlan {
    pub units: Vec<CodegenUnit>,
}

/// 为一个 contract 构建高层生成计划。
pub fn plan_codegen(contract: &ContractIr) -> CodegenPlan {
    let mut units = Vec::new();
    if has_language(contract, LanguageKind::Cpp) {
        units.push(CodegenUnit {
            language: CodegenLanguage::Cpp,
            artifact_group: "runtime_shell",
        });
    }
    if has_language(contract, LanguageKind::Rust) {
        units.push(CodegenUnit {
            language: CodegenLanguage::Rust,
            artifact_group: "runtime_shell",
        });
    }
    CodegenPlan { units }
}

/// 生成首批 FlowRT 管理的应用产物。
pub fn emit_artifacts(contract: &ContractIr) -> Result<ArtifactBundle> {
    validate_contract(contract)?;

    if contract.graphs.len() != 1 {
        return Err(CodegenError::GraphCount {
            count: contract.graphs.len(),
        });
    }

    let mut artifacts = Vec::new();
    let abi_expectations = fixed_message_abi_expectations(contract)?;
    let has_cpp = has_language(contract, LanguageKind::Cpp);
    let has_rust = has_language(contract, LanguageKind::Rust);

    if has_cpp {
        artifacts.push(artifact(
            "cpp/include/flowrt_app/messages.hpp",
            emit_cpp_messages(contract),
        ));
        artifacts.push(artifact(
            "cpp/include/flowrt_app/selfdesc.hpp",
            emit_cpp_selfdesc_header(contract),
        ));
        artifacts.push(artifact(
            "cpp/include/flowrt_app/components.hpp",
            emit_cpp_components(contract),
        ));
        artifacts.push(artifact(
            "cpp/include/flowrt_app/runtime_shell.hpp",
            emit_cpp_runtime_shell_header(contract),
        ));
        artifacts.push(artifact(
            "cpp/src/selfdesc.cpp",
            emit_cpp_selfdesc_source(contract),
        ));
        artifacts.push(artifact(
            "cpp/src/runtime_shell.cpp",
            emit_cpp_runtime_shell(contract),
        ));
        artifacts.push(artifact("cpp/src/main.cpp", emit_cpp_main()));
        if !abi_expectations.is_empty() {
            artifacts.push(artifact(
                "cpp/tests/message_abi.cpp",
                emit_cpp_message_abi_tests(contract, &abi_expectations),
            ));
        }
    }

    if has_cpp || has_rust {
        artifacts.push(artifact(
            "rust/src/selfdesc.rs",
            emit_rust_selfdesc(contract),
        ));
    }

    if has_rust {
        artifacts.push(artifact(
            "rust/src/messages.rs",
            emit_rust_messages(contract),
        ));
        artifacts.push(artifact(
            "rust/src/components.rs",
            emit_rust_components(contract),
        ));
        artifacts.push(artifact(
            "rust/src/runtime_shell.rs",
            emit_rust_runtime_shell(contract),
        ));
        artifacts.push(artifact("rust/src/main.rs", emit_rust_main()));
        if !abi_expectations.is_empty() {
            artifacts.push(artifact(
                "rust/tests/message_abi.rs",
                emit_rust_message_abi_tests(contract, &abi_expectations),
            ));
        }
    }

    artifacts.push(artifact(
        "selfdesc/selfdesc.json",
        emit_self_description(contract)?,
    ));

    if has_cpp || has_rust {
        artifacts.push(artifact(
            "rust/src/supervisor.rs",
            emit_rust_supervisor(contract),
        ));
        artifacts.push(artifact("rust/src/lib.rs", emit_rust_lib(has_rust)));
        artifacts.push(artifact(
            "rust/src/supervisor_main.rs",
            emit_rust_supervisor_main(),
        ));
    }

    artifacts.push(artifact(
        "launch/launch.json",
        emit_launch_manifest(contract)?,
    ));
    artifacts.push(artifact("build/CMakeLists.txt", emit_cmake(contract)));
    artifacts.push(artifact("build/Cargo.toml", emit_cargo_manifest(contract)));

    Ok(ArtifactBundle { artifacts })
}

fn artifact(path: impl Into<PathBuf>, content: String) -> Artifact {
    Artifact {
        relative_path: path.into(),
        content,
    }
}

pub(crate) fn has_language(contract: &ContractIr, language: LanguageKind) -> bool {
    contract
        .components
        .iter()
        .any(|component| component.language == language)
}

pub(crate) fn language_name(language: LanguageKind) -> &'static str {
    match language {
        LanguageKind::Cpp => "cpp",
        LanguageKind::Rust => "rust",
    }
}

fn param_value_for_instance<'a>(instance: &'a InstanceIr, param: &'a ParamIr) -> &'a ParamValue {
    instance
        .params
        .iter()
        .find(|value| value.name == param.name)
        .map(|value| &value.value)
        .unwrap_or(&param.default)
}

fn param_type_name(ty: ParamType) -> &'static str {
    match ty {
        ParamType::Bool => "bool",
        ParamType::U8 => "u8",
        ParamType::U16 => "u16",
        ParamType::U32 => "u32",
        ParamType::U64 => "u64",
        ParamType::I8 => "i8",
        ParamType::I16 => "i16",
        ParamType::I32 => "i32",
        ParamType::I64 => "i64",
        ParamType::F32 => "f32",
        ParamType::F64 => "f64",
        ParamType::String => "string",
        ParamType::Array => "array",
        ParamType::Table => "table",
    }
}

fn param_update_name(update: ParamUpdatePolicy) -> &'static str {
    match update {
        ParamUpdatePolicy::Startup => "startup",
        ParamUpdatePolicy::OnTick => "on_tick",
    }
}

fn param_value_json(value: &ParamValue) -> serde_json::Value {
    match value {
        ParamValue::Bool(value) => serde_json::Value::Bool(*value),
        ParamValue::Integer(value) => serde_json::json!(value),
        ParamValue::Float(value) => serde_json::json!(value),
        ParamValue::String(value) => serde_json::Value::String(value.clone()),
        ParamValue::Array(values) => {
            serde_json::Value::Array(values.iter().map(param_value_json).collect())
        }
        ParamValue::Table(values) => serde_json::Value::Object(
            values
                .iter()
                .map(|(name, value)| (name.clone(), param_value_json(value)))
                .collect(),
        ),
    }
}

fn param_json_literal(value: &ParamValue) -> String {
    serde_json::to_string(&param_value_json(value))
        .expect("FlowRT parameter values should always serialize as JSON")
}

fn param_json_value_literal(value: &ParamValue) -> String {
    format!("serde_json::json!({})", param_json_literal(value))
}

fn rust_param_type(ty: ParamType) -> &'static str {
    match ty {
        ParamType::Bool => "bool",
        ParamType::U8 => "u8",
        ParamType::U16 => "u16",
        ParamType::U32 => "u32",
        ParamType::U64 => "u64",
        ParamType::I8 => "i8",
        ParamType::I16 => "i16",
        ParamType::I32 => "i32",
        ParamType::I64 => "i64",
        ParamType::F32 => "f32",
        ParamType::F64 => "f64",
        ParamType::String => "String",
        ParamType::Array | ParamType::Table => "serde_json::Value",
    }
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

fn rust_param_literal(param: &ParamIr, value: &ParamValue) -> String {
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
        ) => format!("{} as {}", value, rust_param_type(param.ty)),
        (ParamType::F32, ParamValue::Float(value)) => format!("{}f32", float_literal(*value)),
        (ParamType::F32, ParamValue::Integer(value)) => format!("{}f32", value),
        (ParamType::F64, ParamValue::Float(value)) => format!("{}f64", float_literal(*value)),
        (ParamType::F64, ParamValue::Integer(value)) => format!("{}f64", value),
        (ParamType::String, ParamValue::String(value)) => {
            format!("{}.to_string()", rust_string_literal(value))
        }
        (ParamType::Array | ParamType::Table, _) => param_json_value_literal(value),
        _ => param_json_value_literal(value),
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

fn float_literal(value: f64) -> String {
    if value.is_finite() {
        let mut output = value.to_string();
        if !output.contains('.') && !output.contains('e') && !output.contains('E') {
            output.push_str(".0");
        }
        output
    } else {
        "0.0".to_string()
    }
}

fn emit_cpp_components(contract: &ContractIr) -> String {
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

fn emit_cpp_runtime_shell(contract: &ContractIr) -> String {
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

fn emit_cpp_runtime_shell_header(contract: &ContractIr) -> String {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskEmissionPhase {
    Scheduler,
    Startup,
    Shutdown,
}

impl TaskEmissionPhase {
    fn includes(self, trigger: TriggerKind) -> bool {
        match self {
            TaskEmissionPhase::Scheduler => {
                matches!(trigger, TriggerKind::Periodic | TriggerKind::OnMessage)
            }
            TaskEmissionPhase::Startup => trigger == TriggerKind::Startup,
            TaskEmissionPhase::Shutdown => trigger == TriggerKind::Shutdown,
        }
    }
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

fn on_message_trigger_guard<F>(task: &TaskIr, input_name: F) -> Option<String>
where
    F: Fn(&str) -> String,
{
    if task.trigger != TriggerKind::OnMessage || task.inputs.is_empty() {
        return None;
    }

    Some(
        task.inputs
            .iter()
            .map(|input| format!("{}.present()", input_name(input)))
            .collect::<Vec<_>>()
            .join(" || "),
    )
}

fn indent_generated_block(block: &str, nested: bool) -> String {
    if !nested {
        return block.to_string();
    }

    block
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("    {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn step_indent(nested: bool) -> &'static str {
    if nested { "        " } else { "    " }
}

fn nested_step_indent(nested: bool) -> &'static str {
    if nested { "            " } else { "        " }
}

fn rust_step_indent(nested: bool) -> &'static str {
    if nested { "            " } else { "        " }
}

fn rust_nested_step_indent(nested: bool) -> &'static str {
    if nested {
        "                "
    } else {
        "            "
    }
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

fn emit_cpp_main() -> String {
    let mut output = managed_header();
    output.push_str("#include \"flowrt_app/runtime_shell.hpp\"\n\n");
    output.push_str("#include <charconv>\n#include <cstddef>\n#include <optional>\n#include <string_view>\n#include <system_error>\n\n");
    output.push_str(
        "int main(int argc, char** argv) {\n    std::string_view process;\n    std::optional<std::size_t> run_ticks;\n    for (int index = 1; index < argc; ++index) {\n        const std::string_view arg(argv[index]);\n        if (arg == \"--process\") {\n            if (index + 1 >= argc) {\n                return 2;\n            }\n            process = argv[++index];\n        } else if (arg == \"--flowrt-run-ticks\") {\n            if (index + 1 >= argc) {\n                return 2;\n            }\n            const std::string_view raw(argv[++index]);\n            std::size_t ticks = 0;\n            const auto result = std::from_chars(raw.data(), raw.data() + raw.size(), ticks);\n            if (result.ec != std::errc{} || result.ptr != raw.data() + raw.size() || ticks == 0) {\n                return 2;\n            }\n            run_ticks = ticks;\n        } else {\n            return 2;\n        }\n    }\n\n    const auto status = process.empty() ? flowrt_app::run(run_ticks) : flowrt_app::run_process(process, run_ticks);\n    return status == flowrt::Status::Ok ? 0 : 1;\n}\n",
    );
    output
}

fn emit_rust_components(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str("\nuse crate::messages::*;\n\n");
    for component in contract
        .components
        .iter()
        .filter(|component| component.language == LanguageKind::Rust)
    {
        if !component.params.is_empty() {
            output.push_str(&rust_params_struct(component));
        }
        output.push_str(&rust_component_trait_doc(component));
        output.push_str(&format!("pub trait {} {{\n", pascal_case(&component.name)));
        output.push_str(&rust_lifecycle_doc("组件初始化钩子"));
        output.push_str(
            "    fn on_init(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {\n",
        );
        output.push_str("        flowrt::Status::ok()\n    }\n\n");
        output.push_str(&rust_lifecycle_doc("组件启动钩子"));
        output.push_str(
            "    fn on_start(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {\n",
        );
        output.push_str("        flowrt::Status::ok()\n    }\n\n");
        output.push_str(&rust_lifecycle_doc("组件停止钩子"));
        output.push_str(
            "    fn on_stop(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {\n",
        );
        output.push_str("        flowrt::Status::ok()\n    }\n\n");
        output.push_str(&rust_lifecycle_doc("组件关闭钩子"));
        output.push_str(
            "    fn on_shutdown(&mut self, _context: &mut flowrt::Context) -> flowrt::Status {\n",
        );
        output.push_str("        flowrt::Status::ok()\n    }\n\n");
        output.push_str(&rust_params_update_signature(component));
        output.push_str(&rust_tick_signature(component));
        output.push_str("}\n\n");
    }
    output
}

fn emit_rust_runtime_shell(contract: &ContractIr) -> String {
    let graph = contract
        .graphs
        .first()
        .expect("normalized contract must contain at least one graph");
    let order = topo_order_instances_for_language(contract, graph, LanguageKind::Rust);
    let process_plans = process_runtime_plans(&order);
    let bind_plans = bind_runtime_plans(contract, graph);
    let incoming_bind_index = incoming_bind_index_map(&bind_plans);
    let outgoing_bind_indices = outgoing_bind_indices_map(&bind_plans);
    let selected_backend = selected_backend_name(contract);

    let mut output = managed_header();
    output.push_str(
        "\nuse crate::components::*;\nuse crate::messages::*;\nuse crate::selfdesc;\nuse crate::user;\n\n",
    );
    output.push_str(&format!(
        "const SELECTED_BACKEND: &str = {};\n\n",
        rust_string_literal(&selected_backend)
    ));
    output.push_str(&format!(
        "const PACKAGE_NAME: &str = {};\n\n",
        rust_string_literal(&contract.package.name)
    ));
    let has_active_rust_channels = !active_binds_for_instances(&bind_plans, &order).is_empty();
    output.push_str(&emit_rust_introspection_helpers(
        has_active_rust_channels,
        contract_has_runtime_params_for_language(contract, LanguageKind::Rust),
    ));
    output.push_str("pub struct App {\n");
    for instance in &order {
        let component = component_by_name(contract, &instance.component.name);
        output.push_str(&format!(
            "    {}: Box<dyn {}>,\n",
            instance.name,
            pascal_case(&component.name)
        ));
        if !component.params.is_empty() {
            output.push_str(&format!(
                "    {}_params: {}Params,\n",
                instance.name,
                pascal_case(&component.name)
            ));
        }
    }
    for bind in &bind_plans {
        output.push_str(&format!(
            "    {}: {},\n",
            bind.field_name,
            runtime_channel_type(bind, &selected_backend)
        ));
        output.push_str(&format!(
            "    {}: flowrt::IntrospectionChannelProbe,\n",
            bind.probe_field_name
        ));
    }
    output.push_str("}\n\n");

    output.push_str("impl App {\n");
    output.push_str(&emit_rust_app_new(
        contract,
        graph,
        &order,
        &bind_plans,
        &selected_backend,
    ));
    let step_emission = RustStepEmission {
        contract,
        graph,
        binds: &bind_plans,
        incoming_bind_index: &incoming_bind_index,
        outgoing_bind_indices: &outgoing_bind_indices,
        selected_backend: &selected_backend,
    };

    output.push_str(&emit_rust_app_step(
        &step_emission,
        &order,
        "step",
        TaskEmissionPhase::Scheduler,
    ));
    output.push_str(&emit_rust_app_step(
        &step_emission,
        &order,
        "step_startup",
        TaskEmissionPhase::Startup,
    ));
    output.push_str(&emit_rust_app_step(
        &step_emission,
        &order,
        "step_shutdown",
        TaskEmissionPhase::Shutdown,
    ));
    for process in &process_plans {
        output.push_str(&emit_rust_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}", process.method_suffix),
            TaskEmissionPhase::Scheduler,
        ));
        output.push_str(&emit_rust_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}_startup", process.method_suffix),
            TaskEmissionPhase::Startup,
        ));
        output.push_str(&emit_rust_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}_shutdown", process.method_suffix),
            TaskEmissionPhase::Shutdown,
        ));
    }
    output.push_str(&emit_rust_app_run(contract, &order, &bind_plans));
    output.push_str(&emit_rust_app_run_process_dispatch(&process_plans));
    for process in &process_plans {
        let step_function_name = format!("step_process_{}", process.method_suffix);
        let startup_function_name = format!("step_process_{}_startup", process.method_suffix);
        let shutdown_function_name = format!("step_process_{}_shutdown", process.method_suffix);
        output.push_str(&emit_rust_app_run_function(
            contract,
            &format!("run_process_{}", process.method_suffix),
            RustRunStepFunctions {
                scheduler: &step_function_name,
                startup: &startup_function_name,
                shutdown: &shutdown_function_name,
            },
            &process.instances,
            &bind_plans,
            &process.name,
            false,
        ));
    }
    output.push_str("}\n\n");
    output.push_str(
        "pub fn backend() -> Box<dyn flowrt::Backend> {\n    match SELECTED_BACKEND {\n        \"inproc\" => Box::new(flowrt::inproc_backend()),\n        \"iox2\" => Box::new(flowrt::iox2_backend()),\n        \"zenoh\" => Box::new(flowrt::zenoh_backend()),\n        other => panic!(\"unsupported generated FlowRT backend `{other}`\"),\n    }\n}\n\npub fn run(run_ticks: Option<usize>) -> flowrt::Status {\n    let backend = backend();\n    user::build_app().run(backend.as_ref(), run_ticks)\n}\n\npub fn run_process(process: &str, run_ticks: Option<usize>) -> flowrt::Status {\n    let backend = backend();\n    user::build_app().run_process(backend.as_ref(), process, run_ticks)\n}\n",
    );
    output
}

pub(crate) fn selected_backend_name(contract: &ContractIr) -> String {
    contract
        .profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| contract.profiles.first())
        .map(|profile| profile.backend.0.clone())
        .unwrap_or_else(|| "inproc".to_string())
}

pub(crate) fn rust_string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn emit_rust_lib(include_runtime_shell: bool) -> String {
    let mut output = managed_header();
    if include_runtime_shell {
        output.push_str(
            "\npub(crate) mod selfdesc;\npub mod components;\npub mod messages;\npub mod runtime_shell;\npub mod supervisor;\n#[path = \"../../../src/rust/mod.rs\"]\npub mod user;\n\npub use runtime_shell::{run, run_process, App};\n",
        );
    } else {
        output.push_str("\npub(crate) mod selfdesc;\npub mod supervisor;\n");
    }
    output
}

fn emit_rust_main() -> String {
    let mut output = managed_header();
    output.push_str(
        "\nfn main() {\n    let mut args = std::env::args().skip(1);\n    let mut process = None;\n    let mut run_ticks = None;\n    while let Some(arg) = args.next() {\n        match arg.as_str() {\n            \"--process\" => process = args.next(),\n            \"--flowrt-run-ticks\" => {\n                let Some(raw_ticks) = args.next() else {\n                    eprintln!(\"missing value for --flowrt-run-ticks\");\n                    std::process::exit(2);\n                };\n                match raw_ticks.parse::<usize>() {\n                    Ok(ticks) if ticks > 0 => run_ticks = Some(ticks),\n                    _ => {\n                        eprintln!(\"invalid value for --flowrt-run-ticks: {raw_ticks}\");\n                        std::process::exit(2);\n                    }\n                }\n            }\n            _ => {\n                eprintln!(\"unknown FlowRT app argument: {arg}\");\n                std::process::exit(2);\n            }\n        }\n    }\n\n    let status = match process.as_deref() {\n        Some(process) => flowrt_app::runtime_shell::run_process(process, run_ticks),\n        None => flowrt_app::runtime_shell::run(run_ticks),\n    };\n    let code = match status {\n        flowrt::Status::Ok => 0,\n        _ => 1,\n    };\n    std::process::exit(code);\n}\n",
    );
    output
}

#[derive(Debug, Clone)]
struct BindRuntimePlan {
    index: usize,
    field_name: String,
    probe_field_name: String,
    channel: ChannelKind,
    overflow: IrOverflowPolicy,
    stale: IrStalePolicy,
    max_age_ms: Option<u64>,
    depth: Option<u32>,
    source_type: TypeExpr,
    source_uses_variable_frame: bool,
    source_instance: String,
    source_port: String,
    target_instance: String,
    target_port: String,
}

#[derive(Debug, Clone)]
struct ProcessRuntimePlan<'a> {
    name: String,
    method_suffix: String,
    instances: Vec<&'a InstanceIr>,
}

fn process_runtime_plans<'a>(order: &[&'a InstanceIr]) -> Vec<ProcessRuntimePlan<'a>> {
    let mut by_process = BTreeMap::<String, Vec<&'a InstanceIr>>::new();
    for &instance in order {
        by_process
            .entry(
                instance
                    .process
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
            )
            .or_default()
            .push(instance);
    }

    let mut used_suffixes = BTreeSet::new();
    by_process
        .into_iter()
        .enumerate()
        .map(|(index, (name, instances))| {
            let base = snake_identifier(&name);
            let mut suffix = base.clone();
            if !used_suffixes.insert(suffix.clone()) {
                suffix = format!("{}_{}", base, index);
                while !used_suffixes.insert(suffix.clone()) {
                    suffix.push('_');
                }
            }
            ProcessRuntimePlan {
                name,
                method_suffix: suffix,
                instances,
            }
        })
        .collect()
}

fn bind_runtime_plans(contract: &ContractIr, graph: &GraphIr) -> Vec<BindRuntimePlan> {
    graph
        .binds
        .iter()
        .enumerate()
        .map(|(index, bind)| {
            let source_instance = instance_by_name(graph, &bind.from.instance.name);
            let source_component = component_by_name(contract, &source_instance.component.name);
            let source_port = port_by_name(&source_component.outputs, &bind.from.port);
            BindRuntimePlan {
                index,
                field_name: format!("bind_{index}"),
                probe_field_name: format!("introspection_probe_bind_{index}"),
                channel: bind.channel,
                overflow: bind.overflow,
                stale: bind.stale,
                max_age_ms: bind.max_age_ms,
                depth: bind.depth,
                source_type: source_port.ty.clone(),
                source_uses_variable_frame: type_contains_variable_data(contract, &source_port.ty),
                source_instance: source_instance.name.clone(),
                source_port: bind.from.port.clone(),
                target_instance: bind.to.instance.name.clone(),
                target_port: bind.to.port.clone(),
            }
        })
        .collect()
}

fn incoming_bind_index_map(plans: &[BindRuntimePlan]) -> BTreeMap<(String, String), usize> {
    plans
        .iter()
        .map(|plan| {
            (
                (plan.target_instance.clone(), plan.target_port.clone()),
                plan.index,
            )
        })
        .collect()
}

fn outgoing_bind_indices_map(plans: &[BindRuntimePlan]) -> BTreeMap<(String, String), Vec<usize>> {
    let mut map = BTreeMap::new();
    for plan in plans {
        map.entry((plan.source_instance.clone(), plan.source_port.clone()))
            .or_insert_with(Vec::new)
            .push(plan.index);
    }
    map
}

fn active_binds_for_instances<'a>(
    binds: &'a [BindRuntimePlan],
    order: &[&InstanceIr],
) -> Vec<&'a BindRuntimePlan> {
    let active_instances = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();
    binds
        .iter()
        .filter(|bind| {
            active_instances.contains(bind.source_instance.as_str())
                || active_instances.contains(bind.target_instance.as_str())
        })
        .collect()
}

fn runtime_channel_name(bind: &BindRuntimePlan) -> String {
    format!(
        "{}.{}_to_{}.{}",
        bind.source_instance, bind.source_port, bind.target_instance, bind.target_port
    )
}

fn runtime_channel_message_type(bind: &BindRuntimePlan) -> String {
    bind.source_type.canonical_syntax()
}

fn runtime_channel_probe_capacity(contract: &ContractIr, bind: &BindRuntimePlan) -> usize {
    match &bind.source_type {
        TypeExpr::Named { name } if bind.source_uses_variable_frame => {
            frame_max_size_for_type(contract, type_by_name(contract, name))
        }
        TypeExpr::Named { name } => fixed_message_abi_size(contract, name)
            .unwrap_or_else(|| rust_wire_size(contract, &bind.source_type)),
        other => rust_wire_size(contract, other),
    }
}

fn fixed_message_abi_size(contract: &ContractIr, type_name: &str) -> Option<usize> {
    fixed_message_abi_expectations(contract)
        .ok()?
        .into_iter()
        .find(|expectation| expectation.type_name == type_name)
        .map(|expectation| expectation.size_bytes)
}

fn runtime_param_name(instance: &InstanceIr, param: &ParamIr) -> String {
    format!("{}.{}", instance.name, param.name)
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

fn emit_rust_introspection_helpers(
    include_channel_helpers: bool,
    include_param_decode: bool,
) -> String {
    let mut output = String::new();
    if include_channel_helpers {
        output.push_str(
            r#"fn register_introspection_channel(
    state: &flowrt::IntrospectionState,
    name: &'static str,
    message_type: &'static str,
    max_payload_len: usize,
) -> flowrt::IntrospectionChannelProbe {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.register_channel_with_probe_capacity(name, message_type, Some(max_payload_len));
        state.channel_probe(name).unwrap_or_default()
    }))
    .unwrap_or_default()
}

"#,
        );
    }
    if include_param_decode {
        output.push_str(
            r#"fn decode_flowrt_param_value<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, serde_json::Error> {
    serde_json::from_value(value)
}

"#,
        );
    }
    if include_channel_helpers {
        output.push_str(
            r#"#[allow(dead_code)]
fn record_introspection_publish_copy<T: Copy>(
    probe: &flowrt::IntrospectionChannelProbe,
    value: &T,
    published_at_ms: u64,
) {
    probe.record_publish_event();
    if !probe.enabled() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let payload = unsafe {
            std::slice::from_raw_parts(
                (value as *const T).cast::<u8>(),
                std::mem::size_of::<T>(),
            )
        };
        probe.try_record_bytes(payload, Some(published_at_ms));
    }));
}

#[allow(dead_code)]
fn record_introspection_publish_frame<T: flowrt::FrameCodec>(
    probe: &flowrt::IntrospectionChannelProbe,
    value: &T,
    published_at_ms: u64,
) {
    probe.record_publish_event();
    if !probe.enabled() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(payload) = value.to_frame_vec() {
            probe.try_record_bytes(&payload, Some(published_at_ms));
        }
    }));
}

"#,
        );
    }
    output
}

fn contract_has_runtime_params_for_language(contract: &ContractIr, language: LanguageKind) -> bool {
    contract.components.iter().any(|component| {
        component.language == language
            && component
                .params
                .iter()
                .any(|param| param.update == ParamUpdatePolicy::OnTick)
    })
}

fn emit_rust_introspection_channel_registration(
    contract: &ContractIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    for bind in active_binds_for_instances(binds, order) {
        output.push_str(&format!(
            "        self.{probe} = register_introspection_channel(&introspection_state, {}, {}, {});\n",
            rust_string_literal(&runtime_channel_name(bind)),
            rust_string_literal(&runtime_channel_message_type(bind)),
            runtime_channel_probe_capacity(contract, bind),
            probe = bind.probe_field_name
        ));
    }
    output
}

fn emit_rust_app_new(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    selected_backend: &str,
) -> String {
    let mut output = String::new();
    output.push_str("    pub fn new(\n");
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        output.push_str(&format!(
            "        {}: Box<dyn {}>,\n",
            instance.name,
            pascal_case(&component.name)
        ));
    }
    output.push_str("    ) -> Self {\n        Self {\n");
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        output.push_str(&format!("            {},\n", instance.name));
        if !component.params.is_empty() {
            output.push_str(&format!(
                "            {}_params: {},\n",
                instance.name,
                rust_params_initializer(component, instance)
            ));
        }
    }
    for bind in binds {
        output.push_str(&format!(
            "            {}: {},\n",
            bind.field_name,
            runtime_channel_initializer(contract, graph, bind, selected_backend)
        ));
        output.push_str(&format!(
            "            {}: flowrt::IntrospectionChannelProbe::default(),\n",
            bind.probe_field_name
        ));
    }
    output.push_str("        }\n    }\n");
    output
}

struct RustStepEmission<'a> {
    contract: &'a ContractIr,
    graph: &'a GraphIr,
    binds: &'a [BindRuntimePlan],
    incoming_bind_index: &'a BTreeMap<(String, String), usize>,
    outgoing_bind_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    selected_backend: &'a str,
}

fn emit_rust_app_step(
    emission: &RustStepEmission<'_>,
    order: &[&InstanceIr],
    function_name: &str,
    phase: TaskEmissionPhase,
) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "    fn {function_name}(\n        &mut self,\n        tick: usize,\n        _tick_context: &mut flowrt::Context,\n        introspection_state: &flowrt::IntrospectionState,\n    ) -> flowrt::Status {{\n",
    ));
    output.push_str("        let _ = tick;\n");
    output.push_str("        let _ = introspection_state;\n");
    if runtime_step_uses_tick_time(emission.binds, emission.selected_backend) {
        output.push_str("        let tick_time_ms = tick as u64;\n        let _ = tick_time_ms;\n");
    }

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
        let trigger_guard = on_message_trigger_guard(task, |input| input.to_string());

        for input in &component.inputs {
            if task_inputs.contains(input.name.as_str()) {
                let bind_index = emission
                    .incoming_bind_index
                    .get(&(instance.name.clone(), input.name.clone()))
                    .expect("validated graph must provide a bind for each task input");
                let bind = &emission.binds[*bind_index];
                output.push_str(&runtime_channel_read(
                    input,
                    bind,
                    emission.selected_backend,
                ));
                output.push_str(&runtime_stale_error_guard(input, bind));
            } else {
                output.push_str(&format!(
                    "        let {input} = flowrt::Latest::new(None, false);\n",
                    input = input.name
                ));
            }
        }

        if let Some(guard) = &trigger_guard {
            output.push_str(&format!("        if {guard} {{\n"));
        }

        if !component.params.is_empty() && phase == TaskEmissionPhase::Scheduler {
            output.push_str(&rust_apply_pending_params(
                instance,
                component,
                trigger_guard.is_some(),
            ));
        }

        if task.deadline_ms.is_some() {
            output.push_str(&format!(
                "{indent}let {name}_deadline_started_at = std::time::Instant::now();\n",
                indent = rust_step_indent(trigger_guard.is_some()),
                name = instance.name
            ));
        }

        for port in &component.outputs {
            output.push_str(&format!(
                "{indent}let mut {port} = flowrt::Output::<{ty}>::new();\n",
                indent = rust_step_indent(trigger_guard.is_some()),
                port = port.name,
                ty = rust_type(&port.ty)
            ));
        }

        let mut call_args = Vec::new();
        for input in &component.inputs {
            call_args.push(input.name.clone());
        }
        if !component.params.is_empty() {
            call_args.push(format!("&self.{}_params", instance.name));
        }
        for port in &component.outputs {
            call_args.push(format!("&mut {}", port.name));
        }
        output.push_str(&format!(
            "{indent}if self.{name}.on_tick({args}) != flowrt::Status::Ok {{\n{inner_indent}return flowrt::Status::Error;\n{indent}}}\n",
            indent = rust_step_indent(trigger_guard.is_some()),
            inner_indent = rust_nested_step_indent(trigger_guard.is_some()),
            name = instance.name,
            args = call_args.join(", ")
        ));

        if let Some(deadline_ms) = task.deadline_ms {
            output.push_str(&format!(
                "{indent}if {name}_deadline_started_at.elapsed() > std::time::Duration::from_millis({deadline_ms}) {{\n{inner_indent}return flowrt::Status::Error;\n{indent}}}\n",
                indent = rust_step_indent(trigger_guard.is_some()),
                inner_indent = rust_nested_step_indent(trigger_guard.is_some()),
                name = instance.name
            ));
        }

        for port in &component.outputs {
            if !task_outputs.contains(port.name.as_str()) {
                continue;
            }
            let outgoing = emission
                .outgoing_bind_indices
                .get(&(instance.name.clone(), port.name.clone()))
                .cloned()
                .unwrap_or_default();
            if outgoing.is_empty() {
                continue;
            }
            output.push_str(&format!(
                "{indent}if let Some(value) = {port}.as_ref().cloned() {{\n",
                indent = rust_step_indent(trigger_guard.is_some()),
                port = port.name
            ));
            for bind_index in outgoing {
                let bind = &emission.binds[bind_index];
                output.push_str(&indent_generated_block(
                    &runtime_channel_write(bind, emission.selected_backend),
                    trigger_guard.is_some(),
                ));
            }
            output.push_str(&format!(
                "{}}}\n",
                rust_step_indent(trigger_guard.is_some())
            ));
        }

        if trigger_guard.is_some() {
            output.push_str("        }\n");
        }
    }

    output.push_str("        flowrt::Status::Ok\n    }\n");
    output
}

fn emit_rust_app_run(
    contract: &ContractIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
) -> String {
    emit_rust_app_run_function(
        contract,
        "run",
        RustRunStepFunctions {
            scheduler: "step",
            startup: "step_startup",
            shutdown: "step_shutdown",
        },
        order,
        binds,
        "main",
        true,
    )
}

fn emit_rust_app_run_process_dispatch(processes: &[ProcessRuntimePlan<'_>]) -> String {
    let mut output = String::new();
    output.push_str(
        "    pub fn run_process(self, backend: &dyn flowrt::Backend, process: &str, run_ticks: Option<usize>) -> flowrt::Status {\n        match process {\n",
    );
    for process in processes {
        output.push_str(&format!(
            "            {} => self.run_process_{}(backend, run_ticks),\n",
            rust_string_literal(&process.name),
            process.method_suffix
        ));
    }
    output.push_str("            _ => flowrt::Status::Error,\n        }\n    }\n");
    output
}

#[derive(Debug, Clone, Copy)]
struct RustRunStepFunctions<'a> {
    scheduler: &'a str,
    startup: &'a str,
    shutdown: &'a str,
}

fn emit_rust_app_run_function(
    contract: &ContractIr,
    function_name: &str,
    steps: RustRunStepFunctions<'_>,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    process_name: &str,
    public: bool,
) -> String {
    let mut output = String::new();
    let visibility = if public { "pub " } else { "" };
    output.push_str(&format!(
        "    {visibility}fn {function_name}(mut self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {{\n        let mut lifecycle_context = flowrt::Context::default();\n        let mut status = flowrt::Status::Ok;\n",
    ));
    output.push_str("        let shutdown = flowrt::install_signal_shutdown_token();\n");
    output.push_str("        let introspection_state = flowrt::IntrospectionState::new();\n");
    output.push_str(
        "        introspection_state.set_self_description_json(selfdesc::self_description_json());\n",
    );
    output.push_str(&emit_rust_introspection_channel_registration(
        contract, order, binds,
    ));
    output.push_str(&emit_rust_introspection_param_registration(contract, order));
    output.push_str(&format!(
        "        let _introspection_server = flowrt::spawn_status_server(\n            flowrt::IntrospectionIdentity {{\n                self_description_hash: selfdesc::self_description_hash().to_string(),\n                package: {}.to_string(),\n                process: {}.to_string(),\n                runtime: \"rust\".to_string(),\n            }},\n            introspection_state.clone(),\n        )\n        .ok();\n",
        "PACKAGE_NAME",
        rust_string_literal(process_name)
    ));
    for instance in order {
        output.push_str(&format!(
            "        let mut {name}_initialized = false;\n        let mut {name}_started = false;\n",
            name = instance.name
        ));
    }
    for instance in order {
        output.push_str(&format!(
            "        if status == flowrt::Status::Ok {{\n            status = self.{name}.on_init(&mut lifecycle_context);\n            {name}_initialized = status == flowrt::Status::Ok;\n        }}\n",
            name = instance.name
        ));
    }
    for instance in order {
        output.push_str(&format!(
            "        if status == flowrt::Status::Ok && {name}_initialized {{\n            status = self.{name}.on_start(&mut lifecycle_context);\n            {name}_started = status == flowrt::Status::Ok;\n        }}\n",
            name = instance.name
        ));
    }
    output.push_str(&format!(
        "        if status == flowrt::Status::Ok {{\n            status = self.{startup_function_name}(0, &mut lifecycle_context, &introspection_state);\n        }}\n",
        startup_function_name = steps.startup
    ));
    output.push_str(&format!(
        "        let mut tick_base: usize = 0;\n        while status == flowrt::Status::Ok\n            && !shutdown.is_requested()\n            && run_ticks\n                .map(|limit| tick_base < limit)\n                .unwrap_or(true)\n        {{\n            status = backend.scheduler().run_ticks_until_shutdown(1, &shutdown, &mut |tick, tick_context| {{\n                introspection_state.record_tick();\n                self.{step_function_name}(tick_base + tick, tick_context, &introspection_state)\n            }});\n            tick_base += 1;\n        }}\n",
        step_function_name = steps.scheduler
    ));
    output.push_str(&format!(
        "        if status == flowrt::Status::Ok {{\n            status = self.{shutdown_function_name}(0, &mut lifecycle_context, &introspection_state);\n        }}\n",
        shutdown_function_name = steps.shutdown
    ));
    for instance in order.iter().rev() {
        output.push_str(&format!(
            "        if {name}_started {{\n            let stop_status = self.{name}.on_stop(&mut lifecycle_context);\n            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {{\n                status = flowrt::Status::Error;\n            }}\n        }}\n",
            name = instance.name
        ));
    }
    for instance in order.iter().rev() {
        output.push_str(&format!(
            "        if {name}_initialized {{\n            let shutdown_status = self.{name}.on_shutdown(&mut lifecycle_context);\n            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {{\n                status = flowrt::Status::Error;\n            }}\n        }}\n",
            name = instance.name
        ));
    }
    output.push_str("        status\n    }\n");
    output
}

fn runtime_step_uses_tick_time(binds: &[BindRuntimePlan], selected_backend: &str) -> bool {
    (!binds.is_empty() && matches!(selected_backend, "iox2" | "zenoh"))
        || binds
            .iter()
            .any(|bind| matches!(bind.channel, ChannelKind::Latest | ChannelKind::Fifo))
}

fn runtime_channel_type(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let ty = rust_type(&bind.source_type);
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

fn runtime_channel_initializer(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
    selected_backend: &str,
) -> String {
    if selected_backend == "iox2" {
        if bind.source_uses_variable_frame {
            return format!(
                "flowrt::iox2::Iox2FramePubSub::<{}, {}>::open_with_config({}, {}).expect(\"failed to open FlowRT iox2 frame channel\")",
                rust_type(&bind.source_type),
                iox2_frame_slot_type_for_expr(&bind.source_type),
                rust_string_literal(&iox2_service_name(contract, graph, bind)),
                iox2_channel_config_expr(bind),
            );
        }
        return format!(
            "flowrt::iox2::Iox2PubSub::open_with_config({}, {}).expect(\"failed to open FlowRT iox2 channel\")",
            rust_string_literal(&iox2_service_name(contract, graph, bind)),
            iox2_channel_config_expr(bind),
        );
    }
    if selected_backend == "zenoh" {
        return format!(
            "flowrt::zenoh::ZenohPubSub::open_with_config({}, {}).expect(\"failed to open FlowRT zenoh channel\")",
            rust_string_literal(&zenoh_key_expr(contract, graph, bind)),
            zenoh_channel_config_expr(bind),
        );
    }

    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::LatestChannel::with_stale_config({})",
            runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => runtime_fifo_channel_initializer(bind),
    }
}

fn zenoh_channel_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::zenoh::ZenohChannelConfig::latest().with_stale_config({})",
            runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => format!(
            "flowrt::zenoh::ZenohChannelConfig::fifo({}, {}).with_stale_config({})",
            bind.depth.unwrap_or(1),
            runtime_overflow_policy(bind.overflow),
            runtime_stale_config_expr(bind)
        ),
    }
}

fn runtime_fifo_channel_initializer(bind: &BindRuntimePlan) -> String {
    let depth = bind.depth.unwrap_or(1);
    let overflow = runtime_overflow_policy(bind.overflow);
    if bind.max_age_ms.is_none() && bind.stale == IrStalePolicy::Warn {
        return format!("flowrt::FifoChannel::new({depth}, {overflow})");
    }

    format!(
        "flowrt::FifoChannel::with_stale_config({}, {}, {})",
        depth,
        overflow,
        runtime_stale_config_expr(bind)
    )
}

fn iox2_channel_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config({})",
            runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => format!(
            "flowrt::iox2::Iox2ChannelConfig::fifo({}, {}).with_stale_config({})",
            bind.depth.unwrap_or(1),
            runtime_overflow_policy(bind.overflow),
            runtime_stale_config_expr(bind)
        ),
    }
}

fn runtime_stale_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.max_age_ms {
        Some(max_age_ms) => format!(
            "flowrt::StaleConfig::new(Some({max_age_ms}), {})",
            runtime_stale_policy(bind.stale)
        ),
        None => format!(
            "flowrt::StaleConfig::new(None, {})",
            runtime_stale_policy(bind.stale)
        ),
    }
}

fn runtime_channel_read(input: &PortIr, bind: &BindRuntimePlan, selected_backend: &str) -> String {
    if matches!(selected_backend, "iox2" | "zenoh") {
        return format!(
            "        let {input} = match self.{field}.receive_latest_at(tick_time_ms) {{\n            Ok(value) => value,\n            Err(_) => return flowrt::Status::Error,\n        }};\n",
            input = input.name,
            field = bind.field_name
        );
    }

    match bind.channel {
        ChannelKind::Latest => {
            format!(
                "        let {input} = self.{field}.view_at(tick_time_ms);\n",
                input = input.name,
                field = bind.field_name
            )
        }
        ChannelKind::Fifo => {
            format!(
                "        let {input}_read = self.{field}.pop_at(tick_time_ms);\n        let {input} = {input}_read.view();\n",
                input = input.name,
                field = bind.field_name
            )
        }
    }
}

fn runtime_stale_error_guard(input: &PortIr, bind: &BindRuntimePlan) -> String {
    if bind.stale != IrStalePolicy::Error {
        return String::new();
    }

    format!(
        "        if {input}.stale() {{\n            return flowrt::Status::Error;\n        }}\n",
        input = input.name
    )
}

fn runtime_introspection_publish_record(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let helper = if bind.source_uses_variable_frame || selected_backend == "zenoh" {
        "record_introspection_publish_frame"
    } else {
        "record_introspection_publish_copy"
    };
    format!(
        "            {helper}(&self.{probe}, &value, tick_time_ms);\n",
        probe = bind.probe_field_name
    )
}

fn runtime_channel_write(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let introspection_record = runtime_introspection_publish_record(bind, selected_backend);
    if matches!(selected_backend, "iox2" | "zenoh") {
        return format!(
            "            if self.{field}.publish_at(value.clone(), tick_time_ms).is_err() {{\n                return flowrt::Status::Error;\n            }}\n{introspection_record}",
            field = bind.field_name
        );
    }

    match bind.channel {
        ChannelKind::Latest => {
            format!(
                "            self.{field}.publish_at(value.clone(), tick_time_ms);\n{introspection_record}",
                field = bind.field_name
            )
        }
        ChannelKind::Fifo => {
            format!(
                "            match self.{field}.push_at(value.clone(), tick_time_ms) {{\n                Ok(flowrt::ChannelWriteOutcome::Accepted) | Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {{\n{introspection_record}                }}\n                Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {{}},\n                Ok(flowrt::ChannelWriteOutcome::Backpressured) => return flowrt::Status::Retry,\n                Err(flowrt::ChannelError::Overflow) => return flowrt::Status::Error,\n            }}\n",
                field = bind.field_name
            )
        }
    }
}

fn iox2_service_name(contract: &ContractIr, graph: &GraphIr, bind: &BindRuntimePlan) -> String {
    iox2_service_name_from_parts(
        &contract.package.name,
        &graph.name,
        bind.index,
        &bind.source_instance,
        &bind.source_port,
        &bind.target_instance,
        &bind.target_port,
    )
}

fn zenoh_key_expr(contract: &ContractIr, graph: &GraphIr, bind: &BindRuntimePlan) -> String {
    zenoh_key_expr_from_parts(
        "flowrt",
        &contract.package.name,
        &selected_profile_name(contract),
        &graph.name,
        bind.index,
        &bind.source_instance,
        &bind.source_port,
        &bind.target_instance,
        &bind.target_port,
    )
}

pub(crate) fn zenoh_key_expr_for_edge(
    contract: &ContractIr,
    graph: &GraphIr,
    index: usize,
    bind: &ChannelEdgeIr,
) -> String {
    zenoh_key_expr_from_parts(
        "flowrt",
        &contract.package.name,
        &selected_profile_name(contract),
        &graph.name,
        index,
        &bind.from.instance.name,
        &bind.from.port,
        &bind.to.instance.name,
        &bind.to.port,
    )
}

#[allow(clippy::too_many_arguments)]
fn zenoh_key_expr_from_parts(
    namespace: &str,
    package: &str,
    profile: &str,
    graph: &str,
    index: usize,
    source_instance: &str,
    source_port: &str,
    target_instance: &str,
    target_port: &str,
) -> String {
    format!(
        "{}/{}/{}/{}/bind_{}/{}_{}_to_{}_{}",
        flowrt_path_part(namespace),
        flowrt_path_part(package),
        flowrt_path_part(profile),
        flowrt_path_part(graph),
        index,
        flowrt_path_part(source_instance),
        flowrt_path_part(source_port),
        flowrt_path_part(target_instance),
        flowrt_path_part(target_port),
    )
}

pub(crate) fn iox2_service_name_for_edge(
    contract: &ContractIr,
    graph: &GraphIr,
    index: usize,
    bind: &ChannelEdgeIr,
) -> String {
    iox2_service_name_from_parts(
        &contract.package.name,
        &graph.name,
        index,
        &bind.from.instance.name,
        &bind.from.port,
        &bind.to.instance.name,
        &bind.to.port,
    )
}

fn selected_profile_name(contract: &ContractIr) -> String {
    contract
        .profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| contract.profiles.first())
        .map(|profile| profile.name.clone())
        .unwrap_or_else(|| "default".to_string())
}

fn iox2_service_name_from_parts(
    package: &str,
    graph: &str,
    index: usize,
    source_instance: &str,
    source_port: &str,
    target_instance: &str,
    target_port: &str,
) -> String {
    format!(
        "FlowRT/{}/{}/bind_{}/{}_{}_to_{}_{}",
        flowrt_path_part(package),
        flowrt_path_part(graph),
        index,
        flowrt_path_part(source_instance),
        flowrt_path_part(source_port),
        flowrt_path_part(target_instance),
        flowrt_path_part(target_port),
    )
}

fn flowrt_path_part(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch);
        } else if !output.ends_with('_') {
            output.push('_');
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    if output.is_empty() {
        "unnamed".to_string()
    } else {
        output
    }
}

fn runtime_overflow_policy(policy: IrOverflowPolicy) -> &'static str {
    match policy {
        IrOverflowPolicy::DropOldest => "flowrt::OverflowPolicy::DropOldest",
        IrOverflowPolicy::DropNewest => "flowrt::OverflowPolicy::DropNewest",
        IrOverflowPolicy::Error => "flowrt::OverflowPolicy::Error",
        IrOverflowPolicy::Block => "flowrt::OverflowPolicy::Block",
    }
}

fn runtime_stale_policy(policy: IrStalePolicy) -> &'static str {
    match policy {
        IrStalePolicy::Warn => "flowrt::StalePolicy::Warn",
        IrStalePolicy::Drop => "flowrt::StalePolicy::Drop",
        IrStalePolicy::HoldLast => "flowrt::StalePolicy::HoldLast",
        IrStalePolicy::Error => "flowrt::StalePolicy::Error",
    }
}

fn instance_by_name<'a>(graph: &'a GraphIr, name: &str) -> &'a InstanceIr {
    graph
        .instances
        .iter()
        .find(|instance| instance.name == name)
        .expect("validated graph must reference known instances")
}

fn port_by_name<'a>(ports: &'a [PortIr], name: &str) -> &'a PortIr {
    ports
        .iter()
        .find(|port| port.name == name)
        .expect("validated component must contain referenced port")
}

pub(crate) fn type_by_name<'a>(contract: &'a ContractIr, name: &str) -> &'a TypeIr {
    contract
        .types
        .iter()
        .find(|ty| ty.name == name)
        .expect("normalized contract must reference known message types")
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

fn rust_callback_args(component: &ComponentIr) -> Vec<String> {
    let mut args = Vec::new();
    for input in &component.inputs {
        args.push(format!(
            "{}: flowrt::Latest<'_, {}>",
            input.name,
            rust_type(&input.ty)
        ));
    }
    if !component.params.is_empty() {
        args.push(format!("params: &{}Params", pascal_case(&component.name)));
    }
    for output in &component.outputs {
        args.push(format!(
            "{}: &mut flowrt::Output<{}>",
            output.name,
            rust_type(&output.ty)
        ));
    }
    args
}

fn rust_tick_signature(component: &ComponentIr) -> String {
    let args = rust_callback_args(component);
    let doc = rust_tick_doc(component);
    if args.is_empty() {
        format!("{doc}    fn on_tick(&mut self) -> flowrt::Status;\n")
    } else {
        let joined = args
            .iter()
            .map(|arg| format!("        {arg}"))
            .collect::<Vec<_>>()
            .join(",\n");
        format!("{doc}    fn on_tick(\n        &mut self,\n{joined},\n    ) -> flowrt::Status;\n")
    }
}

fn rust_component_trait_doc(component: &ComponentIr) -> String {
    format!(
        "/// `{}` 组件的 Rust 用户实现 trait。\n///\n/// 用户代码实现该 trait 并交给 FlowRT 管理的 runtime shell。接口只暴露组件算法所需的生命周期、输入视图和输出句柄，不暴露具体 backend API。\n",
        component.name
    )
}

fn rust_lifecycle_doc(brief: &str) -> String {
    format!(
        "    /// {brief}。\n    ///\n    /// `context` 是 runtime 上下文；v0.1 暂不暴露资源句柄，后续可承载 clock、logger 和参数快照。\n    /// 返回本次生命周期步骤的 FlowRT 执行状态。\n"
    )
}

fn rust_tick_doc(component: &ComponentIr) -> String {
    let mut output = format!(
        "    /// 执行一次 `{}` 组件调度回调。\n    ///\n    /// runtime shell 按 Contract IR 中的 task 和 dataflow 顺序调用该方法。输入使用 latest snapshot 视图，输出通过 `flowrt::Output<T>` 写入，本方法不得保存输入引用到回调之外。\n",
        component.name
    );
    if !component.inputs.is_empty() || !component.outputs.is_empty() {
        output.push_str("    ///\n");
    }
    for input in &component.inputs {
        output.push_str(&format!(
            "    /// - `{}`: latest snapshot 输入视图。\n",
            input.name
        ));
    }
    for output_port in &component.outputs {
        output.push_str(&format!(
            "    /// - `{}`: 输出端口写入句柄。\n",
            output_port.name
        ));
    }
    output.push_str("    /// 返回本次回调的 FlowRT 执行状态。\n");
    output
}

fn rust_params_struct(component: &ComponentIr) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "#[derive(Clone, Debug, PartialEq)]\npub struct {}Params {{\n",
        pascal_case(&component.name)
    ));
    for param in &component.params {
        output.push_str(&format!(
            "    pub {}: {},\n",
            param.name,
            rust_param_type(param.ty)
        ));
    }
    output.push_str("}\n\n");
    output
}

fn rust_params_initializer(component: &ComponentIr, instance: &InstanceIr) -> String {
    let mut output = format!("{}Params {{", pascal_case(&component.name));
    for param in &component.params {
        let value = param_value_for_instance(instance, param);
        output.push_str(&format!(
            "\n                {}: {},",
            param.name,
            rust_param_literal(param, value)
        ));
    }
    output.push_str("\n            }");
    output
}

fn rust_params_update_signature(component: &ComponentIr) -> String {
    if component.params.is_empty() {
        return String::new();
    }
    let params_ty = format!("{}Params", pascal_case(&component.name));
    format!(
        "    /// 参数 pending 值在 tick 边界通过校验后调用。\n    ///\n    /// 返回 `Ok` 后 shell 才会提交新参数。\n    fn on_params_update(\n        &mut self,\n        _old: &{params_ty},\n        _new: &{params_ty},\n        _context: &mut flowrt::Context,\n    ) -> flowrt::Status {{\n        flowrt::Status::ok()\n    }}\n\n"
    )
}

fn emit_rust_introspection_param_registration(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        for param in &component.params {
            output.push_str(&format!(
                "        introspection_state.register_param(flowrt::IntrospectionParamSchema {{\n            name: {}.to_string(),\n            ty: {}.to_string(),\n            update: {}.to_string(),\n            current: {},\n            min: {},\n            max: {},\n            choices: {},\n        }});\n",
                rust_string_literal(&runtime_param_name(instance, param)),
                rust_string_literal(param_type_name(param.ty)),
                rust_string_literal(param_update_name(param.update)),
                param_json_value_literal(param_value_for_instance(instance, param)),
                rust_optional_param_json_value(param.min.as_ref()),
                rust_optional_param_json_value(param.max.as_ref()),
                rust_param_json_vec(&param.choices),
            ));
        }
    }
    output
}

fn rust_optional_param_json_value(value: Option<&ParamValue>) -> String {
    match value {
        Some(value) => format!("Some({})", param_json_value_literal(value)),
        None => "None".to_string(),
    }
}

fn rust_param_json_vec(values: &[ParamValue]) -> String {
    if values.is_empty() {
        return "Vec::new()".to_string();
    }
    let values = values
        .iter()
        .map(param_json_value_literal)
        .collect::<Vec<_>>()
        .join(", ");
    format!("vec![{values}]")
}

fn rust_apply_pending_params(
    instance: &InstanceIr,
    component: &ComponentIr,
    nested: bool,
) -> String {
    let mut output = String::new();
    let indent = rust_step_indent(nested);
    let inner_indent = rust_nested_step_indent(nested);
    let deep_indent = if nested {
        "                    "
    } else {
        "                "
    };
    for param in &component.params {
        if param.update != ParamUpdatePolicy::OnTick {
            continue;
        }
        let runtime_name = runtime_param_name(instance, param);
        let pending = format!("{}_{}_pending", instance.name, param.name);
        let next = format!("{}_{}_next_params", instance.name, param.name);
        output.push_str(&format!(
            "{indent}if let Some({pending}) = introspection_state.take_pending_param({}) {{\n",
            rust_string_literal(&runtime_name)
        ));
        output.push_str(&format!(
            "{inner_indent}let mut {next} = self.{}_params.clone();\n",
            instance.name
        ));
        output.push_str(&format!(
            "{inner_indent}{next}.{field} = match decode_flowrt_param_value::<{}>({pending}.clone()) {{\n{deep_indent}Ok(value) => value,\n{deep_indent}Err(_) => return flowrt::Status::Error,\n{inner_indent}}};\n",
            rust_param_type(param.ty),
            field = param.name
        ));
        output.push_str(&format!(
            "{inner_indent}if self.{instance}.on_params_update(&self.{instance}_params, &{next}, _tick_context) != flowrt::Status::Ok {{\n{deep_indent}return flowrt::Status::Error;\n{inner_indent}}}\n",
            instance = instance.name
        ));
        output.push_str(&format!(
            "{inner_indent}self.{instance}_params = {next};\n{inner_indent}introspection_state.record_param_applied({}, {pending});\n",
            rust_string_literal(&runtime_name),
            instance = instance.name
        ));
        output.push_str(&format!("{indent}}}\n"));
    }
    output
}

pub(crate) fn snake_identifier(name: &str) -> String {
    let mut output = String::new();
    let mut previous_was_separator = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && !previous_was_separator && !output.ends_with('_') {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !output.ends_with('_') {
            output.push('_');
            previous_was_separator = true;
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    if output.is_empty() {
        "message".to_string()
    } else {
        output
    }
}

fn pascal_case(name: &str) -> String {
    let mut output = String::new();
    for part in name.split('_').filter(|part| !part.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            output.extend(first.to_uppercase());
            output.push_str(chars.as_str());
        }
    }
    output
}

pub(crate) fn sanitize_package_name(name: &str) -> String {
    let mut output = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "flowrt_app".to_string()
    } else {
        output
    }
}

pub(crate) fn managed_header() -> String {
    "// FlowRT 管理产物。不要手工修改。\n".to_string()
}

fn component_by_name<'a>(contract: &'a ContractIr, name: &str) -> &'a ComponentIr {
    contract
        .components
        .iter()
        .find(|component| component.name == name)
        .expect("normalized contract must reference known components")
}

fn task_for_instance<'a>(graph: &'a GraphIr, instance: &InstanceIr) -> Option<&'a TaskIr> {
    graph
        .tasks
        .iter()
        .find(|task| task.instance.id == instance.id)
}

fn topo_order_instances(graph: &GraphIr) -> Vec<&InstanceIr> {
    let mut indegree: BTreeMap<String, usize> = graph
        .instances
        .iter()
        .map(|instance| (instance.name.clone(), 0usize))
        .collect();
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for bind in &graph.binds {
        let source = bind.from.instance.name.clone();
        let target = bind.to.instance.name.clone();
        if source == target {
            continue;
        }
        let inserted = edges
            .entry(source.clone())
            .or_default()
            .insert(target.clone());
        if inserted {
            *indegree.entry(target).or_default() += 1;
        }
    }

    let mut ready: BTreeSet<String> = indegree
        .iter()
        .filter_map(|(name, degree)| (*degree == 0).then_some(name.clone()))
        .collect();
    let mut order = Vec::with_capacity(graph.instances.len());

    while let Some(name) = ready.iter().next().cloned() {
        ready.remove(&name);
        order.push(name.clone());

        if let Some(next) = edges.get(&name) {
            for target in next {
                let entry = indegree
                    .get_mut(target)
                    .expect("all graph instances have an indegree entry");
                *entry -= 1;
                if *entry == 0 {
                    ready.insert(target.clone());
                }
            }
        }
    }

    assert_eq!(
        order.len(),
        graph.instances.len(),
        "validated graph must be acyclic"
    );

    order
        .iter()
        .map(|name| {
            graph
                .instances
                .iter()
                .find(|instance| &instance.name == name)
                .expect("ordered instance must exist")
        })
        .collect()
}

fn topo_order_instances_for_language<'a>(
    contract: &ContractIr,
    graph: &'a GraphIr,
    language: LanguageKind,
) -> Vec<&'a InstanceIr> {
    topo_order_instances(graph)
        .into_iter()
        .filter(|instance| {
            component_by_name(contract, &instance.component.name).language == language
        })
        .collect()
}

#[allow(dead_code)]
fn _port_type(port: &PortIr) -> &TypeExpr {
    &port.ty
}

#[allow(dead_code)]
fn _field_type(field: &FieldIr) -> &TypeExpr {
    &field.ty
}

#[cfg(test)]
mod tests;
