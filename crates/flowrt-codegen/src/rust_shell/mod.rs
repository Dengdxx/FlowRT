pub(crate) mod backend_emit;
mod introspection_emit;
mod lifecycle_emit;
pub(crate) mod operation_emit;
pub(crate) mod params_emit;
mod scheduler_emit;
pub(crate) mod service_emit;
mod step_emit;

use flowrt_ir::{
    ComponentIr, ContractIr, DeterminismMode, LanguageKind, TaskConcurrency, TypeExpr,
};

use crate::runtime_plan::{
    active_boundaries_for_instances, bind_runtime_plans, boundary_runtime_plans,
    bridge_runtime_plans, contract_has_runtime_params_for_language, incoming_bind_index_map,
    incoming_boundary_index_map, incoming_bridge_index_map, operation_runtime_plans,
    outgoing_bind_indices_map, outgoing_boundary_indices_map, outgoing_bridge_indices_map,
    process_runtime_plans, service_runtime_plans,
};
use crate::{component_by_name, component_rust_name, managed_header};

use backend_emit::selected_backend_name;
use introspection_emit::emit_rust_introspection_helpers;
use lifecycle_emit::{emit_rust_app_new, emit_rust_app_run, emit_rust_app_run_tick};
use operation_emit::{
    emit_rust_operation_client_handles, operation_client_handle_name,
    rust_operation_handler_methods,
};
use params_emit::{emit_rust_param_constraint_helpers, rust_params_struct};
use scheduler_emit::emit_all_step_functions;
use service_emit::{
    client_handle_name, emit_rust_service_client_handles, rust_service_handler_methods,
};
use step_emit::RustStepEmission;

fn rust_backend_constructor(selected_backend: &str) -> &'static str {
    match selected_backend {
        "inproc" => "flowrt::inproc_backend()",
        "iox2" => "flowrt::iox2_backend()",
        "zenoh" => "flowrt::zenoh_backend()",
        _ => unreachable!("validated contract selected backend must be known"),
    }
}

fn selected_profile_uses_global_tick(contract: &ContractIr) -> bool {
    contract
        .profiles
        .first()
        .is_some_and(|profile| profile.determinism.mode == DeterminismMode::GlobalTick)
}

fn rust_component_ports_need_message_import(
    contract: &ContractIr,
    instances: &[&flowrt_ir::InstanceIr],
) -> bool {
    instances.iter().any(|instance| {
        let component = component_by_name(contract, &instance.component.name);
        component
            .inputs
            .iter()
            .chain(component.outputs.iter())
            .any(|port| type_expr_uses_named_message(&port.ty))
    })
}

fn type_expr_uses_named_message(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Named { .. } => true,
        TypeExpr::Array { element, .. } | TypeExpr::VarSequence { element, .. } => {
            type_expr_uses_named_message(element)
        }
        TypeExpr::Primitive { .. } | TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } => {
            false
        }
    }
}

pub(crate) fn emit_rust_components(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str("\nuse crate::messages::*;\n\n");

    // 预先计算 service/operation plans（整个 contract 的）
    let graph = contract.graphs.first();
    let service_plans = graph
        .map(|g| crate::runtime_plan::service_runtime_plans(contract, g))
        .unwrap_or_default();
    let operation_plans = graph
        .map(|g| crate::runtime_plan::operation_runtime_plans(contract, g))
        .unwrap_or_default();
    if let Some(g) = graph {
        output.push_str(&emit_rust_service_client_handles(contract, g));
        output.push_str(&emit_rust_operation_client_handles(contract, g));
    }

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
            "pub trait {}{} {{\n",
            component_rust_name(component),
            rust_component_trait_bound(component)
        ));
        output.push_str(&rust_lifecycle_doc("组件初始化钩子"));
        output.push_str(
            "    ///\n    /// `restart` 故障策略会在同一对象上重新调用本钩子，实现必须可重入：不得依赖仅首次成立的前置状态。\n",
        );
        output.push_str(&format!(
            "    fn on_init({}self, _context: &mut flowrt::Context) -> flowrt::Status {{\n",
            rust_component_receiver(component)
        ));
        output.push_str("        flowrt::Status::ok()\n    }\n\n");
        output.push_str(&rust_lifecycle_doc("组件启动钩子"));
        output.push_str(&format!(
            "    fn on_start({}self, _context: &mut flowrt::Context) -> flowrt::Status {{\n",
            rust_component_receiver(component)
        ));
        output.push_str("        flowrt::Status::ok()\n    }\n\n");
        output.push_str(&rust_lifecycle_doc("组件停止钩子"));
        output.push_str(&format!(
            "    fn on_stop({}self, _context: &mut flowrt::Context) -> flowrt::Status {{\n",
            rust_component_receiver(component)
        ));
        output.push_str("        flowrt::Status::ok()\n    }\n\n");
        output.push_str(&rust_lifecycle_doc("组件关闭钩子"));
        output.push_str(&format!(
            "    fn on_shutdown({}self, _context: &mut flowrt::Context) -> flowrt::Status {{\n",
            rust_component_receiver(component)
        ));
        output.push_str("        flowrt::Status::ok()\n    }\n\n");
        output.push_str(&params_emit::rust_params_update_signature(component));
        // service handler 方法
        if let Some(g) = graph {
            output.push_str(&rust_service_handler_methods(component, g, &service_plans));
            output.push_str(&rust_operation_handler_methods(
                component,
                g,
                &operation_plans,
            ));
        }
        output.push_str(&rust_tick_signature(
            component,
            &service_plans,
            &operation_plans,
        ));
        output.push_str("}\n\n");
    }
    crate::normalize_text_eof_newline(&mut output);
    output
}

pub(crate) fn emit_rust_runtime_shell(contract: &ContractIr) -> String {
    let graph = contract
        .graphs
        .first()
        .expect("normalized contract must contain at least one graph");
    let order = crate::topo_order_instances_for_language(contract, graph, LanguageKind::Rust);
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
    let needs_message_import = !bind_plans.is_empty()
        || !bridge_plans.is_empty()
        || !active_boundaries_for_instances(&boundary_plans, &order).is_empty()
        || rust_component_ports_need_message_import(contract, &order)
        || service_runtime_plans(contract, graph)
            .iter()
            .any(|plan| plan.backend.0 != "zenoh")
        || !operation_runtime_plans(contract, graph).is_empty();

    let mut output = managed_header();
    output.push_str("\nuse crate::components::*;\n");
    if needs_message_import {
        output.push_str("use crate::messages::*;\n");
    }
    output.push_str("use crate::selfdesc;\nuse crate::user;\n\n");
    output.push_str(&format!(
        "const PACKAGE_NAME: &str = {};\n\n",
        crate::rust_string_literal(&contract.package.name)
    ));
    output.push_str(
        "type FlowrtOutputCommit = Box<dyn FnOnce(&App, &flowrt::IntrospectionState, &flowrt::ScheduleWaiter, &mut std::collections::BTreeMap<String, flowrt::IntrospectionTaskHealth>) -> flowrt::Status + Send>;\n\n",
    );
    let has_active_rust_channels =
        !crate::runtime_plan::active_binds_for_instances(&bind_plans, &order).is_empty()
            || bridge_plans.iter().any(|bridge| {
                order
                    .iter()
                    .any(|instance| instance.name == bridge.source_instance)
            });
    output.push_str(&emit_rust_introspection_helpers(
        has_active_rust_channels,
        contract_has_runtime_params_for_language(contract, LanguageKind::Rust),
    ));
    output.push_str(&emit_rust_param_constraint_helpers(&order, contract));
    output.push_str("pub struct App {\n");
    output.push_str("    startup_status: flowrt::Status,\n");
    for instance in &order {
        let component = component_by_name(contract, &instance.component.name);
        output.push_str(&format!(
            "    {}: {},\n",
            instance.name,
            rust_component_storage_type(component)
        ));
        if !component.params.is_empty() {
            output.push_str(&format!(
                "    {}_params: std::sync::Arc<std::sync::Mutex<{}Params>>,\n",
                instance.name,
                component_rust_name(component)
            ));
        }
    }
    for bind in &bind_plans {
        output.push_str(&format!(
            "    {}: std::sync::Arc<std::sync::Mutex<{}>>,\n",
            bind.field_name,
            backend_emit::runtime_channel_type(contract, bind)
        ));
        output.push_str(&format!(
            "    {}: std::sync::OnceLock<flowrt::IntrospectionChannelProbe>,\n",
            bind.probe_field_name
        ));
    }
    for bridge in &bridge_plans {
        output.push_str(&format!(
            "    {}: std::sync::Arc<std::sync::Mutex<{}>>,\n",
            bridge.field_name,
            backend_emit::bridge_runtime_channel_type(bridge)
        ));
    }
    for boundary in active_boundaries_for_instances(&boundary_plans, &order) {
        let ty = crate::messages::rust_type(&boundary.ty);
        let field_ty = match boundary.direction {
            flowrt_ir::BoundaryDirection::Input => format!("flowrt::BoundaryInput<{ty}>"),
            flowrt_ir::BoundaryDirection::Output => format!("flowrt::BoundaryOutput<{ty}>"),
        };
        output.push_str(&format!("    {}: {},\n", boundary.field_name, field_ty));
    }
    for task in step_emit::on_synchronized_tasks(graph, &order) {
        output.push_str(&format!(
            "    {}: {},\n",
            step_emit::rust_synchronizer_field_name(task),
            step_emit::rust_synchronizer_field_type()
        ));
    }
    // service client/server fields
    output.push_str(&service_emit::rust_app_service_fields(contract, graph));
    output.push_str(&operation_emit::rust_app_operation_fields(contract, graph));
    output.push_str("}\n\n");

    output.push_str("impl App {\n");
    let dataflow_tasks = crate::scheduler_tasks_for_order(graph, &order);
    let dataflow_lane_count = step_emit::scheduler_lane_ids(&dataflow_tasks).len();
    output.push_str(&emit_rust_app_new(
        contract,
        graph,
        &order,
        &bind_plans,
        &bridge_plans,
        &boundary_plans,
        dataflow_lane_count,
    ));
    let step_emission = RustStepEmission {
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

    emit_all_step_functions(&step_emission, graph, &order, &mut output);
    scheduler_emit::emit_process_step_functions(&step_emission, graph, &process_plans, &mut output);
    output.push_str(&emit_rust_app_run(
        contract,
        graph,
        &order,
        &bind_plans,
        &bridge_plans,
        &boundary_plans,
    ));
    let global_tick = selected_profile_uses_global_tick(contract);
    if global_tick {
        output.push_str(&emit_rust_app_run_tick(
            contract,
            graph,
            &order,
            &bind_plans,
            &bridge_plans,
            &boundary_plans,
        ));
    }
    output.push_str(&lifecycle_emit::emit_rust_app_run_process_dispatch(
        &process_plans,
    ));
    lifecycle_emit::emit_process_run_functions(
        contract,
        graph,
        &bind_plans,
        &bridge_plans,
        &boundary_plans,
        &process_plans,
        &mut output,
    );
    output.push_str("}\n\n");
    let backend_constructor = rust_backend_constructor(&selected_backend);
    output.push_str(&format!(
        "pub fn backend() -> Box<dyn flowrt::Backend> {{\n    Box::new({backend_constructor})\n}}\n\n",
    ));
    if global_tick {
        output.push_str(
            "pub fn flowrt_run_tick(grant: flowrt::ExternalTick) -> flowrt::ExternalTickReport {\n    let backend = backend();\n    let status = user::build_app().run_tick(backend.as_ref(), grant);\n    flowrt::ExternalTickReport::new(grant.tick_id, status)\n}\n\n",
        );
    }
    output.push_str(
        "pub fn run(run_ticks: Option<usize>) -> flowrt::Status {\n    let backend = backend();\n    user::build_app().run(backend.as_ref(), run_ticks)\n}\n\npub fn run_process(process: &str, run_ticks: Option<usize>) -> flowrt::Status {\n    let backend = backend();\n    user::build_app().run_process(backend.as_ref(), process, run_ticks)\n}\n",
    );
    output
}

pub(crate) fn emit_rust_lib(include_runtime_shell: bool) -> String {
    let mut output = managed_header();
    if include_runtime_shell {
        output.push_str(
            "\npub(crate) mod selfdesc;\npub mod components;\npub mod messages;\npub mod runtime_shell;\npub mod supervisor;\n#[path = \"../../../app/rust/mod.rs\"]\npub mod user;\n\npub use runtime_shell::{run, run_process, App};\n",
        );
    } else {
        output.push_str("\npub(crate) mod selfdesc;\npub mod supervisor;\n");
    }
    output
}

pub(crate) fn emit_rust_main() -> String {
    let mut output = managed_header();
    output.push_str(
        "\nfn main() {\n    let mut args = std::env::args().skip(1);\n    let mut process = None;\n    let mut run_ticks = None;\n    while let Some(arg) = args.next() {\n        match arg.as_str() {\n            \"--process\" => process = args.next(),\n            \"--flowrt-run-ticks\" | \"--flowrt-run-steps\" => {\n                let Some(raw_ticks) = args.next() else {\n                    eprintln!(\"missing value for {arg}\");\n                    std::process::exit(2);\n                };\n                match raw_ticks.parse::<usize>() {\n                    Ok(ticks) if ticks > 0 => run_ticks = Some(ticks),\n                    _ => {\n                        eprintln!(\"invalid value for {arg}: {raw_ticks}\");\n                        std::process::exit(2);\n                    }\n                }\n            }\n            _ => {\n                eprintln!(\"unknown FlowRT app argument: {arg}\");\n                std::process::exit(2);\n            }\n        }\n    }\n\n    let status = match process.as_deref() {\n        Some(process) => flowrt_app::runtime_shell::run_process(process, run_ticks),\n        None => flowrt_app::runtime_shell::run(run_ticks),\n    };\n    let code = match status {\n        flowrt::Status::Ok => 0,\n        _ => 1,\n    };\n    std::process::exit(code);\n}\n",
    );
    output
}

pub(crate) fn rust_callback_args(
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
            args.push(format!("{arg_name}: &{}", client_handle_name(plan)));
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
                "{arg_name}: &{}",
                operation_client_handle_name(plan)
            ));
        }
    }
    for input in &component.inputs {
        args.push(format!(
            "{}: flowrt::Latest<'_, {}>",
            input.name,
            crate::messages::rust_type(&input.ty)
        ));
    }
    if !component.params.is_empty() {
        args.push(format!("params: &{}Params", component_rust_name(component)));
    }
    for output in &component.outputs {
        args.push(format!(
            "{}: &mut flowrt::Output<{}>",
            output.name,
            crate::messages::rust_type(&output.ty)
        ));
    }
    args
}

fn rust_tick_signature(
    component: &ComponentIr,
    service_plans: &[crate::runtime_plan::ServiceRuntimePlan],
    operation_plans: &[crate::runtime_plan::OperationRuntimePlan],
) -> String {
    let args = rust_callback_args(component, service_plans, operation_plans);
    let doc = rust_tick_doc(component, service_plans, operation_plans);
    if args.is_empty() {
        format!(
            "{doc}    fn on_tick({}self) -> flowrt::Status;\n",
            rust_component_receiver(component)
        )
    } else {
        let joined = args
            .iter()
            .map(|arg| format!("        {arg}"))
            .collect::<Vec<_>>()
            .join(",\n");
        format!(
            "{doc}    fn on_tick(\n        {}self,\n{joined},\n    ) -> flowrt::Status;\n",
            rust_component_receiver(component)
        )
    }
}

pub(crate) fn rust_component_is_parallel(component: &ComponentIr) -> bool {
    component.concurrency == TaskConcurrency::Parallel
}

pub(crate) fn rust_component_trait_bound(component: &ComponentIr) -> &'static str {
    if rust_component_is_parallel(component) {
        ": Send + Sync"
    } else {
        ": Send"
    }
}

pub(crate) fn rust_component_receiver(component: &ComponentIr) -> &'static str {
    if rust_component_is_parallel(component) {
        "&"
    } else {
        "&mut "
    }
}

pub(crate) fn rust_component_constructor_type(component: &ComponentIr) -> String {
    if rust_component_is_parallel(component) {
        format!("Box<dyn {} + Send + Sync>", component_rust_name(component))
    } else {
        format!("Box<dyn {} + Send>", component_rust_name(component))
    }
}

pub(crate) fn rust_component_storage_type(component: &ComponentIr) -> String {
    if rust_component_is_parallel(component) {
        format!(
            "std::sync::Arc<Box<dyn {} + Send + Sync>>",
            component_rust_name(component)
        )
    } else {
        format!(
            "std::sync::Arc<std::sync::Mutex<Box<dyn {} + Send>>>",
            component_rust_name(component)
        )
    }
}

pub(crate) fn rust_component_method_call(
    component: &ComponentIr,
    instance_name: &str,
    method_call: &str,
) -> String {
    rust_component_method_call_for_receiver(
        component,
        &format!("self.{instance_name}"),
        method_call,
    )
}

pub(crate) fn rust_component_method_call_for_receiver(
    component: &ComponentIr,
    receiver: &str,
    method_call: &str,
) -> String {
    if rust_component_is_parallel(component) {
        format!("{receiver}.as_ref().as_ref().{method_call}")
    } else {
        format!("{receiver}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).{method_call}")
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

fn rust_tick_doc(
    component: &ComponentIr,
    service_plans: &[crate::runtime_plan::ServiceRuntimePlan],
    operation_plans: &[crate::runtime_plan::OperationRuntimePlan],
) -> String {
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
    let mut emitted_service_args = std::collections::BTreeSet::new();
    for plan in service_plans.iter().filter(|plan| {
        plan.client_component == component.name
            || plan.client_component == component.generated_name
            || plan.client_component == component.qualified_name
    }) {
        let arg_name = crate::snake_identifier(&plan.client_port);
        if emitted_service_args.insert(arg_name.clone()) {
            output.push_str(&format!(
                "    /// - `{arg_name}`: typed service client handle。\n"
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
                "    /// - `{arg_name}`: typed Operation client handle。\n"
            ));
        }
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
