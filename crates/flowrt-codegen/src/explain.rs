use std::collections::BTreeSet;

use flowrt_ir::{
    BoundaryDirection, ComponentIr, ContractIr, GraphIr, GraphMode, LanguageKind, OperationPortIr,
    ParamIr, ParamType, ParamUpdatePolicy, ParamValue, PortIr, ServicePortIr, TaskConcurrency,
    TaskIr, TaskReadiness, TriggerKind,
};
use serde::Serialize;

use crate::runtime_plan::{
    OperationRuntimePlan, ServiceRuntimePlan, operation_runtime_plans, resolved_task_lane_name,
    service_runtime_plans,
};
use crate::signature_summary::{
    component_kind_name, language_name, on_tick_signature, params_update_signature,
};

/// `flowrt explain` 使用的稳定结构化报告。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExplainReport {
    pub package: ExplainPackage,
    pub graphs: Vec<ExplainGraph>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExplainPackage {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub rsdl_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExplainGraph {
    pub name: String,
    pub profiles: Vec<ExplainProfile>,
    pub components: Vec<ExplainComponent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainProfile {
    pub name: String,
    pub mode: String,
    pub backend: String,
    pub worker_threads: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExplainComponent {
    pub name: String,
    pub language: String,
    pub kind: String,
    pub user_file_path: String,
    pub tasks: Vec<ExplainTask>,
    pub handlers: ExplainHandlers,
    pub inputs: Vec<ExplainInput>,
    pub outputs: Vec<ExplainOutput>,
    pub params: Vec<ExplainParam>,
    pub service_clients: Vec<ExplainServiceClient>,
    pub service_servers: Vec<ExplainServiceServer>,
    pub operation_clients: Vec<ExplainOperationClient>,
    pub operation_servers: Vec<ExplainOperationServer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainTask {
    pub name: String,
    pub instance: String,
    pub trigger: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_ms: Option<u64>,
    pub readiness: String,
    pub lane: String,
    pub concurrency: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainHandlers {
    pub on_tick: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_params_update: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainInput {
    pub name: String,
    pub ty: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainOutput {
    pub name: String,
    pub ty: String,
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainParam {
    pub name: String,
    pub ty: String,
    pub update: String,
    pub default: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainServiceClient {
    pub name: String,
    pub request_type: String,
    pub response_type: String,
    pub handle: String,
    pub backend: String,
    pub server: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainServiceServer {
    pub name: String,
    pub request_type: String,
    pub response_type: String,
    pub backend: String,
    pub clients: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainOperationClient {
    pub name: String,
    pub goal_type: String,
    pub feedback_type: String,
    pub result_type: String,
    pub handle: String,
    pub backend: String,
    pub server: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainOperationServer {
    pub name: String,
    pub goal_type: String,
    pub feedback_type: String,
    pub result_type: String,
    pub backend: String,
    pub clients: Vec<String>,
}

/// 生成 `flowrt explain` 的结构化报告。
pub fn explain_report(contract: &ContractIr) -> ExplainReport {
    ExplainReport {
        package: ExplainPackage {
            name: contract.package.name.clone(),
            version: contract.package.version.clone(),
            rsdl_version: contract.package.rsdl_version.clone(),
        },
        graphs: contract
            .graphs
            .iter()
            .map(|graph| graph_explain_report(contract, graph))
            .collect(),
    }
}

/// 将 `flowrt explain --format text` 报告渲染为稳定文本。
pub fn format_explain_report_text(report: &ExplainReport) -> String {
    let mut output = format!(
        "flowrt explain:\npackage {} rsdl_version={}",
        report.package.name, report.package.rsdl_version
    );
    if let Some(version) = &report.package.version {
        output.push_str(&format!(" version={version}"));
    }
    for graph in &report.graphs {
        output.push_str(&format!("\ngraph {}", graph.name));
        for profile in &graph.profiles {
            output.push_str(&format!(
                "\n  profile {} mode={} backend={} worker_threads={}",
                profile.name, profile.mode, profile.backend, profile.worker_threads
            ));
        }
        for component in &graph.components {
            output.push_str(&format!(
                "\n  component {} language={} kind={} user_file={}",
                component.name, component.language, component.kind, component.user_file_path
            ));
            push_tasks_text(&mut output, &component.tasks);
            output.push_str("\n    handlers:");
            output.push_str(&format!("\n      on_tick: {}", component.handlers.on_tick));
            if let Some(signature) = &component.handlers.on_params_update {
                output.push_str(&format!("\n      on_params_update: {signature}"));
            }
            push_inputs_text(&mut output, &component.inputs);
            push_outputs_text(&mut output, &component.outputs);
            push_params_text(&mut output, &component.params);
            push_service_clients_text(&mut output, &component.service_clients);
            push_service_servers_text(&mut output, &component.service_servers);
            push_operation_clients_text(&mut output, &component.operation_clients);
            push_operation_servers_text(&mut output, &component.operation_servers);
        }
    }
    output
}

fn graph_explain_report(contract: &ContractIr, graph: &GraphIr) -> ExplainGraph {
    let service_plans = service_runtime_plans(contract, graph);
    let operation_plans = operation_runtime_plans(contract, graph);
    let used_components = graph
        .instances
        .iter()
        .map(|instance| instance.component.name.as_str())
        .collect::<BTreeSet<_>>();

    ExplainGraph {
        name: graph.name.clone(),
        profiles: contract
            .profiles
            .iter()
            .map(|profile| ExplainProfile {
                name: profile.name.clone(),
                mode: graph_mode_name(profile.mode).to_string(),
                backend: profile.backend.0.clone(),
                worker_threads: profile.scheduler.worker_threads,
            })
            .collect(),
        components: contract
            .components
            .iter()
            .filter(|component| {
                used_components.contains(component.name.as_str())
                    || used_components.contains(component.qualified_name.as_str())
            })
            .map(|component| {
                component_explain_report(graph, component, &service_plans, &operation_plans)
            })
            .collect(),
    }
}

fn component_explain_report(
    graph: &GraphIr,
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> ExplainComponent {
    ExplainComponent {
        name: component.name.clone(),
        language: language_name(component.language).to_string(),
        kind: component_kind_name(component.kind).to_string(),
        user_file_path: user_file_path(component).to_string(),
        tasks: component_tasks(graph, component),
        handlers: ExplainHandlers {
            on_tick: on_tick_signature(component, service_plans, operation_plans),
            on_params_update: (!component.params.is_empty())
                .then(|| params_update_signature(component)),
        },
        inputs: component
            .inputs
            .iter()
            .map(|input| input_explain(graph, component, input))
            .collect(),
        outputs: component
            .outputs
            .iter()
            .map(|output| output_explain(graph, component, output))
            .collect(),
        params: component.params.iter().map(param_explain).collect(),
        service_clients: component
            .service_clients
            .iter()
            .map(|port| service_client_explain(component, port, service_plans))
            .collect(),
        service_servers: component
            .service_servers
            .iter()
            .map(|port| service_server_explain(component, port, service_plans))
            .collect(),
        operation_clients: component
            .operation_clients
            .iter()
            .map(|port| operation_client_explain(component, port, operation_plans))
            .collect(),
        operation_servers: component
            .operation_servers
            .iter()
            .map(|port| operation_server_explain(component, port, operation_plans))
            .collect(),
    }
}

fn component_tasks(graph: &GraphIr, component: &ComponentIr) -> Vec<ExplainTask> {
    let instance_names = graph
        .instances
        .iter()
        .filter(|instance| {
            instance.component.name == component.name
                || instance.component.name == component.qualified_name
        })
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();

    graph
        .tasks
        .iter()
        .filter(|task| instance_names.contains(task.instance.name.as_str()))
        .map(task_explain)
        .collect()
}

fn task_explain(task: &TaskIr) -> ExplainTask {
    ExplainTask {
        name: task.name.clone(),
        instance: task.instance.name.clone(),
        trigger: trigger_name(task.trigger).to_string(),
        period_ms: task.period_ms,
        readiness: readiness_name(task.readiness).to_string(),
        lane: resolved_task_lane_name(task),
        concurrency: concurrency_name(task.concurrency).to_string(),
    }
}

fn input_explain(graph: &GraphIr, component: &ComponentIr, input: &PortIr) -> ExplainInput {
    let instance_names = graph
        .instances
        .iter()
        .filter(|instance| {
            instance.component.name == component.name
                || instance.component.name == component.qualified_name
        })
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut sources = graph
        .binds
        .iter()
        .filter(|bind| {
            bind.to.port == input.name && instance_names.contains(bind.to.instance.name.as_str())
        })
        .map(|bind| format!("{}.{}", bind.from.instance.name, bind.from.port))
        .collect::<Vec<_>>();
    sources.extend(
        graph
            .boundary_endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.direction == BoundaryDirection::Input
                    && endpoint.port.port == input.name
                    && instance_names.contains(endpoint.port.instance.name.as_str())
            })
            .map(|endpoint| format!("boundary.{}", endpoint.name)),
    );

    ExplainInput {
        name: input.name.clone(),
        ty: input.ty.canonical_syntax(),
        source: if sources.is_empty() {
            "unbound".to_string()
        } else {
            sources.join("|")
        },
    }
}

fn output_explain(graph: &GraphIr, component: &ComponentIr, output: &PortIr) -> ExplainOutput {
    let instance_names = graph
        .instances
        .iter()
        .filter(|instance| {
            instance.component.name == component.name
                || instance.component.name == component.qualified_name
        })
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut targets = graph
        .binds
        .iter()
        .filter(|bind| {
            bind.from.port == output.name
                && instance_names.contains(bind.from.instance.name.as_str())
        })
        .map(|bind| format!("{}.{}", bind.to.instance.name, bind.to.port))
        .collect::<Vec<_>>();
    targets.extend(
        graph
            .boundary_endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.direction == BoundaryDirection::Output
                    && endpoint.port.port == output.name
                    && instance_names.contains(endpoint.port.instance.name.as_str())
            })
            .map(|endpoint| format!("boundary.{}", endpoint.name)),
    );

    ExplainOutput {
        name: output.name.clone(),
        ty: output.ty.canonical_syntax(),
        targets,
    }
}

fn param_explain(param: &ParamIr) -> ExplainParam {
    ExplainParam {
        name: param.name.clone(),
        ty: param_type_name(param.ty).to_string(),
        update: param_update_name(param.update).to_string(),
        default: param_value_text(&param.default),
    }
}

fn service_client_explain(
    component: &ComponentIr,
    port: &ServicePortIr,
    plans: &[ServiceRuntimePlan],
) -> ExplainServiceClient {
    if let Some(plan) = plans.iter().find(|plan| {
        plan.client_port == port.name
            && (plan.client_component == component.name
                || plan.client_component == component.qualified_name)
    }) {
        return ExplainServiceClient {
            name: port.name.clone(),
            request_type: plan.request_type.canonical_syntax(),
            response_type: plan.response_type.canonical_syntax(),
            handle: service_client_handle_name(plan),
            backend: plan.backend.0.clone(),
            server: format!("{}.{}", plan.server_instance, plan.server_port),
        };
    }

    ExplainServiceClient {
        name: port.name.clone(),
        request_type: port.request.canonical_syntax(),
        response_type: port.response.canonical_syntax(),
        handle: service_client_handle_name_for_component(component, &port.name),
        backend: "unbound".to_string(),
        server: "unbound".to_string(),
    }
}

fn service_server_explain(
    component: &ComponentIr,
    port: &ServicePortIr,
    plans: &[ServiceRuntimePlan],
) -> ExplainServiceServer {
    let matching = plans
        .iter()
        .filter(|plan| {
            plan.server_port == port.name
                && (plan.server_component == component.name
                    || plan.server_component == component.qualified_name)
        })
        .collect::<Vec<_>>();

    ExplainServiceServer {
        name: port.name.clone(),
        request_type: port.request.canonical_syntax(),
        response_type: port.response.canonical_syntax(),
        backend: matching
            .first()
            .map(|plan| plan.backend.0.clone())
            .unwrap_or_else(|| "unbound".to_string()),
        clients: matching
            .iter()
            .map(|plan| format!("{}.{}", plan.client_instance, plan.client_port))
            .collect(),
    }
}

fn operation_client_explain(
    component: &ComponentIr,
    port: &OperationPortIr,
    plans: &[OperationRuntimePlan],
) -> ExplainOperationClient {
    if let Some(plan) = plans.iter().find(|plan| {
        plan.client_port == port.name
            && (plan.client_component == component.name
                || plan.client_component == component.qualified_name)
    }) {
        return ExplainOperationClient {
            name: port.name.clone(),
            goal_type: plan.goal_type.canonical_syntax(),
            feedback_type: plan.feedback_type.canonical_syntax(),
            result_type: plan.result_type.canonical_syntax(),
            handle: operation_client_handle_name(plan),
            backend: plan.backend.0.clone(),
            server: format!("{}.{}", plan.server_instance, plan.server_port),
        };
    }

    ExplainOperationClient {
        name: port.name.clone(),
        goal_type: port.goal.canonical_syntax(),
        feedback_type: port.feedback.canonical_syntax(),
        result_type: port.result.canonical_syntax(),
        handle: operation_client_handle_name_for_component(component, &port.name),
        backend: "unbound".to_string(),
        server: "unbound".to_string(),
    }
}

fn operation_server_explain(
    component: &ComponentIr,
    port: &OperationPortIr,
    plans: &[OperationRuntimePlan],
) -> ExplainOperationServer {
    let matching = plans
        .iter()
        .filter(|plan| {
            plan.server_port == port.name
                && (plan.server_component == component.name
                    || plan.server_component == component.qualified_name)
        })
        .collect::<Vec<_>>();

    ExplainOperationServer {
        name: port.name.clone(),
        goal_type: port.goal.canonical_syntax(),
        feedback_type: port.feedback.canonical_syntax(),
        result_type: port.result.canonical_syntax(),
        backend: matching
            .first()
            .map(|plan| plan.backend.0.clone())
            .unwrap_or_else(|| "unbound".to_string()),
        clients: matching
            .iter()
            .map(|plan| format!("{}.{}", plan.client_instance, plan.client_port))
            .collect(),
    }
}

fn push_tasks_text(output: &mut String, tasks: &[ExplainTask]) {
    output.push_str("\n    tasks:");
    if tasks.is_empty() {
        output.push_str(" none");
        return;
    }
    for task in tasks {
        output.push_str(&format!(
            "\n      task {} trigger={} period={} readiness={} lane={} concurrency={} instance={}",
            task.name,
            task.trigger,
            period_text(task.period_ms),
            task.readiness,
            task.lane,
            task.concurrency,
            task.instance
        ));
    }
}

fn push_inputs_text(output: &mut String, inputs: &[ExplainInput]) {
    output.push_str("\n    inputs:");
    if inputs.is_empty() {
        output.push_str(" none");
        return;
    }
    output.push(' ');
    output.push_str(
        &inputs
            .iter()
            .map(|input| format!("{}:{} source={}", input.name, input.ty, input.source))
            .collect::<Vec<_>>()
            .join(", "),
    );
}

fn push_outputs_text(output: &mut String, outputs: &[ExplainOutput]) {
    output.push_str("\n    outputs:");
    if outputs.is_empty() {
        output.push_str(" none");
        return;
    }
    output.push(' ');
    output.push_str(
        &outputs
            .iter()
            .map(|port| {
                let targets = if port.targets.is_empty() {
                    "none".to_string()
                } else {
                    port.targets.join("|")
                };
                format!("{}:{} targets={}", port.name, port.ty, targets)
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
}

fn push_params_text(output: &mut String, params: &[ExplainParam]) {
    output.push_str("\n    params:");
    if params.is_empty() {
        output.push_str(" none");
        return;
    }
    output.push(' ');
    output.push_str(
        &params
            .iter()
            .map(|param| {
                format!(
                    "{}:{} update={} default={}",
                    param.name, param.ty, param.update, param.default
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
}

fn push_service_clients_text(output: &mut String, clients: &[ExplainServiceClient]) {
    output.push_str("\n    service clients:");
    if clients.is_empty() {
        output.push_str(" none");
        return;
    }
    output.push(' ');
    output.push_str(
        &clients
            .iter()
            .map(|client| {
                format!(
                    "{}:{}->{} handle={} backend={} server={}",
                    client.name,
                    client.request_type,
                    client.response_type,
                    client.handle,
                    client.backend,
                    client.server
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
}

fn push_service_servers_text(output: &mut String, servers: &[ExplainServiceServer]) {
    output.push_str("\n    service servers:");
    if servers.is_empty() {
        output.push_str(" none");
        return;
    }
    output.push(' ');
    output.push_str(
        &servers
            .iter()
            .map(|server| {
                let clients = if server.clients.is_empty() {
                    "none".to_string()
                } else {
                    server.clients.join("|")
                };
                format!(
                    "{}:{}->{} backend={} clients={}",
                    server.name, server.request_type, server.response_type, server.backend, clients
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
}

fn push_operation_clients_text(output: &mut String, clients: &[ExplainOperationClient]) {
    output.push_str("\n    operation clients:");
    if clients.is_empty() {
        output.push_str(" none");
        return;
    }
    output.push(' ');
    output.push_str(
        &clients
            .iter()
            .map(|client| {
                format!(
                    "{}:{}->{}->{} handle={} backend={} server={}",
                    client.name,
                    client.goal_type,
                    client.feedback_type,
                    client.result_type,
                    client.handle,
                    client.backend,
                    client.server
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
}

fn push_operation_servers_text(output: &mut String, servers: &[ExplainOperationServer]) {
    output.push_str("\n    operation servers:");
    if servers.is_empty() {
        output.push_str(" none");
        return;
    }
    output.push(' ');
    output.push_str(
        &servers
            .iter()
            .map(|server| {
                let clients = if server.clients.is_empty() {
                    "none".to_string()
                } else {
                    server.clients.join("|")
                };
                format!(
                    "{}:{}->{}->{} backend={} clients={}",
                    server.name,
                    server.goal_type,
                    server.feedback_type,
                    server.result_type,
                    server.backend,
                    clients
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
}

fn user_file_path(component: &ComponentIr) -> String {
    match component.language {
        LanguageKind::Rust => "app/rust/mod.rs".to_string(),
        LanguageKind::Cpp => "app/cpp/**".to_string(),
        LanguageKind::C => format!("app/c/{}.c", component.name),
        LanguageKind::External => "external package".to_string(),
    }
}

fn graph_mode_name(mode: GraphMode) -> &'static str {
    match mode {
        GraphMode::Strict => "strict",
        GraphMode::Island => "island",
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

fn readiness_name(readiness: TaskReadiness) -> &'static str {
    match readiness {
        TaskReadiness::AnyReady => "any_ready",
        TaskReadiness::AllReady => "all_ready",
    }
}

fn concurrency_name(concurrency: TaskConcurrency) -> &'static str {
    match concurrency {
        TaskConcurrency::Exclusive => "exclusive",
        TaskConcurrency::Parallel => "parallel",
    }
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

fn param_value_text(value: &ParamValue) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid-json>".to_string())
}

fn period_text(period_ms: Option<u64>) -> String {
    period_ms
        .map(|period| format!("{period}ms"))
        .unwrap_or_else(|| "none".to_string())
}

fn service_client_handle_name(plan: &ServiceRuntimePlan) -> String {
    service_client_handle_name_for_parts(&plan.client_component, &plan.client_port)
}

fn service_client_handle_name_for_component(component: &ComponentIr, port: &str) -> String {
    service_client_handle_name_for_parts(&component.name, port)
}

fn service_client_handle_name_for_parts(component: &str, port: &str) -> String {
    format!(
        "ServiceClient_{}_{}",
        crate::snake_identifier(component),
        crate::snake_identifier(port)
    )
}

fn operation_client_handle_name(plan: &OperationRuntimePlan) -> String {
    operation_client_handle_name_for_parts(&plan.client_component, &plan.client_port)
}

fn operation_client_handle_name_for_component(component: &ComponentIr, port: &str) -> String {
    operation_client_handle_name_for_parts(&component.name, port)
}

fn operation_client_handle_name_for_parts(component: &str, port: &str) -> String {
    format!(
        "OperationClient_{}_{}",
        crate::snake_identifier(component),
        crate::snake_identifier(port)
    )
}
