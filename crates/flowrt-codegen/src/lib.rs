//! FlowRT 管理应用产物的生成入口。
//!
//! 本 crate 只从 Contract IR 生成 glue：消息类型、组件接口、runtime shell、启动配置和构建文件。
//! 生成内容必须位于用户项目可见的 `flowrt/` 目录下，并且不得承载用户业务逻辑。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use flowrt_ir::{
    ChannelEdgeIr, ChannelKind, ComponentIr, ContractIr, FieldIr, GraphIr, InstanceIr,
    LanguageKind, OverflowPolicy as IrOverflowPolicy, ParamIr, ParamType, ParamUpdatePolicy,
    ParamValue, PortIr, StalePolicy as IrStalePolicy, TaskIr, TypeExpr, TypeIr,
};
use flowrt_validate::validate_contract;

mod build_files;
mod cpp_shell;
mod launch_manifest;
mod messages;
mod ros2_bridge;
mod runtime_plan;
mod selfdesc;
mod supervisor;

use build_files::{emit_cargo_manifest, emit_cmake};
pub(crate) use cpp_shell::cpp_string_literal;
use cpp_shell::{
    emit_cpp_components, emit_cpp_main, emit_cpp_runtime_shell, emit_cpp_runtime_shell_header,
};
use launch_manifest::emit_launch_manifest;
use messages::{
    emit_cpp_message_abi_tests, emit_cpp_messages, emit_rust_message_abi_tests, emit_rust_messages,
    fixed_message_abi_expectations, frame_header_size_for_expr, frame_header_size_for_type,
    frame_max_size_for_type, rust_type, rust_wire_size, type_contains_variable_data,
    variable_tail_max_size,
};
use ros2_bridge::emit_ros2_bridge_adapter;
use runtime_plan::{
    BindRuntimePlan, BridgeRuntimePlan, ProcessRuntimePlan, TaskEmissionPhase,
    active_binds_for_instances, bind_backend, bind_runtime_plans, bridge_runtime_plans,
    incoming_bind_index_map, indent_generated_block, indent_generated_block_levels,
    on_message_trigger_guard, outgoing_bind_indices_map, outgoing_bridge_indices_map,
    process_runtime_plans, runtime_channel_message_type, runtime_channel_name,
    runtime_channel_probe_capacity, runtime_param_name, rust_nested_step_indent, rust_step_indent,
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
    let has_cpp_components = has_language(contract, LanguageKind::Cpp);
    let has_ros2_bridge = contract_has_ros2_bridge(contract);
    let has_cpp = has_cpp_components || has_ros2_bridge;
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
        if has_cpp_components {
            artifacts.push(artifact(
                "cpp/include/flowrt_app/components.hpp",
                emit_cpp_components(contract),
            ));
            artifacts.push(artifact(
                "cpp/include/flowrt_app/runtime_shell.hpp",
                emit_cpp_runtime_shell_header(contract),
            ));
        }
        artifacts.push(artifact(
            "cpp/src/selfdesc.cpp",
            emit_cpp_selfdesc_source(contract),
        ));
        if has_cpp_components {
            artifacts.push(artifact(
                "cpp/src/runtime_shell.cpp",
                emit_cpp_runtime_shell(contract),
            ));
            artifacts.push(artifact("cpp/src/main.cpp", emit_cpp_main()));
        }
        if has_ros2_bridge {
            artifacts.push(artifact(
                "cpp/src/ros2_bridge.cpp",
                emit_ros2_bridge_adapter(contract),
            ));
        }
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

pub(crate) fn contract_has_ros2_bridge(contract: &ContractIr) -> bool {
    contract
        .graphs
        .iter()
        .any(|graph| !graph.ros2_bridges.is_empty())
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

pub(crate) fn param_type_name(ty: ParamType) -> &'static str {
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

pub(crate) fn param_update_name(update: ParamUpdatePolicy) -> &'static str {
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

pub(crate) fn param_json_literal(value: &ParamValue) -> String {
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

pub(crate) fn float_literal(value: f64) -> String {
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
        output.push_str(&format!(
            "pub trait {} {{\n",
            component_rust_name(component)
        ));
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
    let bridge_plans = bridge_runtime_plans(contract, graph);
    let incoming_bind_index = incoming_bind_index_map(&bind_plans);
    let outgoing_bind_indices = outgoing_bind_indices_map(&bind_plans);
    let outgoing_bridge_indices = outgoing_bridge_indices_map(&bridge_plans);
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
            component_rust_name(component)
        ));
        if !component.params.is_empty() {
            output.push_str(&format!(
                "    {}_params: {}Params,\n",
                instance.name,
                component_rust_name(component)
            ));
        }
    }
    for bind in &bind_plans {
        output.push_str(&format!(
            "    {}: {},\n",
            bind.field_name,
            runtime_channel_type(bind)
        ));
        output.push_str(&format!(
            "    {}: flowrt::IntrospectionChannelProbe,\n",
            bind.probe_field_name
        ));
    }
    for bridge in &bridge_plans {
        output.push_str(&format!(
            "    {}: {},\n",
            bridge.field_name,
            bridge_runtime_channel_type(bridge)
        ));
    }
    output.push_str("}\n\n");

    output.push_str("impl App {\n");
    output.push_str(&emit_rust_app_new(
        contract,
        graph,
        &order,
        &bind_plans,
        &bridge_plans,
    ));
    let step_emission = RustStepEmission {
        contract,
        graph,
        binds: &bind_plans,
        bridges: &bridge_plans,
        incoming_bind_index: &incoming_bind_index,
        outgoing_bind_indices: &outgoing_bind_indices,
        outgoing_bridge_indices: &outgoing_bridge_indices,
    };

    output.push_str(&emit_rust_app_step(
        &step_emission,
        &order,
        "step",
        TaskEmissionPhase::Scheduler,
        None,
    ));
    output.push_str(&emit_rust_app_step(
        &step_emission,
        &order,
        "step_startup",
        TaskEmissionPhase::Startup,
        None,
    ));
    output.push_str(&emit_rust_app_step(
        &step_emission,
        &order,
        "step_shutdown",
        TaskEmissionPhase::Shutdown,
        None,
    ));
    for task in scheduler_tasks_for_order(graph, &order) {
        output.push_str(&emit_rust_app_step(
            &step_emission,
            &order,
            &rust_task_step_function_name(task),
            TaskEmissionPhase::Scheduler,
            Some(task),
        ));
    }
    for process in &process_plans {
        output.push_str(&emit_rust_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}", process.method_suffix),
            TaskEmissionPhase::Scheduler,
            None,
        ));
        output.push_str(&emit_rust_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}_startup", process.method_suffix),
            TaskEmissionPhase::Startup,
            None,
        ));
        output.push_str(&emit_rust_app_step(
            &step_emission,
            &process.instances,
            &format!("step_process_{}_shutdown", process.method_suffix),
            TaskEmissionPhase::Shutdown,
            None,
        ));
        for task in scheduler_tasks_for_order(graph, &process.instances) {
            output.push_str(&emit_rust_app_step(
                &step_emission,
                &process.instances,
                &rust_process_task_step_function_name(process, task),
                TaskEmissionPhase::Scheduler,
                Some(task),
            ));
        }
    }
    output.push_str(&emit_rust_app_run(contract, graph, &order, &bind_plans));
    output.push_str(&emit_rust_app_run_process_dispatch(&process_plans));
    for process in &process_plans {
        let step_function_name = format!("step_process_{}", process.method_suffix);
        let startup_function_name = format!("step_process_{}_startup", process.method_suffix);
        let shutdown_function_name = format!("step_process_{}_shutdown", process.method_suffix);
        output.push_str(&emit_rust_app_run_function(RustRunFunctionEmission {
            contract,
            function_name: &format!("run_process_{}", process.method_suffix),
            steps: RustRunStepFunctions {
                scheduler: &step_function_name,
                startup: &startup_function_name,
                shutdown: &shutdown_function_name,
            },
            order: &process.instances,
            binds: &bind_plans,
            graph,
            process: Some(process),
            process_name: &process.name,
            public: false,
        }));
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

pub(crate) fn selected_profile_worker_threads(contract: &ContractIr) -> u32 {
    contract
        .profiles
        .first()
        .map(|profile| profile.scheduler.worker_threads)
        .unwrap_or(1)
}

pub(crate) fn scheduler_tasks_for_order<'a>(
    graph: &'a GraphIr,
    order: &[&InstanceIr],
) -> Vec<&'a TaskIr> {
    let instances = order
        .iter()
        .map(|instance| instance.id.clone())
        .collect::<BTreeSet<_>>();
    graph
        .tasks
        .iter()
        .filter(|task| instances.contains(&task.instance.id))
        .filter(|task| {
            matches!(
                task.trigger,
                flowrt_ir::TriggerKind::Periodic | flowrt_ir::TriggerKind::OnMessage
            )
        })
        .collect()
}

fn task_lane_name(task: &TaskIr) -> String {
    task.lane
        .clone()
        .unwrap_or_else(|| format!("{}_serial", task.instance.name))
}

fn scheduler_lane_ids(tasks: &[&TaskIr]) -> BTreeMap<String, usize> {
    let mut lanes = BTreeMap::new();
    for task in tasks {
        let lane = task_lane_name(task);
        if !lanes.contains_key(&lane) {
            let next_id = lanes.len() + 1;
            lanes.insert(lane, next_id);
        }
    }
    lanes
}

fn rust_task_step_function_name(task: &TaskIr) -> String {
    format!(
        "step_task_{}_{}",
        snake_identifier(&task.instance.name),
        snake_identifier(&task.name)
    )
}

fn rust_process_task_step_function_name(process: &ProcessRuntimePlan<'_>, task: &TaskIr) -> String {
    format!(
        "step_process_{}_task_{}_{}",
        process.method_suffix,
        snake_identifier(&task.instance.name),
        snake_identifier(&task.name)
    )
}

fn task_seen_revision_name(bind: &BindRuntimePlan, task: &TaskIr) -> String {
    format!(
        "{}_seen_revision_for_{}_{}",
        bind.field_name,
        snake_identifier(&task.instance.name),
        snake_identifier(&task.name)
    )
}

fn input_binds_for_task<'a>(
    task: &TaskIr,
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

fn emit_rust_on_message_revision_state(tasks: &[&TaskIr], binds: &[BindRuntimePlan]) -> String {
    let mut output = String::new();
    for task in tasks
        .iter()
        .copied()
        .filter(|task| task.trigger == flowrt_ir::TriggerKind::OnMessage)
    {
        for bind in input_binds_for_task(task, binds) {
            output.push_str(&format!(
                "        let mut {seen}: u64 = 0;\n",
                seen = task_seen_revision_name(bind, task)
            ));
        }
    }
    output
}

fn emit_rust_on_message_wake_checks(tasks: &[&TaskIr], binds: &[BindRuntimePlan]) -> String {
    let mut output = String::new();
    for (index, task) in tasks.iter().enumerate() {
        if task.trigger != flowrt_ir::TriggerKind::OnMessage {
            continue;
        }
        let input_binds = input_binds_for_task(task, binds);
        if input_binds.is_empty() {
            continue;
        }
        for bind in &input_binds {
            if matches!(bind_backend(bind), "iox2" | "zenoh") {
                output.push_str(&format!(
                    "            let _ = self.{field}.receive_latest_at(tick_time_ms);\n",
                    field = bind.field_name
                ));
            }
        }
        let checks = input_binds
            .iter()
            .map(|bind| {
                let revision_changed = format!(
                    "self.{field}.revision() != {seen}",
                    field = bind.field_name,
                    seen = task_seen_revision_name(bind, task)
                );
                if bind.channel == ChannelKind::Fifo && bind_backend(bind) == "inproc" {
                    format!(
                        "({revision_changed} || !self.{field}.is_empty())",
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
        output.push_str(&format!("            if {} {{\n", checks.join(joiner)));
        for bind in &input_binds {
            output.push_str(&format!(
                "                {seen} = self.{field}.revision();\n",
                seen = task_seen_revision_name(bind, task),
                field = bind.field_name
            ));
        }
        output.push_str(&format!(
            "                scheduler.wake(flowrt::TaskId({}));\n                woke_on_message = true;\n            }}\n",
            index + 1
        ));
    }
    output
}

fn emit_rust_apply_pending_params_for_order(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if !component.params.is_empty() {
            output.push_str(&rust_apply_pending_params(
                instance,
                component,
                false,
                "&mut lifecycle_context",
            ));
        }
    }
    output
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
        "\nfn main() {\n    let mut args = std::env::args().skip(1);\n    let mut process = None;\n    let mut run_ticks = None;\n    while let Some(arg) = args.next() {\n        match arg.as_str() {\n            \"--process\" => process = args.next(),\n            \"--flowrt-run-ticks\" | \"--flowrt-run-steps\" => {\n                let Some(raw_ticks) = args.next() else {\n                    eprintln!(\"missing value for {arg}\");\n                    std::process::exit(2);\n                };\n                match raw_ticks.parse::<usize>() {\n                    Ok(ticks) if ticks > 0 => run_ticks = Some(ticks),\n                    _ => {\n                        eprintln!(\"invalid value for {arg}: {raw_ticks}\");\n                        std::process::exit(2);\n                    }\n                }\n            }\n            _ => {\n                eprintln!(\"unknown FlowRT app argument: {arg}\");\n                std::process::exit(2);\n            }\n        }\n    }\n\n    let status = match process.as_deref() {\n        Some(process) => flowrt_app::runtime_shell::run_process(process, run_ticks),\n        None => flowrt_app::runtime_shell::run(run_ticks),\n    };\n    let code = match status {\n        flowrt::Status::Ok => 0,\n        _ => 1,\n    };\n    std::process::exit(code);\n}\n",
    );
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
    max_payload_len: Option<usize>,
) -> flowrt::IntrospectionChannelProbe {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.register_channel_with_probe_capacity(name, message_type, max_payload_len);
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
            rust_optional_usize_literal(runtime_channel_probe_capacity(contract, bind)),
            probe = bind.probe_field_name
        ));
    }
    output
}

fn rust_optional_usize_literal(value: Option<usize>) -> String {
    value.map_or_else(|| "None".to_string(), |value| format!("Some({value})"))
}

fn emit_rust_app_new(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
) -> String {
    let mut output = String::new();
    output.push_str("    pub fn new(\n");
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        output.push_str(&format!(
            "        {}: Box<dyn {}>,\n",
            instance.name,
            component_rust_name(component)
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
            runtime_channel_initializer(contract, graph, bind)
        ));
        output.push_str(&format!(
            "            {}: flowrt::IntrospectionChannelProbe::default(),\n",
            bind.probe_field_name
        ));
    }
    for bridge in bridges {
        output.push_str(&format!(
            "            {}: {},\n",
            bridge.field_name,
            bridge_runtime_channel_initializer(contract, graph, bridge)
        ));
    }
    output.push_str("        }\n    }\n");
    output
}

struct RustStepEmission<'a> {
    contract: &'a ContractIr,
    graph: &'a GraphIr,
    binds: &'a [BindRuntimePlan],
    bridges: &'a [BridgeRuntimePlan],
    incoming_bind_index: &'a BTreeMap<(String, String), usize>,
    outgoing_bind_indices: &'a BTreeMap<(String, String), Vec<usize>>,
    outgoing_bridge_indices: &'a BTreeMap<(String, String), Vec<usize>>,
}

fn emit_rust_app_step(
    emission: &RustStepEmission<'_>,
    order: &[&InstanceIr],
    function_name: &str,
    phase: TaskEmissionPhase,
    task_filter: Option<&TaskIr>,
) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "    #[allow(dead_code)]\n    fn {function_name}(\n        &mut self,\n        tick: usize,\n        _tick_context: &mut flowrt::Context,\n        introspection_state: &flowrt::IntrospectionState,\n        scheduler_events: &flowrt::ScheduleWaiter,\n    ) -> flowrt::Status {{\n",
    ));
    output.push_str("        let _ = tick;\n");
    output.push_str("        let _ = introspection_state;\n");
    output.push_str("        let _ = scheduler_events;\n");
    if runtime_step_uses_tick_time(emission.binds, emission.bridges) {
        output.push_str("        let tick_time_ms = tick as u64;\n        let _ = tick_time_ms;\n");
    }

    for instance in order {
        let component = component_by_name(emission.contract, &instance.component.name);
        if task_filter.is_none()
            && !component.params.is_empty()
            && phase == TaskEmissionPhase::Scheduler
        {
            output.push_str(&rust_apply_pending_params(
                instance,
                component,
                false,
                "_tick_context",
            ));
        }
        for task in tasks_for_instance(emission.graph, instance) {
            if !phase.includes(task.trigger) {
                continue;
            }
            if task_filter.is_some_and(|filter| filter.id != task.id) {
                continue;
            }
            output.push_str("        {\n");
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
                    output.push_str(&indent_generated_block(
                        &runtime_channel_read(
                            input,
                            bind,
                            task.trigger == flowrt_ir::TriggerKind::OnMessage,
                        ),
                        true,
                    ));
                    output.push_str(&indent_generated_block(
                        &runtime_stale_error_guard(input, bind),
                        true,
                    ));
                } else {
                    output.push_str(&format!(
                        "            let {input} = flowrt::Latest::new(None, false);\n",
                        input = input.name
                    ));
                }
            }

            if let Some(guard) = &trigger_guard {
                output.push_str(&format!("            if {guard} {{\n"));
            }
            let body_indent = if trigger_guard.is_some() {
                "                "
            } else {
                "            "
            };
            let body_inner_indent = if trigger_guard.is_some() {
                "                    "
            } else {
                "                "
            };
            let write_indent_levels = if trigger_guard.is_some() { 2 } else { 1 };

            if task.deadline_ms.is_some() {
                output.push_str(&format!(
                    "{body_indent}let {name}_deadline_started_at = std::time::Instant::now();\n",
                    name = instance.name
                ));
            }

            for port in &component.outputs {
                output.push_str(&format!(
                    "{body_indent}let mut {port} = flowrt::Output::<{ty}>::new();\n",
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
                "{body_indent}match self.{name}.on_tick({args}) {{\n{body_inner_indent}flowrt::Status::Ok => {{}}\n{body_inner_indent}flowrt::Status::Retry => return flowrt::Status::Retry,\n{body_inner_indent}flowrt::Status::Error => return flowrt::Status::Error,\n{body_indent}}}\n",
                name = instance.name,
                args = call_args.join(", ")
            ));

            if let Some(deadline_ms) = task.deadline_ms {
                output.push_str(&format!(
                    "{body_indent}if {name}_deadline_started_at.elapsed() > std::time::Duration::from_millis({deadline_ms}) {{\n{body_inner_indent}return flowrt::Status::Error;\n{body_indent}}}\n",
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
                let bridge_outgoing = emission
                    .outgoing_bridge_indices
                    .get(&(instance.name.clone(), port.name.clone()))
                    .cloned()
                    .unwrap_or_default();
                if outgoing.is_empty() && bridge_outgoing.is_empty() {
                    continue;
                }
                output.push_str(&format!(
                    "{body_indent}if let Some(value) = {port}.as_ref().cloned() {{\n",
                    port = port.name
                ));
                for bind_index in outgoing {
                    let bind = &emission.binds[bind_index];
                    output.push_str(&indent_generated_block_levels(
                        &runtime_channel_write(bind),
                        write_indent_levels,
                    ));
                }
                for bridge_index in bridge_outgoing {
                    let bridge = &emission.bridges[bridge_index];
                    output.push_str(&indent_generated_block_levels(
                        &bridge_runtime_channel_write(bridge),
                        write_indent_levels,
                    ));
                }
                output.push_str(&format!("{body_indent}}}\n"));
            }

            if trigger_guard.is_some() {
                output.push_str("            }\n");
            }
            output.push_str("        }\n");
        }
    }

    output.push_str("        flowrt::Status::Ok\n    }\n");
    output
}

fn emit_rust_app_run(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
) -> String {
    emit_rust_app_run_function(RustRunFunctionEmission {
        contract,
        function_name: "run",
        steps: RustRunStepFunctions {
            scheduler: "step",
            startup: "step_startup",
            shutdown: "step_shutdown",
        },
        order,
        binds,
        graph,
        process: None,
        process_name: "main",
        public: true,
    })
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

struct RustRunFunctionEmission<'a> {
    contract: &'a ContractIr,
    function_name: &'a str,
    steps: RustRunStepFunctions<'a>,
    order: &'a [&'a InstanceIr],
    binds: &'a [BindRuntimePlan],
    graph: &'a GraphIr,
    process: Option<&'a ProcessRuntimePlan<'a>>,
    process_name: &'a str,
    public: bool,
}

fn emit_rust_app_run_function(emission: RustRunFunctionEmission<'_>) -> String {
    let mut output = String::new();
    let visibility = if emission.public { "pub " } else { "" };
    let function_name = emission.function_name;
    output.push_str(&format!(
        "    {visibility}fn {function_name}(mut self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {{\n        let mut lifecycle_context = flowrt::Context::default();\n        let mut status = flowrt::Status::Ok;\n",
    ));
    output.push_str("        let _ = backend;\n");
    output.push_str("        let shutdown = flowrt::install_signal_shutdown_token();\n");
    output.push_str("        let introspection_state = flowrt::IntrospectionState::new();\n");
    output.push_str("        let scheduler_events = flowrt::ScheduleWaiter::new();\n");
    output.push_str(&emit_rust_scheduler_event_registration(emission.binds));
    output.push_str(
        "        introspection_state.set_self_description_json(selfdesc::self_description_json());\n",
    );
    output.push_str(&emit_rust_introspection_channel_registration(
        emission.contract,
        emission.order,
        emission.binds,
    ));
    output.push_str(&emit_rust_introspection_param_registration(
        emission.contract,
        emission.order,
    ));
    output.push_str(&format!(
        "        let _introspection_server = flowrt::spawn_status_server(\n            flowrt::IntrospectionIdentity {{\n                self_description_hash: selfdesc::self_description_hash().to_string(),\n                package: {}.to_string(),\n                process: {}.to_string(),\n                runtime: \"rust\".to_string(),\n            }},\n            introspection_state.clone(),\n        )\n        .ok();\n",
        "PACKAGE_NAME",
        rust_string_literal(emission.process_name)
    ));
    for instance in emission.order {
        output.push_str(&format!(
            "        let mut {name}_initialized = false;\n        let mut {name}_started = false;\n",
            name = instance.name
        ));
    }
    for instance in emission.order {
        output.push_str(&format!(
            "        if status == flowrt::Status::Ok {{\n            status = self.{name}.on_init(&mut lifecycle_context);\n            {name}_initialized = status == flowrt::Status::Ok;\n        }}\n",
            name = instance.name
        ));
    }
    for instance in emission.order {
        output.push_str(&format!(
            "        if status == flowrt::Status::Ok && {name}_initialized {{\n            status = self.{name}.on_start(&mut lifecycle_context);\n            {name}_started = status == flowrt::Status::Ok;\n        }}\n",
            name = instance.name
        ));
    }
    output.push_str(&format!(
        "        if status == flowrt::Status::Ok {{\n            status = self.{startup_function_name}(0, &mut lifecycle_context, &introspection_state, &scheduler_events);\n        }}\n",
        startup_function_name = emission.steps.startup
    ));
    output.push_str(&emit_rust_scheduler_v2_loop(
        emission.contract,
        emission.graph,
        emission.order,
        emission.binds,
        emission.process,
        emission.steps.scheduler,
    ));
    output.push_str(&format!(
        "        if status == flowrt::Status::Ok {{\n            status = self.{shutdown_function_name}(0, &mut lifecycle_context, &introspection_state, &scheduler_events);\n        }}\n",
        shutdown_function_name = emission.steps.shutdown
    ));
    for instance in emission.order.iter().rev() {
        output.push_str(&format!(
            "        if {name}_started {{\n            let stop_status = self.{name}.on_stop(&mut lifecycle_context);\n            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {{\n                status = flowrt::Status::Error;\n            }}\n        }}\n",
            name = instance.name
        ));
    }
    for instance in emission.order.iter().rev() {
        output.push_str(&format!(
            "        if {name}_initialized {{\n            let shutdown_status = self.{name}.on_shutdown(&mut lifecycle_context);\n            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {{\n                status = flowrt::Status::Error;\n            }}\n        }}\n",
            name = instance.name
        ));
    }
    output.push_str("        status\n    }\n");
    output
}

fn emit_rust_scheduler_v2_loop(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    process: Option<&ProcessRuntimePlan<'_>>,
    fallback_step_function: &str,
) -> String {
    let tasks = scheduler_tasks_for_order(graph, order);
    let mut output = String::new();
    output.push_str(&format!(
        "        let mut scheduler = flowrt::DeterministicExecutor::new({});\n",
        selected_profile_worker_threads(contract)
    ));

    let lane_ids = scheduler_lane_ids(&tasks);
    for (lane, lane_id) in &lane_ids {
        output.push_str(&format!(
            "        scheduler.add_lane(flowrt::LaneId({lane_id}), flowrt::LaneKind::Serial);\n        let _ = {lane:?};\n"
        ));
    }
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let lane_id = lane_ids[&task_lane_name(task)];
        let priority = task.priority.unwrap_or(0);
        output.push_str(&format!(
            "        scheduler.add_task(flowrt::TaskSpec {{ id: flowrt::TaskId({task_id}), lane: flowrt::LaneId({lane_id}), priority: {priority} }});\n"
        ));
        if task.trigger == flowrt_ir::TriggerKind::Periodic {
            output.push_str(&format!(
                "        scheduler.add_periodic(flowrt::PeriodicSpec {{ task: flowrt::TaskId({task_id}), period_ms: {} }});\n        scheduler.wake(flowrt::TaskId({task_id}));\n",
                task.period_ms.unwrap_or(1)
            ));
        }
    }
    output.push_str(&emit_rust_on_message_revision_state(&tasks, binds));
    output.push_str(&format!(
        "        let scheduler_base_period_ms: u64 = {};\n",
        scheduler_base_period_ms(&tasks)
    ));
    output.push_str(
        "        let mut tick_base: usize = 0;\n        let mut scheduler_now_ms: u64 = 0;\n        while status == flowrt::Status::Ok\n            && !shutdown.is_requested()\n            && run_ticks\n                .map(|limit| tick_base < limit)\n                .unwrap_or(true)\n        {\n            let mut observed_data_generation: u64;\n            let tick_time_ms = scheduler_now_ms;\n            scheduler.advance_to_ms(tick_time_ms);\n",
    );
    output.push_str(&emit_rust_apply_pending_params_for_order(contract, order));
    let woke_on_message_decl = if tasks
        .iter()
        .any(|task| task.trigger == flowrt_ir::TriggerKind::OnMessage)
    {
        "let mut woke_on_message = false;"
    } else {
        "let woke_on_message = false;"
    };
    output.push_str(&format!(
        "            introspection_state.record_tick();\n            loop {{\n                observed_data_generation = scheduler_events.data_generation();\n                {woke_on_message_decl}\n"
    ));
    output.push_str(&indent_generated_block_levels(
        &emit_rust_on_message_wake_checks(&tasks, binds),
        1,
    ));
    output
        .push_str("                let task_statuses = scheduler.run_ready(|task| match task {\n");
    for (index, task) in tasks.iter().enumerate() {
        let task_id = index + 1;
        let function_name = match process {
            Some(process) => rust_process_task_step_function_name(process, task),
            None => rust_task_step_function_name(task),
        };
        output.push_str(&format!(
            "                flowrt::TaskId({task_id}) => self.{function_name}(tick_time_ms as usize, &mut lifecycle_context, &introspection_state, &scheduler_events),\n"
        ));
    }
    if tasks.is_empty() {
        output.push_str(&format!(
            "                _ => self.{fallback_step_function}(tick_time_ms as usize, &mut lifecycle_context, &introspection_state, &scheduler_events),\n"
        ));
    } else {
        output.push_str("                _ => flowrt::Status::Error,\n");
    }
    output.push_str(&format!(
        "                }});\n                if !woke_on_message && task_statuses.is_empty() {{\n                    break;\n                }}\n                for task_status in task_statuses {{\n                    if task_status == flowrt::Status::Error {{\n                        status = flowrt::Status::Error;\n                        break;\n                    }}\n                }}\n                if status != flowrt::Status::Ok {{\n                    break;\n                }}\n            }}\n            if status == flowrt::Status::Ok {{\n                tick_base += 1;\n                if run_ticks.is_some() {{\n                    scheduler_now_ms = scheduler_now_ms.saturating_add(scheduler_base_period_ms);\n                    continue;\n                }}\n                let next_periodic_deadline_ms = {next_deadline_expr};\n                let next_wake_deadline = next_periodic_deadline_ms.map(|deadline_ms| {{\n                    std::time::Instant::now()\n                        + std::time::Duration::from_millis(deadline_ms.saturating_sub(scheduler_now_ms))\n                }});\n                match scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, &shutdown) {{\n                    flowrt::ScheduleEvent::Shutdown => break,\n                    flowrt::ScheduleEvent::Timer => {{\n                        scheduler_now_ms = next_periodic_deadline_ms\n                            .unwrap_or_else(|| scheduler_now_ms.saturating_add(scheduler_base_period_ms));\n                    }}\n                    flowrt::ScheduleEvent::Data => {{}}\n                }}\n            }}\n        }}\n",
        next_deadline_expr = rust_next_periodic_deadline_expr(&tasks)
    ));
    output
}

fn emit_rust_scheduler_event_registration(binds: &[BindRuntimePlan]) -> String {
    let mut output = String::new();
    for bind in binds
        .iter()
        .filter(|bind| matches!(bind_backend(bind), "iox2" | "zenoh"))
    {
        output.push_str(&format!(
            "        self.{field}.set_schedule_waiter(scheduler_events.clone());\n",
            field = bind.field_name
        ));
    }
    output
}

fn scheduler_base_period_ms(tasks: &[&TaskIr]) -> u64 {
    tasks
        .iter()
        .filter(|task| task.trigger == flowrt_ir::TriggerKind::Periodic)
        .filter_map(|task| task.period_ms)
        .min()
        .unwrap_or(1)
}

fn rust_next_periodic_deadline_expr(tasks: &[&TaskIr]) -> String {
    let deadlines = tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| task.trigger == flowrt_ir::TriggerKind::Periodic)
        .map(|(index, _)| format!("scheduler.next_deadline_ms(flowrt::TaskId({}))", index + 1))
        .collect::<Vec<_>>();
    if deadlines.is_empty() {
        "None::<u64>".to_string()
    } else {
        format!("[{}].into_iter().flatten().min()", deadlines.join(", "))
    }
}

fn runtime_step_uses_tick_time(binds: &[BindRuntimePlan], bridges: &[BridgeRuntimePlan]) -> bool {
    if !bridges.is_empty() {
        return true;
    }
    binds
        .iter()
        .any(|bind| matches!(bind.channel, ChannelKind::Latest | ChannelKind::Fifo))
}

fn bridge_runtime_channel_type(bridge: &BridgeRuntimePlan) -> String {
    format!(
        "flowrt::zenoh::ZenohPubSub<{}>",
        rust_type(&bridge.source_type)
    )
}

fn runtime_channel_type(bind: &BindRuntimePlan) -> String {
    let ty = rust_type(&bind.source_type);
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

fn runtime_channel_initializer(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
) -> String {
    if bind_backend(bind) == "iox2" {
        return format!(
            "flowrt::iox2::Iox2PubSub::open_with_config({}, {}).expect(\"failed to open FlowRT iox2 channel\")",
            rust_string_literal(&iox2_service_name(contract, graph, bind)),
            iox2_channel_config_expr(bind),
        );
    }
    if bind_backend(bind) == "zenoh" {
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

fn bridge_runtime_channel_initializer(
    contract: &ContractIr,
    graph: &GraphIr,
    bridge: &BridgeRuntimePlan,
) -> String {
    format!(
        "flowrt::zenoh::ZenohPubSub::open_with_config({}, flowrt::zenoh::ZenohChannelConfig::latest()).expect(\"failed to open FlowRT ROS2 bridge zenoh channel\")",
        rust_string_literal(&ros2_bridge_key_expr(contract, graph, bridge)),
    )
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

fn runtime_channel_read(
    input: &PortIr,
    bind: &BindRuntimePlan,
    use_cached_transport: bool,
) -> String {
    if matches!(bind_backend(bind), "iox2" | "zenoh") {
        if use_cached_transport {
            return format!(
                "        let {input} = self.{field}.cached_latest_at(tick_time_ms);\n",
                input = input.name,
                field = bind.field_name
            );
        }
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

fn runtime_introspection_publish_record(bind: &BindRuntimePlan) -> String {
    let helper = if bind.source_uses_variable_frame || bind_backend(bind) == "zenoh" {
        "record_introspection_publish_frame"
    } else {
        "record_introspection_publish_copy"
    };
    format!(
        "            {helper}(&self.{probe}, &value, tick_time_ms);\n",
        probe = bind.probe_field_name
    )
}

fn runtime_channel_write(bind: &BindRuntimePlan) -> String {
    let introspection_record = runtime_introspection_publish_record(bind);
    if matches!(bind_backend(bind), "iox2" | "zenoh") {
        return format!(
            "            if self.{field}.publish_at(value.clone(), tick_time_ms).is_err() {{\n                return flowrt::Status::Error;\n            }}\n            scheduler_events.notify_data();\n{introspection_record}",
            field = bind.field_name
        );
    }

    match bind.channel {
        ChannelKind::Latest => {
            format!(
                "            self.{field}.publish_at(value.clone(), tick_time_ms);\n            scheduler_events.notify_data();\n{introspection_record}",
                field = bind.field_name
            )
        }
        ChannelKind::Fifo => {
            format!(
                "            match self.{field}.push_at(value.clone(), tick_time_ms) {{\n                Ok(flowrt::ChannelWriteOutcome::Accepted) | Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {{\n                    scheduler_events.notify_data();\n{introspection_record}                }}\n                Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {{}},\n                Ok(flowrt::ChannelWriteOutcome::Backpressured) => return flowrt::Status::Retry,\n                Err(flowrt::ChannelError::Overflow) => return flowrt::Status::Error,\n            }}\n",
                field = bind.field_name
            )
        }
    }
}

fn bridge_runtime_channel_write(bridge: &BridgeRuntimePlan) -> String {
    format!(
        "            if self.{field}.publish_at(value.clone(), tick_time_ms).is_err() {{\n                return flowrt::Status::Error;\n            }}\n",
        field = bridge.field_name
    )
}

pub(crate) fn iox2_service_name(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
) -> String {
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

pub(crate) fn zenoh_key_expr(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
) -> String {
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

pub(crate) fn ros2_bridge_key_expr(
    contract: &ContractIr,
    graph: &GraphIr,
    bridge: &BridgeRuntimePlan,
) -> String {
    ros2_bridge_key_expr_from_parts(
        &contract.package.name,
        &selected_profile_name(contract),
        &graph.name,
        bridge.index,
        &bridge.source_instance,
        &bridge.source_port,
        &bridge.ros2_topic,
    )
}

fn ros2_bridge_key_expr_from_parts(
    package: &str,
    profile: &str,
    graph: &str,
    index: usize,
    source_instance: &str,
    source_port: &str,
    ros2_topic: &str,
) -> String {
    format!(
        "flowrt/{}/{}/{}/ros2_bridge_{}/{}_{}_to_{}",
        flowrt_path_part(package),
        flowrt_path_part(profile),
        flowrt_path_part(graph),
        index,
        flowrt_path_part(source_instance),
        flowrt_path_part(source_port),
        flowrt_topic_path_part(ros2_topic),
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

fn flowrt_topic_path_part(value: &str) -> String {
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
        .find(|ty| ty.qualified_name == name || ty.generated_name == name || ty.name == name)
        .expect("normalized contract must reference known message types")
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
        args.push(format!("params: &{}Params", component_rust_name(component)));
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
        component_rust_name(component)
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
    let mut output = format!("{}Params {{", component_rust_name(component));
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
    let params_ty = format!("{}Params", component_rust_name(component));
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
    context_name: &str,
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
            "{inner_indent}if self.{instance}.on_params_update(&self.{instance}_params, &{next}, {context_name}) != flowrt::Status::Ok {{\n{deep_indent}return flowrt::Status::Error;\n{inner_indent}}}\n",
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

pub(crate) fn pascal_case(name: &str) -> String {
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

pub(crate) fn component_rust_name(component: &ComponentIr) -> String {
    pascal_case(&component.generated_name)
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
        .find(|component| {
            component.qualified_name == name
                || component.generated_name == name
                || component.name == name
        })
        .expect("normalized contract must reference known components")
}

pub(crate) fn tasks_for_instance<'a>(
    graph: &'a GraphIr,
    instance: &'a InstanceIr,
) -> impl Iterator<Item = &'a TaskIr> {
    graph
        .tasks
        .iter()
        .filter(move |task| task.instance.id == instance.id)
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
