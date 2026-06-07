use flowrt_ir::{ContractIr, GraphIr, InstanceIr};

use crate::runtime_plan::{BindRuntimePlan, BridgeRuntimePlan, ProcessRuntimePlan, bind_backend};

use super::backend_emit;
use super::scheduler_emit;
use super::service_emit;

pub(super) fn emit_rust_app_new(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
    dataflow_lane_count: usize,
) -> String {
    let mut output = String::new();
    output.push_str("    pub fn new(\n");
    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    let server_instances: std::collections::BTreeSet<&str> = service_plans
        .iter()
        .map(|p| p.server_instance.as_str())
        .collect();
    for instance in order {
        let component = crate::component_by_name(contract, &instance.component.name);
        let send_bound = if server_instances.contains(instance.name.as_str()) {
            " + Send"
        } else {
            ""
        };
        output.push_str(&format!(
            "        {}: Box<dyn {}{}>,\n",
            instance.name,
            crate::component_rust_name(component),
            send_bound
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
    // wrap service server components in Arc<Mutex<...>>
    for instance in order {
        if server_instances.contains(instance.name.as_str()) {
            let _component = crate::component_by_name(contract, &instance.component.name);
            output.push_str(&format!(
                "        let {name} = std::sync::Arc::new(std::sync::Mutex::new({name}));\n",
                name = instance.name,
            ));
        }
    }
    // service registration (before Self construction)
    let (service_registration, _service_initializers) =
        service_emit::emit_rust_service_new(contract, graph, dataflow_lane_count);
    if !service_registration.is_empty() {
        output.push_str(&service_registration);
    }
    output.push_str("        Self {\n");
    for instance in order {
        let component = crate::component_by_name(contract, &instance.component.name);
        if server_instances.contains(instance.name.as_str()) {
            output.push_str(&format!(
                "            {name}: {name}.clone(),\n",
                name = instance.name
            ));
        } else {
            output.push_str(&format!("            {},\n", instance.name));
        }
        if !component.params.is_empty() {
            output.push_str(&format!(
                "            {}_params: {},\n",
                instance.name,
                super::params_emit::rust_params_initializer(component, instance)
            ));
        }
    }
    for bind in binds {
        output.push_str(&format!(
            "            {}: {},\n",
            bind.field_name,
            backend_emit::runtime_channel_initializer(contract, graph, bind)
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
            backend_emit::bridge_runtime_channel_initializer(contract, graph, bridge)
        ));
    }
    // service field initializers
    let (_service_registration, service_initializers) =
        service_emit::emit_rust_service_new(contract, graph, dataflow_lane_count);
    if !service_initializers.is_empty() {
        output.push_str(&service_initializers);
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
) -> String {
    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    let service_server_instances: std::collections::BTreeSet<String> = service_plans
        .iter()
        .map(|p| p.server_instance.clone())
        .collect();
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
        service_server_instances: &service_server_instances,
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
    processes: &[ProcessRuntimePlan<'_>],
    output: &mut String,
) {
    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    let service_server_instances: std::collections::BTreeSet<String> = service_plans
        .iter()
        .map(|p| p.server_instance.clone())
        .collect();
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
            graph,
            process: Some(process),
            process_name: &process.name,
            public: false,
            service_server_instances: &service_server_instances,
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
    graph: &'a GraphIr,
    process: Option<&'a ProcessRuntimePlan<'a>>,
    process_name: &'a str,
    public: bool,
    service_server_instances: &'a std::collections::BTreeSet<String>,
}

fn emit_rust_app_run_function(emission: RustRunFunctionEmission<'_>) -> String {
    let mut output = String::new();
    let visibility = if emission.public { "pub " } else { "" };
    let function_name = emission.function_name;
    output.push_str(&format!(
        "    {visibility}fn {function_name}(mut self, backend: &dyn flowrt::Backend, run_ticks: Option<usize>) -> flowrt::Status {{\n        if self.startup_status != flowrt::Status::Ok {{\n            return self.startup_status;\n        }}\n        let mut lifecycle_context = flowrt::Context::default();\n        let mut status = flowrt::Status::Ok;\n",
    ));
    output.push_str("        let _ = backend;\n");
    output.push_str("        let shutdown = flowrt::install_signal_shutdown_token();\n");
    output.push_str("        let introspection_state = flowrt::IntrospectionState::new();\n");
    output.push_str("        let scheduler_events = flowrt::ScheduleWaiter::new();\n");
    output.push_str(&scheduler_emit::emit_rust_scheduler_event_registration(
        emission.binds,
    ));
    output.push_str(
        "        introspection_state.set_self_description_json(selfdesc::self_description_json());\n",
    );
    output.push_str(
        &super::introspection_emit::emit_rust_introspection_channel_registration(
            emission.contract,
            emission.order,
            emission.binds,
        ),
    );
    output.push_str(
        &super::params_emit::emit_rust_introspection_param_registration(
            emission.contract,
            emission.order,
        ),
    );
    output.push_str(&service_emit::emit_rust_service_introspection_registration(
        emission.contract,
        emission.graph,
    ));
    output.push_str(&format!(
        "        let _introspection_server = flowrt::spawn_status_server(\n            flowrt::IntrospectionIdentity {{\n                self_description_hash: selfdesc::self_description_hash().to_string(),\n                package: {}.to_string(),\n                process: {}.to_string(),\n                runtime: \"rust\".to_string(),\n            }},\n            introspection_state.clone(),\n        )\n        .ok();\n",
        "PACKAGE_NAME",
        crate::rust_string_literal(emission.process_name)
    ));
    for instance in emission.order {
        output.push_str(&format!(
            "        let mut {name}_initialized = false;\n        let mut {name}_started = false;\n",
            name = instance.name
        ));
    }
    for instance in emission.order {
        let call = component_call_expr(
            instance,
            emission.service_server_instances,
            "on_init(&mut lifecycle_context)",
        );
        output.push_str(&format!(
            "        if status == flowrt::Status::Ok {{\n            status = {call};\n            {name}_initialized = status == flowrt::Status::Ok;\n        }}\n",
            name = instance.name
        ));
    }
    for instance in emission.order {
        let call = component_call_expr(
            instance,
            emission.service_server_instances,
            "on_start(&mut lifecycle_context)",
        );
        output.push_str(&format!(
            "        if status == flowrt::Status::Ok && {name}_initialized {{\n            status = {call};\n            {name}_started = status == flowrt::Status::Ok;\n        }}\n",
            name = instance.name
        ));
    }
    output.push_str(&format!(
        "        if status == flowrt::Status::Ok {{\n            status = self.{startup_function_name}(0, &mut lifecycle_context, &introspection_state, &scheduler_events);\n        }}\n",
        startup_function_name = emission.steps.startup
    ));
    output.push_str(&scheduler_emit::emit_rust_scheduler_v2_loop(
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
        let call = component_call_expr(
            instance,
            emission.service_server_instances,
            "on_stop(&mut lifecycle_context)",
        );
        output.push_str(&format!(
            "        if {name}_started {{\n            let stop_status = {call};\n            if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok {{\n                status = flowrt::Status::Error;\n            }}\n        }}\n",
            name = instance.name
        ));
    }
    for instance in emission.order.iter().rev() {
        let call = component_call_expr(
            instance,
            emission.service_server_instances,
            "on_shutdown(&mut lifecycle_context)",
        );
        output.push_str(&format!(
            "        if {name}_initialized {{\n            let shutdown_status = {call};\n            if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok {{\n                status = flowrt::Status::Error;\n            }}\n        }}\n",
            name = instance.name
        ));
    }
    output.push_str("        status\n    }\n");
    output
}

/// 生成组件方法调用表达式。对于 service server 实例使用 `Mutex` 保护可变访问。
fn component_call_expr(
    instance: &InstanceIr,
    service_server_instances: &std::collections::BTreeSet<String>,
    method_call: &str,
) -> String {
    if service_server_instances.contains(&instance.name) {
        format!(
            "self.{name}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).{method_call}",
            name = instance.name
        )
    } else {
        format!("self.{name}.{method_call}", name = instance.name)
    }
}
