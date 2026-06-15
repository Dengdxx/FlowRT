use super::*;

pub(crate) fn self_description_summary(self_description: &SelfDescription) -> String {
    let total_services: usize = self_description
        .graphs
        .iter()
        .map(|graph| graph.services.len())
        .sum();
    let total_operations: usize = self_description
        .graphs
        .iter()
        .map(|graph| graph.operations.len())
        .sum();
    let total_boundaries: usize = self_description
        .graphs
        .iter()
        .map(|graph| graph.boundary_endpoints.len())
        .sum();
    let island_profiles = self_description
        .profiles
        .iter()
        .filter(|profile| profile.mode == "island")
        .count();
    let mut output = format!(
        "package={} selfdesc={} source_hash={} artifact_mode={} temporary_island={} test_only={} temporary_overlay={} clock_source={} clock_unit={} clock_field={} profiles={} island_profiles={} graphs={} component_types={} instances={} tasks={} channels={} boundary_endpoints={} services={} operations={} messages={}",
        self_description.package.name,
        self_description.self_description_version,
        self_description.source_hash,
        self_description.artifact.mode,
        self_description.artifact.temporary_island,
        self_description.artifact.test_only,
        self_description.artifact.temporary_overlay.is_some(),
        self_description.artifact.clock.source,
        self_description.artifact.clock.unit,
        self_description.artifact.clock.field,
        self_description.profiles.len(),
        island_profiles,
        self_description.graphs.len(),
        self_description.component_types.len(),
        self_description
            .graphs
            .iter()
            .map(|graph| graph.instances.len())
            .sum::<usize>(),
        self_description
            .graphs
            .iter()
            .map(|graph| graph.tasks.len())
            .sum::<usize>(),
        self_description
            .graphs
            .iter()
            .map(|graph| graph.channels.len())
            .sum::<usize>(),
        total_boundaries,
        total_services,
        total_operations,
        self_description.message_abi.len()
    );

    // 按名称索引 component types。
    let component_types: BTreeMap<&str, &SelfDescriptionComponentType> = self_description
        .component_types
        .iter()
        .map(|ct| (ct.name.as_str(), ct))
        .collect();

    for graph in &self_description.graphs {
        output.push_str(&format!("\ngraph {} mode={}", graph.name, graph.mode));
        if !graph.resource_contract.providers.is_empty()
            || !graph.resource_contract.requirements.is_empty()
            || !graph.resource_contract.satisfactions.is_empty()
        {
            output.push_str(&format!(
                "\n  resource_contract_version={}",
                graph.resource_contract.resource_contract_version
            ));
            for provider in &graph.resource_contract.providers {
                let mut scope_detail = String::new();
                if let Some(target) = &provider.target {
                    scope_detail.push_str(&format!(" target={target}"));
                }
                if let Some(process) = &provider.process {
                    scope_detail.push_str(&format!(" process={process}"));
                }
                if let Some(package) = &provider.external_package {
                    scope_detail.push_str(&format!(" external_package={package}"));
                }
                output.push_str(&format!(
                    "\n  resource_provider {} scope={} capabilities={} readiness_source={} health_source={}{}",
                    provider.name,
                    provider.scope,
                    provider.capabilities.join("|"),
                    provider.readiness_source,
                    provider.health_source,
                    scope_detail
                ));
            }
            for requirement in &graph.resource_contract.requirements {
                output.push_str(&format!(
                    "\n  resource_requirement {}.{} component={} capability={} access={} required={} readiness={} health={} on_failure={} satisfaction={} provider={}",
                    requirement.instance,
                    requirement.name,
                    requirement.component,
                    requirement.capability,
                    requirement.access,
                    requirement.required,
                    requirement.readiness,
                    requirement.health,
                    requirement.on_failure,
                    requirement.satisfaction,
                    requirement.provider.as_deref().unwrap_or("none")
                ));
                if let Some(diagnostic) = &requirement.diagnostic {
                    output.push_str(&format!(" diagnostic={diagnostic}"));
                }
            }
            for satisfaction in &graph.resource_contract.satisfactions {
                output.push_str(&format!(
                    "\n  resource_satisfaction {}.{} component={} capability={} access={} required={} readiness={} health={} on_failure={} status={} provider={} satisfied={}",
                    satisfaction.instance,
                    satisfaction.resource,
                    satisfaction.component,
                    satisfaction.capability,
                    satisfaction.access,
                    satisfaction.required,
                    satisfaction.readiness,
                    satisfaction.health,
                    satisfaction.on_failure,
                    satisfaction.status,
                    satisfaction.provider.as_deref().unwrap_or("none"),
                    satisfaction.satisfied
                ));
                if let Some(diagnostic) = &satisfaction.diagnostic {
                    output.push_str(&format!(" diagnostic={diagnostic}"));
                }
            }
        }

        // 展示 component types。
        let graph_component_names: BTreeMap<&str, ()> = graph
            .instances
            .iter()
            .map(|inst| (inst.component.as_str(), ()))
            .collect();
        for name in graph_component_names.keys() {
            if let Some(ct) = component_types.get(name) {
                output.push_str(&format!(
                    "\n  component {} language={} kind={}",
                    ct.name, ct.language, ct.kind
                ));
                if !ct.inputs.is_empty() {
                    let ports: Vec<String> = ct
                        .inputs
                        .iter()
                        .map(|p| format!("{}:{}", p.name, p.ty))
                        .collect();
                    output.push_str(&format!("\n    inputs: {}", ports.join(", ")));
                }
                if !ct.outputs.is_empty() {
                    let ports: Vec<String> = ct
                        .outputs
                        .iter()
                        .map(|p| format!("{}:{}", p.name, p.ty))
                        .collect();
                    output.push_str(&format!("\n    outputs: {}", ports.join(", ")));
                }
                if !ct.service_clients.is_empty() {
                    let ports: Vec<String> = ct
                        .service_clients
                        .iter()
                        .map(|p| format!("{}:{}->{}", p.name, p.request_type, p.response_type))
                        .collect();
                    output.push_str(&format!("\n    service_clients: {}", ports.join(", ")));
                }
                if !ct.service_servers.is_empty() {
                    let ports: Vec<String> = ct
                        .service_servers
                        .iter()
                        .map(|p| format!("{}:{}->{}", p.name, p.request_type, p.response_type))
                        .collect();
                    output.push_str(&format!("\n    service_servers: {}", ports.join(", ")));
                }
                if !ct.operation_clients.is_empty() {
                    let ports: Vec<String> = ct
                        .operation_clients
                        .iter()
                        .map(|p| {
                            format!(
                                "{}:{}->{}->{}",
                                p.name, p.goal_type, p.feedback_type, p.result_type
                            )
                        })
                        .collect();
                    output.push_str(&format!("\n    operation_clients: {}", ports.join(", ")));
                }
                if !ct.operation_servers.is_empty() {
                    let ports: Vec<String> = ct
                        .operation_servers
                        .iter()
                        .map(|p| {
                            format!(
                                "{}:{}->{}->{}",
                                p.name, p.goal_type, p.feedback_type, p.result_type
                            )
                        })
                        .collect();
                    output.push_str(&format!("\n    operation_servers: {}", ports.join(", ")));
                }
                if !ct.params.is_empty() {
                    let params: Vec<String> = ct
                        .params
                        .iter()
                        .map(|p| format!("{}:{} update={}", p.name, p.ty, p.update))
                        .collect();
                    output.push_str(&format!("\n    params: {}", params.join(", ")));
                }
            }
        }

        for endpoint in &graph.boundary_endpoints {
            output.push_str(&format!(
                "\n  boundary {} {} endpoint={} type={}",
                endpoint.direction, endpoint.name, endpoint.endpoint, endpoint.message_type
            ));
        }

        // 按 instance 分组展示 tasks、channels、services 和 params。
        for instance in &graph.instances {
            output.push_str(&format!(
                "\n  instance {} component={} process={} runtime={}",
                instance.name, instance.component, instance.process, instance.runtime
            ));
            if let Some(target) = &instance.target {
                output.push_str(&format!(" target={}", target));
            }

            // 该 instance 的 tasks。
            for task in &graph.tasks {
                if task.instance == instance.name {
                    output.push_str(&format!(
                        "\n    task {} trigger={}",
                        task.name, task.trigger
                    ));
                    if !task.lane.is_empty() {
                        output.push_str(&format!(" lane={}", task.lane));
                    }
                    if let Some(period) = task.period_ms {
                        output.push_str(&format!(" period_ms={}", period));
                    }
                    if let Some(deadline) = task.deadline_ms {
                        output.push_str(&format!(" deadline_ms={}", deadline));
                    }
                }
            }

            // 该 instance 作为 from 或 to 的 channels。
            for channel in &graph.channels {
                let from_instance = channel.from.split('.').next().unwrap_or("");
                let to_instance = channel.to.split('.').next().unwrap_or("");
                if from_instance == instance.name || to_instance == instance.name {
                    output.push_str(&format!(
                        "\n    channel {} -> {} type={} backend={}",
                        channel.from, channel.to, channel.message_type, channel.backend
                    ));
                    if !channel.thread_affinity.is_empty() {
                        output.push_str(&format!(" thread_affinity={}", channel.thread_affinity));
                    }
                    if !channel.channel.is_empty() {
                        output.push_str(&format!(" kind={}", channel.channel));
                    }
                }
            }

            // 该 instance 参与的 services。
            for service in &graph.services {
                if service.client_instance == instance.name
                    || service.server_instance == instance.name
                {
                    output.push_str(&format!(
                        "\n    service {} client={}.{} server={}.{} request={} response={} backend={}",
                        service.name,
                        service.client_instance,
                        service.client_port,
                        service.server_instance,
                        service.server_port,
                        service.request_type,
                        service.response_type,
                        service.backend
                    ));
                }
            }

            // 该 instance 参与的 operations。
            for operation in &graph.operations {
                if operation.client_instance == instance.name
                    || operation.server_instance == instance.name
                {
                    output.push_str(&format!(
                        "\n    operation {} client={}.{} server={}.{} goal={} feedback={} result={} backend={}",
                        operation.name,
                        operation.client_instance,
                        operation.client_port,
                        operation.server_instance,
                        operation.server_port,
                        operation.goal_type,
                        operation.feedback_type,
                        operation.result_type,
                        operation.backend
                    ));
                }
            }

            // 该 instance 的 params。
            for param in &instance.params {
                output.push_str(&format!(
                    "\n    param {}:{} update={} current={}",
                    param.name,
                    param.ty,
                    param.update,
                    serde_json::to_string(&param.current).unwrap_or_else(|_| "null".to_string())
                ));
            }
        }

        // 展示未被任何 instance 引用的 orphan services。
        let instance_names: BTreeMap<&str, ()> = graph
            .instances
            .iter()
            .map(|inst| (inst.name.as_str(), ()))
            .collect();
        for service in &graph.services {
            if !instance_names.contains_key(service.client_instance.as_str())
                && !instance_names.contains_key(service.server_instance.as_str())
            {
                output.push_str(&format!(
                    "\n  service {} client={}.{} server={}.{} request={} response={} backend={}",
                    service.name,
                    service.client_instance,
                    service.client_port,
                    service.server_instance,
                    service.server_port,
                    service.request_type,
                    service.response_type,
                    service.backend
                ));
            }
        }
        for operation in &graph.operations {
            if !instance_names.contains_key(operation.client_instance.as_str())
                && !instance_names.contains_key(operation.server_instance.as_str())
            {
                output.push_str(&format!(
                    "\n  operation {} client={}.{} server={}.{} goal={} feedback={} result={} backend={}",
                    operation.name,
                    operation.client_instance,
                    operation.client_port,
                    operation.server_instance,
                    operation.server_port,
                    operation.goal_type,
                    operation.feedback_type,
                    operation.result_type,
                    operation.backend
                ));
            }
        }
    }
    for message in &self_description.message_abi {
        output.push_str(&format!(
            "\nmessage {} size={}",
            message.type_name, message.size_bytes
        ));
    }
    output
}

pub(crate) fn self_description_nodes(self_description: &SelfDescription) -> String {
    let component_types: BTreeMap<&str, &SelfDescriptionComponentType> = self_description
        .component_types
        .iter()
        .map(|ct| (ct.name.as_str(), ct))
        .collect();
    let mut lines = Vec::new();
    for graph in &self_description.graphs {
        lines.push(format!("graph {}", graph.name));
        for instance in &graph.instances {
            let kind = component_types
                .get(instance.component.as_str())
                .map(|ct| ct.kind.as_str())
                .unwrap_or("");
            if kind.is_empty() {
                lines.push(format!(
                    "{} process={} runtime={} component={}",
                    instance.name, instance.process, instance.runtime, instance.component
                ));
            } else {
                lines.push(format!(
                    "{} process={} runtime={} component={} kind={}",
                    instance.name, instance.process, instance.runtime, instance.component, kind
                ));
            }
        }
    }
    if lines.is_empty() {
        "no graphs".to_string()
    } else {
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameDescriptorEcho {
    resource_id_hash: u64,
    slot: u32,
    generation: u64,
    size_bytes: u64,
    timestamp_unix_ns: u64,
    width: u32,
    height: u32,
    stride_bytes: u32,
    format_id: u32,
    encoding_id: u32,
    flags: u32,
}

impl FrameDescriptorEcho {
    fn decode(payload: &[u8]) -> Result<Self> {
        if payload.len() != 64 {
            anyhow::bail!(
                "standard frame descriptor payload must be 64 bytes, found {}",
                payload.len()
            );
        }
        Ok(Self {
            resource_id_hash: read_u64_le(payload, 0)?,
            slot: read_u32_le(payload, 8)?,
            generation: read_u64_le(payload, 16)?,
            size_bytes: read_u64_le(payload, 24)?,
            timestamp_unix_ns: read_u64_le(payload, 32)?,
            width: read_u32_le(payload, 40)?,
            height: read_u32_le(payload, 44)?,
            stride_bytes: read_u32_le(payload, 48)?,
            format_id: read_u32_le(payload, 52)?,
            encoding_id: read_u32_le(payload, 56)?,
            flags: read_u32_le(payload, 60)?,
        })
    }

    fn format(self) -> String {
        format!(
            "resource_id_hash={} slot={} generation={} size_bytes={} timestamp_unix_ns={} width={} height={} stride_bytes={} format_id={} encoding_id={} flags={}",
            self.resource_id_hash,
            self.slot,
            self.generation,
            self.size_bytes,
            self.timestamp_unix_ns,
            self.width,
            self.height,
            self.stride_bytes,
            self.format_id,
            self.encoding_id,
            self.flags
        )
    }
}

pub(crate) fn read_u32_le(payload: &[u8], offset: usize) -> Result<u32> {
    let end = offset + 4;
    let bytes: [u8; 4] = payload[offset..end]
        .try_into()
        .context("failed to read u32 frame descriptor field")?;
    Ok(u32::from_le_bytes(bytes))
}

pub(crate) fn read_u64_le(payload: &[u8], offset: usize) -> Result<u64> {
    let end = offset + 8;
    let bytes: [u8; 8] = payload[offset..end]
        .try_into()
        .context("failed to read u64 frame descriptor field")?;
    Ok(u64::from_le_bytes(bytes))
}

pub(crate) fn format_echo_snapshot(
    channel: &EchoChannelSpec,
    snapshot: &flowrt::introspection::IntrospectionChannelSnapshot,
    options: EchoFormatOptions,
) -> Result<String> {
    let published_at_ms = snapshot
        .published_at_ms
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    let Some(payload) = &snapshot.payload else {
        return Ok(format!(
            "channel={} type={} {} published_count={} published_at_ms={} payload_len=0 no payload",
            channel.name,
            channel.message_type,
            echo_payload_shape_label(&channel.payload_shape),
            snapshot.published_count,
            published_at_ms
        ));
    };
    if payload.is_empty() {
        return Ok(format!(
            "channel={} type={} {} published_count={} published_at_ms={} payload_len=0 no payload",
            channel.name,
            channel.message_type,
            echo_payload_shape_label(&channel.payload_shape),
            snapshot.published_count,
            published_at_ms
        ));
    }
    match &channel.payload_shape {
        EchoPayloadShape::FixedAbi {
            size_bytes,
            messages,
            fields,
            descriptor,
        } => {
            if payload.len() != *size_bytes {
                anyhow::bail!(
                    "channel `{}` payload length {} does not match Message ABI size {} for `{}`",
                    channel.name,
                    payload.len(),
                    size_bytes,
                    channel.message_type
                );
            }
            if *descriptor {
                let descriptor = FrameDescriptorEcho::decode(payload)?;
                return Ok(format!(
                    "channel={} type={} {} published_count={} published_at_ms={} payload_len={} frame_descriptor={{{}}} raw={}",
                    channel.name,
                    channel.message_type,
                    echo_payload_shape_label(&channel.payload_shape),
                    snapshot.published_count,
                    published_at_ms,
                    payload.len(),
                    descriptor.format(),
                    hex_bytes(payload)
                ));
            }
            let fields = flowrt_selfdesc::format_fixed_abi_fields_with_message_abi(
                messages, fields, payload,
            )?;
            if !fields.is_empty() {
                return Ok(format!(
                    "channel={} type={} {} published_count={} published_at_ms={} payload_len={} fields={{{}}} raw={}",
                    channel.name,
                    channel.message_type,
                    echo_payload_shape_label(&channel.payload_shape),
                    snapshot.published_count,
                    published_at_ms,
                    payload.len(),
                    fields,
                    hex_bytes(payload)
                ));
            }
        }
        EchoPayloadShape::CanonicalFrame {
            header_size_bytes,
            max_size_bytes,
            messages,
            fields,
            ..
        } => {
            if let Some(max_size_bytes) = *max_size_bytes {
                if payload.len() > max_size_bytes {
                    anyhow::bail!(
                        "channel `{}` payload length {} exceeds canonical frame max size {} for `{}`",
                        channel.name,
                        payload.len(),
                        max_size_bytes,
                        channel.message_type
                    );
                }
            }
            let fields = flowrt_selfdesc::format_frame_fields_with_message_abi_and_options(
                messages,
                fields,
                *header_size_bytes,
                payload,
                flowrt_selfdesc::FrameFormatOptions { raw: options.raw },
            )?;
            if !fields.is_empty() {
                return Ok(format!(
                    "channel={} type={} {} published_count={} published_at_ms={} payload_len={} fields={{{}}} raw={}",
                    channel.name,
                    channel.message_type,
                    echo_payload_shape_label(&channel.payload_shape),
                    snapshot.published_count,
                    published_at_ms,
                    payload.len(),
                    fields,
                    hex_bytes(payload)
                ));
            }
        }
    }
    Ok(format!(
        "channel={} type={} {} published_count={} published_at_ms={} payload_len={} raw={}",
        channel.name,
        channel.message_type,
        echo_payload_shape_label(&channel.payload_shape),
        snapshot.published_count,
        published_at_ms,
        payload.len(),
        hex_bytes(payload)
    ))
}

pub(crate) fn echo_payload_shape_label(shape: &EchoPayloadShape) -> String {
    match shape {
        EchoPayloadShape::FixedAbi {
            size_bytes,
            descriptor,
            ..
        } => {
            if *descriptor {
                format!("abi_size={size_bytes} descriptor=frame")
            } else {
                format!("abi_size={size_bytes}")
            }
        }
        EchoPayloadShape::CanonicalFrame {
            max_size_bytes,
            variable,
            ..
        } => {
            let max_size = max_size_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unbounded".to_string());
            format!("frame_max_size={max_size} variable={variable}")
        }
    }
}

pub(crate) fn format_param_status(param: &flowrt::IntrospectionParamStatus) -> String {
    let pending = param
        .pending
        .as_ref()
        .map(json_inline)
        .unwrap_or_else(|| "none".to_string());
    let min = param
        .min
        .as_ref()
        .map(json_inline)
        .unwrap_or_else(|| "none".to_string());
    let max = param
        .max
        .as_ref()
        .map(json_inline)
        .unwrap_or_else(|| "none".to_string());
    let choices = if param.choices.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            param
                .choices
                .iter()
                .map(json_inline)
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let runtime_update = match param.update.as_str() {
        "startup" => "startup-only",
        "on_tick" => "pending-on-tick",
        _ => "unknown",
    };
    let apply_state = match param.update.as_str() {
        "startup" => "startup-only",
        _ if param.pending.is_some() => "pending",
        _ => "applied",
    };
    format!(
        "{} type={} update={} current={} pending={} apply_state={} min={} max={} choices={} runtime_update={}",
        param.name,
        param.ty,
        param.update,
        json_inline(&param.current),
        pending,
        apply_state,
        min,
        max,
        choices,
        runtime_update
    )
}

pub(crate) fn format_operation_status(
    operation: &flowrt::IntrospectionOperationStatus,
    socket: Option<&Path>,
) -> String {
    let current_ids = if operation.current_operation_ids.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", operation.current_operation_ids.join(","))
    };
    let last_transition = operation
        .last_transition_ms
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    let state = operation.current_state.as_deref().unwrap_or("idle");
    let owner = operation.current_owner.as_deref().unwrap_or("none");
    let deadline = operation
        .current_deadline_ms
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    let last_event = operation.last_event.as_deref().unwrap_or("none");
    let last_error = operation.last_error.as_deref().unwrap_or("none");
    let mut line = format!(
        "operation={} ready={} state={} owner={} deadline_ms={} running={} queued={} current_operation_ids={} total_started={} succeeded={} failed={} canceled={} timeout={} preempted={} last_event={} last_error={} last_transition_ms={}",
        operation.name,
        operation.ready,
        state,
        owner,
        deadline,
        operation.running,
        operation.queued,
        current_ids,
        operation.total_started,
        operation.succeeded_count,
        operation.failed_count,
        operation.canceled_count,
        operation.timeout_count,
        operation.preempted_count,
        last_event,
        last_error,
        last_transition
    );
    if let Some(socket) = socket {
        line.push_str(&format!(" socket={}", socket.display()));
    }
    line
}

pub(crate) fn format_diagnostic_status(
    diagnostic: &flowrt::IntrospectionDiagnostic,
    socket: &Path,
) -> String {
    let metrics = serde_json::to_string(&diagnostic.metrics).unwrap_or_else(|_| "[]".to_string());
    format!(
        "diagnostic={} category={} entity_kind={} state={} severity={} reason={} suggestion={} updated_unix_ms={} observed_ms={} metrics={} socket={}",
        diagnostic.entity_id,
        diagnostic.category,
        diagnostic.entity_kind,
        diagnostic.state,
        diagnostic.severity,
        option_str(diagnostic.reason.as_deref()),
        option_str(diagnostic.suggestion.as_deref()),
        option_u64(diagnostic.updated_unix_ms),
        option_u64(diagnostic.observed_ms),
        metrics,
        socket.display()
    )
}

pub(crate) fn operation_topology_summary(self_description: &SelfDescription) -> String {
    let mut lines = Vec::new();
    for graph in &self_description.graphs {
        for operation in &graph.operations {
            lines.push(format_operation_endpoint(operation));
        }
    }
    if lines.is_empty() {
        "no FlowRT operations".to_string()
    } else {
        lines.join("\n")
    }
}

pub(crate) fn format_operation_endpoint(operation: &SelfDescriptionOperationEndpoint) -> String {
    let mut line = format!(
        "operation={} canonical_id={} client={}.{} server={}.{} goal={} feedback={} result={} backend={}",
        operation.name,
        operation.canonical_id,
        operation.client_instance,
        operation.client_port,
        operation.server_instance,
        operation.server_port,
        operation.goal_type,
        operation.feedback_type,
        operation.result_type,
        operation.backend
    );
    if let Some(timeout_ms) = operation.timeout_ms {
        line.push_str(&format!(" timeout_ms={timeout_ms}"));
    }
    if let Some(queue_depth) = operation.queue_depth {
        line.push_str(&format!(" queue_depth={queue_depth}"));
    }
    if let Some(max_in_flight) = operation.max_in_flight {
        line.push_str(&format!(" max_in_flight={max_in_flight}"));
    }
    if !operation.concurrency.is_empty() {
        line.push_str(&format!(" concurrency={}", operation.concurrency));
    }
    if !operation.preempt.is_empty() {
        line.push_str(&format!(" preempt={}", operation.preempt));
    }
    if !operation.feedback.is_empty() {
        line.push_str(&format!(" feedback_policy={}", operation.feedback));
    }
    line
}

pub(crate) fn json_inline(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(crate) fn format_descriptor_schema(descriptor: &SelfDescriptionResourceDescriptor) -> String {
    let metadata = if descriptor.metadata.is_empty() {
        "none".to_string()
    } else {
        descriptor
            .metadata
            .iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        " descriptor_kind={} descriptor_port={} descriptor_format={} descriptor_encoding={} descriptor_record_payload={} descriptor_metadata=[{}]",
        empty_as_none(&descriptor.kind),
        empty_as_none(&descriptor.port),
        empty_as_none(&descriptor.format),
        empty_as_none(&descriptor.encoding),
        descriptor.record_payload,
        metadata
    )
}

pub(crate) fn option_u32(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

pub(crate) fn option_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

pub(crate) fn option_usize(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

pub(crate) fn option_i32(value: Option<i32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

pub(crate) fn option_bool(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

pub(crate) fn option_str(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or("none")
        .to_string()
}

pub(crate) fn empty_as_none(value: &str) -> String {
    if value.is_empty() {
        "none".to_string()
    } else {
        value.to_string()
    }
}

pub(crate) fn input_display_name(input: &flowrt::IntrospectionInputStatus) -> String {
    if input.task.is_empty() {
        input.input.clone()
    } else if input.input.is_empty() {
        input.task.clone()
    } else {
        format!("{}.{}", input.task, input.input)
    }
}

pub(crate) fn hz_status_or_stale(
    socket: &Path,
    response: flowrt::IntrospectionResponse,
    lines: &mut Vec<String>,
) -> Option<flowrt::IntrospectionResponse> {
    match response {
        status @ flowrt::IntrospectionResponse::Status { .. } => Some(status),
        flowrt::IntrospectionResponse::Error { message, .. } => {
            lines.push(format!("stale socket={} error={message}", socket.display()));
            None
        }
        _ => {
            lines.push(format!(
                "stale socket={} error=unexpected introspection response",
                socket.display()
            ));
            None
        }
    }
}

pub(crate) fn hz_summary_line_matches_channel(line: &str, channel: &str) -> bool {
    line.split_ascii_whitespace()
        .any(|field| field.strip_prefix("channel=") == Some(channel))
}

pub(crate) fn format_hz_summary_from_status_pair(
    first: &flowrt::IntrospectionResponse,
    second: &flowrt::IntrospectionResponse,
    elapsed: Duration,
) -> Result<String> {
    let flowrt::IntrospectionResponse::Status {
        handshake,
        status: first_status,
    } = first
    else {
        anyhow::bail!("first hz sample returned non-status response");
    };
    let flowrt::IntrospectionResponse::Status {
        status: second_status,
        ..
    } = second
    else {
        anyhow::bail!("second hz sample returned non-status response");
    };
    let elapsed_secs = elapsed.as_secs_f64();
    if elapsed_secs <= 0.0 {
        anyhow::bail!("hz sample window must be positive");
    }

    let first_channels = first_status
        .channels
        .iter()
        .map(|channel| (channel.name.as_str(), channel))
        .collect::<BTreeMap<_, _>>();
    let first_routes = first_status
        .routes
        .iter()
        .map(|route| (route.name.as_str(), route))
        .collect::<BTreeMap<_, _>>();
    let second_routes = second_status
        .routes
        .iter()
        .map(|route| (route.name.as_str(), route))
        .collect::<BTreeMap<_, _>>();
    let mut lines = Vec::new();
    let mut emitted_routes = std::collections::BTreeSet::new();
    for second_channel in &second_status.channels {
        let Some(first_channel) = first_channels.get(second_channel.name.as_str()) else {
            continue;
        };
        let delta = second_channel
            .published_count
            .saturating_sub(first_channel.published_count);
        let hz = delta as f64 / elapsed_secs;
        let mut line = format!(
            "pid={} package={} process={} channel={} type={} delta={} hz={:.2}",
            handshake.pid,
            handshake.package,
            handshake.process,
            second_channel.name,
            second_channel.message_type,
            delta,
            hz
        );
        if let Some(second_route) = second_routes.get(second_channel.name.as_str()) {
            let (dropped_delta, backpressure_delta, overflow_delta) = first_routes
                .get(second_channel.name.as_str())
                .map(|first_route| {
                    (
                        second_route
                            .dropped_samples
                            .saturating_sub(first_route.dropped_samples),
                        second_route
                            .backpressure_count
                            .saturating_sub(first_route.backpressure_count),
                        second_route
                            .overflow_count
                            .saturating_sub(first_route.overflow_count),
                    )
                })
                .unwrap_or((
                    second_route.dropped_samples,
                    second_route.backpressure_count,
                    second_route.overflow_count,
                ));
            line.push_str(&format!(
                " dropped_delta={} backpressure_delta={} overflow_delta={}",
                dropped_delta, backpressure_delta, overflow_delta
            ));
            emitted_routes.insert(second_channel.name.as_str());
        } else {
            let dropped_delta = second_channel
                .dropped_samples
                .saturating_sub(first_channel.dropped_samples);
            if dropped_delta != 0 {
                line.push_str(&format!(" dropped_delta={dropped_delta}"));
            }
        }
        lines.push(line);
    }
    for second_route in &second_status.routes {
        if emitted_routes.contains(second_route.name.as_str()) {
            continue;
        }
        let (published_delta, dropped_delta, backpressure_delta, overflow_delta) = first_routes
            .get(second_route.name.as_str())
            .map(|first_route| {
                (
                    second_route
                        .published_count
                        .saturating_sub(first_route.published_count),
                    second_route
                        .dropped_samples
                        .saturating_sub(first_route.dropped_samples),
                    second_route
                        .backpressure_count
                        .saturating_sub(first_route.backpressure_count),
                    second_route
                        .overflow_count
                        .saturating_sub(first_route.overflow_count),
                )
            })
            .unwrap_or((
                second_route.published_count,
                second_route.dropped_samples,
                second_route.backpressure_count,
                second_route.overflow_count,
            ));
        let hz = published_delta as f64 / elapsed_secs;
        lines.push(format!(
            "pid={} package={} process={} channel={} type={} delta={} hz={:.2} dropped_delta={} backpressure_delta={} overflow_delta={}",
            handshake.pid,
            handshake.package,
            handshake.process,
            second_route.name,
            second_route.message_type,
            published_delta,
            hz,
            dropped_delta,
            backpressure_delta,
            overflow_delta
        ));
    }

    if lines.is_empty() {
        Ok("no live FlowRT channels".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}
