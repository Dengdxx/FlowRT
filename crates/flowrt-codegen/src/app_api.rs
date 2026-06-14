use std::collections::BTreeSet;
use std::path::PathBuf;

use flowrt_ir::{
    BoundaryDirection, ComponentIr, ComponentKind, ContractIr, GraphIr, GraphMode, InstanceIr,
    LanguageKind, OperationPortIr, ParamIr, ParamValue, PortIr, ResourceRequirementIr,
    ServicePortIr, TaskConcurrency, TaskIr, TaskReadiness, TriggerKind, TypeExpr,
};
use serde::Serialize;

use crate::messages::{cpp_type, rust_type};
use crate::resource_names::{
    resource_access_name, resource_failure_name, resource_health_name, resource_readiness_name,
};
use crate::runtime_plan::{
    OperationRuntimePlan, ServiceRuntimePlan, operation_runtime_plans, resolved_task_lane_name,
    service_runtime_plans,
};
use crate::{
    Artifact, component_rust_name, param_type_name, param_update_name, snake_identifier,
    tasks_for_instance,
};

const APP_API_VERSION: &str = "0.1";

#[derive(Debug, Serialize)]
pub struct AppApiManifest {
    app_api_version: &'static str,
    contract: AppApiContract,
    package: AppApiPackage,
    graph: AppApiGraph,
    runtime_context: AppApiRuntimeContext,
    components: Vec<AppApiComponent>,
    stubs: Vec<AppApiStub>,
}

#[derive(Debug, Serialize)]
struct AppApiContract {
    ir_version: String,
    schema_version: String,
    source_hash: String,
    package_id: String,
}

#[derive(Debug, Serialize)]
struct AppApiPackage {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    rsdl_version: String,
}

#[derive(Debug, Serialize)]
struct AppApiGraph {
    name: String,
    mode: &'static str,
    profile: String,
    backend: String,
    worker_threads: u32,
}

#[derive(Debug, Serialize)]
struct AppApiRuntimeContext {
    task_timing: AppApiTaskTimingContext,
}

#[derive(Debug, Serialize)]
struct AppApiTaskTimingContext {
    access: AppApiTaskTimingAccess,
    available_in_task_context: bool,
    available_in_lifecycle_context: bool,
    handler_signature_changed: bool,
    fields: Vec<&'static str>,
    clock_sources: Vec<&'static str>,
    realtime_semantics: &'static str,
    replay_semantics: &'static str,
    non_goals: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct AppApiTaskTimingAccess {
    rust: &'static str,
    cpp: &'static str,
    c: &'static str,
}

#[derive(Debug, Serialize)]
struct AppApiComponent {
    name: String,
    qualified_name: String,
    language: &'static str,
    kind: &'static str,
    concurrency: &'static str,
    instances: Vec<String>,
    user_file_path: String,
    handlers: Vec<AppApiHandler>,
    tasks: Vec<AppApiTask>,
    inputs: Vec<AppApiPortView>,
    outputs: Vec<AppApiPortView>,
    resources: Vec<AppApiResourceRequirement>,
    params: Vec<AppApiParam>,
    #[serde(skip_serializing_if = "Option::is_none")]
    params_update_hook: Option<&'static str>,
    service_clients: Vec<AppApiServiceClient>,
    service_servers: Vec<AppApiServiceServer>,
    operation_clients: Vec<AppApiOperationClient>,
    operation_servers: Vec<AppApiOperationServer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    c_callback_table: Option<AppApiCCallbackTable>,
}

#[derive(Debug, Serialize)]
struct AppApiHandler {
    name: String,
    signature: String,
    required: bool,
    source: &'static str,
}

#[derive(Debug, Serialize)]
struct AppApiTask {
    name: String,
    instance: String,
    trigger: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    period_ms: Option<u64>,
    readiness: &'static str,
    lane: String,
    concurrency: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    deadline_ms: Option<u64>,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AppApiPortView {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    handler_argument: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    targets: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AppApiParam {
    name: String,
    #[serde(rename = "type")]
    ty: &'static str,
    update: &'static str,
    default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    min: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    choices: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct AppApiResourceRequirement {
    name: String,
    capability: String,
    access: &'static str,
    required: bool,
    readiness: &'static str,
    health: &'static str,
    on_failure: &'static str,
}

#[derive(Debug, Serialize)]
struct AppApiServiceClient {
    name: String,
    request_type: String,
    response_type: String,
    handle_type: String,
    handler_argument: String,
    backend: String,
    server: String,
}

#[derive(Debug, Serialize)]
struct AppApiServiceServer {
    name: String,
    request_type: String,
    response_type: String,
    handler_name: String,
    handler_signature: String,
    backend: String,
    clients: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AppApiOperationClient {
    name: String,
    goal_type: String,
    feedback_type: String,
    result_type: String,
    handle_type: String,
    handler_argument: String,
    backend: String,
    server: String,
}

#[derive(Debug, Serialize)]
struct AppApiOperationServer {
    name: String,
    goal_type: String,
    feedback_type: String,
    result_type: String,
    handler_name: String,
    handler_signature: String,
    backend: String,
    clients: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AppApiCCallbackTable {
    generated_header: &'static str,
    factory_symbol: String,
    entries: Vec<AppApiCCallbackEntry>,
    lifecycle_callbacks: Vec<AppApiCCallbackField>,
    task_callbacks: Vec<AppApiCTaskCallback>,
}

#[derive(Debug, Serialize)]
struct AppApiCCallbackEntry {
    instance: String,
    factory_symbol: String,
}

#[derive(Debug, Serialize)]
struct AppApiCCallbackField {
    field: &'static str,
    signature: &'static str,
    required: bool,
}

#[derive(Debug, Serialize)]
struct AppApiCTaskCallback {
    task: String,
    instance: String,
    trigger: &'static str,
    field: &'static str,
    signature: &'static str,
    required: bool,
}

#[derive(Debug, Serialize)]
struct AppApiStub {
    language: &'static str,
    component: String,
    path: String,
}

pub(crate) fn emit_app_api_artifacts(contract: &ContractIr) -> Vec<Artifact> {
    let manifest = app_api_manifest(contract);
    let mut artifacts = vec![
        artifact("app/app_api.json", pretty_json(&manifest)),
        artifact("app/implementation.md", emit_implementation_md(&manifest)),
    ];
    for stub in emit_reference_stubs(contract) {
        artifacts.push(stub);
    }
    artifacts
}

pub(crate) fn app_api_manifest(contract: &ContractIr) -> AppApiManifest {
    let graph = contract
        .graphs
        .first()
        .expect("validated contract must contain a graph");
    let profile = contract
        .profiles
        .first()
        .expect("validated contract must contain a profile");
    let service_plans = service_runtime_plans(contract, graph);
    let operation_plans = operation_runtime_plans(contract, graph);
    let components = graph_components(contract, graph)
        .into_iter()
        .map(|component| component_manifest(graph, component, &service_plans, &operation_plans))
        .collect::<Vec<_>>();
    let stubs = components
        .iter()
        .map(|component| AppApiStub {
            language: component.language,
            component: component.name.clone(),
            path: stub_path(component.language, &component.name),
        })
        .collect();

    AppApiManifest {
        app_api_version: APP_API_VERSION,
        contract: AppApiContract {
            ir_version: contract.ir_version.clone(),
            schema_version: contract.schema_version.clone(),
            source_hash: contract.source_hash.clone(),
            package_id: contract.package_id.0.clone(),
        },
        package: AppApiPackage {
            name: contract.package.name.clone(),
            version: contract.package.version.clone(),
            rsdl_version: contract.package.rsdl_version.clone(),
        },
        graph: AppApiGraph {
            name: graph.name.clone(),
            mode: graph_mode_name(profile.mode),
            profile: profile.name.clone(),
            backend: profile.backend.0.clone(),
            worker_threads: profile.scheduler.worker_threads,
        },
        runtime_context: runtime_context_manifest(),
        components,
        stubs,
    }
}

fn runtime_context_manifest() -> AppApiRuntimeContext {
    AppApiRuntimeContext {
        task_timing: AppApiTaskTimingContext {
            access: AppApiTaskTimingAccess {
                rust: "context.timing()",
                cpp: "context.timing()",
                c: "context->has_timing / context->timing",
            },
            available_in_task_context: true,
            available_in_lifecycle_context: false,
            handler_signature_changed: false,
            fields: task_timing_fields(),
            clock_sources: vec!["runtime", "replay"],
            realtime_semantics: "realtime 运行时读取 runtime observed scheduling time",
            replay_semantics: "replay / temporary island 使用 fixture 驱动的 deterministic timing",
            non_goals: vec![
                "不承诺硬实时",
                "不建模 sensor timestamp / event-time",
                "不建模 clock domain / PTP / NTP / approximate sync",
            ],
        },
    }
}

fn task_timing_fields() -> Vec<&'static str> {
    vec![
        "scheduled_time_ms",
        "observed_time_ms",
        "scheduled_delta_ms",
        "observed_delta_ms",
        "lateness_ms",
        "missed_periods",
        "deadline_missed",
        "overrun",
    ]
}

fn component_manifest(
    graph: &GraphIr,
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> AppApiComponent {
    let instances = component_instances(graph, component);
    let instance_names = instances
        .iter()
        .map(|instance| instance.name.clone())
        .collect::<Vec<_>>();
    let tasks = instances
        .iter()
        .flat_map(|instance| tasks_for_instance(graph, instance))
        .map(task_manifest)
        .collect::<Vec<_>>();

    AppApiComponent {
        name: component.name.clone(),
        qualified_name: component.qualified_name.clone(),
        language: language_name(component.language),
        kind: component_kind_name(component.kind),
        concurrency: concurrency_name(component.concurrency),
        instances: instance_names,
        user_file_path: user_file_path(component),
        handlers: handlers(component, graph, service_plans, operation_plans),
        tasks,
        inputs: component
            .inputs
            .iter()
            .map(|port| input_view(graph, component, port))
            .collect(),
        outputs: component
            .outputs
            .iter()
            .map(|port| output_view(graph, component, port))
            .collect(),
        resources: component
            .resources
            .iter()
            .map(resource_requirement)
            .collect(),
        params: component.params.iter().map(param_manifest).collect(),
        params_update_hook: (!component.params.is_empty()).then_some("on_params_update"),
        service_clients: component
            .service_clients
            .iter()
            .map(|port| service_client(component, port, service_plans))
            .collect(),
        service_servers: component
            .service_servers
            .iter()
            .map(|port| service_server(graph, component, port, service_plans))
            .collect(),
        operation_clients: component
            .operation_clients
            .iter()
            .map(|port| operation_client(component, port, operation_plans))
            .collect(),
        operation_servers: component
            .operation_servers
            .iter()
            .map(|port| operation_server(graph, component, port, operation_plans))
            .collect(),
        c_callback_table: (component.language == LanguageKind::C)
            .then(|| c_callback_table(component, graph)),
    }
}

fn handlers(
    component: &ComponentIr,
    graph: &GraphIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> Vec<AppApiHandler> {
    let mut handlers = vec![
        lifecycle_handler(component, "on_init"),
        lifecycle_handler(component, "on_start"),
        lifecycle_handler(component, "on_stop"),
        lifecycle_handler(component, "on_shutdown"),
        on_tick_handler(component, service_plans, operation_plans),
    ];
    if !component.params.is_empty() {
        handlers.push(params_update_handler(component));
    }
    handlers.extend(
        component
            .service_servers
            .iter()
            .map(|port| service_server_handler(component, port)),
    );
    handlers.extend(
        component
            .operation_servers
            .iter()
            .map(|port| operation_server_handler(component, port)),
    );
    if component.language == LanguageKind::C {
        for task in component_instances(graph, component)
            .iter()
            .flat_map(|instance| tasks_for_instance(graph, instance))
        {
            handlers.push(AppApiHandler {
                name: c_task_callback_field(task.trigger).to_string(),
                signature: C_TASK_CALLBACK_SIGNATURE.to_string(),
                required: true,
                source: "c_callback_table",
            });
        }
    }
    handlers
}

fn lifecycle_handler(component: &ComponentIr, name: &str) -> AppApiHandler {
    let signature = match component.language {
        LanguageKind::Rust => format!(
            "fn {name}({}self, context: &mut flowrt::Context) -> flowrt::Status",
            crate::rust_shell::rust_component_receiver(component)
        ),
        LanguageKind::Cpp | LanguageKind::C => {
            format!("flowrt::Status {name}(flowrt::Context& context)")
        }
        LanguageKind::External => "no generated lifecycle hook".to_string(),
    };
    AppApiHandler {
        name: name.to_string(),
        signature,
        required: false,
        source: handler_source(component.language),
    }
}

fn on_tick_handler(
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> AppApiHandler {
    AppApiHandler {
        name: "on_tick".to_string(),
        signature: on_tick_signature(component, service_plans, operation_plans),
        required: component.language != LanguageKind::C,
        source: handler_source(component.language),
    }
}

fn params_update_handler(component: &ComponentIr) -> AppApiHandler {
    AppApiHandler {
        name: "on_params_update".to_string(),
        signature: params_update_signature(component),
        required: false,
        source: handler_source(component.language),
    }
}

fn service_server_handler(component: &ComponentIr, port: &ServicePortIr) -> AppApiHandler {
    AppApiHandler {
        name: service_handler_name(&port.name),
        signature: service_server_signature(component, port),
        required: false,
        source: handler_source(component.language),
    }
}

fn operation_server_handler(component: &ComponentIr, port: &OperationPortIr) -> AppApiHandler {
    AppApiHandler {
        name: operation_handler_name(&port.name),
        signature: operation_server_signature(component, port),
        required: false,
        source: handler_source(component.language),
    }
}

fn task_manifest(task: &TaskIr) -> AppApiTask {
    AppApiTask {
        name: task.name.clone(),
        instance: task.instance.name.clone(),
        trigger: trigger_name(task.trigger),
        period_ms: task.period_ms,
        readiness: readiness_name(task.readiness),
        lane: resolved_task_lane_name(task),
        concurrency: concurrency_name(task.concurrency),
        deadline_ms: task.deadline_ms,
        inputs: task.inputs.clone(),
        outputs: task.outputs.clone(),
    }
}

fn input_view(graph: &GraphIr, component: &ComponentIr, port: &PortIr) -> AppApiPortView {
    let instance_names = component_instance_name_set(graph, component);
    let mut sources = graph
        .binds
        .iter()
        .filter(|bind| {
            bind.to.port == port.name && instance_names.contains(bind.to.instance.name.as_str())
        })
        .map(|bind| format!("{}.{}", bind.from.instance.name, bind.from.port))
        .collect::<BTreeSet<_>>();
    sources.extend(
        graph
            .boundary_endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.direction == BoundaryDirection::Input
                    && endpoint.port.port == port.name
                    && instance_names.contains(endpoint.port.instance.name.as_str())
            })
            .map(|endpoint| format!("boundary.{}", endpoint.name)),
    );
    AppApiPortView {
        name: port.name.clone(),
        ty: port.ty.canonical_syntax(),
        handler_argument: input_handler_argument(component.language, port),
        source: if sources.is_empty() {
            None
        } else {
            Some(sources.into_iter().collect::<Vec<_>>().join("|"))
        },
        targets: Vec::new(),
    }
}

fn output_view(graph: &GraphIr, component: &ComponentIr, port: &PortIr) -> AppApiPortView {
    let instance_names = component_instance_name_set(graph, component);
    let mut targets = graph
        .binds
        .iter()
        .filter(|bind| {
            bind.from.port == port.name && instance_names.contains(bind.from.instance.name.as_str())
        })
        .map(|bind| format!("{}.{}", bind.to.instance.name, bind.to.port))
        .collect::<BTreeSet<_>>();
    targets.extend(
        graph
            .boundary_endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.direction == BoundaryDirection::Output
                    && endpoint.port.port == port.name
                    && instance_names.contains(endpoint.port.instance.name.as_str())
            })
            .map(|endpoint| format!("boundary.{}", endpoint.name)),
    );
    AppApiPortView {
        name: port.name.clone(),
        ty: port.ty.canonical_syntax(),
        handler_argument: output_handler_argument(component.language, port),
        source: None,
        targets: targets.into_iter().collect(),
    }
}

fn param_manifest(param: &ParamIr) -> AppApiParam {
    AppApiParam {
        name: param.name.clone(),
        ty: param_type_name(param.ty),
        update: param_update_name(param.update),
        default: param_value_json(&param.default),
        min: param.min.as_ref().map(param_value_json),
        max: param.max.as_ref().map(param_value_json),
        choices: param.choices.iter().map(param_value_json).collect(),
    }
}

fn resource_requirement(resource: &ResourceRequirementIr) -> AppApiResourceRequirement {
    AppApiResourceRequirement {
        name: resource.name.clone(),
        capability: resource.capability.0.clone(),
        access: resource_access_name(resource.access),
        required: resource.required,
        readiness: resource_readiness_name(resource.readiness),
        health: resource_health_name(resource.health),
        on_failure: resource_failure_name(resource.on_failure),
    }
}

fn service_client(
    component: &ComponentIr,
    port: &ServicePortIr,
    plans: &[ServiceRuntimePlan],
) -> AppApiServiceClient {
    let plan = plans.iter().find(|plan| {
        plan.client_port == port.name
            && (plan.client_component == component.name
                || plan.client_component == component.qualified_name
                || plan.client_component == component.generated_name)
    });
    let handle_type = plan
        .map(service_client_handle_type)
        .unwrap_or_else(|| service_client_handle_type_for(&component.name, &port.name));
    AppApiServiceClient {
        name: port.name.clone(),
        request_type: port.request.canonical_syntax(),
        response_type: port.response.canonical_syntax(),
        handler_argument: client_handle_argument(component.language, &port.name, &handle_type),
        handle_type,
        backend: plan
            .map(|plan| plan.backend.0.clone())
            .unwrap_or_else(|| "unbound".to_string()),
        server: plan
            .map(|plan| format!("{}.{}", plan.server_instance, plan.server_port))
            .unwrap_or_else(|| "unbound".to_string()),
    }
}

fn service_server(
    graph: &GraphIr,
    component: &ComponentIr,
    port: &ServicePortIr,
    plans: &[ServiceRuntimePlan],
) -> AppApiServiceServer {
    let instance_names = component_instance_name_set(graph, component);
    let matching = plans
        .iter()
        .filter(|plan| {
            plan.server_port == port.name && instance_names.contains(plan.server_instance.as_str())
        })
        .collect::<Vec<_>>();
    AppApiServiceServer {
        name: port.name.clone(),
        request_type: port.request.canonical_syntax(),
        response_type: port.response.canonical_syntax(),
        handler_name: service_handler_name(&port.name),
        handler_signature: service_server_signature(component, port),
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

fn operation_client(
    component: &ComponentIr,
    port: &OperationPortIr,
    plans: &[OperationRuntimePlan],
) -> AppApiOperationClient {
    let plan = plans.iter().find(|plan| {
        plan.client_port == port.name
            && (plan.client_component == component.name
                || plan.client_component == component.qualified_name
                || plan.client_component == component.generated_name)
    });
    let handle_type = plan
        .map(operation_client_handle_type)
        .unwrap_or_else(|| operation_client_handle_type_for(&component.name, &port.name));
    AppApiOperationClient {
        name: port.name.clone(),
        goal_type: port.goal.canonical_syntax(),
        feedback_type: port.feedback.canonical_syntax(),
        result_type: port.result.canonical_syntax(),
        handler_argument: client_handle_argument(component.language, &port.name, &handle_type),
        handle_type,
        backend: plan
            .map(|plan| plan.backend.0.clone())
            .unwrap_or_else(|| "unbound".to_string()),
        server: plan
            .map(|plan| format!("{}.{}", plan.server_instance, plan.server_port))
            .unwrap_or_else(|| "unbound".to_string()),
    }
}

fn operation_server(
    graph: &GraphIr,
    component: &ComponentIr,
    port: &OperationPortIr,
    plans: &[OperationRuntimePlan],
) -> AppApiOperationServer {
    let instance_names = component_instance_name_set(graph, component);
    let matching = plans
        .iter()
        .filter(|plan| {
            plan.server_port == port.name && instance_names.contains(plan.server_instance.as_str())
        })
        .collect::<Vec<_>>();
    AppApiOperationServer {
        name: port.name.clone(),
        goal_type: port.goal.canonical_syntax(),
        feedback_type: port.feedback.canonical_syntax(),
        result_type: port.result.canonical_syntax(),
        handler_name: operation_handler_name(&port.name),
        handler_signature: operation_server_signature(component, port),
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

fn c_callback_table(component: &ComponentIr, graph: &GraphIr) -> AppApiCCallbackTable {
    let instances = component_instances(graph, component);
    let entries = instances
        .iter()
        .map(|instance| AppApiCCallbackEntry {
            instance: instance.name.clone(),
            factory_symbol: c_callback_factory_symbol(instance),
        })
        .collect::<Vec<_>>();
    let factory_symbol = entries
        .first()
        .map(|entry| entry.factory_symbol.clone())
        .unwrap_or_else(|| format!("flowrt_app_{}_callbacks", snake_identifier(&component.name)));
    let task_callbacks = instances
        .iter()
        .flat_map(|instance| tasks_for_instance(graph, instance))
        .map(|task| AppApiCTaskCallback {
            task: task.name.clone(),
            instance: task.instance.name.clone(),
            trigger: trigger_name(task.trigger),
            field: c_task_callback_field(task.trigger),
            signature: C_TASK_CALLBACK_SIGNATURE,
            required: true,
        })
        .collect();

    AppApiCCallbackTable {
        generated_header: "flowrt_app/c_components.h",
        factory_symbol,
        entries,
        lifecycle_callbacks: ["on_init", "on_start", "on_stop", "on_shutdown"]
            .into_iter()
            .map(|field| AppApiCCallbackField {
                field,
                signature: C_LIFECYCLE_CALLBACK_SIGNATURE,
                required: false,
            })
            .collect(),
        task_callbacks,
    }
}

fn emit_implementation_md(manifest: &AppApiManifest) -> String {
    let mut output = String::from(
        "# FlowRT App API 实现清单\n\nFlowRT 管理产物，可删除后由 `flowrt prepare` 重建。用户业务代码仍放在项目 `app/` 目录；本目录下 `stubs/` 只提供参考模板，不会被自动复制。\n\n",
    );
    output.push_str(&format!(
        "- App API manifest: `flowrt/app/app_api.json`\n- package: `{}`\n- graph: `{}` mode=`{}` profile=`{}` backend=`{}`\n\n",
        manifest.package.name,
        manifest.graph.name,
        manifest.graph.mode,
        manifest.graph.profile,
        manifest.graph.backend
    ));
    output.push_str(&format!(
        "## Runtime Context\n\n- task context timing: `{}`\n- C callback context: `{}`\n- 不改变用户 handler 签名；已有 `Context` 或 C callback context 指针用于读取 timing。\n- realtime 运行时读取 runtime observed scheduling time；`observed_delta_ms` 表示相邻 observed 时间差。\n- replay / temporary island 使用 fixture 驱动的 deterministic timing。\n- 生命周期 context 默认不携带 timing；读取前需判断 `Option`、指针或 `has_timing`。\n- fields: `{}`\n- non-goals: 不承诺硬实时，不定义 sensor timestamp / event-time、clock domain、PTP、NTP 或 approximate sync。\n\n",
        manifest.runtime_context.task_timing.access.rust,
        manifest.runtime_context.task_timing.access.c,
        manifest.runtime_context.task_timing.fields.join("`, `")
    ));
    output.push_str("## Components\n\n");
    for component in &manifest.components {
        output.push_str(&format!(
            "### `{}`\n\n- language: `{}`\n- kind: `{}`\n- user file: `{}`\n- reference stub: `{}`\n",
            component.name,
            component.language,
            component.kind,
            component.user_file_path,
            stub_path(component.language, &component.name)
        ));
        if let Some(table) = &component.c_callback_table {
            output.push_str(&format!(
                "- C callback header: `{}`\n- C callback factory: `{}`\n",
                table.generated_header, table.factory_symbol
            ));
        }
        if !component.handlers.is_empty() {
            output.push_str("- handlers:\n");
            for handler in &component.handlers {
                output.push_str(&format!(
                    "  - `{}`: `{}`\n",
                    handler.name, handler.signature
                ));
            }
        }
        if !component.resources.is_empty() {
            output.push_str("- resources:\n");
            for resource in &component.resources {
                output.push_str(&format!(
                    "  - resource {} capability={} access={} required={} readiness={} health={} on_failure={}\n",
                    resource.name,
                    resource.capability,
                    resource.access,
                    resource.required,
                    resource.readiness,
                    resource.health,
                    resource.on_failure
                ));
            }
        }
        output.push('\n');
    }
    output
}

pub(crate) fn format_app_api_manifest_text(manifest: &AppApiManifest) -> String {
    let mut output = format!(
        "flowrt explain:\npackage {} rsdl_version={}",
        manifest.package.name, manifest.package.rsdl_version
    );
    if let Some(version) = &manifest.package.version {
        output.push_str(&format!(" version={version}"));
    }
    output.push_str(&format!(
        "\ngraph {} profile={} mode={} backend={} worker_threads={}",
        manifest.graph.name,
        manifest.graph.profile,
        manifest.graph.mode,
        manifest.graph.backend,
        manifest.graph.worker_threads
    ));
    push_app_api_runtime_context_text(&mut output, &manifest.runtime_context);
    for component in &manifest.components {
        output.push_str(&format!(
            "\n  component {} language={} kind={} concurrency={} user_file={} reference_stub={}",
            component.name,
            component.language,
            component.kind,
            component.concurrency,
            component.user_file_path,
            stub_path(component.language, &component.name)
        ));
        push_app_api_tasks_text(&mut output, &component.tasks);
        push_app_api_handlers_text(&mut output, &component.handlers);
        push_app_api_ports_text(&mut output, "inputs", &component.inputs);
        push_app_api_ports_text(&mut output, "outputs", &component.outputs);
        push_app_api_resources_text(&mut output, &component.resources);
        push_app_api_params_text(&mut output, &component.params);
        push_app_api_service_clients_text(&mut output, &component.service_clients);
        push_app_api_service_servers_text(&mut output, &component.service_servers);
        push_app_api_operation_clients_text(&mut output, &component.operation_clients);
        push_app_api_operation_servers_text(&mut output, &component.operation_servers);
        if let Some(table) = &component.c_callback_table {
            push_app_api_c_callback_table_text(&mut output, table);
        }
    }
    output
}

fn push_app_api_runtime_context_text(output: &mut String, runtime_context: &AppApiRuntimeContext) {
    let timing = &runtime_context.task_timing;
    output.push_str(&format!(
        "\n  task timing context: access=rust:{} cpp:{} c:{} available_in_task_context={} available_in_lifecycle_context={} handler_signature_changed={}",
        timing.access.rust,
        timing.access.cpp,
        timing.access.c.replace(" / ", "/"),
        timing.available_in_task_context,
        timing.available_in_lifecycle_context,
        timing.handler_signature_changed
    ));
    output.push_str(&format!(
        "\n  timing fields={} clock_sources={}",
        timing.fields.join("|"),
        timing.clock_sources.join("|")
    ));
}

pub(crate) fn format_app_api_signature_summary(manifest: &AppApiManifest) -> String {
    let mut output = String::from("generated user API summary:");
    output.push_str(&format!("\ngraph {}", manifest.graph.name));
    for component in &manifest.components {
        output.push_str(&format!(
            "\n  component {} language={} kind={}",
            component.name, component.language, component.kind
        ));
        output.push_str("\n    user handlers:");
        for handler in &component.handlers {
            if handler.source == "external_process" {
                continue;
            }
            output.push_str(&format!("\n      {}", handler.signature));
        }
    }
    output
}

fn push_app_api_tasks_text(output: &mut String, tasks: &[AppApiTask]) {
    output.push_str("\n    tasks:");
    if tasks.is_empty() {
        output.push_str(" none");
        return;
    }
    for task in tasks {
        output.push_str(&format!(
            "\n      task {} instance={} trigger={} period={} readiness={} lane={} concurrency={}",
            task.name,
            task.instance,
            task.trigger,
            period_text(task.period_ms),
            task.readiness,
            task.lane,
            task.concurrency
        ));
        if let Some(deadline_ms) = task.deadline_ms {
            output.push_str(&format!(" deadline={deadline_ms}ms"));
        }
        if !task.inputs.is_empty() {
            output.push_str(&format!(" inputs={}", task.inputs.join("|")));
        }
        if !task.outputs.is_empty() {
            output.push_str(&format!(" outputs={}", task.outputs.join("|")));
        }
    }
}

fn push_app_api_handlers_text(output: &mut String, handlers: &[AppApiHandler]) {
    output.push_str("\n    handlers:");
    if handlers.is_empty() {
        output.push_str(" none");
        return;
    }
    for handler in handlers {
        output.push_str(&format!(
            "\n      {} source={} required={}: {}",
            handler.name, handler.source, handler.required, handler.signature
        ));
    }
}

fn push_app_api_ports_text(output: &mut String, label: &str, ports: &[AppApiPortView]) {
    output.push_str(&format!("\n    {label}:"));
    if ports.is_empty() {
        output.push_str(" none");
        return;
    }
    for port in ports {
        output.push_str(&format!(
            "\n      {}:{} arg={}",
            port.name, port.ty, port.handler_argument
        ));
        if let Some(source) = &port.source {
            output.push_str(&format!(" source={source}"));
        }
        if !port.targets.is_empty() {
            output.push_str(&format!(" targets={}", port.targets.join("|")));
        }
    }
}

fn push_app_api_resources_text(output: &mut String, resources: &[AppApiResourceRequirement]) {
    output.push_str("\n    resources:");
    if resources.is_empty() {
        output.push_str(" none");
        return;
    }
    for resource in resources {
        output.push_str(&format!(
            "\n      resource {} capability={} access={} required={} readiness={} health={} on_failure={}",
            resource.name,
            resource.capability,
            resource.access,
            resource.required,
            resource.readiness,
            resource.health,
            resource.on_failure
        ));
    }
}

fn push_app_api_params_text(output: &mut String, params: &[AppApiParam]) {
    output.push_str("\n    params:");
    if params.is_empty() {
        output.push_str(" none");
        return;
    }
    for param in params {
        output.push_str(&format!(
            "\n      {}:{} update={} default={}",
            param.name, param.ty, param.update, param.default
        ));
    }
}

fn push_app_api_service_clients_text(output: &mut String, clients: &[AppApiServiceClient]) {
    output.push_str("\n    service clients:");
    if clients.is_empty() {
        output.push_str(" none");
        return;
    }
    for client in clients {
        output.push_str(&format!(
            "\n      {}:{}->{} handle={} arg={} backend={} server={}",
            client.name,
            client.request_type,
            client.response_type,
            client.handle_type,
            client.handler_argument,
            client.backend,
            client.server
        ));
    }
}

fn push_app_api_service_servers_text(output: &mut String, servers: &[AppApiServiceServer]) {
    output.push_str("\n    service servers:");
    if servers.is_empty() {
        output.push_str(" none");
        return;
    }
    for server in servers {
        let clients = if server.clients.is_empty() {
            "none".to_string()
        } else {
            server.clients.join("|")
        };
        output.push_str(&format!(
            "\n      {}:{}->{} handler={} signature={} backend={} clients={}",
            server.name,
            server.request_type,
            server.response_type,
            server.handler_name,
            server.handler_signature,
            server.backend,
            clients
        ));
    }
}

fn push_app_api_operation_clients_text(output: &mut String, clients: &[AppApiOperationClient]) {
    output.push_str("\n    operation clients:");
    if clients.is_empty() {
        output.push_str(" none");
        return;
    }
    for client in clients {
        output.push_str(&format!(
            "\n      {}:{}->{}->{} handle={} arg={} backend={} server={}",
            client.name,
            client.goal_type,
            client.feedback_type,
            client.result_type,
            client.handle_type,
            client.handler_argument,
            client.backend,
            client.server
        ));
    }
}

fn push_app_api_operation_servers_text(output: &mut String, servers: &[AppApiOperationServer]) {
    output.push_str("\n    operation servers:");
    if servers.is_empty() {
        output.push_str(" none");
        return;
    }
    for server in servers {
        let clients = if server.clients.is_empty() {
            "none".to_string()
        } else {
            server.clients.join("|")
        };
        output.push_str(&format!(
            "\n      {}:{}->{}->{} handler={} signature={} backend={} clients={}",
            server.name,
            server.goal_type,
            server.feedback_type,
            server.result_type,
            server.handler_name,
            server.handler_signature,
            server.backend,
            clients
        ));
    }
}

fn push_app_api_c_callback_table_text(output: &mut String, table: &AppApiCCallbackTable) {
    output.push_str(&format!(
        "\n    C callback table: header={} factory={}",
        table.generated_header, table.factory_symbol
    ));
    for entry in &table.entries {
        output.push_str(&format!(
            "\n      entry instance={} factory={}",
            entry.instance, entry.factory_symbol
        ));
    }
    for callback in &table.lifecycle_callbacks {
        output.push_str(&format!(
            "\n      lifecycle {} required={}: {}",
            callback.field, callback.required, callback.signature
        ));
    }
    for callback in &table.task_callbacks {
        output.push_str(&format!(
            "\n      task {} instance={} trigger={} field={} required={}: {}",
            callback.task,
            callback.instance,
            callback.trigger,
            callback.field,
            callback.required,
            callback.signature
        ));
    }
}

fn emit_reference_stubs(contract: &ContractIr) -> Vec<Artifact> {
    let graph = contract
        .graphs
        .first()
        .expect("validated contract must contain a graph");
    let service_plans = service_runtime_plans(contract, graph);
    let operation_plans = operation_runtime_plans(contract, graph);
    graph_components(contract, graph)
        .into_iter()
        .map(|component| match component.language {
            LanguageKind::Rust => artifact(
                stub_path("rust", &component.name),
                rust_stub(component, &service_plans, &operation_plans),
            ),
            LanguageKind::Cpp => artifact(
                stub_path("cpp", &component.name),
                cpp_stub(component, &service_plans, &operation_plans),
            ),
            LanguageKind::C => artifact(stub_path("c", &component.name), c_stub(graph, component)),
            LanguageKind::External => artifact(
                stub_path("external", &component.name),
                external_stub(component),
            ),
        })
        .collect()
}

fn rust_stub(
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> String {
    let impl_name = component_rust_name(component);
    let receiver = crate::rust_shell::rust_component_receiver(component);
    let args = rust_stub_on_tick_args(component, service_plans, operation_plans);
    let mut output = managed_reference_header("Rust");
    output.push_str(&format!(
        "#[derive(Default)]\npub struct {impl_name};\n\nimpl flowrt_app::components::{impl_name} for {impl_name} {{\n"
    ));
    if !component.params.is_empty() {
        output.push_str(&rust_stub_params_update(component, receiver));
    }
    output.push_str(&rust_stub_service_handlers(component, receiver));
    output.push_str(&rust_stub_operation_handlers(component, receiver));
    if args.is_empty() {
        output.push_str(&format!(
            "    fn on_tick({receiver}self) -> flowrt::Status {{\n"
        ));
    } else {
        output.push_str(&format!(
            "    fn on_tick(\n        {receiver}self,\n{},\n    ) -> flowrt::Status {{\n",
            args.iter()
                .map(|arg| format!("        {arg}"))
                .collect::<Vec<_>>()
                .join(",\n")
        ));
    }
    for input in &component.inputs {
        output.push_str(&format!("        let _ = {};\n", input.name));
    }
    for output_port in &component.outputs {
        output.push_str(&format!(
            "        {}.write({}::default());\n",
            output_port.name,
            rust_stub_type(&output_port.ty)
        ));
    }
    output.push_str("        flowrt::Status::Ok\n    }\n}\n\n");
    output.push_str(&format!(
        "pub fn build_app() -> flowrt_app::runtime_shell::App {{\n    flowrt_app::runtime_shell::App::new(Box::new({impl_name}::default()))\n}}\n"
    ));
    output
}

fn rust_stub_params_update(component: &ComponentIr, receiver: &str) -> String {
    let params_ty = format!(
        "flowrt_app::components::{}Params",
        component_rust_name(component)
    );
    format!(
        "    fn on_params_update(\n        {receiver}self,\n        old_params: &{params_ty},\n        new_params: &{params_ty},\n        context: &mut flowrt::Context,\n    ) -> flowrt::Status {{\n        let _ = (old_params, new_params, context);\n        flowrt::Status::Ok\n    }}\n\n"
    )
}

fn rust_stub_service_handlers(component: &ComponentIr, receiver: &str) -> String {
    let mut output = String::new();
    for port in &component.service_servers {
        output.push_str(&format!(
            "    fn {name}(\n        {receiver}self,\n        request: &{request},\n    ) -> flowrt::ServiceResult<{response}> {{\n        let _ = request;\n        flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError)\n    }}\n\n",
            name = service_handler_name(&port.name),
            request = rust_stub_type(&port.request),
            response = rust_stub_type(&port.response),
        ));
    }
    output
}

fn rust_stub_operation_handlers(component: &ComponentIr, receiver: &str) -> String {
    let mut output = String::new();
    for port in &component.operation_servers {
        output.push_str(&format!(
            "    fn {name}(\n        {receiver}self,\n        goal: &{goal},\n        cancel: flowrt::OperationCancelToken,\n        progress: &mut flowrt::OperationProgressPublisher<{feedback}>,\n    ) -> flowrt::OperationHandlerResult<{result}> {{\n        let _ = (goal, cancel, progress);\n        flowrt::OperationHandlerResult::failed()\n    }}\n\n",
            name = operation_handler_name(&port.name),
            goal = rust_stub_type(&port.goal),
            feedback = rust_stub_type(&port.feedback),
            result = rust_stub_type(&port.result),
        ));
    }
    output
}

fn cpp_stub(
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> String {
    let class_name = crate::cpp_shell::component_cpp_name(component);
    let args = cpp_stub_on_tick_args(component, service_plans, operation_plans);
    let mut output = managed_reference_header("C++");
    output.push_str(
        "#include \"flowrt_app/runtime_shell.hpp\"\n\n#include <memory>\n\nnamespace {\n\n",
    );
    output.push_str(&format!(
        "class {class_name} final : public flowrt_app::{class_name}Interface {{\npublic:\n"
    ));
    output.push_str(&cpp_stub_service_handlers(component));
    output.push_str(&cpp_stub_operation_handlers(component));
    if args.is_empty() {
        output.push_str("    flowrt::Status on_tick() override {\n");
    } else {
        output.push_str(&format!(
            "    flowrt::Status on_tick(\n{}) override {{\n",
            args.iter()
                .map(|arg| format!("        {arg}"))
                .collect::<Vec<_>>()
                .join(",\n")
        ));
    }
    for input in &component.inputs {
        output.push_str(&format!("        (void){};\n", input.name));
    }
    for output_port in &component.outputs {
        output.push_str(&format!(
            "        {}.write({}{{}});\n",
            output_port.name,
            cpp_stub_type(&output_port.ty)
        ));
    }
    output.push_str("        return flowrt::ok();\n    }\n};\n\n}  // namespace\n\n");
    output.push_str(&format!(
        "namespace flowrt_user {{\n\nflowrt_app::App build_app() {{\n    return flowrt_app::App(std::make_unique<{class_name}>());\n}}\n\n}}  // namespace flowrt_user\n"
    ));
    output
}

fn cpp_stub_service_handlers(component: &ComponentIr) -> String {
    let mut output = String::new();
    for port in &component.service_servers {
        output.push_str(&format!(
            "    flowrt::ServiceResult<{response}> {name}(const {request}& request) override {{\n        (void)request;\n        return flowrt::ServiceResult<{response}>::err(flowrt::ServiceError::HandlerError);\n    }}\n\n",
            name = service_handler_name(&port.name),
            request = cpp_stub_type(&port.request),
            response = cpp_stub_type(&port.response),
        ));
    }
    output
}

fn cpp_stub_operation_handlers(component: &ComponentIr) -> String {
    let mut output = String::new();
    for port in &component.operation_servers {
        output.push_str(&format!(
            "    flowrt::OperationHandlerResult<{result}> {name}(\n        const {goal}& goal,\n        flowrt::OperationCancelToken cancel,\n        flowrt::OperationProgressPublisher<{feedback}>& progress) override {{\n        (void)goal;\n        (void)cancel;\n        (void)progress;\n        return flowrt::OperationHandlerResult<{result}>::failed();\n    }}\n\n",
            name = operation_handler_name(&port.name),
            goal = cpp_stub_type(&port.goal),
            feedback = cpp_stub_type(&port.feedback),
            result = cpp_stub_type(&port.result),
        ));
    }
    output
}

fn c_stub(graph: &GraphIr, component: &ComponentIr) -> String {
    let mut callback_fields = component_instances(graph, component)
        .iter()
        .flat_map(|instance| tasks_for_instance(graph, instance))
        .map(|task| c_task_callback_field(task.trigger))
        .collect::<BTreeSet<_>>();
    if callback_fields.is_empty() {
        callback_fields.insert("run_periodic");
    }
    let component_snake = snake_identifier(&component.name);
    let factory_symbol = component_instances(graph, component)
        .first()
        .map(|instance| c_callback_factory_symbol(instance))
        .unwrap_or_else(|| format!("flowrt_app_{component_snake}_callbacks"));
    let mut output = managed_reference_header("C");
    output.push_str(
        "#include \"flowrt_app/c_components.h\"\n\n#include <stddef.h>\n#include <stdint.h>\n#include <string.h>\n\n",
    );
    for hook in ["on_init", "on_start", "on_stop", "on_shutdown"] {
        output.push_str(&format!(
            "static flowrt_status_t {component_snake}_{hook}(void *user_data,\n                                          const flowrt_c_component_context_t *context) {{\n    (void)user_data;\n    (void)context;\n    return FLOWRT_STATUS_OK;\n}}\n\n"
        ));
    }
    for field in &callback_fields {
        output.push_str(&format!(
            "static flowrt_status_t {component_snake}_{field}(void *user_data,\n                                             const flowrt_c_component_context_t *context,\n                                             const flowrt_c_input_array_view_t *inputs,\n                                             flowrt_c_output_array_view_t *outputs) {{\n    (void)user_data;\n    (void)context;\n"
        ));
        if component.inputs.is_empty() {
            output.push_str("    (void)inputs;\n");
        } else {
            output.push_str(
                "    if (inputs == NULL || (inputs->len > 0U && inputs->data == NULL)) {\n        return FLOWRT_STATUS_ERROR;\n    }\n",
            );
        }
        if component.outputs.is_empty() {
            output.push_str("    (void)outputs;\n");
        } else {
            output.push_str(
                "    if (outputs == NULL || (outputs->len > 0U && outputs->data == NULL)) {\n        return FLOWRT_STATUS_ERROR;\n    }\n    for (size_t index = 0U; index < outputs->len; ++index) {\n        flowrt_c_output_slot_t *slot = &outputs->data[index];\n        if (slot->data == NULL || slot->capacity < slot->size_bytes) {\n            return FLOWRT_STATUS_ERROR;\n        }\n        memset(slot->data, 0, slot->size_bytes);\n        slot->written_len = slot->size_bytes;\n        slot->status = FLOWRT_C_OUTPUT_WRITTEN;\n    }\n",
            );
        }
        output.push_str("    return FLOWRT_STATUS_OK;\n}\n\n");
    }
    output.push_str(&format!(
        "const flowrt_c_component_callback_table_t *{factory_symbol}(void) {{\n    static const flowrt_c_component_callback_table_t callbacks = {{\n        .size = (uint32_t)sizeof(flowrt_c_component_callback_table_t),\n        .version_major = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR,\n        .version_minor = FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR,\n        .reserved0 = 0U,\n        .feature_flags = FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0 |\n                         FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1,\n        .user_data = NULL,\n        .on_init = {component_snake}_on_init,\n        .on_start = {component_snake}_on_start,\n        .on_stop = {component_snake}_on_stop,\n        .on_shutdown = {component_snake}_on_shutdown,\n        .run_periodic = {run_periodic},\n        .run_on_message = {run_on_message},\n        .run_startup = {run_startup},\n        .run_shutdown = {run_shutdown},\n        .reserved = {{0U}},\n    }};\n    return &callbacks;\n}}\n",
        run_periodic = c_callback_assignment(&component_snake, &callback_fields, "run_periodic"),
        run_on_message = c_callback_assignment(&component_snake, &callback_fields, "run_on_message"),
        run_startup = c_callback_assignment(&component_snake, &callback_fields, "run_startup"),
        run_shutdown = c_callback_assignment(&component_snake, &callback_fields, "run_shutdown"),
    ));
    output
}

fn external_stub(component: &ComponentIr) -> String {
    format!(
        "{}external component `{}` has no generated in-process App API stub.\n",
        managed_reference_header("external"),
        component.name
    )
}

fn rust_stub_on_tick_args(
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> Vec<String> {
    let mut args = Vec::new();
    for plan in service_plans
        .iter()
        .filter(|plan| component_name_matches(component, &plan.client_component))
    {
        args.push(format!(
            "{}: &flowrt_app::components::{}",
            snake_identifier(&plan.client_port),
            service_client_handle_type(plan)
        ));
    }
    for plan in operation_plans
        .iter()
        .filter(|plan| component_name_matches(component, &plan.client_component))
    {
        args.push(format!(
            "{}: &flowrt_app::components::{}",
            snake_identifier(&plan.client_port),
            operation_client_handle_type(plan)
        ));
    }
    for input in &component.inputs {
        args.push(format!(
            "{}: flowrt::Latest<'_, {}>",
            input.name,
            rust_stub_type(&input.ty)
        ));
    }
    if !component.params.is_empty() {
        args.push(format!(
            "params: &flowrt_app::components::{}Params",
            component_rust_name(component)
        ));
    }
    for output in &component.outputs {
        args.push(format!(
            "{}: &mut flowrt::Output<{}>",
            output.name,
            rust_stub_type(&output.ty)
        ));
    }
    args
}

fn cpp_stub_on_tick_args(
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> Vec<String> {
    let mut args = Vec::new();
    for plan in service_plans
        .iter()
        .filter(|plan| component_name_matches(component, &plan.client_component))
    {
        args.push(format!(
            "flowrt_app::{}& {}",
            service_client_handle_type(plan),
            snake_identifier(&plan.client_port)
        ));
    }
    for plan in operation_plans
        .iter()
        .filter(|plan| component_name_matches(component, &plan.client_component))
    {
        args.push(format!(
            "flowrt_app::{}& {}",
            operation_client_handle_type(plan),
            snake_identifier(&plan.client_port)
        ));
    }
    for input in &component.inputs {
        args.push(format!(
            "const flowrt::Latest<{}>& {}",
            cpp_stub_type(&input.ty),
            input.name
        ));
    }
    if !component.params.is_empty() {
        args.push(format!(
            "const flowrt_app::{}Params& params",
            crate::cpp_shell::component_cpp_name(component)
        ));
    }
    for output in &component.outputs {
        args.push(format!(
            "flowrt::Output<{}>& {}",
            cpp_stub_type(&output.ty),
            output.name
        ));
    }
    args
}

fn on_tick_signature(
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> String {
    match component.language {
        LanguageKind::Rust => {
            let args =
                crate::rust_shell::rust_callback_args(component, service_plans, operation_plans);
            if args.is_empty() {
                format!(
                    "fn on_tick({}self) -> flowrt::Status",
                    crate::rust_shell::rust_component_receiver(component)
                )
            } else {
                format!(
                    "fn on_tick({}self, {}) -> flowrt::Status",
                    crate::rust_shell::rust_component_receiver(component),
                    args.join(", ")
                )
            }
        }
        LanguageKind::Cpp => {
            let args =
                crate::cpp_shell::cpp_callback_args(component, service_plans, operation_plans);
            if args.is_empty() {
                "flowrt::Status on_tick()".to_string()
            } else {
                format!("flowrt::Status on_tick({})", args.join(", "))
            }
        }
        LanguageKind::C => "C task callback table entry".to_string(),
        LanguageKind::External => "no generated on_tick handler".to_string(),
    }
}

fn params_update_signature(component: &ComponentIr) -> String {
    match component.language {
        LanguageKind::Rust => format!(
            "fn on_params_update({}self, old_params: &{}Params, new_params: &{}Params, context: &mut flowrt::Context) -> flowrt::Status",
            crate::rust_shell::rust_component_receiver(component),
            component_rust_name(component),
            component_rust_name(component)
        ),
        LanguageKind::Cpp => format!(
            "flowrt::Status on_params_update(const {}Params& old_params, const {}Params& new_params, flowrt::Context& context)",
            crate::cpp_shell::component_cpp_name(component),
            crate::cpp_shell::component_cpp_name(component)
        ),
        LanguageKind::C => "no generated C params handler yet".to_string(),
        LanguageKind::External => "no generated params handler".to_string(),
    }
}

fn service_server_signature(component: &ComponentIr, port: &ServicePortIr) -> String {
    match component.language {
        LanguageKind::Rust => format!(
            "fn {}({}self, request: &{}) -> flowrt::ServiceResult<{}>",
            service_handler_name(&port.name),
            crate::rust_shell::rust_component_receiver(component),
            rust_type(&port.request),
            rust_type(&port.response)
        ),
        LanguageKind::Cpp | LanguageKind::C => format!(
            "flowrt::ServiceResult<{}> {}(const {}& request)",
            cpp_type(&port.response),
            service_handler_name(&port.name),
            cpp_type(&port.request)
        ),
        LanguageKind::External => "no generated service handler".to_string(),
    }
}

fn operation_server_signature(component: &ComponentIr, port: &OperationPortIr) -> String {
    match component.language {
        LanguageKind::Rust => format!(
            "fn {}({}self, goal: &{}, cancel: flowrt::OperationCancelToken, progress: &mut flowrt::OperationProgressPublisher<{}>) -> flowrt::OperationHandlerResult<{}>",
            operation_handler_name(&port.name),
            crate::rust_shell::rust_component_receiver(component),
            rust_type(&port.goal),
            rust_type(&port.feedback),
            rust_type(&port.result)
        ),
        LanguageKind::Cpp | LanguageKind::C => format!(
            "flowrt::OperationHandlerResult<{}> {}(const {}& goal, flowrt::OperationCancelToken cancel, flowrt::OperationProgressPublisher<{}>& progress)",
            cpp_type(&port.result),
            operation_handler_name(&port.name),
            cpp_type(&port.goal),
            cpp_type(&port.feedback)
        ),
        LanguageKind::External => "no generated operation handler".to_string(),
    }
}

fn input_handler_argument(language: LanguageKind, port: &PortIr) -> String {
    match language {
        LanguageKind::Rust => format!("{}: flowrt::Latest<'_, {}>", port.name, rust_type(&port.ty)),
        LanguageKind::Cpp => format!(
            "const flowrt::Latest<{}>& {}",
            cpp_type(&port.ty),
            port.name
        ),
        LanguageKind::C => format!("flowrt_c_input_view_t {}", port.name),
        LanguageKind::External => "external process input".to_string(),
    }
}

fn output_handler_argument(language: LanguageKind, port: &PortIr) -> String {
    match language {
        LanguageKind::Rust => format!(
            "{}: &mut flowrt::Output<{}>",
            port.name,
            rust_type(&port.ty)
        ),
        LanguageKind::Cpp => format!("flowrt::Output<{}>& {}", cpp_type(&port.ty), port.name),
        LanguageKind::C => format!("flowrt_c_output_slot_t {}", port.name),
        LanguageKind::External => "external process output".to_string(),
    }
}

fn client_handle_argument(language: LanguageKind, port_name: &str, handle_type: &str) -> String {
    match language {
        LanguageKind::Rust => format!("{}: &{}", snake_identifier(port_name), handle_type),
        LanguageKind::Cpp => format!("{}& {}", handle_type, snake_identifier(port_name)),
        LanguageKind::C => "no generated C service/operation client handle".to_string(),
        LanguageKind::External => "external process handle".to_string(),
    }
}

fn graph_components<'a>(contract: &'a ContractIr, graph: &'a GraphIr) -> Vec<&'a ComponentIr> {
    let used = graph
        .instances
        .iter()
        .map(|instance| instance.component.name.as_str())
        .collect::<BTreeSet<_>>();
    contract
        .components
        .iter()
        .filter(|component| {
            used.contains(component.name.as_str())
                || used.contains(component.qualified_name.as_str())
                || used.contains(component.generated_name.as_str())
        })
        .collect()
}

fn component_instances<'a>(graph: &'a GraphIr, component: &ComponentIr) -> Vec<&'a InstanceIr> {
    graph
        .instances
        .iter()
        .filter(|instance| component_name_matches(component, &instance.component.name))
        .collect()
}

fn component_name_matches(component: &ComponentIr, name: &str) -> bool {
    name == component.name || name == component.qualified_name || name == component.generated_name
}

fn component_instance_name_set<'a>(
    graph: &'a GraphIr,
    component: &ComponentIr,
) -> BTreeSet<&'a str> {
    component_instances(graph, component)
        .into_iter()
        .map(|instance| instance.name.as_str())
        .collect()
}

fn user_file_path(component: &ComponentIr) -> String {
    match component.language {
        LanguageKind::Rust => "app/rust/mod.rs".to_string(),
        LanguageKind::Cpp => "app/cpp/components.cpp".to_string(),
        LanguageKind::C => format!("app/c/{}.c", snake_identifier(&component.name)),
        LanguageKind::External => format!("app/external/{}.md", snake_identifier(&component.name)),
    }
}

fn stub_path(language: &str, component_name: &str) -> String {
    let dir = match language {
        "rust" => "rust",
        "cpp" => "cpp",
        "c" => "c",
        _ => "external",
    };
    let ext = match language {
        "rust" => "rs",
        "cpp" => "cpp",
        "c" => "c",
        _ => "md",
    };
    format!(
        "app/stubs/{dir}/{}.{}",
        snake_identifier(component_name),
        ext
    )
}

fn artifact(path: impl Into<PathBuf>, content: String) -> Artifact {
    Artifact {
        relative_path: path.into(),
        content,
    }
}

fn pretty_json<T: Serialize>(value: &T) -> String {
    let mut json = serde_json::to_string_pretty(value)
        .expect("App API manifest should always serialize as JSON");
    json.push('\n');
    json
}

fn period_text(period_ms: Option<u64>) -> String {
    period_ms
        .map(|period| format!("{period}ms"))
        .unwrap_or_else(|| "none".to_string())
}

fn managed_reference_header(language: &str) -> String {
    format!("// FlowRT 管理参考模板（{language}）。可删除重建；复制到用户 app/ 后再修改。\n\n")
}

fn handler_source(language: LanguageKind) -> &'static str {
    match language {
        LanguageKind::Rust => "rust_trait",
        LanguageKind::Cpp => "cpp_interface",
        LanguageKind::C => "c_callback_table",
        LanguageKind::External => "external_process",
    }
}

fn service_client_handle_type(plan: &ServiceRuntimePlan) -> String {
    service_client_handle_type_for(&plan.client_component, &plan.client_port)
}

fn service_client_handle_type_for(component: &str, port: &str) -> String {
    format!(
        "ServiceClient_{}_{}",
        snake_identifier(component),
        snake_identifier(port)
    )
}

fn operation_client_handle_type(plan: &OperationRuntimePlan) -> String {
    operation_client_handle_type_for(&plan.client_component, &plan.client_port)
}

fn operation_client_handle_type_for(component: &str, port: &str) -> String {
    format!(
        "OperationClient_{}_{}",
        snake_identifier(component),
        snake_identifier(port)
    )
}

fn service_handler_name(port: &str) -> String {
    format!("on_{}_request", snake_identifier(port))
}

fn operation_handler_name(port: &str) -> String {
    format!("on_{}_operation", snake_identifier(port))
}

fn c_callback_factory_symbol(instance: &InstanceIr) -> String {
    format!("flowrt_app_{}_callbacks", snake_identifier(&instance.name))
}

fn c_task_callback_field(trigger: TriggerKind) -> &'static str {
    match trigger {
        TriggerKind::Periodic => "run_periodic",
        TriggerKind::OnMessage => "run_on_message",
        TriggerKind::Startup => "run_startup",
        TriggerKind::Shutdown => "run_shutdown",
    }
}

fn c_callback_assignment(component_snake: &str, fields: &BTreeSet<&str>, field: &str) -> String {
    if fields.contains(field) {
        format!("{component_snake}_{field}")
    } else {
        "NULL".to_string()
    }
}

fn rust_stub_type(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive { .. } => rust_type(expr),
        TypeExpr::Named { name } => format!("flowrt_app::messages::{name}"),
        TypeExpr::Array { element, len } => format!("[{}; {len}]", rust_stub_type(element)),
        TypeExpr::VarBytes => "Vec<u8>".to_string(),
        TypeExpr::VarString { .. } => "String".to_string(),
        TypeExpr::VarSequence { element } => format!("Vec<{}>", rust_stub_type(element)),
    }
}

fn cpp_stub_type(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive { .. } => cpp_type(expr),
        TypeExpr::Named { name } => format!("flowrt_app::{name}"),
        TypeExpr::Array { element, len } => {
            format!("std::array<{}, {len}>", cpp_stub_type(element))
        }
        TypeExpr::VarBytes => "std::vector<std::uint8_t>".to_string(),
        TypeExpr::VarString { .. } => "std::string".to_string(),
        TypeExpr::VarSequence { element } => format!("std::vector<{}>", cpp_stub_type(element)),
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

fn graph_mode_name(mode: GraphMode) -> &'static str {
    match mode {
        GraphMode::Strict => "strict",
        GraphMode::Island => "island",
    }
}

fn language_name(language: LanguageKind) -> &'static str {
    match language {
        LanguageKind::Rust => "rust",
        LanguageKind::Cpp => "cpp",
        LanguageKind::C => "c",
        LanguageKind::External => "external",
    }
}

fn component_kind_name(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Native => "native",
        ComponentKind::IoBoundary => "io_boundary",
        ComponentKind::External => "external",
    }
}

fn concurrency_name(concurrency: TaskConcurrency) -> &'static str {
    match concurrency {
        TaskConcurrency::Exclusive => "exclusive",
        TaskConcurrency::Parallel => "parallel",
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

const C_LIFECYCLE_CALLBACK_SIGNATURE: &str =
    "flowrt_status_t (*)(void*, const flowrt_c_component_context_t*)";
const C_TASK_CALLBACK_SIGNATURE: &str = "flowrt_status_t (*)(void*, const flowrt_c_component_context_t*, const flowrt_c_input_array_view_t*, flowrt_c_output_array_view_t*)";
