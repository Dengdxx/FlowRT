use flowrt_ir::{ComponentKind, ContractIr, GraphIr, InstanceIr, IoBoundaryReadiness};

use crate::runtime_plan::{
    BindRuntimePlan, BoundaryRuntimePlan, BridgeRuntimePlan, ProcessRuntimePlan,
    active_boundaries_for_instances, bind_backend,
};

use super::backend_emit;
use super::operation_emit;
use super::scheduler_emit;
use super::service_emit;

pub(super) fn emit_rust_app_new(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
    dataflow_lane_count: usize,
) -> String {
    let mut output = String::new();
    output.push_str("    pub fn new(\n");
    for instance in order {
        let component = crate::component_by_name(contract, &instance.component.name);
        output.push_str(&format!(
            "        {}: {},\n",
            instance.name,
            super::rust_component_constructor_type(component)
        ));
    }
    let startup_status_binding = if has_fallible_transport_startup(binds, bridges) {
        "let mut startup_status = flowrt::Status::Ok;"
    } else {
        "let startup_status = flowrt::Status::Ok;"
    };
    output.push_str(&format!(
        "    ) -> Self {{\n        {startup_status_binding}\n",
    ));
    for instance in order {
        let component = crate::component_by_name(contract, &instance.component.name);
        if super::rust_component_is_parallel(component) {
            output.push_str(&format!(
                "        let {name} = std::sync::Arc::new({name});\n",
                name = instance.name,
            ));
        } else {
            output.push_str(&format!(
                "        let {name} = std::sync::Arc::new(std::sync::Mutex::new({name}));\n",
                name = instance.name,
            ));
        }
    }
    // service registration (before Self construction)
    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    let (service_registration, _service_initializers) =
        service_emit::emit_rust_service_new(contract, graph, dataflow_lane_count);
    if !service_registration.is_empty() {
        output.push_str(&service_registration);
    }
    let inproc_service_count = service_plans
        .iter()
        .filter(|plan| plan.backend.0 != "zenoh")
        .count();
    let (operation_registration, _operation_initializers) = operation_emit::emit_rust_operation_new(
        contract,
        graph,
        dataflow_lane_count + inproc_service_count,
    );
    if !operation_registration.is_empty() {
        output.push_str(&operation_registration);
    }
    output.push_str("        Self {\n");
    for instance in order {
        let component = crate::component_by_name(contract, &instance.component.name);
        output.push_str(&format!(
            "            {}: {}.clone(),\n",
            instance.name, instance.name
        ));
        if !component.params.is_empty() {
            output.push_str(&format!(
                "            {}_params: std::sync::Arc::new(std::sync::Mutex::new({})),\n",
                instance.name,
                super::params_emit::rust_params_initializer(component, instance)
            ));
        }
    }
    for bind in binds {
        output.push_str(&format!(
            "            {}: std::sync::Arc::new(std::sync::Mutex::new({})),\n",
            bind.field_name,
            backend_emit::runtime_channel_initializer(contract, graph, bind)
        ));
        output.push_str(&format!(
            "            {}: std::sync::OnceLock::new(),\n",
            bind.probe_field_name
        ));
    }
    for bridge in bridges {
        output.push_str(&format!(
            "            {}: std::sync::Arc::new(std::sync::Mutex::new({})),\n",
            bridge.field_name,
            backend_emit::bridge_runtime_channel_initializer(contract, graph, bridge)
        ));
    }
    for boundary in active_boundaries_for_instances(boundaries, order) {
        let initializer = match boundary.direction {
            flowrt_ir::BoundaryDirection::Input => "flowrt::BoundaryInput::new()",
            flowrt_ir::BoundaryDirection::Output => "flowrt::BoundaryOutput::new()",
        };
        output.push_str(&format!(
            "            {}: {},\n",
            boundary.field_name, initializer
        ));
    }
    // service field initializers
    let (_service_registration, service_initializers) =
        service_emit::emit_rust_service_new(contract, graph, dataflow_lane_count);
    if !service_initializers.is_empty() {
        output.push_str(&service_initializers);
    }
    let (_operation_registration, operation_initializers) = operation_emit::emit_rust_operation_new(
        contract,
        graph,
        dataflow_lane_count + inproc_service_count,
    );
    if !operation_initializers.is_empty() {
        output.push_str(&operation_initializers);
    }
    output.push_str("            startup_status,\n");
    output.push_str("        }\n    }\n");
    output
}

fn has_fallible_transport_startup(
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
) -> bool {
    !bridges.is_empty()
        || binds
            .iter()
            .any(|bind| matches!(bind_backend(bind), "iox2" | "zenoh"))
}

pub(super) fn emit_rust_app_run(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
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
        bridges,
        boundaries,
        graph,
        process: None,
        process_name: "main",
        public: true,
    })
}

pub(super) fn emit_rust_app_run_process_dispatch(processes: &[ProcessRuntimePlan<'_>]) -> String {
    let mut output = String::new();
    output.push_str(
        "    pub fn run_process(self, backend: &dyn flowrt::Backend, process: &str, run_ticks: Option<usize>) -> flowrt::Status {\n        match process {\n",
    );
    for process in processes {
        output.push_str(&format!(
            "            {} => self.run_process_{}(backend, run_ticks),\n",
            crate::rust_string_literal(&process.name),
            process.method_suffix
        ));
    }
    output.push_str("            _ => flowrt::Status::Error,\n        }\n    }\n");
    output
}

pub(super) fn emit_process_run_functions(
    contract: &ContractIr,
    graph: &GraphIr,
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    boundaries: &[BoundaryRuntimePlan],
    processes: &[ProcessRuntimePlan<'_>],
    output: &mut String,
) {
    for process in processes {
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
            binds,
            bridges,
            boundaries,
            graph,
            process: Some(process),
            process_name: &process.name,
            public: false,
        }));
    }
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
    bridges: &'a [BridgeRuntimePlan],
    boundaries: &'a [BoundaryRuntimePlan],
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
        "    {visibility}fn {function_name}(self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {{\n        if self.startup_status != flowrt::Status::Ok {{\n            return self.startup_status;\n        }}\n        let app = std::sync::Arc::new(self);\n        let mut lifecycle_context = flowrt::Context::default();\n        let mut status = flowrt::Status::Ok;\n",
    ));
    output.push_str("        let _ = backend;\n");
    output.push_str("        let shutdown = flowrt::install_signal_shutdown_token();\n");
    output.push_str("        let introspection_state = flowrt::IntrospectionState::new();\n");
    output.push_str("        let scheduler_events = flowrt::ScheduleWaiter::new();\n");
    output.push_str(&run_scope_receiver(
        &scheduler_emit::emit_rust_scheduler_event_registration(
            emission.binds,
            emission.bridges,
            emission.boundaries,
        ),
    ));
    output.push_str(
        "        introspection_state.set_self_description_json(selfdesc::self_description_json());\n",
    );
    output.push_str(&run_scope_receiver(
        &super::introspection_emit::emit_rust_introspection_channel_registration(
            emission.contract,
            emission.graph,
            emission.order,
            emission.binds,
        ),
    ));
    output.push_str(&run_scope_receiver(
        &super::introspection_emit::emit_rust_introspection_bridge_registration(
            emission.graph,
            emission.order,
            emission.bridges,
        ),
    ));
    output.push_str(
        &super::params_emit::emit_rust_introspection_param_registration(
            emission.contract,
            emission.order,
        ),
    );
    output.push_str(&emit_rust_io_boundary_registration(
        emission.contract,
        emission.order,
    ));
    output.push_str(&run_scope_receiver(&emit_rust_boundary_input_registration(
        emission.boundaries,
    )));
    output.push_str(&run_scope_receiver(
        &emit_rust_boundary_output_probe_registration(emission.boundaries),
    ));
    output.push_str(&service_emit::emit_rust_service_introspection_registration(
        emission.contract,
        emission.graph,
    ));
    output.push_str(&format!(
        "        let _introspection_server = flowrt::spawn_status_server(\n            flowrt::IntrospectionIdentity {{\n                self_description_hash: selfdesc::self_description_hash().to_string(),\n                package: {}.to_string(),\n                process: {}.to_string(),\n                runtime: \"rust\".to_string(),\n            }},\n            introspection_state.clone(),\n        )\n        .ok();\n",
        "PACKAGE_NAME",
        crate::rust_string_literal(emission.process_name)
    ));
    if crate::runtime_plan::contract_has_params_for_language(
        emission.contract,
        flowrt_ir::LanguageKind::Rust,
    ) {
        output.push_str(&format!(
            "        let remote_params_key_expr = flowrt::params_key_expr(PACKAGE_NAME, selfdesc::self_description_hash(), std::process::id());\n        let _remote_params_server = match flowrt::ZenohParamsServer::open_from_environment(\n            &remote_params_key_expr,\n            flowrt::IntrospectionHandshake {{\n                protocol_version: flowrt::INTROSPECTION_PROTOCOL_VERSION.to_string(),\n                pid: std::process::id(),\n                started_at_unix_ms: 0,\n                self_description_hash: selfdesc::self_description_hash().to_string(),\n                package: PACKAGE_NAME.to_string(),\n                process: {}.to_string(),\n                runtime: \"rust\".to_string(),\n            }},\n            introspection_state.clone(),\n        ) {{\n            Ok(server) => Some(server),\n            Err(error) => {{\n                eprintln!(\"FlowRT: failed to start zenoh params control-plane {{}}: {{error}}\", remote_params_key_expr);\n                None\n            }}\n        }};\n",
            crate::rust_string_literal(emission.process_name)
        ));
    }
    for instance in emission.order {
        output.push_str(&format!(
            "        let mut {name}_initialized = false;\n        let mut {name}_started = false;\n",
            name = instance.name
        ));
    }
    output.push_str(&emit_rust_io_boundary_contexts(
        emission.contract,
        emission.order,
    ));
    for instance in emission.order {
        let component = crate::component_by_name(emission.contract, &instance.component.name);
        let context_name = lifecycle_context_name(component, instance);
        let call = run_scope_receiver(&component_call_expr(
            component,
            instance,
            &format!("on_init(&mut {context_name})"),
        ));
        output.push_str(&format!(
            "        if status == flowrt::Status::Ok {{\n            status = {call};\n            {name}_initialized = status == flowrt::Status::Ok;\n        }}\n",
            name = instance.name
        ));
    }
    for instance in emission.order {
        let component = crate::component_by_name(emission.contract, &instance.component.name);
        let context_name = lifecycle_context_name(component, instance);
        let call = run_scope_receiver(&component_call_expr(
            component,
            instance,
            &format!("on_start(&mut {context_name})"),
        ));
        output.push_str(&format!(
            "        if status == flowrt::Status::Ok && {name}_initialized {{\n            status = {call};\n            {name}_started = status == flowrt::Status::Ok;\n        }}\n",
            name = instance.name
        ));
        if component
            .io_boundary
            .as_ref()
            .is_some_and(|policy| policy.readiness == IoBoundaryReadiness::ComponentStarted)
        {
            output.push_str(&format!(
                "        if {name}_started {{\n            if let Some(boundary) = {context_name}.boundary() {{\n                boundary.mark_ready();\n            }}\n        }}\n",
                name = instance.name,
                context_name = context_name,
            ));
        }
    }
    output.push_str(&format!(
        "        if status == flowrt::Status::Ok {{\n            status = app.{startup_function_name}(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());\n        }}\n",
        startup_function_name = emission.steps.startup
    ));
    let service_ready_marks =
        service_emit::emit_rust_service_ready_marks(emission.contract, emission.graph);
    if !service_ready_marks.is_empty() {
        output.push_str("        if status == flowrt::Status::Ok {\n");
        output.push_str(&service_ready_marks);
        output.push_str("        }\n");
    }
    output.push_str(&run_scope_receiver(
        &scheduler_emit::emit_rust_scheduler_v2_loop(scheduler_emit::RustSchedulerLoopEmission {
            contract: emission.contract,
            graph: emission.graph,
            order: emission.order,
            binds: emission.binds,
            bridges: emission.bridges,
            boundaries: emission.boundaries,
            process: emission.process,
            fallback_step_function: emission.steps.scheduler,
        }),
    ));
    output.push_str(&format!(
        "        if status == flowrt::Status::Ok {{\n            status = app.{shutdown_function_name}(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());\n        }}\n",
        shutdown_function_name = emission.steps.shutdown
    ));
    for instance in emission.order.iter().rev() {
        let component = crate::component_by_name(emission.contract, &instance.component.name);
        let context_name = lifecycle_context_name(component, instance);
        let call = run_scope_receiver(&component_call_expr(
            component,
            instance,
            &format!("on_stop(&mut {context_name})"),
        ));
        output.push_str(&format!(
            "        if {name}_started {{\n            let stop_status = {call};\n            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {{\n                status = flowrt::Status::Error;\n            }}\n        }}\n",
            name = instance.name
        ));
    }
    for instance in emission.order.iter().rev() {
        let component = crate::component_by_name(emission.contract, &instance.component.name);
        let context_name = lifecycle_context_name(component, instance);
        let call = run_scope_receiver(&component_call_expr(
            component,
            instance,
            &format!("on_shutdown(&mut {context_name})"),
        ));
        output.push_str(&format!(
            "        if {name}_initialized {{\n            let shutdown_status = {call};\n            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {{\n                status = flowrt::Status::Error;\n            }}\n        }}\n",
            name = instance.name
        ));
    }
    output.push_str("        status\n    }\n");
    output
}

fn run_scope_receiver(code: &str) -> String {
    code.replace("self.", "app.")
}

fn emit_rust_io_boundary_registration(contract: &ContractIr, order: &[&InstanceIr]) -> String {
    let mut output = String::new();
    for instance in order {
        let component = crate::component_by_name(contract, &instance.component.name);
        if component.kind != ComponentKind::IoBoundary {
            continue;
        }
        output.push_str(&format!(
            "        introspection_state.register_io_boundary({}, {}, vec![\n",
            crate::rust_string_literal(&instance.name),
            crate::rust_string_literal(&component.name)
        ));
        for resource in &component.resources {
            output.push_str(&format!(
                "            flowrt::IntrospectionIoBoundaryResourceStatus {{\n                name: {}.to_string(),\n                kind: {}.to_string(),\n                ready: false,\n                message: None,\n                last_error: None,\n                updated_unix_ms: None,\n            }},\n",
                crate::rust_string_literal(&resource.name),
                crate::rust_string_literal(&resource.capability.0)
            ));
        }
        output.push_str("        ]);\n");
    }
    output
}

fn emit_rust_boundary_input_registration(boundaries: &[BoundaryRuntimePlan]) -> String {
    let mut output = String::new();
    for boundary in boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Input)
    {
        let ty = crate::messages::rust_type(&boundary.ty);
        output.push_str(&format!(
            "        introspection_state.register_boundary_input::<{ty}>({}, {}, self.{}.clone());\n",
            crate::rust_string_literal(&boundary.endpoint_name),
            crate::rust_string_literal(&boundary.ty.canonical_syntax()),
            boundary.field_name,
        ));
    }
    output
}

fn emit_rust_boundary_output_probe_registration(boundaries: &[BoundaryRuntimePlan]) -> String {
    let mut output = String::new();
    for boundary in boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Output)
    {
        let ty = crate::messages::rust_type(&boundary.ty);
        output.push_str(&format!(
            "        introspection_state.register_channel({}, {});\n        let _{field}_probe = self.{field}.register_sink({{\n            let introspection_state = introspection_state.clone();\n            move |value, published_at_ms| {{\n                let mut payload = vec![0u8; <{ty} as flowrt::FrameCodec>::encoded_frame_size(value)];\n                if <{ty} as flowrt::FrameCodec>::encode_frame(value, &mut payload).is_ok() {{\n                    introspection_state.record_channel_publish_bytes({}, {}, payload, published_at_ms);\n                }}\n            }}\n        }});\n",
            crate::rust_string_literal(&boundary.endpoint_name),
            crate::rust_string_literal(&boundary.ty.canonical_syntax()),
            crate::rust_string_literal(&boundary.endpoint_name),
            crate::rust_string_literal(&boundary.ty.canonical_syntax()),
            field = boundary.field_name,
        ));
    }
    output
}

fn emit_rust_io_boundary_contexts(contract: &ContractIr, order: &[&InstanceIr]) -> String {
    let mut output = String::new();
    for instance in order {
        let component = crate::component_by_name(contract, &instance.component.name);
        if component.kind != ComponentKind::IoBoundary {
            continue;
        }
        output.push_str(&format!(
            "        let mut {context_name} = flowrt::Context::for_boundary(flowrt::BoundaryContext::new({instance_name}, {component_name}, introspection_state.clone()));\n",
            context_name = lifecycle_context_name(component, instance),
            instance_name = crate::rust_string_literal(&instance.name),
            component_name = crate::rust_string_literal(&component.name)
        ));
    }
    output
}

fn lifecycle_context_name(component: &flowrt_ir::ComponentIr, instance: &InstanceIr) -> String {
    if component.kind == ComponentKind::IoBoundary {
        format!(
            "{}_boundary_context",
            crate::snake_identifier(&instance.name)
        )
    } else {
        "lifecycle_context".to_string()
    }
}

/// 生成组件方法调用表达式。对于 service server 实例使用 `Mutex` 保护可变访问。
fn component_call_expr(
    component: &flowrt_ir::ComponentIr,
    instance: &InstanceIr,
    method_call: &str,
) -> String {
    super::rust_component_method_call(component, &instance.name, method_call)
}
