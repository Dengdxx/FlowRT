use super::*;

pub(super) fn emit_cpp_app_constructor_declaration(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut params = Vec::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.language != LanguageKind::Cpp {
            continue;
        }
        params.push(format!(
            "std::unique_ptr<{}Interface> {}",
            component_cpp_name(component),
            instance.name
        ));
    }

    let mut output = String::new();
    output.push_str("    /**\n     * @brief 构造 C++ 应用 shell。\n     *\n");
    if params.is_empty() {
        output.push_str("     * 该 contract 没有需要注入的 C++ 组件实例。\n");
    } else {
        for instance in order {
            let component = component_by_name(contract, &instance.component.name);
            if component.language != LanguageKind::Cpp {
                continue;
            }
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

pub(super) fn cpp_instance_needs_operation_shared_ptr(
    instance: &InstanceIr,
    operation_plans: &[crate::runtime_plan::OperationRuntimePlan],
) -> bool {
    operation_plans
        .iter()
        .any(|plan| plan.server_instance == instance.name)
}

pub(super) fn emit_cpp_app_constructor(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
    bridges: &[BridgeRuntimePlan],
) -> String {
    let mut params = Vec::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.language != LanguageKind::Cpp {
            continue;
        }
        params.push(format!(
            "std::unique_ptr<{}Interface> {}",
            component_cpp_name(component),
            instance.name
        ));
    }

    let mut initializers = Vec::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.language == LanguageKind::C {
            initializers.push(format!(
                "{}_(make_c_{}_adapter())",
                instance.name,
                crate::snake_identifier(&instance.name)
            ));
        } else {
            initializers.push(format!("{}_(std::move({}))", instance.name, instance.name));
        }
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
            cpp_runtime_channel_initializer(contract, graph, bind)
        ));
    }
    for bridge in bridges {
        initializers.push(format!(
            "{}_({})",
            bridge.field_name,
            cpp_bridge_runtime_channel_initializer(contract, graph, bridge)
        ));
    }
    for task in super::step_emit::cpp_on_synchronized_tasks(graph, order) {
        initializers.push(format!(
            "{}_({})",
            super::step_emit::cpp_synchronizer_field_name(task),
            super::step_emit::cpp_synchronizer_ctor_args(graph, task)
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
    output.push_str(" {\n");

    // service registration
    let service_plans = crate::runtime_plan::service_runtime_plans(contract, graph);
    for plan in &service_plans {
        if plan.backend.0 == "zenoh" || plan.backend.0 == "iox2" {
            continue;
        }
        let service_name_literal = cpp_string_literal(&plan.service_name);
        let client_field = cpp_service_client_field_name(plan);
        let server_field = cpp_service_server_field_name(plan);
        let req_ty = cpp_type(&plan.request_type);
        let resp_ty = cpp_type(&plan.response_type);
        let queue_depth = plan.queue_depth.max(1);
        let max_in_flight = plan.max_in_flight.max(1);
        let default_timeout = plan.timeout_ms.max(1);
        let server_lane = crate::runtime_plan::service_server_lane(plan);
        let server_lane_id = cpp_lane_id_u64_expr(&server_lane);

        output.push_str(&format!(
            "    {{\n        flowrt::InprocServiceConfig config;\n        config.queue_depth = {queue_depth};\n        config.max_in_flight = {max_in_flight};\n        config.default_timeout_ms = {default_timeout};\n        {server_field}_.emplace(\n            {service_name_literal},\n            [this](const {req_ty}& request) -> flowrt::ServiceResult<{resp_ty}> {{\n                if (!this->{server_instance}_) {{\n                    return flowrt::ServiceResult<{resp_ty}>::err(flowrt::ServiceError::Unavailable);\n                }}\n                return this->{server_instance}_->on_{port}_request(request);\n            }},\n            config);\n        {client_field}_ = {cpp_handle_name}(\n            flowrt::InprocServiceClient<{req_ty}, {resp_ty}>(\n                {service_name_literal}, *{server_field}_, 0, {server_lane_id}));\n    }}\n",
            port = crate::snake_identifier(&plan.server_port),
            cpp_handle_name = cpp_service_client_handle_name(plan),
            server_instance = plan.server_instance,
        ));
    }
    let operation_plans = crate::runtime_plan::operation_runtime_plans(contract, graph);
    for plan in &operation_plans {
        if plan.backend.0 == "zenoh" {
            let operation_key_name = cpp_string_literal(&plan.operation_name);
            let queue_depth = plan.queue_depth.max(1);
            let max_in_flight = plan.max_in_flight.max(1);
            let timeout_ms = plan.timeout_ms.max(1);
            let result_retention_ms = plan.result_retention_ms;
            let concurrency = cpp_operation_concurrency(plan.concurrency);
            let preempt = cpp_operation_preempt(plan.preempt);
            let operation_index = plan.index;
            output.push_str(&format!(
                "    {{\n        const auto operation_policy_{operation_index} = flowrt::OperationPolicy::make(\n            std::chrono::milliseconds{{{timeout_ms}}},\n            {concurrency},\n            {preempt},\n            {queue_depth}U,\n            {max_in_flight}U,\n            std::chrono::milliseconds{{{result_retention_ms}}});\n        this->operation_control_{operation_index}_ = std::make_shared<flowrt::OperationControl>(\n            flowrt::fnv1a64({operation_key_name}),\n            operation_policy_{operation_index}.value());\n    }}\n"
            ));
            continue;
        }
        output.push_str(&cpp_operation_registration_block(plan));
    }

    output.push_str("}\n\n");
    output
}

pub(super) fn emit_cpp_resource_registration(
    graph: &GraphIr,
    order: &[&InstanceIr],
    process_name: &str,
) -> String {
    let instance_names = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut output = String::new();
    for satisfaction in &graph.resource_satisfactions {
        if !instance_names.contains(satisfaction.instance.name.as_str()) {
            continue;
        }
        let provider = satisfaction.provider.as_ref();
        output.push_str(&format!(
            "    introspection_state.register_resource(flowrt::IntrospectionResourceStatus{{\n        .name = {},\n        .capability = {},\n        .access = std::optional<std::string>{{{}}},\n        .state = \"unknown\",\n        .required = {},\n        .readiness = std::optional<std::string>{{{}}},\n        .health = std::optional<std::string>{{{}}},\n        .on_failure = std::optional<std::string>{{{}}},\n        .contract_status = std::optional<std::string>{{{}}},\n        .satisfied = std::optional<bool>{{{}}},\n        .provider = {},\n        .provider_scope = std::nullopt,\n        .provider_readiness_source = std::nullopt,\n        .provider_health_source = std::nullopt,\n        .diagnostic = {},\n        .suggestion = std::nullopt,\n        .source = std::optional<std::string>{{\"contract\"}},\n        .owner_process = std::optional<std::string>{{{}}},\n        .last_error = std::nullopt,\n        .updated_unix_ms = std::nullopt,\n    }});\n",
            cpp_string_literal(&format!(
                "{}.{}",
                satisfaction.instance.name, satisfaction.resource
            )),
            cpp_string_literal(&satisfaction.capability.0),
            cpp_string_literal(resource_access_name(satisfaction.access)),
            satisfaction.required,
            cpp_string_literal(resource_readiness_name(satisfaction.readiness)),
            cpp_string_literal(resource_health_name(satisfaction.health)),
            cpp_string_literal(resource_failure_name(satisfaction.on_failure)),
            cpp_string_literal(resource_satisfaction_status(satisfaction)),
            satisfaction.satisfied,
            provider.map_or_else(
                || "std::nullopt".to_string(),
                |provider| format!(
                    "std::optional<std::string>{{{}}}",
                    cpp_string_literal(&provider.name)
                )
            ),
            satisfaction.diagnostic.as_ref().map_or_else(
                || "std::nullopt".to_string(),
                |diagnostic| format!(
                    "std::optional<std::string>{{{}}}",
                    cpp_string_literal(diagnostic)
                )
            ),
            cpp_string_literal(process_name),
        ));
    }
    output
}

pub(super) fn emit_cpp_io_boundary_registration(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.kind != ComponentKind::IoBoundary {
            continue;
        }
        output.push_str(&format!(
            "    introspection_state.register_io_boundary({}, {}, {});\n",
            cpp_string_literal(&instance.name),
            cpp_string_literal(&component.name),
            cpp_boundary_resources_literal(component)
        ));
    }
    output
}

pub(super) fn emit_cpp_boundary_input_registration(
    contract: &ContractIr,
    boundaries: &[BoundaryRuntimePlan],
) -> String {
    let mut output = String::new();
    for boundary in boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Input)
    {
        match crate::runtime_plan::boundary_sample_time_source(contract, &boundary.ty) {
            Some((stamp_field, unit_to_ns)) => {
                let ty = cpp_type(&boundary.ty);
                output.push_str(&format!(
                    "    introspection_state.register_boundary_input_with_sample_time<{ty}>({}, {}, {}_, [](std::span<const std::uint8_t> payload) -> std::optional<std::uint64_t> {{ try {{ return std::optional<std::uint64_t>{{static_cast<std::uint64_t>(flowrt::detail::decode_frame<{ty}>(payload).{stamp_field}) * {unit_to_ns}U}}; }} catch (const flowrt::WireCodecError&) {{ return std::nullopt; }} }});\n",
                    cpp_string_literal(&boundary.endpoint_name),
                    cpp_string_literal(&boundary.ty.canonical_syntax()),
                    boundary.field_name,
                ));
            }
            None => {
                output.push_str(&format!(
                    "    introspection_state.register_boundary_input({}, {}, {}_);\n",
                    cpp_string_literal(&boundary.endpoint_name),
                    cpp_string_literal(&boundary.ty.canonical_syntax()),
                    boundary.field_name,
                ));
            }
        }
    }
    output
}

pub(super) fn emit_cpp_boundary_output_probe_registration(
    boundaries: &[BoundaryRuntimePlan],
) -> String {
    let mut output = String::new();
    for boundary in boundaries
        .iter()
        .filter(|boundary| boundary.direction == flowrt_ir::BoundaryDirection::Output)
    {
        let ty = cpp_type(&boundary.ty);
        output.push_str(&format!(
            "    introspection_state.register_channel({}, {});\n    auto {field}_probe = {field}_.register_sink(\n        [&introspection_state](const {ty}& value, std::optional<std::uint64_t> published_at_ms) {{\n            try {{\n                std::vector<std::uint8_t> payload(flowrt::detail::encoded_frame_size(value));\n                flowrt::detail::encode_frame(value, std::span<std::uint8_t>{{payload}});\n                introspection_state.record_channel_publish_bytes({}, {}, std::move(payload), published_at_ms);\n            }} catch (...) {{\n            }}\n        }});\n    (void){field}_probe;\n",
            cpp_string_literal(&boundary.endpoint_name),
            cpp_string_literal(&boundary.ty.canonical_syntax()),
            cpp_string_literal(&boundary.endpoint_name),
            cpp_string_literal(&boundary.ty.canonical_syntax()),
            field = boundary.field_name,
        ));
    }
    output
}

pub(super) fn emit_cpp_io_boundary_contexts(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if component.kind != ComponentKind::IoBoundary {
            continue;
        }
        output.push_str(&format!(
            "    auto {context} = flowrt::Context::for_boundary(flowrt::BoundaryContext{{{}, {}, {}, [&introspection_state](flowrt::BoundaryStatus status) {{\n        introspection_state.record_io_boundary_health(std::move(status));\n    }}, [&introspection_state](std::string_view name, const flowrt::FrameDescriptor& descriptor, flowrt::FrameLeaseStatus status, bool payload_recording, std::optional<flowrt::FramePayloadArtifact> artifact) {{\n        const auto record = artifact.has_value()\n            ? introspection_state.record_frame_descriptor_payload_event(name, descriptor, status, std::move(*artifact))\n            : introspection_state.record_frame_descriptor_event(name, descriptor, status, payload_recording);\n        return flowrt::BoundaryRecordOutcome{{.recorded = record.recorded, .dropped = record.dropped}};\n    }}}});\n",
            cpp_string_literal(&instance.name),
            cpp_string_literal(&component.name),
            cpp_boundary_resources_literal(component),
            context = cpp_lifecycle_context_name(component, instance),
        ));
    }
    output
}

pub(super) fn cpp_boundary_resources_literal(component: &ComponentIr) -> String {
    let mut output = String::from("std::vector<flowrt::BoundaryResourceStatus>{");
    for resource in &component.resources {
        output.push_str(&format!(
            "flowrt::BoundaryResourceStatus{{.name = {}, .kind = {}}},",
            cpp_string_literal(&resource.name),
            cpp_string_literal(&resource.capability.0)
        ));
    }
    output.push('}');
    output
}

pub(super) fn cpp_lifecycle_context_name(component: &ComponentIr, instance: &InstanceIr) -> String {
    if component.kind == ComponentKind::IoBoundary {
        format!(
            "{}_boundary_context",
            crate::snake_identifier(&instance.name)
        )
    } else {
        "lifecycle_context".to_string()
    }
}
