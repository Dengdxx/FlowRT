//! FlowRT 管理应用产物的生成入口。
//!
//! 本 crate 只从 Contract IR 生成 glue：消息类型、组件接口、runtime shell、启动配置和构建文件。
//! 生成内容必须位于用户项目可见的 `flowrt/` 目录下，并且不得承载用户业务逻辑。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use flowrt_ir::{
    ComponentIr, ContractIr, FieldIr, GraphIr, InstanceIr, LanguageKind, ParamIr, ParamType,
    ParamUpdatePolicy, ParamValue, PortIr, TaskIr, TypeExpr, TypeIr,
};
use flowrt_validate::validate_contract;

mod build_files;
mod cpp_shell;
mod launch_manifest;
mod messages;
mod ros2_bridge;
mod runtime_plan;
mod rust_shell;
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
    frame_max_size_for_type, rust_wire_size, type_contains_variable_data, variable_tail_max_size,
};
use ros2_bridge::emit_ros2_bridge_adapter;
use selfdesc::{
    emit_cpp_selfdesc_header, emit_cpp_selfdesc_source, emit_rust_selfdesc, emit_self_description,
};
use supervisor::{emit_rust_supervisor, emit_rust_supervisor_main};

// Re-export functions moved to rust_shell that other modules depend on.
pub(crate) use rust_shell::backend_emit::{
    iox2_service_name, iox2_service_name_for_edge, ros2_bridge_key_expr, selected_backend_name,
    zenoh_key_expr, zenoh_key_expr_for_edge,
};

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
            rust_shell::emit_rust_components(contract),
        ));
        artifacts.push(artifact(
            "rust/src/runtime_shell.rs",
            rust_shell::emit_rust_runtime_shell(contract),
        ));
        artifacts.push(artifact("rust/src/main.rs", rust_shell::emit_rust_main()));
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
        artifacts.push(artifact(
            "rust/src/lib.rs",
            rust_shell::emit_rust_lib(has_rust),
        ));
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

pub(crate) fn param_value_for_instance<'a>(
    instance: &'a InstanceIr,
    param: &'a ParamIr,
) -> &'a ParamValue {
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

pub(crate) fn param_json_value_literal(value: &ParamValue) -> String {
    format!("serde_json::json!({})", param_json_literal(value))
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

pub(crate) fn rust_string_literal(value: &str) -> String {
    format!("{value:?}")
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

pub(crate) fn component_by_name<'a>(contract: &'a ContractIr, name: &str) -> &'a ComponentIr {
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

pub(crate) fn flowrt_path_part(value: &str) -> String {
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

pub(crate) fn flowrt_topic_path_part(value: &str) -> String {
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

pub(crate) fn topo_order_instances_for_language<'a>(
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
