//! FlowRT 管理应用产物的生成入口。
//!
//! 本 crate 只从 Contract IR 生成 glue：消息类型、组件接口、runtime shell、启动配置和构建文件。
//! 生成内容必须位于用户项目可见的 `flowrt/` 目录下，并且不得承载用户业务逻辑。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use flowrt_conformance::{MessageAbiExpectation, message_abi_expectations};
use flowrt_ir::{
    ChannelEdgeIr, ChannelKind, ComponentIr, ContractIr, FieldIr, GraphIr, InstanceIr,
    LanguageKind, OverflowPolicy as IrOverflowPolicy, PortIr, PrimitiveType,
    StalePolicy as IrStalePolicy, TaskIr, TriggerKind, TypeExpr, TypeIr,
};
use flowrt_validate::validate_contract;
use serde::Serialize;
use sha2::{Digest, Sha256};

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

const SELF_DESCRIPTION_SCHEMA_VERSION: &str = "0.1";
const SELF_DESCRIPTION_SECTION: &str = ".flowrt.selfdesc";

#[derive(Debug, Serialize)]
struct SelfDescription<'a> {
    self_description_version: &'static str,
    ir_version: &'a str,
    schema_version: &'a str,
    source_hash: &'a str,
    package: SelfDescriptionPackage<'a>,
    profiles: Vec<SelfDescriptionProfile<'a>>,
    targets: Vec<SelfDescriptionTarget<'a>>,
    deployments: Vec<SelfDescriptionDeployment<'a>>,
    graphs: Vec<SelfDescriptionGraph<'a>>,
    message_abi: Vec<SelfDescriptionMessageAbi>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionPackage<'a> {
    name: &'a str,
    version: Option<&'a str>,
    rsdl_version: &'a str,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionProfile<'a> {
    name: &'a str,
    backend: &'a str,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionTarget<'a> {
    name: &'a str,
    platform: Option<&'a str>,
    runtimes: Vec<&'static str>,
    backends: Vec<&'a str>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionDeployment<'a> {
    graph: &'a str,
    profile: &'a str,
    target: &'a str,
    backend: &'a str,
    satisfied: bool,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionGraph<'a> {
    name: &'a str,
    instances: Vec<SelfDescriptionInstance<'a>>,
    tasks: Vec<SelfDescriptionTask<'a>>,
    channels: Vec<SelfDescriptionChannel>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionInstance<'a> {
    name: &'a str,
    component: &'a str,
    process: &'a str,
    target: Option<&'a str>,
    runtime: &'static str,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionTask<'a> {
    instance: &'a str,
    trigger: &'static str,
    period_ms: Option<u64>,
    deadline_ms: Option<u64>,
    priority: Option<u32>,
    inputs: &'a [String],
    outputs: &'a [String],
}

#[derive(Debug, Serialize)]
struct SelfDescriptionChannel {
    from: String,
    to: String,
    message_type: String,
    backend: String,
    service: Option<String>,
    channel: &'static str,
    depth: Option<u32>,
    overflow: &'static str,
    stale_policy: &'static str,
    max_age_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionMessageAbi {
    type_name: String,
    size_bytes: usize,
    align_bytes: usize,
    fields: Vec<SelfDescriptionFieldAbi>,
}

#[derive(Debug, Serialize)]
struct SelfDescriptionFieldAbi {
    name: String,
    offset_bytes: usize,
    size_bytes: usize,
    align_bytes: usize,
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
    let abi_expectations = message_abi_expectations(contract)?;
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

fn has_language(contract: &ContractIr, language: LanguageKind) -> bool {
    contract
        .components
        .iter()
        .any(|component| component.language == language)
}

fn emit_self_description(contract: &ContractIr) -> Result<String> {
    let self_description = self_description(contract)?;
    let mut output = serde_json::to_string_pretty(&self_description)?;
    output.push('\n');
    Ok(output)
}

fn self_description(contract: &ContractIr) -> Result<SelfDescription<'_>> {
    let selected_backend = selected_backend_name(contract);
    Ok(SelfDescription {
        self_description_version: SELF_DESCRIPTION_SCHEMA_VERSION,
        ir_version: &contract.ir_version,
        schema_version: &contract.schema_version,
        source_hash: &contract.source_hash,
        package: SelfDescriptionPackage {
            name: &contract.package.name,
            version: contract.package.version.as_deref(),
            rsdl_version: &contract.package.rsdl_version,
        },
        profiles: contract
            .profiles
            .iter()
            .map(|profile| SelfDescriptionProfile {
                name: &profile.name,
                backend: &profile.backend.0,
            })
            .collect(),
        targets: contract
            .targets
            .iter()
            .map(|target| SelfDescriptionTarget {
                name: &target.name,
                platform: target.platform.as_deref(),
                runtimes: target
                    .runtime
                    .iter()
                    .copied()
                    .map(language_name)
                    .collect::<Vec<_>>(),
                backends: target
                    .backends
                    .iter()
                    .map(|backend| backend.0.as_str())
                    .collect::<Vec<_>>(),
            })
            .collect(),
        deployments: contract
            .deployments
            .iter()
            .map(|deployment| SelfDescriptionDeployment {
                graph: &deployment.graph.name,
                profile: &deployment.profile.name,
                target: &deployment.target.name,
                backend: &deployment.backend.0,
                satisfied: deployment.satisfied,
            })
            .collect(),
        graphs: contract
            .graphs
            .iter()
            .map(|graph| self_description_graph(contract, graph, selected_backend.clone()))
            .collect(),
        message_abi: message_abi_expectations(contract)?
            .into_iter()
            .map(self_description_message_abi)
            .collect(),
    })
}

fn self_description_graph<'a>(
    contract: &'a ContractIr,
    graph: &'a GraphIr,
    backend: String,
) -> SelfDescriptionGraph<'a> {
    SelfDescriptionGraph {
        name: &graph.name,
        instances: graph
            .instances
            .iter()
            .map(|instance| {
                let component = component_by_name(contract, &instance.component.name);
                SelfDescriptionInstance {
                    name: &instance.name,
                    component: &instance.component.name,
                    process: instance.process.as_deref().unwrap_or("main"),
                    target: instance.target.as_ref().map(|target| target.name.as_str()),
                    runtime: language_name(component.language),
                }
            })
            .collect(),
        tasks: graph
            .tasks
            .iter()
            .map(|task| SelfDescriptionTask {
                instance: &task.instance.name,
                trigger: trigger_name(task.trigger),
                period_ms: task.period_ms,
                deadline_ms: task.deadline_ms,
                priority: task.priority,
                inputs: &task.inputs,
                outputs: &task.outputs,
            })
            .collect(),
        channels: graph
            .binds
            .iter()
            .enumerate()
            .map(|(index, bind)| SelfDescriptionChannel {
                from: format!("{}.{}", bind.from.instance.name, bind.from.port),
                to: format!("{}.{}", bind.to.instance.name, bind.to.port),
                message_type: channel_message_type(contract, graph, bind),
                backend: backend.clone(),
                service: (backend == "iox2")
                    .then(|| iox2_service_name_for_edge(contract, graph, index, bind)),
                channel: channel_name(bind.channel),
                depth: bind.depth,
                overflow: overflow_name(bind.overflow),
                stale_policy: stale_name(bind.stale),
                max_age_ms: bind.max_age_ms,
            })
            .collect(),
    }
}

fn self_description_message_abi(expectation: MessageAbiExpectation) -> SelfDescriptionMessageAbi {
    SelfDescriptionMessageAbi {
        type_name: expectation.type_name,
        size_bytes: expectation.size_bytes,
        align_bytes: expectation.align_bytes,
        fields: expectation
            .fields
            .into_iter()
            .map(|field| SelfDescriptionFieldAbi {
                name: field.name,
                offset_bytes: field.offset_bytes,
                size_bytes: field.size_bytes,
                align_bytes: field.align_bytes,
            })
            .collect(),
    }
}

fn channel_message_type(contract: &ContractIr, graph: &GraphIr, bind: &ChannelEdgeIr) -> String {
    let instances = graph
        .instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let source_instance = instances
        .get(bind.from.instance.name.as_str())
        .expect("validated bind source instance must exist");
    let component = component_by_name(contract, &source_instance.component.name);
    component
        .outputs
        .iter()
        .find(|port| port.name == bind.from.port)
        .map(|port| port.ty.canonical_syntax())
        .expect("validated bind source port must exist")
}

fn channel_name(channel: ChannelKind) -> &'static str {
    match channel {
        ChannelKind::Latest => "latest",
        ChannelKind::Fifo => "fifo",
    }
}

fn overflow_name(policy: IrOverflowPolicy) -> &'static str {
    match policy {
        IrOverflowPolicy::DropOldest => "drop_oldest",
        IrOverflowPolicy::DropNewest => "drop_newest",
        IrOverflowPolicy::Error => "error",
        IrOverflowPolicy::Block => "block",
    }
}

fn stale_name(policy: IrStalePolicy) -> &'static str {
    match policy {
        IrStalePolicy::Warn => "warn",
        IrStalePolicy::Drop => "drop",
        IrStalePolicy::HoldLast => "hold_last",
        IrStalePolicy::Error => "error",
    }
}

fn trigger_name(trigger: TriggerKind) -> &'static str {
    match trigger {
        TriggerKind::Periodic => "periodic",
        TriggerKind::OnMessage => "on_message",
        TriggerKind::Startup => "startup",
        TriggerKind::Shutdown => "shutdown",
    }
}

fn ordered_types(contract: &ContractIr) -> Vec<&flowrt_ir::TypeIr> {
    let type_map = contract
        .types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    let mut visited = BTreeSet::new();
    let mut visiting = BTreeSet::new();
    let mut order = Vec::with_capacity(contract.types.len());

    for ty in &contract.types {
        visit_type(ty, &type_map, &mut visited, &mut visiting, &mut order);
    }

    order
}

fn visit_type<'a>(
    ty: &'a flowrt_ir::TypeIr,
    type_map: &BTreeMap<&str, &'a flowrt_ir::TypeIr>,
    visited: &mut BTreeSet<String>,
    visiting: &mut BTreeSet<String>,
    order: &mut Vec<&'a flowrt_ir::TypeIr>,
) {
    if visited.contains(&ty.name) {
        return;
    }
    if !visiting.insert(ty.name.clone()) {
        panic!("validated contract must not contain recursive message types");
    }

    let mut deps = BTreeSet::new();
    for field in &ty.fields {
        collect_type_dependencies(&field.ty, &mut deps);
    }
    for dep in deps {
        if let Some(next) = type_map.get(dep.as_str()) {
            visit_type(next, type_map, visited, visiting, order);
        }
    }

    visiting.remove(&ty.name);
    visited.insert(ty.name.clone());
    order.push(ty);
}

fn collect_type_dependencies(expr: &TypeExpr, dependencies: &mut BTreeSet<String>) {
    match expr {
        TypeExpr::Primitive { .. } => {}
        TypeExpr::Named { name } => {
            dependencies.insert(name.clone());
        }
        TypeExpr::Array { element, .. } => collect_type_dependencies(element, dependencies),
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } => {}
        TypeExpr::VarSequence { element, .. } => collect_type_dependencies(element, dependencies),
    }
}

fn emit_cpp_messages(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str("#include <array>\n#include <cstdint>\n\n");
    output.push_str("namespace flowrt_app {\n\n");
    let needs_iox2_type_name = selected_backend_name(contract) == "iox2";
    for ty in ordered_types(contract) {
        output.push_str(&format!("struct {} {{\n", ty.name));
        if needs_iox2_type_name {
            output.push_str(&format!(
                "    static constexpr const char* IOX2_TYPE_NAME = \"{}\";\n",
                ty.name
            ));
        }
        for field in &ty.fields {
            output.push_str(&format!(
                "    {} {}{{}};\n",
                cpp_type(&field.ty),
                field.name
            ));
        }
        output.push_str("};\n\n");
    }
    output.push_str("}  // namespace flowrt_app\n");
    output
}

fn emit_cpp_selfdesc_header(_contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str("#include <cstddef>\n#include <string_view>\n\n");
    output.push_str("namespace flowrt_app {\n\n");
    output.push_str("std::string_view self_description_json() noexcept;\n\n");
    output.push_str("std::string_view self_description_hash() noexcept;\n\n");
    output.push_str("}  // namespace flowrt_app\n");
    output
}

fn emit_cpp_selfdesc_source(contract: &ContractIr) -> String {
    let json = emit_self_description(contract)
        .expect("validated contract self-description should serialize");
    let hash = hex_sha256(&json);
    let mut output = managed_header();
    output.push_str("#include \"flowrt_app/selfdesc.hpp\"\n\n");
    output.push_str("#include <string_view>\n\n");
    output.push_str("namespace flowrt_app {\nnamespace {\n\n");
    output.push_str(&format!(
        "#if defined(__GNUC__) || defined(__clang__)\n[[gnu::used, gnu::section(\"{SELF_DESCRIPTION_SECTION}\")]]\n#endif\n"
    ));
    output.push_str("const char kFlowrtSelfDescription[] = ");
    output.push_str(&cpp_raw_string_literal(&json));
    output.push_str(";\n\n");
    output.push_str("const char kFlowrtSelfDescriptionHash[] = ");
    output.push_str(&cpp_string_literal(&hash));
    output.push_str(";\n\n");
    output.push_str("}  // namespace\n\n");
    output.push_str(
        "std::string_view self_description_json() noexcept {\n    return std::string_view{kFlowrtSelfDescription, sizeof(kFlowrtSelfDescription) - 1};\n}\n\n",
    );
    output.push_str(
        "std::string_view self_description_hash() noexcept {\n    return std::string_view{kFlowrtSelfDescriptionHash, sizeof(kFlowrtSelfDescriptionHash) - 1};\n}\n\n",
    );
    output.push_str("}  // namespace flowrt_app\n");
    output
}

fn emit_cpp_components(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str("#pragma once\n\n");
    output.push_str("#include <flowrt/runtime.hpp>\n\n");
    output.push_str("#include \"flowrt_app/messages.hpp\"\n\n");
    output.push_str("namespace flowrt_app {\n\n");

    for component in contract
        .components
        .iter()
        .filter(|component| component.language == LanguageKind::Cpp)
    {
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
    output.push_str("#include <charconv>\n#include <chrono>\n#include <cstdint>\n#include <cstdlib>\n#include <optional>\n#include <string>\n#include <string_view>\n#include <system_error>\n#include <utility>\n#include <variant>\n\n");
    output.push_str("namespace {\n\n");
    output.push_str(
        "flowrt::Status status_from_push_result(const flowrt::ChannelPushResult& result) {\n    if (std::holds_alternative<flowrt::ChannelError>(result)) {\n        return flowrt::Status::Error;\n    }\n\n    switch (std::get<flowrt::ChannelWriteOutcome>(result)) {\n        case flowrt::ChannelWriteOutcome::Accepted:\n        case flowrt::ChannelWriteOutcome::DroppedOldest:\n        case flowrt::ChannelWriteOutcome::DroppedNewest:\n            return flowrt::Status::Ok;\n        case flowrt::ChannelWriteOutcome::Backpressured:\n            return flowrt::Status::Retry;\n    }\n\n    return flowrt::Status::Error;\n}\n\n",
    );
    output.push_str(
        "std::optional<std::size_t> flowrt_run_tick_limit() {\n    const auto* raw = std::getenv(\"FLOWRT_RUN_TICKS\");\n    if (raw == nullptr || *raw == '\\0') {\n        return std::nullopt;\n    }\n\n    const auto text = std::string_view{raw};\n    std::size_t ticks = 0;\n    const auto result = std::from_chars(text.data(), text.data() + text.size(), ticks);\n    if (result.ec != std::errc{} || result.ptr != text.data() + text.size() || ticks == 0) {\n        return std::nullopt;\n    }\n    return ticks;\n}\n\n",
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
        "flowrt::Status run() {{\n    auto backend = {backend_factory};\n    return flowrt_user::build_app().run(backend);\n}}\n\n"
    ));
    output.push_str(&format!(
        "flowrt::Status run_process(std::string_view process) {{\n    auto backend = {backend_factory};\n    return flowrt_user::build_app().run_process(backend, process);\n}}\n\n"
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
    output.push_str("#include <memory>\n#include <string_view>\n\n");
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
        "    /**\n     * @brief 使用指定 backend 运行完整 C++ 应用图。\n     *\n     * @param backend 提供调度器和 capability 的 FlowRT backend。\n     * @return 应用执行状态。\n     */\n    flowrt::Status run(const flowrt::Backend& backend);\n\n    /**\n     * @brief 运行指定 RSDL process group。\n     *\n     * @param backend 提供调度器和 capability 的 FlowRT backend。\n     * @param process Contract IR 中声明的 process group 名称。\n     * @return 应用执行状态。\n     */\n    flowrt::Status run_process(const flowrt::Backend& backend, std::string_view process);\n\nprivate:\n    flowrt::Status step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);\n    flowrt::Status step_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);\n    flowrt::Status step_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);\n",
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
            "    flowrt::Status run_process_{}(const flowrt::Backend& backend);\n",
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
    }
    for bind in &bind_plans {
        output.push_str(&format!(
            "    {} {}_;\n",
            cpp_runtime_channel_type(bind, &selected_backend),
            bind.field_name
        ));
    }
    output.push_str("};\n\n");
    output.push_str(
        "/**\n * @brief 运行默认 C++ inproc 应用。\n *\n * @return runtime shell 执行状态。\n */\nflowrt::Status run();\n\n",
    );
    output.push_str(
        "/**\n * @brief 运行默认 C++ inproc 应用中的指定 process group。\n *\n * @param process process group 名称。\n * @return runtime shell 执行状态。\n */\nflowrt::Status run_process(std::string_view process);\n\n",
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
        initializers.push(format!("{}_(std::move({}))", instance.name, instance.name));
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

#[derive(Debug, Clone, Copy)]
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
        "flowrt::Status App::run_process(const flowrt::Backend& backend, std::string_view process) {\n",
    );
    for process in processes {
        output.push_str(&format!(
            "    if (process == {}) {{\n        return run_process_{}(backend);\n    }}\n",
            cpp_string_literal(&process.name),
            process.method_suffix
        ));
    }
    output.push_str("    return flowrt::Status::Error;\n}\n\n");
    output
}

struct CppRunEmission<'a> {
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
        "flowrt::Status App::{}(const flowrt::Backend& backend) {{\n    flowrt::Context lifecycle_context;\n    auto status = flowrt::Status::Ok;\n",
        run.function_name
    ));
    output.push_str("    flowrt::IntrospectionState introspection_state;\n");
    output.push_str(&emit_cpp_introspection_channel_registration(
        run.order, run.binds,
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
        "    {{\n        const auto run_tick_limit = flowrt_run_tick_limit();\n        std::size_t tick_base = 0;\n        while (status == flowrt::Status::Ok && (!run_tick_limit.has_value() || tick_base < *run_tick_limit)) {{\n            status = backend.scheduler().run_ticks(\n                1, [this, &introspection_state, tick_base](std::size_t tick, flowrt::Context& tick_context) {{\n                    introspection_state.record_tick();\n                    return {}(tick_base + tick, tick_context, introspection_state);\n                }});\n            ++tick_base;\n        }}\n    }}\n",
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

fn cpp_string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn cpp_runtime_channel_type(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let ty = cpp_type(&bind.source_type);
    if selected_backend == "iox2" {
        return format!("flowrt::iox2::Iox2PubSub<{ty}>");
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
        return format!(
            "flowrt::iox2::Iox2PubSub<{}>::open_with_config({}, {})",
            cpp_type(&bind.source_type),
            cpp_string_literal(&iox2_service_name(contract, graph, bind)),
            cpp_iox2_channel_config_expr(bind)
        );
    }

    match bind.channel {
        ChannelKind::Latest => cpp_runtime_latest_channel_initializer(bind),
        ChannelKind::Fifo => cpp_runtime_fifo_channel_initializer(bind),
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
    if selected_backend == "iox2" {
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

fn cpp_introspection_publish_record(bind: &BindRuntimePlan) -> String {
    format!(
        "        record_introspection_publish(introspection_state, {}, {}, *value, tick_time_ms);\n",
        cpp_string_literal(&runtime_channel_name(bind)),
        cpp_string_literal(&runtime_channel_message_type(bind))
    )
}

fn cpp_runtime_channel_write(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let introspection_record = cpp_introspection_publish_record(bind);
    if selected_backend == "iox2" {
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
    (!binds.is_empty() && selected_backend == "iox2")
        || binds
            .iter()
            .any(|bind| matches!(bind.channel, ChannelKind::Latest | ChannelKind::Fifo))
}

fn cpp_backend_factory(selected_backend: &str) -> &'static str {
    if selected_backend == "iox2" {
        "flowrt::iox2_backend()"
    } else {
        "flowrt::inproc_backend()"
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

fn emit_cpp_main() -> String {
    let mut output = managed_header();
    output.push_str("#include \"flowrt_app/runtime_shell.hpp\"\n\n");
    output.push_str("#include <string_view>\n\n");
    output.push_str(
        "int main(int argc, char** argv) {\n    std::string_view process;\n    for (int index = 1; index < argc; ++index) {\n        const std::string_view arg(argv[index]);\n        if (arg == \"--process\") {\n            if (index + 1 >= argc) {\n                return 2;\n            }\n            process = argv[++index];\n        } else {\n            return 2;\n        }\n    }\n\n    const auto status = process.empty() ? flowrt_app::run() : flowrt_app::run_process(process);\n    return status == flowrt::Status::Ok ? 0 : 1;\n}\n",
    );
    output
}

fn emit_rust_messages(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push('\n');
    let needs_iox2_type_name = selected_backend_name(contract) == "iox2";
    let zero_copy_derive = if needs_iox2_type_name {
        output.push_str("use flowrt::ZeroCopySend;\n\n");
        ", flowrt::ZeroCopySend"
    } else {
        ""
    };
    for ty in ordered_types(contract) {
        output.push_str("#[repr(C)]\n");
        output.push_str(&format!(
            "#[derive(Clone, Copy, Debug, PartialEq{zero_copy_derive})]\n"
        ));
        if needs_iox2_type_name {
            output.push_str(&format!(
                "#[type_name({})]\n",
                rust_string_literal(&ty.name)
            ));
        }
        output.push_str(&format!("pub struct {} {{\n", ty.name));
        for field in &ty.fields {
            output.push_str(&format!(
                "    pub {}: {},\n",
                field.name,
                rust_type(&field.ty)
            ));
        }
        output.push_str("}\n\n");
        output.push_str(&format!("impl Default for {} {{\n", ty.name));
        output.push_str("    fn default() -> Self {\n");
        output.push_str(
            "        // Safety：FlowRT Message ABI v0.1 只允许拥有有效全零位模式的 plain-data 类型。\n",
        );
        output.push_str("        unsafe { std::mem::zeroed() }\n");
        output.push_str("    }\n");
        output.push_str("}\n\n");
    }
    output
}

fn emit_rust_selfdesc(contract: &ContractIr) -> String {
    let json = emit_self_description(contract)
        .expect("validated contract self-description should serialize");
    let hash = hex_sha256(&json);
    let mut output = managed_header();
    output.push('\n');
    output.push_str(&format!(
        "#[used]\n#[unsafe(link_section = \"{SELF_DESCRIPTION_SECTION}\")]\nstatic FLOWRT_SELF_DESCRIPTION: [u8; {}] = *{};\n\n",
        json.len(),
        rust_byte_string_literal(&json),
    ));
    output.push_str(
        "#[allow(dead_code)]\npub fn self_description_json() -> &'static str {\n    std::str::from_utf8(&FLOWRT_SELF_DESCRIPTION).expect(\"generated FlowRT self-description is UTF-8\")\n}\n",
    );
    output.push_str(&format!(
        "\npub fn self_description_hash() -> &'static str {{\n    {}\n}}\n",
        rust_string_literal(&hash)
    ));
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
    output.push_str(
        "fn flowrt_run_tick_limit() -> Option<usize> {\n    std::env::var(\"FLOWRT_RUN_TICKS\")\n        .ok()\n        .and_then(|raw| raw.parse::<usize>().ok())\n        .filter(|ticks| *ticks > 0)\n}\n\n",
    );
    output.push_str(&format!(
        "const SELECTED_BACKEND: &str = {};\n\n",
        rust_string_literal(&selected_backend)
    ));
    output.push_str(&format!(
        "const PACKAGE_NAME: &str = {};\n\n",
        rust_string_literal(&contract.package.name)
    ));
    output.push_str(&emit_rust_introspection_helpers());
    output.push_str("pub struct App {\n");
    for instance in &order {
        let component = component_by_name(contract, &instance.component.name);
        output.push_str(&format!(
            "    {}: Box<dyn {}>,\n",
            instance.name,
            pascal_case(&component.name)
        ));
    }
    for bind in &bind_plans {
        output.push_str(&format!(
            "    {}: {},\n",
            bind.field_name,
            runtime_channel_type(bind, &selected_backend)
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
    output.push_str(&emit_rust_app_run(&order, &bind_plans));
    output.push_str(&emit_rust_app_run_process_dispatch(&process_plans));
    for process in &process_plans {
        let step_function_name = format!("step_process_{}", process.method_suffix);
        let startup_function_name = format!("step_process_{}_startup", process.method_suffix);
        let shutdown_function_name = format!("step_process_{}_shutdown", process.method_suffix);
        output.push_str(&emit_rust_app_run_function(
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
        "pub fn backend() -> Box<dyn flowrt::Backend> {\n    match SELECTED_BACKEND {\n        \"iox2\" => Box::new(flowrt::iox2_backend()),\n        _ => Box::new(flowrt::inproc_backend()),\n    }\n}\n\npub fn run() -> flowrt::Status {\n    let backend = backend();\n    user::build_app().run(backend.as_ref())\n}\n\npub fn run_process(process: &str) -> flowrt::Status {\n    let backend = backend();\n    user::build_app().run_process(backend.as_ref(), process)\n}\n",
    );
    output
}

fn selected_backend_name(contract: &ContractIr) -> String {
    contract
        .profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| contract.profiles.first())
        .map(|profile| profile.backend.0.clone())
        .unwrap_or_else(|| "inproc".to_string())
}

fn rust_string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn rust_byte_string_literal(value: &str) -> String {
    let mut hashes = String::from("#");
    while value.contains(&format!("\"{hashes}")) {
        hashes.push('#');
    }
    format!("br{hashes}\"{value}\"{hashes}")
}

fn hex_sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn cpp_raw_string_literal(value: &str) -> String {
    let mut hashes = String::new();
    while value.contains(&format!("){hashes}\"")) {
        hashes.push('#');
    }
    format!("R\"{hashes}({value}){hashes}\"")
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
        "\nfn main() {\n    let mut args = std::env::args().skip(1);\n    let mut process = None;\n    while let Some(arg) = args.next() {\n        match arg.as_str() {\n            \"--process\" => process = args.next(),\n            _ => {\n                eprintln!(\"unknown FlowRT app argument: {arg}\");\n                std::process::exit(2);\n            }\n        }\n    }\n\n    let status = match process.as_deref() {\n        Some(process) => flowrt_app::runtime_shell::run_process(process),\n        None => flowrt_app::runtime_shell::run(),\n    };\n    let code = match status {\n        flowrt::Status::Ok => 0,\n        _ => 1,\n    };\n    std::process::exit(code);\n}\n",
    );
    output
}

fn emit_rust_supervisor_main() -> String {
    let mut output = managed_header();
    output.push_str(
        "\nfn main() {\n    match flowrt_app::supervisor::launch() {\n        Ok(()) => std::process::exit(0),\n        Err(error) => {\n            eprintln!(\"FlowRT supervisor failed: {error}\");\n            std::process::exit(1);\n        }\n    }\n}\n",
    );
    output
}

fn emit_rust_supervisor(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str(&format!(
        "\nuse std::path::{{Path, PathBuf}};\nuse std::process::Command;\n\nconst RUST_APP_STEM: &str = {};\nconst CPP_APP_STEM: &str = {};\n",
        rust_string_literal(&rust_app_stem(contract)),
        rust_string_literal(&cpp_app_stem(contract))
    ));
    output.push_str(
        r#"
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

#[derive(Debug, serde::Deserialize)]
struct LaunchManifest {
    graphs: Vec<LaunchGraph>,
}

#[derive(Debug, serde::Deserialize)]
struct LaunchGraph {
    processes: Vec<LaunchProcess>,
}

#[derive(Debug, serde::Deserialize)]
struct LaunchProcess {
    name: String,
    runtime_kind: String,
}

pub fn launch() -> Result<(), String> {
    let manifest: LaunchManifest = serde_json::from_str(LAUNCH_MANIFEST)
        .map_err(|error| format!("failed to parse FlowRT launch manifest: {error}"))?;
    if manifest.graphs.is_empty() {
        return Err("FlowRT launch manifest does not contain a graph".to_string());
    }

    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;

    let mut children = Vec::new();
    for graph in &manifest.graphs {
        for process in &graph.processes {
            let app_exe = app_executable_for_runtime(&current_exe, &process.runtime_kind)?;
            let child = Command::new(&app_exe)
                .arg("--process")
                .arg(&process.name)
                .spawn()
                .map_err(|error| format!("failed to start FlowRT process `{}`: {error}", process.name))?;
            children.push((process.name.clone(), child));
        }
    }
    if children.is_empty() {
        return Err("FlowRT launch manifest does not contain process groups".to_string());
    }

    let mut failures = Vec::new();
    for (process, mut child) in children {
        let status = child
            .wait()
            .map_err(|error| format!("failed to wait for FlowRT process `{process}`: {error}"))?;
        if !status.success() {
            failures.push(format!("{process} exited with {status}"));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn app_executable_for_runtime(current_exe: &Path, runtime_kind: &str) -> Result<PathBuf, String> {
    match runtime_kind {
        "rust" => rust_app_executable(current_exe),
        "cpp" => cpp_app_executable(current_exe),
        "mixed" => Err("FlowRT mixed process groups are not launchable yet".to_string()),
        other => Err(format!("unknown FlowRT process runtime_kind `{other}`")),
    }
}

fn rust_app_executable(current_exe: &Path) -> Result<PathBuf, String> {
    let mut path = current_exe.to_path_buf();
    path.set_file_name(binary_name(RUST_APP_STEM));
    Ok(path)
}

fn cpp_app_executable(current_exe: &Path) -> Result<PathBuf, String> {
    let build_dir = current_exe
        .parent()
        .and_then(|profile_dir| profile_dir.parent())
        .and_then(|target_dir| target_dir.parent())
        .ok_or_else(|| format!("failed to resolve FlowRT build directory from `{}`", current_exe.display()))?;
    let mut path = build_dir.join("cmake");
    path.push(binary_name(CPP_APP_STEM));
    Ok(path)
}

fn binary_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}
"#,
    );
    output
}

fn rust_app_stem(contract: &ContractIr) -> String {
    format!(
        "{}-flowrt-app",
        sanitize_package_name(&contract.package.name).replace('_', "-")
    )
}

fn cpp_app_stem(contract: &ContractIr) -> String {
    format!(
        "{}_cpp_app",
        sanitize_package_name(&contract.package.name).replace('-', "_")
    )
}

#[derive(Debug, Clone)]
struct BindRuntimePlan {
    index: usize,
    field_name: String,
    channel: ChannelKind,
    overflow: IrOverflowPolicy,
    stale: IrStalePolicy,
    max_age_ms: Option<u64>,
    depth: Option<u32>,
    source_type: TypeExpr,
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
                channel: bind.channel,
                overflow: bind.overflow,
                stale: bind.stale,
                max_age_ms: bind.max_age_ms,
                depth: bind.depth,
                source_type: source_port.ty.clone(),
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

fn emit_cpp_introspection_helpers() -> String {
    r#"void register_introspection_channel(
    flowrt::IntrospectionState& state,
    std::string_view name,
    std::string_view message_type
) {
    try {
        state.register_channel(std::string{name}, std::string{message_type});
    } catch (...) {
    }
}

template <typename T>
void record_introspection_publish(
    flowrt::IntrospectionState& state,
    std::string_view name,
    std::string_view message_type,
    const T& value,
    std::uint64_t published_at_ms
) {
    try {
        state.record_channel_publish(
            std::string{name},
            std::string{message_type},
            value,
            std::optional<std::uint64_t>{published_at_ms});
    } catch (...) {
    }
}

"#
    .to_string()
}

fn emit_cpp_introspection_channel_registration(
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    for bind in active_binds_for_instances(binds, order) {
        output.push_str(&format!(
            "    register_introspection_channel(introspection_state, {}, {});\n",
            cpp_string_literal(&runtime_channel_name(bind)),
            cpp_string_literal(&runtime_channel_message_type(bind))
        ));
    }
    output
}

fn emit_rust_introspection_helpers() -> String {
    r#"fn register_introspection_channel(
    state: &flowrt::IntrospectionState,
    name: &'static str,
    message_type: &'static str,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.register_channel(name, message_type);
    }));
}

fn record_introspection_publish<T: Copy>(
    state: &flowrt::IntrospectionState,
    name: &'static str,
    message_type: &'static str,
    value: &T,
    published_at_ms: u64,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.record_channel_publish(name, message_type, value, Some(published_at_ms));
    }));
}

"#
    .to_string()
}

fn emit_rust_introspection_channel_registration(
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    for bind in active_binds_for_instances(binds, order) {
        output.push_str(&format!(
            "        register_introspection_channel(&introspection_state, {}, {});\n",
            rust_string_literal(&runtime_channel_name(bind)),
            rust_string_literal(&runtime_channel_message_type(bind))
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
        output.push_str(&format!("            {},\n", instance.name));
    }
    for bind in binds {
        output.push_str(&format!(
            "            {}: {},\n",
            bind.field_name,
            runtime_channel_initializer(contract, graph, bind, selected_backend)
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
                "{indent}if let Some(value) = {port}.as_ref().copied() {{\n",
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

fn emit_rust_app_run(order: &[&InstanceIr], binds: &[BindRuntimePlan]) -> String {
    emit_rust_app_run_function(
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
        "    pub fn run_process(self, backend: &dyn flowrt::Backend, process: &str) -> flowrt::Status {\n        match process {\n",
    );
    for process in processes {
        output.push_str(&format!(
            "            {} => self.run_process_{}(backend),\n",
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
        "    {visibility}fn {function_name}(mut self, backend: &dyn flowrt::Backend) -> flowrt::Status {{\n        let mut lifecycle_context = flowrt::Context::default();\n        let mut status = flowrt::Status::Ok;\n",
    ));
    output.push_str("        let introspection_state = flowrt::IntrospectionState::new();\n");
    output.push_str(&emit_rust_introspection_channel_registration(order, binds));
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
        "        let run_tick_limit = flowrt_run_tick_limit();\n        let mut tick_base: usize = 0;\n        while status == flowrt::Status::Ok\n            && run_tick_limit\n                .map(|limit| tick_base < limit)\n                .unwrap_or(true)\n        {{\n            status = backend.scheduler().run_ticks(1, &mut |tick, tick_context| {{\n                introspection_state.record_tick();\n                self.{step_function_name}(tick_base + tick, tick_context, &introspection_state)\n            }});\n            tick_base += 1;\n        }}\n",
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
    (!binds.is_empty() && selected_backend == "iox2")
        || binds
            .iter()
            .any(|bind| matches!(bind.channel, ChannelKind::Latest | ChannelKind::Fifo))
}

fn runtime_channel_type(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let ty = rust_type(&bind.source_type);
    if selected_backend == "iox2" {
        return format!("flowrt::iox2::Iox2PubSub<{ty}>");
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
        return format!(
            "flowrt::iox2::Iox2PubSub::open_with_config({}, {}).expect(\"failed to open FlowRT iox2 channel\")",
            rust_string_literal(&iox2_service_name(contract, graph, bind)),
            iox2_channel_config_expr(bind),
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
    if selected_backend == "iox2" {
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
    format!(
        "            record_introspection_publish(introspection_state, {}, {}, &value, tick_time_ms);\n",
        rust_string_literal(&runtime_channel_name(bind)),
        rust_string_literal(&runtime_channel_message_type(bind))
    )
}

fn runtime_channel_write(bind: &BindRuntimePlan, selected_backend: &str) -> String {
    let introspection_record = runtime_introspection_publish_record(bind);
    if selected_backend == "iox2" {
        return format!(
            "            if self.{field}.publish_at(value, tick_time_ms).is_err() {{\n                return flowrt::Status::Error;\n            }}\n{introspection_record}",
            field = bind.field_name
        );
    }

    match bind.channel {
        ChannelKind::Latest => {
            format!(
                "            self.{field}.publish_at(value, tick_time_ms);\n{introspection_record}",
                field = bind.field_name
            )
        }
        ChannelKind::Fifo => {
            format!(
                "            match self.{field}.push_at(value, tick_time_ms) {{\n                Ok(flowrt::ChannelWriteOutcome::Accepted) | Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {{\n{introspection_record}                }}\n                Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {{}},\n                Ok(flowrt::ChannelWriteOutcome::Backpressured) => return flowrt::Status::Retry,\n                Err(flowrt::ChannelError::Overflow) => return flowrt::Status::Error,\n            }}\n",
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

fn iox2_service_name_for_edge(
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
        iox2_service_part(package),
        iox2_service_part(graph),
        index,
        iox2_service_part(source_instance),
        iox2_service_part(source_port),
        iox2_service_part(target_instance),
        iox2_service_part(target_port),
    )
}

fn iox2_service_part(value: &str) -> String {
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

fn type_by_name<'a>(contract: &'a ContractIr, name: &str) -> &'a TypeIr {
    contract
        .types
        .iter()
        .find(|ty| ty.name == name)
        .expect("normalized contract must reference known message types")
}

fn message_sample_bytes(
    contract: &ContractIr,
    expectation: &MessageAbiExpectation,
    expectations_by_name: &BTreeMap<&str, &MessageAbiExpectation>,
) -> Vec<u8> {
    let ty = type_by_name(contract, &expectation.type_name);
    let mut bytes = vec![0u8; expectation.size_bytes];
    for (index, field) in ty.fields.iter().enumerate() {
        let field_expectation = &expectation.fields[index];
        let field_bytes =
            sample_bytes_for_expr(contract, expectations_by_name, &field.ty, index + 1);
        debug_assert_eq!(field_bytes.len(), field_expectation.size_bytes);
        let start = field_expectation.offset_bytes;
        let end = start + field_bytes.len();
        bytes[start..end].copy_from_slice(&field_bytes);
    }
    bytes
}

fn sample_bytes_for_expr(
    contract: &ContractIr,
    expectations_by_name: &BTreeMap<&str, &MessageAbiExpectation>,
    expr: &TypeExpr,
    seed: usize,
) -> Vec<u8> {
    match expr {
        TypeExpr::Primitive { name } => primitive_sample_bytes(*name, seed),
        TypeExpr::Named { name } => {
            let expectation = expectations_by_name
                .get(name.as_str())
                .copied()
                .expect("ABI expectation must exist for named message type");
            message_sample_bytes(contract, expectation, expectations_by_name)
        }
        TypeExpr::Array { element, len } => {
            let element_bytes =
                sample_bytes_for_expr(contract, expectations_by_name, element, seed);
            let mut bytes = Vec::with_capacity(element_bytes.len() * *len);
            for _ in 0..*len {
                bytes.extend_from_slice(&element_bytes);
            }
            bytes
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn primitive_sample_bytes(primitive: PrimitiveType, seed: usize) -> Vec<u8> {
    let value = ((seed % 9) + 1) as u128;
    match primitive {
        PrimitiveType::Bool => vec![1],
        PrimitiveType::U8 => vec![value as u8],
        PrimitiveType::U16 => (value as u16).to_le_bytes().to_vec(),
        PrimitiveType::U32 => (value as u32).to_le_bytes().to_vec(),
        PrimitiveType::U64 => (value as u64).to_le_bytes().to_vec(),
        PrimitiveType::U128 => value.to_le_bytes().to_vec(),
        PrimitiveType::I8 => vec![-(value as i8) as u8],
        PrimitiveType::I16 => (-(value as i16)).to_le_bytes().to_vec(),
        PrimitiveType::I32 => (-(value as i32)).to_le_bytes().to_vec(),
        PrimitiveType::I64 => (-(value as i64)).to_le_bytes().to_vec(),
        PrimitiveType::I128 => (-(value as i128)).to_le_bytes().to_vec(),
        PrimitiveType::F32 => ((value as f32) + 0.25).to_le_bytes().to_vec(),
        PrimitiveType::F64 => ((value as f64) + 0.25).to_le_bytes().to_vec(),
    }
}

fn byte_array_literal(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn expected_bytes_const_name(type_name: &str) -> String {
    format!(
        "EXPECTED_{}_BYTES",
        snake_identifier(type_name).to_uppercase()
    )
}

fn emit_rust_message_abi_tests(
    contract: &ContractIr,
    expectations: &[MessageAbiExpectation],
) -> String {
    let mut output = managed_header();
    let reads_cpp_fixtures = has_language(contract, LanguageKind::Cpp);
    let expectations_by_name = expectations
        .iter()
        .map(|expectation| (expectation.type_name.as_str(), expectation))
        .collect::<BTreeMap<_, _>>();
    output.push_str(
        "\nfn bytes_of<T>(value: &T) -> Vec<u8> {\n    let mut bytes = vec![0u8; std::mem::size_of::<T>()];\n    // Safety：生成测试只传入 FlowRT ABI v0.1 plain-data 消息，且 padding 已初始化。\n    unsafe {\n        std::ptr::copy_nonoverlapping(\n            (value as *const T).cast::<u8>(),\n            bytes.as_mut_ptr(),\n            bytes.len(),\n        );\n    }\n    bytes\n}\n\nfn assert_default_bytes_zero<T: Copy + Default>() {\n    let value = T::default();\n    assert_eq!(bytes_of(&value), vec![0u8; std::mem::size_of::<T>()]);\n}\n\nfn assert_byte_roundtrip<T: Copy + Default>(value: T) {\n    let bytes = bytes_of(&value);\n    let mut roundtrip = T::default();\n    // Safety：`roundtrip` 是有效 plain-data 存储，`bytes` 长度等于 `size_of::<T>()`。\n    unsafe {\n        std::ptr::copy_nonoverlapping(\n            bytes.as_ptr(),\n            (&mut roundtrip as *mut T).cast::<u8>(),\n            bytes.len(),\n        );\n    }\n    assert_eq!(bytes_of(&roundtrip), bytes);\n}\n\n",
    );
    if reads_cpp_fixtures {
        output.push_str(
            "fn assert_cpp_fixture_roundtrip<T: Copy + Default>(name: &str, expected: &[u8]) {\n    let path = std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\"))\n        .join(\"abi-fixtures\")\n        .join(\"cpp\")\n        .join(name);\n    let bytes = std::fs::read(&path).unwrap_or_else(|error| {\n        panic!(\"failed to read C++ ABI fixture `{}`: {error}\", path.display())\n    });\n    assert_eq!(bytes, expected);\n    assert_eq!(bytes.len(), std::mem::size_of::<T>());\n    let mut value = T::default();\n    // Safety：C++ fixture bytes 已按同一 Contract IR 的 Message ABI v0.1 写出。\n    unsafe {\n        std::ptr::copy_nonoverlapping(\n            bytes.as_ptr(),\n            (&mut value as *mut T).cast::<u8>(),\n            bytes.len(),\n        );\n    }\n    assert_eq!(bytes_of(&value), expected);\n}\n\n",
        );
    }
    output.push_str(
        "fn assert_sample_bytes<T: Copy>(value: T, expected: &[u8]) {\n    assert_eq!(bytes_of(&value), expected);\n}\n\n",
    );

    for expectation in expectations {
        let bytes = message_sample_bytes(contract, expectation, &expectations_by_name);
        output.push_str(&format!(
            "const {}: &[u8] = &[{}];\n",
            expected_bytes_const_name(&expectation.type_name),
            byte_array_literal(&bytes)
        ));
    }
    output.push('\n');

    for ty in ordered_types(contract) {
        output.push_str(&format!(
            "fn {}() -> flowrt_app::messages::{} {{\n",
            sample_function_name(&ty.name),
            ty.name
        ));
        output.push_str(&format!(
            "    let mut value = flowrt_app::messages::{}::default();\n",
            ty.name
        ));
        for (index, field) in ty.fields.iter().enumerate() {
            output.push_str(&format!(
                "    value.{} = {};\n",
                field.name,
                rust_sample_expr(&field.ty, index + 1)
            ));
        }
        output.push_str("    value\n}\n\n");
    }

    for expectation in expectations {
        let ty = format!("flowrt_app::messages::{}", expectation.type_name);
        output.push_str("#[test]\n");
        output.push_str(&format!(
            "fn {}_message_abi() {{\n",
            snake_identifier(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    assert_eq!(std::mem::size_of::<{}>(), {});\n",
            ty, expectation.size_bytes
        ));
        output.push_str(&format!(
            "    assert_eq!(std::mem::align_of::<{}>(), {});\n",
            ty, expectation.align_bytes
        ));
        output.push_str(&format!("    assert_default_bytes_zero::<{}>();\n", ty));
        for field in &expectation.fields {
            output.push_str(&format!(
                "    assert_eq!(std::mem::offset_of!({}, {}), {});\n",
                ty, field.name, field.offset_bytes
            ));
        }
        output.push_str(&format!(
            "    assert_byte_roundtrip({}());\n",
            sample_function_name(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    assert_sample_bytes({}(), {});\n",
            sample_function_name(&expectation.type_name),
            expected_bytes_const_name(&expectation.type_name)
        ));
        if reads_cpp_fixtures {
            output.push_str(&format!(
                "    assert_cpp_fixture_roundtrip::<{}>(\"{}.bin\", {});\n",
                ty,
                snake_identifier(&expectation.type_name),
                expected_bytes_const_name(&expectation.type_name)
            ));
        }
        output.push_str("}\n\n");
    }

    output
}

fn emit_cpp_message_abi_tests(
    contract: &ContractIr,
    expectations: &[MessageAbiExpectation],
) -> String {
    let mut output = managed_header();
    let expectations_by_name = expectations
        .iter()
        .map(|expectation| (expectation.type_name.as_str(), expectation))
        .collect::<BTreeMap<_, _>>();
    output.push_str(
        "\n#include <array>\n#include <cassert>\n#include <cstddef>\n#include <cstdint>\n#include <cstring>\n#include <filesystem>\n#include <fstream>\n#include <stdexcept>\n#include <string>\n#include <string_view>\n#include <type_traits>\n\n#include \"flowrt_app/messages.hpp\"\n\nnamespace {\n\ntemplate <typename T>\nstd::array<std::uint8_t, sizeof(T)> bytes_of(const T& value) {\n    std::array<std::uint8_t, sizeof(T)> bytes{};\n    std::memcpy(bytes.data(), &value, bytes.size());\n    return bytes;\n}\n\ntemplate <typename T>\nvoid assert_default_bytes_zero() {\n    T value{};\n    std::array<std::uint8_t, sizeof(T)> expected{};\n    assert(bytes_of(value) == expected);\n}\n\ntemplate <typename T>\nvoid assert_byte_roundtrip(const T& value) {\n    const auto bytes = bytes_of(value);\n    T roundtrip{};\n    std::memset(&roundtrip, 0, sizeof(roundtrip));\n    std::memcpy(&roundtrip, bytes.data(), bytes.size());\n    assert(std::memcmp(&roundtrip, &value, sizeof(T)) == 0);\n}\n\ntemplate <typename T, std::size_t N>\nvoid assert_sample_bytes(const T& value, const std::array<std::uint8_t, N>& expected) {\n    static_assert(sizeof(T) == N);\n    assert(bytes_of(value) == expected);\n}\n\ntemplate <std::size_t N>\nvoid write_fixture(std::string_view name, const std::array<std::uint8_t, N>& bytes) {\n#ifdef FLOWRT_ABI_FIXTURE_DIR\n    std::filesystem::create_directories(FLOWRT_ABI_FIXTURE_DIR);\n    auto path = std::filesystem::path(FLOWRT_ABI_FIXTURE_DIR) / std::string(name);\n    std::ofstream output(path, std::ios::binary);\n    if (!output) {\n        throw std::runtime_error(\"failed to open ABI fixture output\");\n    }\n    output.write(reinterpret_cast<const char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));\n    if (!output) {\n        throw std::runtime_error(\"failed to write ABI fixture output\");\n    }\n#else\n    (void)name;\n    (void)bytes;\n#endif\n}\n\n",
    );

    for expectation in expectations {
        let bytes = message_sample_bytes(contract, expectation, &expectations_by_name);
        output.push_str(&format!(
            "constexpr std::array<std::uint8_t, {}> {}{{{{{}}}}};\n",
            expectation.size_bytes,
            expected_bytes_const_name(&expectation.type_name),
            byte_array_literal(&bytes)
        ));
    }
    output.push('\n');

    for ty in ordered_types(contract) {
        output.push_str(&format!(
            "flowrt_app::{} {}() {{\n",
            ty.name,
            sample_function_name(&ty.name)
        ));
        output.push_str(&format!("    flowrt_app::{} value{{}};\n", ty.name));
        output.push_str("    std::memset(&value, 0, sizeof(value));\n");
        for (index, field) in ty.fields.iter().enumerate() {
            output.push_str(&format!(
                "    value.{} = {};\n",
                field.name,
                cpp_sample_expr(&field.ty, index + 1)
            ));
        }
        output.push_str("    return value;\n}\n\n");
    }

    for expectation in expectations {
        let ty = format!("flowrt_app::{}", expectation.type_name);
        output.push_str(&format!(
            "void test_{}_message_abi() {{\n",
            snake_identifier(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    static_assert(std::is_standard_layout_v<{}>);\n",
            ty
        ));
        output.push_str(&format!(
            "    static_assert(std::is_trivially_copyable_v<{}>);\n",
            ty
        ));
        output.push_str(&format!(
            "    static_assert(sizeof({}) == {});\n",
            ty, expectation.size_bytes
        ));
        output.push_str(&format!(
            "    static_assert(alignof({}) == {});\n",
            ty, expectation.align_bytes
        ));
        output.push_str(&format!("    assert_default_bytes_zero<{}>();\n", ty));
        for field in &expectation.fields {
            output.push_str(&format!(
                "    static_assert(offsetof({}, {}) == {});\n",
                ty, field.name, field.offset_bytes
            ));
        }
        output.push_str(&format!(
            "    assert_byte_roundtrip({}());\n",
            sample_function_name(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    assert_sample_bytes({}(), {});\n",
            sample_function_name(&expectation.type_name),
            expected_bytes_const_name(&expectation.type_name)
        ));
        output.push_str(&format!(
            "    write_fixture(\"{}.bin\", bytes_of({}()));\n",
            snake_identifier(&expectation.type_name),
            sample_function_name(&expectation.type_name)
        ));
        output.push_str("}\n\n");
    }

    output.push_str("}  // namespace\n\nint main() {\n");
    for expectation in expectations {
        output.push_str(&format!(
            "    test_{}_message_abi();\n",
            snake_identifier(&expectation.type_name)
        ));
    }
    output.push_str("    return 0;\n}\n");
    output
}

fn emit_launch_manifest(contract: &ContractIr) -> Result<String> {
    let selected_backend = selected_backend_name(contract);
    let launch = serde_json::json!({
        "package": contract.package.name,
        "ir_version": contract.ir_version,
        "profiles": contract.profiles.iter().map(|profile| &profile.name).collect::<Vec<_>>(),
        "targets": contract.targets.iter().map(|target| &target.name).collect::<Vec<_>>(),
        "graphs": contract.graphs.iter().map(|graph| serde_json::json!({
            "name": graph.name,
            "processes": launch_processes(contract, graph, &selected_backend),
            "channels": launch_channels(contract, graph, &selected_backend),
            "instances": graph.instances.iter().map(|instance| {
                let component = component_by_name(contract, &instance.component.name);
                serde_json::json!({
                    "name": instance.name,
                    "component": instance.component.name,
                    "runtime": language_name(component.language),
                    "process": instance.process,
                    "target": instance.target.as_ref().map(|target| &target.name),
                })
            }).collect::<Vec<_>>(),
            "tasks": graph.tasks.iter().map(launch_task).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    let mut output = serde_json::to_string_pretty(&launch)?;
    output.push('\n');
    Ok(output)
}

fn launch_channels(
    contract: &ContractIr,
    graph: &GraphIr,
    backend: &str,
) -> Vec<serde_json::Value> {
    graph
        .binds
        .iter()
        .enumerate()
        .map(|(index, bind)| {
            let service = (backend == "iox2")
                .then(|| iox2_service_name_for_edge(contract, graph, index, bind));
            serde_json::json!({
                "from": format!("{}.{}", bind.from.instance.name, bind.from.port),
                "to": format!("{}.{}", bind.to.instance.name, bind.to.port),
                "backend": backend,
                "service": service,
                "channel": bind.channel,
                "depth": bind.depth,
                "overflow": bind.overflow,
                "stale_policy": bind.stale,
                "max_age_ms": bind.max_age_ms,
            })
        })
        .collect()
}

fn launch_processes(
    contract: &ContractIr,
    graph: &GraphIr,
    backend: &str,
) -> Vec<serde_json::Value> {
    let mut processes = BTreeMap::<String, Vec<&InstanceIr>>::new();
    for instance in &graph.instances {
        processes
            .entry(
                instance
                    .process
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
            )
            .or_default()
            .push(instance);
    }

    processes
        .into_iter()
        .map(|(name, instances)| {
            let instance_names = instances
                .iter()
                .map(|instance| instance.name.as_str())
                .collect::<BTreeSet<_>>();
            let runtimes = process_runtimes(contract, &instances);
            let target = common_process_target(&instances);
            serde_json::json!({
                "name": name,
                "backend": backend,
                "target": target,
                "runtimes": runtimes,
                "runtime_kind": process_runtime_kind(&runtimes),
                "instances": instances.iter().map(|instance| &instance.name).collect::<Vec<_>>(),
                "tasks": graph.tasks.iter().filter(|task| instance_names.contains(task.instance.name.as_str())).map(launch_task).collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn launch_task(task: &TaskIr) -> serde_json::Value {
    serde_json::json!({
        "instance": task.instance.name,
        "trigger": task.trigger,
        "period_ms": task.period_ms,
        "deadline_ms": task.deadline_ms,
        "priority": task.priority,
        "inputs": task.inputs,
        "outputs": task.outputs,
    })
}

fn process_runtimes(contract: &ContractIr, instances: &[&InstanceIr]) -> Vec<&'static str> {
    instances
        .iter()
        .map(|instance| component_by_name(contract, &instance.component.name))
        .map(|component| language_name(component.language))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn process_runtime_kind(runtimes: &[&'static str]) -> &'static str {
    if runtimes.len() == 1 {
        runtimes[0]
    } else {
        "mixed"
    }
}

fn language_name(language: LanguageKind) -> &'static str {
    match language {
        LanguageKind::Cpp => "cpp",
        LanguageKind::Rust => "rust",
    }
}

fn common_process_target(instances: &[&InstanceIr]) -> Option<String> {
    let mut targets = instances
        .iter()
        .filter_map(|instance| instance.target.as_ref().map(|target| target.name.clone()))
        .collect::<BTreeSet<_>>();

    if targets.len() == 1 {
        targets.pop_first()
    } else {
        None
    }
}

fn emit_cmake(contract: &ContractIr) -> String {
    let package_name = sanitize_package_name(&contract.package.name);
    let mut output = format!(
        "# FlowRT 管理产物。不要手工修改。\ncmake_minimum_required(VERSION 3.20)\nproject({}_flowrt_app LANGUAGES CXX)\n\nset(CMAKE_EXPORT_COMPILE_COMMANDS ON)\n\nadd_library({}_flowrt_app INTERFACE)\ntarget_compile_features({}_flowrt_app INTERFACE cxx_std_20)\ntarget_include_directories({}_flowrt_app INTERFACE ${{CMAKE_CURRENT_LIST_DIR}}/../cpp/include)\n",
        package_name, package_name, package_name, package_name
    );

    if has_language(contract, LanguageKind::Cpp) {
        let shell_target = format!("{}_cpp_shell", package_name.replace('-', "_"));
        let app_target = format!("{}_cpp_app", package_name.replace('-', "_"));
        if selected_backend_name(contract) == "iox2" {
            output.push_str("\nfind_package(iceoryx2-cxx 0.9.1 REQUIRED)\n");
            output.push_str(&format!(
                "target_link_libraries({package_name}_flowrt_app INTERFACE iceoryx2-cxx::static-lib-cxx)\n"
            ));
            output.push_str(&format!(
                "target_compile_definitions({package_name}_flowrt_app INTERFACE FLOWRT_HAS_ICEORYX2_CXX=1)\n"
            ));
        }
        output.push_str(
            "\nset(FLOWRT_CPP_RUNTIME_DIR \"\" CACHE PATH \"FlowRT C++ runtime root containing include/flowrt/runtime.hpp\")\n",
        );
        output.push_str(
            "if(NOT FLOWRT_CPP_RUNTIME_DIR)\n    get_filename_component(_flowrt_repo_runtime \"${CMAKE_CURRENT_LIST_DIR}/../../../../runtime/cpp\" ABSOLUTE)\n    if(EXISTS \"${_flowrt_repo_runtime}/include/flowrt/runtime.hpp\")\n        set(FLOWRT_CPP_RUNTIME_DIR \"${_flowrt_repo_runtime}\")\n    endif()\nendif()\n",
        );
        output.push_str(
            "if(NOT FLOWRT_CPP_RUNTIME_DIR OR NOT EXISTS \"${FLOWRT_CPP_RUNTIME_DIR}/include/flowrt/runtime.hpp\")\n    message(FATAL_ERROR \"FLOWRT_CPP_RUNTIME_DIR must point to FlowRT runtime/cpp\")\nendif()\n",
        );
        output.push_str(&format!(
            "target_include_directories({package_name}_flowrt_app INTERFACE ${{FLOWRT_CPP_RUNTIME_DIR}}/include)\n"
        ));
        output.push_str(&format!(
            "\nadd_library({shell_target} STATIC ../cpp/src/runtime_shell.cpp ../cpp/src/selfdesc.cpp)\n"
        ));
        output.push_str(&format!(
            "target_link_libraries({shell_target} PUBLIC {package_name}_flowrt_app)\n"
        ));
        output.push_str(
            "\nfile(GLOB FLOWRT_DEFAULT_USER_CPP_SOURCES CONFIGURE_DEPENDS \"${CMAKE_CURRENT_LIST_DIR}/../../src/cpp/*.cpp\")\nset(FLOWRT_USER_CPP_SOURCES ${FLOWRT_DEFAULT_USER_CPP_SOURCES} CACHE STRING \"User C++ sources that implement flowrt_user::build_app\")\n",
        );
        output.push_str("if(FLOWRT_USER_CPP_SOURCES)\n");
        let user_target = format!("{}_cpp_user", package_name.replace('-', "_"));
        output.push_str(&format!(
            "    add_library({user_target} STATIC ${{FLOWRT_USER_CPP_SOURCES}})\n"
        ));
        output.push_str(&format!(
            "    target_link_libraries({user_target} PUBLIC {package_name}_flowrt_app)\n"
        ));
        output.push_str(&format!(
            "    add_executable({app_target} ../cpp/src/main.cpp)\n"
        ));
        output.push_str(&format!(
            "    target_link_libraries({app_target} PRIVATE {shell_target} {user_target})\n"
        ));
        output.push_str("endif()\n");
    }

    if has_language(contract, LanguageKind::Cpp) && !contract.types.is_empty() {
        let test_target = format!("{}_message_abi", package_name.replace('-', "_"));
        output.push_str("\ninclude(CTest)\nif(BUILD_TESTING)\n");
        output.push_str(&format!(
            "    add_executable({test_target} ../cpp/tests/message_abi.cpp)\n"
        ));
        output.push_str(&format!(
            "    target_link_libraries({test_target} PRIVATE {package_name}_flowrt_app)\n"
        ));
        output.push_str(
            "    set(FLOWRT_ABI_CPP_FIXTURE_DIR \"${CMAKE_CURRENT_LIST_DIR}/abi-fixtures/cpp\")\n",
        );
        output.push_str(&format!(
            "    target_compile_definitions({test_target} PRIVATE FLOWRT_ABI_FIXTURE_DIR=\"${{FLOWRT_ABI_CPP_FIXTURE_DIR}}\")\n"
        ));
        output.push_str(&format!(
            "    add_custom_command(TARGET {test_target} POST_BUILD\n        COMMAND $<TARGET_FILE:{test_target}>\n        COMMENT \"Generate C++ Message ABI cross-language fixtures\")\n"
        ));
        output.push_str(&format!(
            "    add_test(NAME message_abi COMMAND {test_target})\n"
        ));
        output.push_str("endif()\n");
    }

    output
}

fn emit_cargo_manifest(contract: &ContractIr) -> String {
    let package_name = sanitize_package_name(&contract.package.name).replace('_', "-");
    let has_rust = has_language(contract, LanguageKind::Rust);
    let has_supervisor = has_rust || has_language(contract, LanguageKind::Cpp);
    let mut output = format!(
        "# FlowRT 管理产物。不要手工修改。\n[package]\nname = \"{}-flowrt-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n\n[lib]\nname = \"flowrt_app\"\npath = \"../rust/src/lib.rs\"\n\n[dependencies]\n",
        package_name
    );
    let mut bins = String::new();

    if has_rust {
        let flowrt_dependency = if selected_backend_name(contract) == "iox2" {
            "flowrt = { version = \"0.1\", features = [\"iox2\"] }"
        } else {
            "flowrt = { version = \"0.1\" }"
        };
        output.push_str(flowrt_dependency);
        output.push('\n');
        bins.push_str(&format!(
            "\n[[bin]]\nname = \"{}-flowrt-app\"\npath = \"../rust/src/main.rs\"\n",
            package_name
        ));
    }

    if has_supervisor {
        output
            .push_str("serde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n");
        bins.push_str(&format!(
            "\n[[bin]]\nname = \"{}-flowrt-supervisor\"\npath = \"../rust/src/supervisor_main.rs\"\n",
            package_name
        ));
    }
    output.push_str(&bins);

    if has_rust && !contract.types.is_empty() {
        output.push_str(
            "\n[[test]]\nname = \"message_abi\"\npath = \"../rust/tests/message_abi.rs\"\n",
        );
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

fn cpp_type(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive { name } => cpp_primitive(*name).to_string(),
        TypeExpr::Named { name } => name.clone(),
        TypeExpr::Array { element, len } => {
            format!("std::array<{}, {}>", cpp_type(element), len)
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn cpp_primitive(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Bool => "bool",
        PrimitiveType::U8 => "std::uint8_t",
        PrimitiveType::U16 => "std::uint16_t",
        PrimitiveType::U32 => "std::uint32_t",
        PrimitiveType::U64 => "std::uint64_t",
        PrimitiveType::U128 => "unsigned __int128",
        PrimitiveType::I8 => "std::int8_t",
        PrimitiveType::I16 => "std::int16_t",
        PrimitiveType::I32 => "std::int32_t",
        PrimitiveType::I64 => "std::int64_t",
        PrimitiveType::I128 => "__int128",
        PrimitiveType::F32 => "float",
        PrimitiveType::F64 => "double",
    }
}

fn rust_type(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive { name } => rust_primitive(*name).to_string(),
        TypeExpr::Named { name } => name.clone(),
        TypeExpr::Array { element, len } => format!("[{}; {}]", rust_type(element), len),
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn rust_primitive(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Bool => "bool",
        PrimitiveType::U8 => "u8",
        PrimitiveType::U16 => "u16",
        PrimitiveType::U32 => "u32",
        PrimitiveType::U64 => "u64",
        PrimitiveType::U128 => "u128",
        PrimitiveType::I8 => "i8",
        PrimitiveType::I16 => "i16",
        PrimitiveType::I32 => "i32",
        PrimitiveType::I64 => "i64",
        PrimitiveType::I128 => "i128",
        PrimitiveType::F32 => "f32",
        PrimitiveType::F64 => "f64",
    }
}

fn rust_sample_expr(expr: &TypeExpr, seed: usize) -> String {
    match expr {
        TypeExpr::Primitive { name } => rust_primitive_sample(*name, seed),
        TypeExpr::Named { name } => format!("{}()", sample_function_name(name)),
        TypeExpr::Array { element, len } => {
            format!("[{}; {}]", rust_sample_expr(element, seed), len)
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn rust_primitive_sample(primitive: PrimitiveType, seed: usize) -> String {
    let value = (seed % 9) + 1;
    match primitive {
        PrimitiveType::Bool => "true".to_string(),
        PrimitiveType::U8 => format!("{value}u8"),
        PrimitiveType::U16 => format!("{value}u16"),
        PrimitiveType::U32 => format!("{value}u32"),
        PrimitiveType::U64 => format!("{value}u64"),
        PrimitiveType::U128 => format!("{value}u128"),
        PrimitiveType::I8 => format!("-{value}i8"),
        PrimitiveType::I16 => format!("-{value}i16"),
        PrimitiveType::I32 => format!("-{value}i32"),
        PrimitiveType::I64 => format!("-{value}i64"),
        PrimitiveType::I128 => format!("-{value}i128"),
        PrimitiveType::F32 => format!("{value}.25f32"),
        PrimitiveType::F64 => format!("{value}.25f64"),
    }
}

fn cpp_sample_expr(expr: &TypeExpr, seed: usize) -> String {
    match expr {
        TypeExpr::Primitive { name } => cpp_primitive_sample(*name, seed),
        TypeExpr::Named { name } => format!("{}()", sample_function_name(name)),
        TypeExpr::Array { element, len: _ } => {
            format!(
                "[] {{ auto value = {}{{}}; value.fill({}); return value; }}()",
                cpp_type(expr),
                cpp_sample_expr(element, seed)
            )
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            panic!(
                "validated Message ABI v0.1 contract must not contain {}",
                expr.canonical_syntax()
            )
        }
    }
}

fn cpp_primitive_sample(primitive: PrimitiveType, seed: usize) -> String {
    let value = (seed % 9) + 1;
    match primitive {
        PrimitiveType::Bool => "true".to_string(),
        PrimitiveType::U8 => format!("std::uint8_t{{{value}}}"),
        PrimitiveType::U16 => format!("std::uint16_t{{{value}}}"),
        PrimitiveType::U32 => format!("std::uint32_t{{{value}}}"),
        PrimitiveType::U64 => format!("std::uint64_t{{{value}}}"),
        PrimitiveType::U128 => format!("static_cast<unsigned __int128>({value})"),
        PrimitiveType::I8 => format!("std::int8_t{{-{value}}}"),
        PrimitiveType::I16 => format!("std::int16_t{{-{value}}}"),
        PrimitiveType::I32 => format!("std::int32_t{{-{value}}}"),
        PrimitiveType::I64 => format!("std::int64_t{{-{value}}}"),
        PrimitiveType::I128 => format!("static_cast<__int128>(-{value})"),
        PrimitiveType::F32 => format!("{value}.25F"),
        PrimitiveType::F64 => format!("{value}.25"),
    }
}

fn sample_function_name(type_name: &str) -> String {
    format!("sample_{}", snake_identifier(type_name))
}

fn snake_identifier(name: &str) -> String {
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

fn sanitize_package_name(name: &str) -> String {
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

fn managed_header() -> String {
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
mod tests {
    use flowrt_ir::{hash_source, normalize_document};
    use flowrt_rsdl::parse_str;

    use super::*;

    #[test]
    fn plans_rust_artifacts_for_rust_component() {
        let ir = contract_from_source(
            r#"
[package]
name = "demo"
rsdl_version = "0.1"

[component.monitor]
language = "rust"
"#,
        );
        let plan = plan_codegen(&ir);
        assert_eq!(plan.units.len(), 1);
        assert_eq!(plan.units[0].language, CodegenLanguage::Rust);
    }

    #[test]
    fn rejects_contract_without_exactly_one_graph() {
        let mut ir = contract_from_source(
            r#"
[package]
name = "demo"
rsdl_version = "0.1"

[component.monitor]
language = "rust"
"#,
        );
        ir.graphs.clear();

        let error = emit_artifacts(&ir).expect_err("codegen should reject a graphless contract");
        assert!(
            error
                .to_string()
                .contains("Contract IR v0.1 must contain exactly one graph; found 0"),
            "{error}"
        );

        let mut ir = contract_from_source(
            r#"
[package]
name = "demo"
rsdl_version = "0.1"

[component.monitor]
language = "rust"
"#,
        );
        ir.graphs.push(ir.graphs[0].clone());

        let error = emit_artifacts(&ir).expect_err("codegen should reject multiple graphs");
        assert!(
            error
                .to_string()
                .contains("Contract IR v0.1 must contain exactly one graph; found 2"),
            "{error}"
        );
    }

    #[test]
    fn rejects_invalid_contract_before_emitting_artifacts() {
        let mut ir = contract_from_source(
            r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#,
        );
        ir.graphs[0].binds[0].from.port = "missing".to_string();

        let result = std::panic::catch_unwind(|| emit_artifacts(&ir));

        assert!(result.is_ok(), "codegen should return an error, not panic");
        let error = result
            .expect("codegen invocation should not panic")
            .expect_err("invalid Contract IR should be rejected before emission");
        assert!(
            error
                .to_string()
                .contains("instance `source` component `source` has no Output port `missing`"),
            "{error}"
        );
    }

    #[test]
    fn rejects_variable_frame_message_fields_before_emitting_artifacts() {
        let ir = contract_from_source(
            r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes<max=262144>"

[component.source]
language = "rust"
output = ["packet:Packet"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]
"#,
        );

        let result = std::panic::catch_unwind(|| emit_artifacts(&ir));

        assert!(result.is_ok(), "codegen should return an error, not panic");
        let error = result
            .expect("codegen invocation should not panic")
            .expect_err("Variable Frame ABI fields should fail validation before emission");
        assert!(matches!(&error, CodegenError::Validation(_)));
        assert!(
            error.to_string().contains(
                "type `Packet` field `payload` uses bytes<max=262144>, which requires the future Variable Frame ABI",
            ),
            "{error}"
        );
    }

    #[test]
    fn emits_cpp_and_rust_application_artifacts() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[type.Cmd]
left = "f32"
right = "f32"

[component.controller]
language = "cpp"
input = ["imu:Imu"]
output = ["cmd:Cmd"]

[component.monitor]
language = "rust"
input = ["imu:Imu"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();

        let paths = bundle
            .artifacts
            .iter()
            .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(paths.contains(&"cpp/include/flowrt_app/messages.hpp".to_string()));
        assert!(paths.contains(&"cpp/include/flowrt_app/selfdesc.hpp".to_string()));
        assert!(paths.contains(&"cpp/src/selfdesc.cpp".to_string()));
        assert!(paths.contains(&"rust/src/selfdesc.rs".to_string()));
        assert!(paths.contains(&"rust/src/components.rs".to_string()));
        assert!(paths.contains(&"cpp/tests/message_abi.cpp".to_string()));
        assert!(paths.contains(&"rust/tests/message_abi.rs".to_string()));
        assert!(paths.contains(&"selfdesc/selfdesc.json".to_string()));
        assert!(paths.contains(&"launch/launch.json".to_string()));

        let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
        assert!(cpp_messages.contains("struct Imu"));
        assert!(cpp_messages.contains("std::uint64_t timestamp{};"));

        let rust_components = artifact_content(&bundle, "rust/src/components.rs");
        assert!(rust_components.contains("pub trait Monitor"));
        assert!(!rust_components.contains("pub trait Controller"));
        assert!(rust_components.contains("imu: flowrt::Latest<'_, Imu>"));

        let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
        assert!(rust_messages.contains("impl Default for Imu"));
        assert!(rust_messages.contains("std::mem::zeroed()"));

        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        assert!(rust_shell.contains("const SELECTED_BACKEND: &str = \"inproc\";"));
        assert!(rust_shell.contains("const PACKAGE_NAME: &str = \"robot_demo\";"));
        assert!(rust_shell.contains("flowrt::spawn_status_server("));
        assert!(
            rust_shell.contains("let introspection_state = flowrt::IntrospectionState::new();")
        );
        assert!(rust_shell.contains("introspection_state.record_tick();"));
        assert!(!rust_shell.contains("flowrt::IntrospectionStatus {"));
        assert!(rust_shell.contains("selfdesc::self_description_hash().to_string()"));

        let sidecar: serde_json::Value =
            serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
        assert_eq!(sidecar["self_description_version"], "0.1");
        assert_eq!(sidecar["package"]["name"], "robot_demo");
        assert_eq!(sidecar["graphs"][0]["name"], "default");
        assert!(
            sidecar["graphs"][0]["instances"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(sidecar["message_abi"][0]["type_name"], "Cmd");

        let rust_selfdesc = artifact_content(&bundle, "rust/src/selfdesc.rs");
        assert!(rust_selfdesc.contains("#[unsafe(link_section = \".flowrt.selfdesc\")]"));
        assert!(rust_selfdesc.contains("static FLOWRT_SELF_DESCRIPTION"));
        assert!(rust_selfdesc.contains("= *br#"));
        assert!(!rust_selfdesc.contains("*bbr#"));

        let cpp_selfdesc = artifact_content(&bundle, "cpp/src/selfdesc.cpp");
        assert!(cpp_selfdesc.contains("[[gnu::used, gnu::section(\".flowrt.selfdesc\")]]"));
        assert!(cpp_selfdesc.contains("const char kFlowrtSelfDescription[]"));
        assert!(rust_shell.contains("flowrt::iox2_backend()"));
    }

    #[test]
    fn generated_shells_cleanup_entered_lifecycle_stages_in_reverse_order() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.cpp_alpha]
language = "cpp"

[component.cpp_beta]
language = "cpp"

[component.rust_alpha]
language = "rust"

[component.rust_beta]
language = "rust"

[instance.cpp_alpha]
component = "cpp_alpha"

[instance.cpp_alpha.task]
trigger = "periodic"
period_ms = 5

[instance.cpp_beta]
component = "cpp_beta"

[instance.cpp_beta.task]
trigger = "periodic"
period_ms = 5

[instance.rust_alpha]
component = "rust_alpha"

[instance.rust_alpha.task]
trigger = "periodic"
period_ms = 5

[instance.rust_beta]
component = "rust_beta"

[instance.rust_beta.task]
trigger = "periodic"
period_ms = 5
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

        assert!(cpp_shell.contains("auto status = flowrt::Status::Ok;"));
        assert!(cpp_shell.contains("bool cpp_alpha_initialized = false;"));
        assert!(cpp_shell.contains("bool cpp_alpha_started = false;"));
        assert!(cpp_shell.contains("if (status == flowrt::Status::Ok) {"));
        assert!(
            cpp_shell
                .contains("if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok)")
        );
        assert!(cpp_shell.contains(
            "if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok)"
        ));
        assert!(cpp_shell.contains("return status;"));
        assert!(!cpp_shell.contains("if (status != flowrt::Status::Ok) {\n        return status;"));
        assert!(
            cpp_shell
                .find("if (cpp_beta_started && cpp_beta_)")
                .unwrap()
                < cpp_shell
                    .find("if (cpp_alpha_started && cpp_alpha_)")
                    .unwrap()
        );
        assert!(
            cpp_shell
                .find("if (cpp_beta_initialized && cpp_beta_)")
                .unwrap()
                < cpp_shell
                    .find("if (cpp_alpha_initialized && cpp_alpha_)")
                    .unwrap()
        );

        assert!(rust_shell.contains("let mut status = flowrt::Status::Ok;"));
        assert!(rust_shell.contains("let mut rust_alpha_initialized = false;"));
        assert!(rust_shell.contains("let mut rust_alpha_started = false;"));
        assert!(rust_shell.contains("if status == flowrt::Status::Ok {"));
        assert!(
            rust_shell
                .contains("if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok")
        );
        assert!(
            rust_shell.contains(
                "if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok"
            )
        );
        assert!(rust_shell.contains("        status\n    }\n"));
        assert!(
            !rust_shell.contains(
                "if status != flowrt::Status::Ok {\n            return status;\n        }"
            )
        );
        assert!(
            rust_shell.find("if rust_beta_started {").unwrap()
                < rust_shell.find("if rust_alpha_started {").unwrap()
        );
        assert!(
            rust_shell.find("if rust_beta_initialized {").unwrap()
                < rust_shell.find("if rust_alpha_initialized {").unwrap()
        );
    }

    #[test]
    fn message_abi_tests_embed_cross_language_byte_fixtures() {
        let ir = contract_from_source(
            r#"
[package]
name = "abi_demo"
rsdl_version = "0.1"

[type.Packet]
tag = "u8"
count = "u32"
temperature = "f32"

[component.producer]
language = "rust"
output = ["packet:Packet"]

[component.consumer]
language = "cpp"
input = ["packet:Packet"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();

        let rust_abi = artifact_content(&bundle, "rust/tests/message_abi.rs");
        assert!(rust_abi.contains("const EXPECTED_PACKET_BYTES: &[u8] = &["));
        assert!(rust_abi.contains("2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 136, 64"));
        assert!(rust_abi.contains("assert_sample_bytes(sample_packet(), EXPECTED_PACKET_BYTES);"));
        assert!(rust_abi.contains("fn assert_cpp_fixture_roundtrip<T: Copy + Default>"));
        assert!(rust_abi.contains(
            "assert_cpp_fixture_roundtrip::<flowrt_app::messages::Packet>(\"packet.bin\", EXPECTED_PACKET_BYTES);"
        ));

        let cpp_abi = artifact_content(&bundle, "cpp/tests/message_abi.cpp");
        assert!(cpp_abi.contains("constexpr std::array<std::uint8_t, 12> EXPECTED_PACKET_BYTES"));
        assert!(cpp_abi.contains("2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 136, 64"));
        assert!(cpp_abi.contains("assert_sample_bytes(sample_packet(), EXPECTED_PACKET_BYTES);"));
        assert!(cpp_abi.contains("write_fixture(\"packet.bin\", bytes_of(sample_packet()));"));

        let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
        assert!(cmake.contains(
            "target_compile_definitions(abi_demo_message_abi PRIVATE FLOWRT_ABI_FIXTURE_DIR="
        ));
        assert!(cmake.contains("add_custom_command(TARGET abi_demo_message_abi POST_BUILD"));
    }

    #[test]
    fn message_abi_tests_assert_default_initialization_zeroes_padding_bytes() {
        let ir = contract_from_source(
            r#"
[package]
name = "abi_demo"
rsdl_version = "0.1"

[type.Padded]
flag = "bool"
count = "u32"

[component.producer]
language = "rust"
output = ["padded:Padded"]

[component.consumer]
language = "cpp"
input = ["padded:Padded"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();

        let rust_abi = artifact_content(&bundle, "rust/tests/message_abi.rs");
        assert!(rust_abi.contains("fn assert_default_bytes_zero<T: Copy + Default>()"));
        assert!(rust_abi.contains("assert_default_bytes_zero::<flowrt_app::messages::Padded>();"));

        let cpp_abi = artifact_content(&bundle, "cpp/tests/message_abi.cpp");
        assert!(cpp_abi.contains("void assert_default_bytes_zero()"));
        assert!(cpp_abi.contains("std::array<std::uint8_t, sizeof(T)> expected{};"));
        assert!(cpp_abi.contains("assert(bytes_of(value) == expected);"));
        assert!(cpp_abi.contains("assert_default_bytes_zero<flowrt_app::Padded>();"));
    }

    #[test]
    fn profile_selection_projects_selected_backend_into_generated_artifacts() {
        let ir = contract_from_source(
            r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[profile.iox2]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );
        let projected = flowrt_ir::project_contract_to_profile(&ir, Some("iox2")).unwrap();
        let bundle = emit_artifacts(&projected).unwrap();
        let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
        assert!(cargo_manifest.contains("features = [\"iox2\"]"));
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        assert!(rust_shell.contains("const SELECTED_BACKEND: &str = \"iox2\";"));
    }

    #[test]
    fn iox2_runtime_shell_omits_tick_timestamp_for_empty_bind_graphs() {
        let ir = contract_from_source(
            r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "main"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 1

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

        assert!(!rust_shell.contains("let tick_time_ms = tick as u64;"));
    }

    #[test]
    fn mixed_rust_shell_does_not_invent_traits_for_cpp_components() {
        let ir = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "cpp_source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "rust_sink"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
        let rust_components = artifact_content(&bundle, "rust/src/components.rs");
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

        assert!(!rust_components.contains("pub trait Source"));
        assert!(rust_components.contains("pub trait Sink"));
        assert!(!rust_shell.contains("source: Box<dyn Source>"));
        assert!(rust_shell.contains("sink: Box<dyn Sink>"));
        assert!(!rust_shell.contains("mixed-language runtime shell is not implemented"));
        assert!(rust_shell.contains("flowrt::iox2::Iox2PubSub<u32>"));
        assert!(rust_shell.contains("receive_latest_at(tick_time_ms)"));
        assert!(!cpp_header.contains("std::unique_ptr<SinkInterface>"));
        assert!(cpp_header.contains("std::unique_ptr<SourceInterface> source"));
        assert!(!cpp_shell.contains("return flowrt::ok();"));
        assert!(cpp_shell.contains("flowrt::iox2::Iox2PubSub<std::uint32_t>"));
        assert!(cpp_shell.contains("bind_0_.publish_at(*value, tick_time_ms)"));
    }

    #[test]
    fn emits_cpp_managed_app_targets() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Odom]
timestamp = "u64"
x = "f32"

[type.Cmd]
left = "f32"
right = "f32"

[component.source]
language = "cpp"
output = ["odom:Odom"]

[component.controller]
language = "cpp"
input = ["odom:Odom"]
output = ["cmd:Cmd"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["odom"]

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "on_message"
input = ["odom"]
output = ["cmd"]

[[bind.dataflow]]
from = "source.odom"
to = "controller.odom"
channel = "latest"
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let paths = bundle
            .artifacts
            .iter()
            .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(paths.contains(&"cpp/include/flowrt_app/runtime_shell.hpp".to_string()));
        assert!(paths.contains(&"cpp/src/runtime_shell.cpp".to_string()));
        assert!(paths.contains(&"cpp/src/main.cpp".to_string()));

        let runtime_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
        assert!(runtime_header.contains("#include <memory>"));
        assert!(runtime_header.contains("class App"));
        assert!(runtime_header.contains("std::unique_ptr<SourceInterface> source"));
        assert!(runtime_header.contains("flowrt::Status run(const flowrt::Backend& backend);"));
        assert!(runtime_header.contains("namespace flowrt_user"));
        assert!(runtime_header.contains("flowrt_app::App build_app();"));

        let runtime_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
        assert!(runtime_shell.contains("#include \"flowrt_app/runtime_shell.hpp\""));
        assert!(runtime_shell.contains("App::App("));
        assert!(runtime_shell.contains("bind_0_"));
        assert!(runtime_shell.contains("flowrt::Output<Odom> source_odom;"));
        assert!(
            runtime_shell.contains("const auto controller_odom = bind_0_.view_at(tick_time_ms);")
        );
        assert!(runtime_shell.contains("source_->on_tick(source_odom)"));
        assert!(runtime_shell.contains("controller_->on_tick(controller_odom, controller_cmd)"));
        assert!(runtime_shell.contains("flowrt_user::build_app().run(backend);"));

        let main = artifact_content(&bundle, "cpp/src/main.cpp");
        assert!(main.contains("#include \"flowrt_app/runtime_shell.hpp\""));
        assert!(main.contains("std::string_view process;"));
        assert!(main.contains("flowrt_app::run_process(process)"));

        let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
        assert!(cmake.contains("set(CMAKE_EXPORT_COMPILE_COMMANDS ON)"));
        assert!(cmake.contains("FLOWRT_CPP_RUNTIME_DIR"));
        assert!(cmake.contains(
            "add_library(robot_demo_cpp_shell STATIC ../cpp/src/runtime_shell.cpp ../cpp/src/selfdesc.cpp)"
        ));
        assert!(
            cmake.contains(
                "target_link_libraries(robot_demo_cpp_shell PUBLIC robot_demo_flowrt_app)"
            )
        );
        assert!(cmake.contains("FLOWRT_USER_CPP_SOURCES"));
        assert!(cmake.contains("add_library(robot_demo_cpp_user STATIC"));
        assert!(cmake.contains("add_executable(robot_demo_cpp_app ../cpp/src/main.cpp)"));
    }

    #[test]
    fn emits_supervisor_only_rust_crate_for_cpp_only_launch() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "cpp"
input = ["value:u32"]

[instance.source]
component = "source"
process = "control"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let paths = bundle
            .artifacts
            .iter()
            .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(paths.contains(&"rust/src/supervisor.rs".to_string()));
        assert!(paths.contains(&"rust/src/supervisor_main.rs".to_string()));
        assert!(paths.contains(&"rust/src/lib.rs".to_string()));
        assert!(paths.contains(&"rust/src/selfdesc.rs".to_string()));
        assert!(!paths.contains(&"rust/src/runtime_shell.rs".to_string()));
        assert!(!paths.contains(&"rust/src/main.rs".to_string()));

        let rust_lib = artifact_content(&bundle, "rust/src/lib.rs");
        assert!(rust_lib.contains("pub(crate) mod selfdesc;"));
        assert!(rust_lib.contains("pub mod supervisor;"));
        assert!(!rust_lib.contains("pub mod runtime_shell;"));
        assert!(!rust_lib.contains("pub mod user;"));

        let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
        assert!(cargo_manifest.contains("[[bin]]\nname = \"robot-demo-flowrt-supervisor\""));
        assert!(cargo_manifest.contains("path = \"../rust/src/supervisor_main.rs\""));
        assert!(!cargo_manifest.contains("path = \"../rust/src/main.rs\""));
    }

    #[test]
    fn emits_documented_component_interfaces() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"

[type.Cmd]
left = "f32"
right = "f32"

[component.controller]
language = "cpp"
input = ["imu:Imu"]
output = ["cmd:Cmd"]

[component.monitor]
language = "rust"
input = ["imu:Imu"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_components = artifact_content(&bundle, "cpp/include/flowrt_app/components.hpp");
        let rust_components = artifact_content(&bundle, "rust/src/components.rs");

        assert!(cpp_components.contains(" * @brief `controller` 组件的 C++ 用户实现接口。"));
        assert!(cpp_components.contains(" * @brief 组件初始化钩子。"));
        assert!(cpp_components.contains(" * @brief 执行一次 `controller` 组件调度回调。"));
        assert!(cpp_components.contains(" * @param imu latest snapshot 输入视图。"));
        assert!(cpp_components.contains(" * @param cmd 输出端口写入句柄。"));
        assert!(cpp_components.contains(" * @return 本次回调的 FlowRT 执行状态。"));

        assert!(rust_components.contains("/// `monitor` 组件的 Rust 用户实现 trait。"));
        assert!(rust_components.contains("/// 组件初始化钩子。"));
        assert!(rust_components.contains("/// 执行一次 `monitor` 组件调度回调。"));
        assert!(rust_components.contains("/// - `imu`: latest snapshot 输入视图。"));
        assert!(rust_components.contains("/// 返回本次回调的 FlowRT 执行状态。"));
    }

    #[test]
    fn enables_flowrt_iox2_feature_when_profile_selects_iox2() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.monitor]
language = "rust"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
        assert!(cargo_manifest.contains("features = [\"iox2\"]"));
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        assert!(rust_shell.contains("const SELECTED_BACKEND: &str = \"iox2\";"));
    }

    #[test]
    fn emits_cpp_iox2_transport_contract_when_profile_selects_iox2() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["cpp"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();

        let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
        assert!(cpp_messages.contains("static constexpr const char* IOX2_TYPE_NAME = \"Imu\";"));

        let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
        assert!(cmake.contains("find_package(iceoryx2-cxx 0.9.1 REQUIRED)"));
        assert!(cmake.contains("iceoryx2-cxx::static-lib-cxx"));
        assert!(cmake.contains(
            "target_compile_definitions(robot_demo_flowrt_app INTERFACE FLOWRT_HAS_ICEORYX2_CXX=1)"
        ));

        let runtime_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
        assert!(runtime_header.contains("flowrt::iox2::Iox2PubSub<Imu> bind_0_;"));

        let runtime_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
        assert!(runtime_shell.contains("flowrt::iox2::Iox2PubSub<Imu>::open_with_config"));
        assert!(
            runtime_shell.contains("\"FlowRT/robot_demo/default/bind_0/source_imu_to_sink_imu\"")
        );
        assert!(runtime_shell.contains(
            "flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config(flowrt::StaleConfig{std::chrono::milliseconds{20}, flowrt::StalePolicy::Error})"
        ));
        assert!(runtime_shell.contains("bind_0_.receive_latest_at(tick_time_ms)"));
        assert!(runtime_shell.contains("bind_0_.publish_at(*value, tick_time_ms)"));
        assert!(runtime_shell.contains("auto backend = flowrt::iox2_backend();"));
    }

    #[test]
    fn emits_iox2_typed_channels_when_profile_selects_iox2() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[component.fifo_sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[instance.fifo_sink]
component = "fifo_sink"
target = "linux"

[instance.fifo_sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "drop"

[[bind.dataflow]]
from = "source.imu"
to = "fifo_sink.imu"
channel = "fifo"
depth = 8
overflow = "drop_oldest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
        assert!(rust_messages.contains("use flowrt::ZeroCopySend;"));
        assert!(rust_messages.contains("flowrt::ZeroCopySend"));
        assert!(rust_messages.contains("#[type_name(\"Imu\")]"));
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        assert!(rust_shell.contains("flowrt::iox2::Iox2PubSub<Imu>"));
        assert!(!rust_shell.contains("flowrt::LatestChannel<Imu>"));
        assert!(rust_shell.contains("flowrt::iox2::Iox2ChannelConfig::latest()"));
        assert!(rust_shell.contains(
            "flowrt::iox2::Iox2ChannelConfig::fifo(8, flowrt::OverflowPolicy::DropOldest)"
        ));
        assert!(
            rust_shell.contains("flowrt::StaleConfig::new(Some(20), flowrt::StalePolicy::Drop)")
        );
        assert!(rust_shell.contains("publish_at(value, tick_time_ms)"));
        assert!(rust_shell.contains("receive_latest_at(tick_time_ms)"));
    }

    #[test]
    fn emits_inproc_stale_channel_reads_from_bind_policy() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "drop"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        assert!(rust_shell.contains(
            "flowrt::LatestChannel::with_stale_config(flowrt::StaleConfig::new(Some(20), flowrt::StalePolicy::Drop))"
        ));
        assert!(rust_shell.contains("publish_at(value, tick_time_ms)"));
        assert!(rust_shell.contains("view_at(tick_time_ms)"));
    }

    #[test]
    fn rust_shell_registers_active_channels_and_records_publish_snapshots() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.sensor_source]
language = "rust"
output = ["sample:Sample"]

[component.sensor_sink]
language = "rust"
input = ["sample:Sample"]

[component.aux_source]
language = "rust"
output = ["sample:Sample"]

[component.aux_sink]
language = "rust"
input = ["sample:Sample"]

[instance.sensor_source]
component = "sensor_source"
process = "sensors"
target = "linux"

[instance.sensor_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sensor_sink]
component = "sensor_sink"
process = "control"
target = "linux"

[instance.sensor_sink.task]
trigger = "on_message"
input = ["sample"]

[instance.aux_source]
component = "aux_source"
process = "aux"
target = "linux"

[instance.aux_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.aux_sink]
component = "aux_sink"
process = "aux"
target = "linux"

[instance.aux_sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "sensor_source.sample"
to = "sensor_sink.sample"
channel = "latest"

[[bind.dataflow]]
from = "aux_source.sample"
to = "aux_sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        let sensor_channel = "sensor_source.sample_to_sensor_sink.sample";
        let aux_channel = "aux_source.sample_to_aux_sink.sample";
        let sensor_register = format!(
            "register_introspection_channel(&introspection_state, {}, \"Sample\");",
            rust_string_literal(sensor_channel)
        );
        let aux_register = format!(
            "register_introspection_channel(&introspection_state, {}, \"Sample\");",
            rust_string_literal(aux_channel)
        );
        let sensor_record = format!(
            "record_introspection_publish(introspection_state, {}, \"Sample\", &value, tick_time_ms);",
            rust_string_literal(sensor_channel)
        );

        let sensors_run = generated_function_block(rust_shell, "fn run_process_sensors");
        assert!(sensors_run.contains(&sensor_register));
        assert!(!sensors_run.contains(&aux_register));
        assert!(rust_shell.contains(&sensor_record));
        let sensor_publish = "self.bind_0.publish_at(value, tick_time_ms)";
        assert!(
            rust_shell.find(sensor_publish).unwrap() < rust_shell.find(&sensor_record).unwrap()
        );
    }

    #[test]
    fn cpp_shell_registers_active_channels_and_records_publish_snapshots() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.sensor_source]
language = "cpp"
output = ["sample:Sample"]

[component.sensor_sink]
language = "cpp"
input = ["sample:Sample"]

[component.aux_source]
language = "cpp"
output = ["sample:Sample"]

[component.aux_sink]
language = "cpp"
input = ["sample:Sample"]

[instance.sensor_source]
component = "sensor_source"
process = "sensors"
target = "linux"

[instance.sensor_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sensor_sink]
component = "sensor_sink"
process = "sensors"
target = "linux"

[instance.sensor_sink.task]
trigger = "on_message"
input = ["sample"]

[instance.aux_source]
component = "aux_source"
process = "aux"
target = "linux"

[instance.aux_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.aux_sink]
component = "aux_sink"
process = "aux"
target = "linux"

[instance.aux_sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "sensor_source.sample"
to = "sensor_sink.sample"
channel = "latest"

[[bind.dataflow]]
from = "aux_source.sample"
to = "aux_sink.sample"
channel = "fifo"
depth = 2
overflow = "drop_oldest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
        let sensor_channel = "sensor_source.sample_to_sensor_sink.sample";
        let aux_channel = "aux_source.sample_to_aux_sink.sample";
        let sensor_register = format!(
            "register_introspection_channel(introspection_state, {}, \"Sample\");",
            cpp_string_literal(sensor_channel)
        );
        let aux_register = format!(
            "register_introspection_channel(introspection_state, {}, \"Sample\");",
            cpp_string_literal(aux_channel)
        );
        let sensor_record = format!(
            "record_introspection_publish(introspection_state, {}, \"Sample\", *value, tick_time_ms);",
            cpp_string_literal(sensor_channel)
        );
        let aux_record = format!(
            "record_introspection_publish(introspection_state, {}, \"Sample\", *value, tick_time_ms);",
            cpp_string_literal(aux_channel)
        );

        assert!(cpp_shell.contains("flowrt::IntrospectionState introspection_state;"));
        assert!(cpp_shell.contains("flowrt::spawn_status_server("));
        assert!(cpp_shell.contains("flowrt_app::self_description_hash()"));
        assert!(cpp_shell.contains("runtime = \"cpp\""));
        assert!(cpp_shell.contains("introspection_state.record_tick();"));

        let sensors_run = generated_function_block(cpp_shell, "App::run_process_sensors");
        assert!(sensors_run.contains(&sensor_register));
        assert!(!sensors_run.contains(&aux_register));
        assert!(cpp_shell.contains(&sensor_record));
        let sensor_record_at = cpp_shell.find(&sensor_record).unwrap();
        let sensor_before_record = &cpp_shell[..sensor_record_at];
        assert!(sensor_before_record.contains("publish_at(*value, tick_time_ms)"));

        let aux_run = generated_function_block(cpp_shell, "App::run_process_aux");
        assert!(aux_run.contains(&aux_register));
        assert!(cpp_shell.contains(&aux_record));
        let aux_record_at = cpp_shell.find(&aux_record).unwrap();
        let aux_before_record = &cpp_shell[..aux_record_at];
        assert!(aux_before_record.contains("push_at(*value, tick_time_ms)"));
        assert!(aux_before_record.contains("ChannelWriteOutcome::DroppedOldest"));
    }

    #[test]
    fn emits_inproc_fifo_stale_channel_reads_from_bind_policy() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "fifo"
depth = 4
overflow = "drop_oldest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

        assert!(rust_shell.contains(
            "flowrt::FifoChannel::with_stale_config(4, flowrt::OverflowPolicy::DropOldest, flowrt::StaleConfig::new(Some(20), flowrt::StalePolicy::Error))"
        ));
        assert!(rust_shell.contains("let imu_read = self.bind_0.pop_at(tick_time_ms);"));
        assert!(rust_shell.contains("let imu = imu_read.view();"));
        assert!(rust_shell.contains("push_at(value, tick_time_ms)"));
        assert!(
            rust_shell
                .contains("if imu.stale() {\n            return flowrt::Status::Error;\n        }")
        );
        assert!(
            rust_shell.find("if imu.stale()").unwrap() < rust_shell.find(".on_tick(imu)").unwrap()
        );
    }

    #[test]
    fn emits_stale_error_guard_before_user_tick() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

        assert!(
            rust_shell
                .contains("if imu.stale() {\n            return flowrt::Status::Error;\n        }")
        );
        assert!(
            rust_shell.find("if imu.stale()").unwrap() < rust_shell.find(".on_tick(imu)").unwrap()
        );
    }

    #[test]
    fn cpp_shell_emits_stale_channel_reads_from_bind_policy() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "drop"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

        assert!(cpp_shell.contains(
            "flowrt::LatestChannel<Imu>::with_stale_config(flowrt::StaleConfig{std::chrono::milliseconds{20}, flowrt::StalePolicy::Drop})"
        ));
        assert!(cpp_shell.contains("const auto tick_time_ms = static_cast<std::uint64_t>(tick);"));
        assert!(cpp_shell.contains("publish_at(*value, tick_time_ms)"));
        assert!(cpp_shell.contains("view_at(tick_time_ms)"));
    }

    #[test]
    fn cpp_shell_emits_fifo_stale_channel_reads_from_bind_policy() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "fifo"
depth = 4
overflow = "drop_oldest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

        assert!(cpp_shell.contains(
            "flowrt::FifoChannel<Imu>::with_stale_config(4, flowrt::OverflowPolicy::DropOldest, flowrt::StaleConfig{std::chrono::milliseconds{20}, flowrt::StalePolicy::Error})"
        ));
        assert!(cpp_shell.contains("auto sink_imu_read = bind_0_.pop_at(tick_time_ms);"));
        assert!(cpp_shell.contains("const auto sink_imu = sink_imu_read.view();"));
        assert!(cpp_shell.contains("push_at(*value, tick_time_ms)"));
        assert!(
            cpp_shell
                .contains("if (sink_imu.stale()) {\n        return flowrt::Status::Error;\n    }")
        );
        assert!(
            cpp_shell.find("if (sink_imu.stale())").unwrap()
                < cpp_shell.find("sink_->on_tick(sink_imu)").unwrap()
        );
    }

    #[test]
    fn cpp_shell_emits_stale_error_guard_before_user_tick() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

        assert!(
            cpp_shell
                .contains("if (sink_imu.stale()) {\n        return flowrt::Status::Error;\n    }")
        );
        assert!(
            cpp_shell.find("if (sink_imu.stale())").unwrap()
                < cpp_shell.find("sink_->on_tick(sink_imu)").unwrap()
        );
    }

    #[test]
    fn rust_shell_accepts_multiple_binds_between_same_instance_pair() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["left:Sample", "right:Sample"]

[component.sink]
language = "rust"
input = ["left:Sample", "right:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["left", "right"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["left", "right"]

[[bind.dataflow]]
from = "source.left"
to = "sink.left"
channel = "latest"

[[bind.dataflow]]
from = "source.right"
to = "sink.right"
channel = "latest"
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

        assert!(rust_shell.contains("source: Box<dyn Source>"));
        assert!(rust_shell.contains("sink: Box<dyn Sink>"));
    }

    #[test]
    fn rust_shell_uses_task_port_subset_for_channel_io() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.used_source]
language = "rust"
output = ["used_out:Sample"]

[component.unused_source]
language = "rust"
output = ["unused_out:Sample"]

[component.sink]
language = "rust"
input = ["used_in:Sample", "unused_in:Sample"]
output = ["used_out:Sample", "unused_out:Sample"]

[component.monitor]
language = "rust"
input = ["used_in:Sample", "unused_in:Sample"]

[instance.used_source]
component = "used_source"

[instance.used_source.task]
trigger = "periodic"
period_ms = 5
output = ["used_out"]

[instance.unused_source]
component = "unused_source"

[instance.unused_source.task]
trigger = "periodic"
period_ms = 5
output = ["unused_out"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["used_in"]
output = ["used_out"]

[instance.monitor]
component = "monitor"

[instance.monitor.task]
trigger = "on_message"
input = ["used_in", "unused_in"]

[[bind.dataflow]]
from = "used_source.used_out"
to = "sink.used_in"
channel = "latest"

[[bind.dataflow]]
from = "unused_source.unused_out"
to = "sink.unused_in"
channel = "latest"

[[bind.dataflow]]
from = "sink.used_out"
to = "monitor.used_in"
channel = "latest"

[[bind.dataflow]]
from = "sink.unused_out"
to = "monitor.unused_in"
channel = "latest"
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        let bind_index = |to_instance: &str, to_port: &str| {
            ir.graphs[0]
                .binds
                .iter()
                .position(|bind| {
                    bind.to.instance.name == to_instance && bind.to.port.as_str() == to_port
                })
                .unwrap()
        };
        let sink_used_bind = bind_index("sink", "used_in");
        let sink_unused_bind = bind_index("sink", "unused_in");
        let monitor_used_bind = bind_index("monitor", "used_in");
        let sink_used_read =
            format!("        let used_in = self.bind_{sink_used_bind}.view_at(tick_time_ms);");
        let monitor_used_read =
            format!("        let used_in = self.bind_{monitor_used_bind}.view_at(tick_time_ms);");
        let sink_step_start = rust_shell.find(&sink_used_read).unwrap();
        let monitor_step_start = rust_shell.find(&monitor_used_read).unwrap();
        let sink_step = &rust_shell[sink_step_start..monitor_step_start];

        assert!(sink_step.contains(&sink_used_read));
        assert!(sink_step.contains("let unused_in = flowrt::Latest::new(None, false);"));
        assert!(sink_step.contains("let mut used_out = flowrt::Output::<Sample>::new();"));
        assert!(sink_step.contains("let mut unused_out = flowrt::Output::<Sample>::new();"));
        assert!(sink_step.contains("if used_in.present() {"));
        assert!(
            sink_step
                .contains("self.sink.on_tick(used_in, unused_in, &mut used_out, &mut unused_out)")
        );
        assert!(sink_step.contains("if let Some(value) = used_out.as_ref().copied()"));
        assert!(!sink_step.contains(&format!(
            "self.bind_{sink_unused_bind}.view_at(tick_time_ms)"
        )));
        assert!(!sink_step.contains("if let Some(value) = unused_out.as_ref().copied()"));
    }

    #[test]
    fn rust_shell_runs_startup_and_shutdown_tasks_outside_tick_loop() {
        let mut ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.boot]
language = "rust"

[component.cleanup]
language = "rust"

[instance.boot]
component = "boot"

[instance.boot.task]
trigger = "periodic"
period_ms = 5

[instance.cleanup]
component = "cleanup"

[instance.cleanup.task]
trigger = "periodic"
period_ms = 5
"#,
        );
        ir.graphs[0].tasks[0].trigger = TriggerKind::Startup;
        ir.graphs[0].tasks[0].period_ms = None;
        ir.graphs[0].tasks[1].trigger = TriggerKind::Shutdown;
        ir.graphs[0].tasks[1].period_ms = None;

        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        let run_start = rust_shell.find("    pub fn run(").unwrap();
        let run = &rust_shell[run_start..];
        let startup_call = run
            .find("self.step_startup(0, &mut lifecycle_context, &introspection_state)")
            .unwrap();
        let scheduler_call = run.find("backend.scheduler().run_ticks").unwrap();
        let shutdown_call = run
            .find("self.step_shutdown(0, &mut lifecycle_context, &introspection_state)")
            .unwrap();
        let startup_step = generated_function_block(rust_shell, "fn step_startup");
        let shutdown_step = generated_function_block(rust_shell, "fn step_shutdown");
        let scheduler_step = generated_function_block(rust_shell, "fn step(");

        assert!(startup_call < scheduler_call);
        assert!(scheduler_call < shutdown_call);
        assert!(startup_step.contains("if self.boot.on_tick()"));
        assert!(shutdown_step.contains("if self.cleanup.on_tick()"));
        assert!(!scheduler_step.contains("if self.boot.on_tick()"));
        assert!(!scheduler_step.contains("if self.cleanup.on_tick()"));
    }

    #[test]
    fn cpp_shell_runs_startup_and_shutdown_tasks_outside_tick_loop() {
        let mut ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.boot]
language = "cpp"

[component.cleanup]
language = "cpp"

[instance.boot]
component = "boot"

[instance.boot.task]
trigger = "periodic"
period_ms = 5

[instance.cleanup]
component = "cleanup"

[instance.cleanup.task]
trigger = "periodic"
period_ms = 5
"#,
        );
        ir.graphs[0].tasks[0].trigger = TriggerKind::Startup;
        ir.graphs[0].tasks[0].period_ms = None;
        ir.graphs[0].tasks[1].trigger = TriggerKind::Shutdown;
        ir.graphs[0].tasks[1].period_ms = None;

        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
        let run_start = cpp_shell.find("flowrt::Status App::run(").unwrap();
        let run = &cpp_shell[run_start..];
        let startup_call = run
            .find("status = step_startup(0, lifecycle_context, introspection_state)")
            .unwrap();
        let scheduler_call = run.find("backend.scheduler().run_ticks").unwrap();
        let shutdown_call = run
            .find("status = step_shutdown(0, lifecycle_context, introspection_state)")
            .unwrap();

        assert!(startup_call < scheduler_call);
        assert!(scheduler_call < shutdown_call);
        assert!(cpp_shell.contains("flowrt::Status App::step_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (boot_ && boot_->on_tick()"));
        assert!(cpp_shell.contains("flowrt::Status App::step_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (cleanup_ && cleanup_->on_tick()"));
        assert!(!cpp_shell.contains("flowrt::Status App::step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (boot_ && boot_->on_tick()"));
        assert!(!cpp_shell.contains("flowrt::Status App::step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (cleanup_ && cleanup_->on_tick()"));
    }

    #[test]
    fn rust_shell_enforces_task_deadline_before_publishing_outputs() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
deadline_ms = 10
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        let deadline_start = rust_shell
            .find("let source_deadline_started_at = std::time::Instant::now();")
            .unwrap();
        let source_call = rust_shell
            .find("self.source.on_tick(&mut sample) != flowrt::Status::Ok")
            .unwrap();
        let deadline_guard = rust_shell
            .find("source_deadline_started_at.elapsed() > std::time::Duration::from_millis(10)")
            .unwrap();
        let publish = rust_shell
            .find("if let Some(value) = sample.as_ref().copied()")
            .unwrap();

        assert!(deadline_start < source_call);
        assert!(source_call < deadline_guard);
        assert!(deadline_guard < publish);
    }

    #[test]
    fn cpp_shell_enforces_task_deadline_before_publishing_outputs() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "cpp"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
deadline_ms = 10
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
        let deadline_start = cpp_shell
            .find("const auto source_deadline_started_at = std::chrono::steady_clock::now();")
            .unwrap();
        let source_call = cpp_shell
            .find("source_ && source_->on_tick(source_sample) != flowrt::Status::Ok")
            .unwrap();
        let deadline_guard = cpp_shell
            .find("std::chrono::steady_clock::now() - source_deadline_started_at > std::chrono::milliseconds{10}")
            .unwrap();
        let publish = cpp_shell
            .find("if (const auto* value = source_sample.as_ref())")
            .unwrap();

        assert!(deadline_start < source_call);
        assert!(source_call < deadline_guard);
        assert!(deadline_guard < publish);
    }

    #[test]
    fn cpp_shell_gates_on_message_instances_on_present_inputs() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "cpp"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
        let source_call = cpp_shell.find("source_->on_tick(source_sample)").unwrap();
        let gate = cpp_shell.find("if (sink_sample.present()) {").unwrap();
        let sink_call = cpp_shell.find("sink_->on_tick(sink_sample)").unwrap();

        assert!(source_call < gate);
        assert!(gate < sink_call);
    }

    #[test]
    fn launch_manifest_groups_instances_by_process() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10
priority = 7

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let launch: serde_json::Value =
            serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
        let processes = launch["graphs"][0]["processes"].as_array().unwrap();

        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0]["name"], "control");
        assert_eq!(processes[0]["backend"], "iox2");
        assert_eq!(processes[0]["target"], "linux");
        assert_eq!(processes[0]["runtimes"], serde_json::json!(["rust"]));
        assert_eq!(processes[0]["runtime_kind"], "rust");
        assert_eq!(processes[0]["instances"], serde_json::json!(["sink"]));
        assert_eq!(
            processes[0]["tasks"],
            serde_json::json!([
                {
                    "instance": "sink",
                    "trigger": "on_message",
                    "period_ms": null,
                    "deadline_ms": 10,
                    "priority": 7,
                    "inputs": ["value"],
                    "outputs": []
                }
            ])
        );
        let graph_tasks = launch["graphs"][0]["tasks"].as_array().unwrap();
        let source_task = graph_tasks
            .iter()
            .find(|task| task["instance"] == "source")
            .unwrap();
        let sink_task = graph_tasks
            .iter()
            .find(|task| task["instance"] == "sink")
            .unwrap();
        assert_eq!(source_task["priority"], serde_json::json!(null));
        assert_eq!(source_task["inputs"], serde_json::json!([]));
        assert_eq!(source_task["outputs"], serde_json::json!(["value"]));
        assert_eq!(sink_task["priority"], 7);
        assert_eq!(sink_task["inputs"], serde_json::json!(["value"]));
        assert_eq!(sink_task["outputs"], serde_json::json!([]));
        assert_eq!(processes[1]["name"], "sensors");
        assert_eq!(processes[1]["backend"], "iox2");
        assert_eq!(processes[1]["target"], "linux");
        assert_eq!(processes[1]["runtimes"], serde_json::json!(["rust"]));
        assert_eq!(processes[1]["runtime_kind"], "rust");
        assert_eq!(processes[1]["instances"], serde_json::json!(["source"]));
    }

    #[test]
    fn launch_manifest_marks_mixed_process_runtime_kind() {
        let ir = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "main"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "main"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let launch: serde_json::Value =
            serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
        let process = &launch["graphs"][0]["processes"][0];

        assert_eq!(process["name"], "main");
        assert_eq!(process["runtimes"], serde_json::json!(["cpp", "rust"]));
        assert_eq!(process["runtime_kind"], "mixed");
    }

    #[test]
    fn launch_manifest_exposes_iox2_channel_services() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let launch: serde_json::Value =
            serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
        let channels = launch["graphs"][0]["channels"].as_array().unwrap();
        let channel = &channels[0];

        assert_eq!(channels.len(), 1);
        assert_eq!(channel["from"], "source.value");
        assert_eq!(channel["to"], "sink.value");
        assert_eq!(channel["backend"], "iox2");
        assert_eq!(
            channel["service"],
            "FlowRT/robot_demo/default/bind_0/source_value_to_sink_value"
        );
        assert_eq!(channel["channel"], "latest");
        assert_eq!(channel["depth"], 1);
        assert_eq!(channel["overflow"], "drop_oldest");
        assert_eq!(channel["stale_policy"], "warn");
        assert!(channel["max_age_ms"].is_null());
    }

    #[test]
    fn rust_shell_exposes_process_run_entrypoint() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
        let rust_main = artifact_content(&bundle, "rust/src/main.rs");
        let rust_lib = artifact_content(&bundle, "rust/src/lib.rs");

        assert!(rust_shell.contains("pub fn run_process(self, backend: &dyn flowrt::Backend, process: &str) -> flowrt::Status"));
        assert!(rust_shell.contains("\"control\" => self.run_process_control(backend)"));
        assert!(rust_shell.contains("\"sensors\" => self.run_process_sensors(backend)"));
        assert!(rust_shell.contains("pub fn run_process(process: &str) -> flowrt::Status"));
        assert!(rust_main.contains("--process"));
        assert!(rust_main.contains("flowrt_app::runtime_shell::run_process(process)"));
        assert!(rust_lib.contains("pub use runtime_shell::{run, run_process, App};"));
    }

    #[test]
    fn cpp_shell_exposes_process_run_entrypoint() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "cpp"
input = ["value:u32"]

[instance.source]
component = "source"
process = "control"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
        let cpp_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
        let cpp_main = artifact_content(&bundle, "cpp/src/main.cpp");

        assert!(cpp_header.contains(
            "flowrt::Status step_process_control(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);"
        ));
        assert!(
            cpp_header
                .contains("flowrt::Status run_process_control(const flowrt::Backend& backend);")
        );
        assert!(cpp_shell.contains("flowrt::Status App::step_process_control"));
        assert!(cpp_shell.contains("flowrt::Status App::run_process_control"));
        assert!(cpp_shell.contains("if (process == \"control\")"));
        assert!(cpp_shell.contains("return run_process_control(backend);"));
        assert!(cpp_main.contains("--process"));
        assert!(cpp_main.contains("flowrt_app::run_process(process)"));
    }

    #[test]
    fn emits_rust_supervisor_artifacts_for_process_launch() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let paths = bundle
            .artifacts
            .iter()
            .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let supervisor = artifact_content(&bundle, "rust/src/supervisor.rs");
        let supervisor_main = artifact_content(&bundle, "rust/src/supervisor_main.rs");
        let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");

        assert!(paths.contains(&"rust/src/supervisor.rs".to_string()));
        assert!(paths.contains(&"rust/src/supervisor_main.rs".to_string()));
        assert!(
            supervisor.contains(
                "const LAUNCH_MANIFEST: &str = include_str!(\"../../launch/launch.json\");"
            )
        );
        assert!(supervisor.contains("runtime_kind: String"));
        assert!(supervisor.contains("const RUST_APP_STEM: &str = \"robot-demo-flowrt-app\";"));
        assert!(supervisor.contains("Command::new(&app_exe)"));
        assert!(supervisor.contains("for graph in &manifest.graphs"));
        assert!(!supervisor.contains(".graphs\n        .first()"));
        assert!(
            supervisor.contains("app_executable_for_runtime(&current_exe, &process.runtime_kind)?")
        );
        assert!(supervisor.contains(".arg(\"--process\")"));
        assert!(supervisor.contains(".arg(&process.name)"));
        assert!(supervisor_main.contains("flowrt_app::supervisor::launch()"));
        assert!(cargo_manifest.contains("[[bin]]\nname = \"robot-demo-flowrt-supervisor\""));
        assert!(cargo_manifest.contains("path = \"../rust/src/supervisor_main.rs\""));
        assert!(cargo_manifest.contains("serde = { version = \"1\", features = [\"derive\"] }"));
        assert!(cargo_manifest.contains("serde_json = \"1\""));
        assert!(cargo_manifest.find("serde =").unwrap() < cargo_manifest.find("[[bin]]").unwrap());
    }

    #[test]
    fn rust_supervisor_selects_app_executable_from_runtime_kind() {
        let ir = contract_from_source(
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "cpp_source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "rust_sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["iox2"]
"#,
        );
        let bundle = emit_artifacts(&ir).unwrap();
        let supervisor = artifact_content(&bundle, "rust/src/supervisor.rs");

        assert!(supervisor.contains("runtime_kind: String"));
        assert!(supervisor.contains("const RUST_APP_STEM: &str = \"robot-demo-flowrt-app\";"));
        assert!(supervisor.contains("const CPP_APP_STEM: &str = \"robot_demo_cpp_app\";"));
        assert!(supervisor.contains("fn app_executable_for_runtime("));
        assert!(supervisor.contains("\"rust\" => rust_app_executable(current_exe),"));
        assert!(supervisor.contains("\"cpp\" => cpp_app_executable(current_exe),"));
        assert!(supervisor.contains("fn cpp_app_executable("));
        assert!(supervisor.contains("let mut path = build_dir.join(\"cmake\");"));
        assert!(supervisor.contains("path.push(binary_name(CPP_APP_STEM));"));
        assert!(supervisor.contains(
            "\"mixed\" => Err(\"FlowRT mixed process groups are not launchable yet\".to_string()),"
        ));
        assert!(
            supervisor.contains("app_executable_for_runtime(&current_exe, &process.runtime_kind)?")
        );
    }

    fn contract_from_source(source: &str) -> ContractIr {
        let raw = parse_str(source).unwrap();
        normalize_document(&raw, hash_source(source)).unwrap()
    }

    fn artifact_content<'a>(bundle: &'a ArtifactBundle, path: &str) -> &'a str {
        bundle
            .artifacts
            .iter()
            .find(|artifact| artifact.relative_path.as_path() == std::path::Path::new(path))
            .map(|artifact| artifact.content.as_str())
            .unwrap()
    }

    fn generated_function_block<'a>(source: &'a str, function: &str) -> &'a str {
        let start = source
            .find(function)
            .expect("generated function must exist");
        let rest = &source[start..];
        let next = rest[function.len()..]
            .find("\n    fn ")
            .map(|offset| function.len() + offset)
            .unwrap_or(rest.len());
        &rest[..next]
    }
}
