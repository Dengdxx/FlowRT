use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use zenoh::Wait;

use flowrt_selfdesc::{
    SelfDescription, SelfDescriptionChannel, SelfDescriptionComponentType, SelfDescriptionFieldAbi,
    SelfDescriptionFrameField, SelfDescriptionInstance, SelfDescriptionMessageAbi,
    SelfDescriptionMessageFrame, SelfDescriptionOperationEndpoint, SelfDescriptionParam,
    SelfDescriptionResourceDescriptor, load_self_description as load_selfdesc,
    load_self_description_with_hash as load_selfdesc_with_hash,
};

pub(crate) use flowrt_selfdesc::self_description_hash;

pub(crate) const LOCAL_INTROSPECTION_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn discover_cli_runtime_sockets() -> Result<Vec<PathBuf>> {
    let sockets =
        flowrt::discover_runtime_sockets().context("failed to scan FlowRT runtime sockets")?;
    Ok(sockets
        .into_iter()
        .filter(|socket| !cleanup_stale_runtime_socket(socket))
        .collect())
}

fn cleanup_stale_runtime_socket(socket: &Path) -> bool {
    match flowrt::request_status_with_timeout(socket, LOCAL_INTROSPECTION_TIMEOUT) {
        Ok(_) => false,
        Err(_) if runtime_socket_is_discoverable(socket) => std::fs::remove_file(socket).is_ok(),
        Err(_) => false,
    }
}

fn remove_discoverable_stale_runtime_socket<T>(
    socket: &Path,
    action: &str,
    error: anyhow::Error,
) -> Result<T> {
    if runtime_socket_is_discoverable(socket) && std::fs::remove_file(socket).is_ok() {
        anyhow::bail!(
            "stale FlowRT runtime socket `{}` was removed after {action} failed: {error}",
            socket.display()
        );
    }
    Err(error).with_context(|| format!("failed to {action} via `{}`", socket.display()))
}

fn runtime_socket_is_discoverable(socket: &Path) -> bool {
    let Ok(socket) = socket.canonicalize().or_else(|_| {
        let parent = socket
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        parent.canonicalize().map(|parent| {
            socket
                .file_name()
                .map(|name| parent.join(name))
                .unwrap_or(parent)
        })
    }) else {
        return false;
    };
    let Ok(runtime_dir) = flowrt::runtime_socket_dir().canonicalize() else {
        return false;
    };
    socket.starts_with(runtime_dir) && socket.extension().is_some_and(|ext| ext == "sock")
}

pub(crate) fn load_self_description(path: &Path) -> Result<SelfDescription> {
    load_selfdesc(path).with_context(|| {
        format!(
            "failed to read FlowRT self-description from `{}`",
            path.display()
        )
    })
}

pub(crate) fn load_self_description_with_hash(path: &Path) -> Result<(SelfDescription, String)> {
    load_selfdesc_with_hash(path).with_context(|| {
        format!(
            "failed to read FlowRT self-description from `{}`",
            path.display()
        )
    })
}

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

#[derive(Debug, Clone)]
pub(crate) struct EchoChannelSpec {
    name: String,
    message_type: String,
    payload_shape: EchoPayloadShape,
}

#[derive(Debug, Clone)]
pub(crate) enum EchoPayloadShape {
    FixedAbi {
        size_bytes: usize,
        messages: Vec<SelfDescriptionMessageAbi>,
        fields: Vec<SelfDescriptionFieldAbi>,
        descriptor: bool,
    },
    CanonicalFrame {
        header_size_bytes: usize,
        max_size_bytes: Option<usize>,
        variable: bool,
        messages: Vec<SelfDescriptionMessageAbi>,
        fields: Vec<SelfDescriptionFrameField>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct EchoTarget {
    pub(crate) image: Option<PathBuf>,
    pub(crate) channel: String,
}

#[derive(Debug, Clone)]
pub(crate) struct EchoSelection {
    pub(crate) image: Option<PathBuf>,
    pub(crate) channels: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EchoFormatOptions {
    pub(crate) raw: bool,
}

impl EchoSelection {
    pub(crate) fn from_cli(
        target: String,
        channels: Vec<String>,
        image: Option<PathBuf>,
    ) -> Result<Self> {
        match (channels.as_slice(), image) {
            ([channel], None) if looks_like_self_description_path(&target) => Ok(Self {
                image: Some(PathBuf::from(target)),
                channels: vec![channel.clone()],
            }),
            ([channel], None) => Ok(Self {
                image: None,
                channels: vec![target, channel.clone()],
            }),
            ([], image) => Ok(Self {
                image,
                channels: vec![target],
            }),
            (_, Some(image)) => {
                let mut all_channels = Vec::with_capacity(channels.len() + 1);
                all_channels.push(target);
                all_channels.extend(channels);
                Ok(Self {
                    image: Some(image),
                    channels: all_channels,
                })
            }
            (_, None) => {
                let mut all_channels = Vec::with_capacity(channels.len() + 1);
                all_channels.push(target);
                all_channels.extend(channels);
                Ok(Self {
                    image: None,
                    channels: all_channels,
                })
            }
        }
    }

    pub(crate) fn to_single_target(&self) -> Result<EchoTarget> {
        let [channel] = self.channels.as_slice() else {
            anyhow::bail!(
                "expected exactly one echo channel, got {}",
                self.channels.len()
            );
        };
        Ok(EchoTarget {
            image: self.image.clone(),
            channel: channel.clone(),
        })
    }
}

fn looks_like_self_description_path(value: &str) -> bool {
    value.ends_with(".json") || value.contains('/') || value.contains('\\')
}

pub(crate) fn echo_channel(
    target: &EchoTarget,
    socket: Option<&Path>,
    options: EchoFormatOptions,
) -> Result<String> {
    let (self_description, self_description_hash, socket) = load_echo_context(target, socket)?;
    let channel_spec = find_echo_channel(&self_description, &target.channel)?;
    let _observe = open_echo_observer(&socket, &channel_spec, &self_description_hash)?;
    let snapshot = wait_for_echo_snapshot(
        &socket,
        &channel_spec,
        &self_description_hash,
        Duration::from_millis(1000),
        Duration::from_millis(50),
    )?;

    format_echo_snapshot(&channel_spec, &snapshot, options)
}

pub(crate) fn echo_channels(
    selection: &EchoSelection,
    socket: Option<&Path>,
    options: EchoFormatOptions,
) -> Result<String> {
    let (self_description, self_description_hash, socket) =
        load_echo_selection_context(selection, socket)?;
    let channel_specs = echo_channel_specs(&self_description, &selection.channels)?;
    let mut lines = Vec::with_capacity(channel_specs.len());
    for channel_spec in &channel_specs {
        let _observe = open_echo_observer(&socket, channel_spec, &self_description_hash)?;
        let snapshot = wait_for_echo_snapshot(
            &socket,
            channel_spec,
            &self_description_hash,
            Duration::from_millis(1000),
            Duration::from_millis(50),
        )?;
        lines.push(format!(
            "channel={} {}",
            channel_spec.name,
            format_echo_snapshot(channel_spec, &snapshot, options)?
        ));
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
pub(crate) fn echo_channel_from_image(
    image: &Path,
    channel: &str,
    socket: Option<&Path>,
) -> Result<String> {
    echo_channel_from_image_with_options(image, channel, socket, EchoFormatOptions::default())
}

#[cfg(test)]
pub(crate) fn echo_channel_from_image_with_options(
    image: &Path,
    channel: &str,
    socket: Option<&Path>,
    options: EchoFormatOptions,
) -> Result<String> {
    echo_channel(
        &EchoTarget {
            image: Some(image.to_path_buf()),
            channel: channel.to_string(),
        },
        socket,
        options,
    )
}

#[cfg(test)]
pub(crate) fn echo_channel_snapshot_from_image(
    image: &Path,
    channel: &str,
    socket: Option<&Path>,
) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    let channel_spec = find_echo_channel(&self_description, channel)?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let snapshot = request_echo_snapshot(&socket, &channel_spec, &self_description_hash)?;
    format_echo_snapshot(&channel_spec, &snapshot, EchoFormatOptions::default())
}

fn load_echo_context(
    target: &EchoTarget,
    socket: Option<&Path>,
) -> Result<(SelfDescription, String, PathBuf)> {
    match &target.image {
        Some(image) => {
            let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
            let socket = select_echo_socket(socket, &self_description_hash)?;
            Ok((self_description, self_description_hash, socket))
        }
        None => load_echo_context_from_live_socket(socket),
    }
}

fn load_echo_selection_context(
    selection: &EchoSelection,
    socket: Option<&Path>,
) -> Result<(SelfDescription, String, PathBuf)> {
    match &selection.image {
        Some(image) => {
            let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
            let socket = select_echo_socket(socket, &self_description_hash)?;
            Ok((self_description, self_description_hash, socket))
        }
        None => load_echo_context_from_live_socket(socket),
    }
}

pub(crate) fn load_echo_context_from_live_socket(
    socket: Option<&Path>,
) -> Result<(SelfDescription, String, PathBuf)> {
    let socket = select_live_self_description_socket(socket)?;
    let response =
        match flowrt::request_self_description_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(response) => response,
            Err(error) => {
                return remove_discoverable_stale_runtime_socket(
                    &socket,
                    "request FlowRT self-description",
                    error.into(),
                );
            }
        };
    let (handshake, json) = match response {
        flowrt::IntrospectionResponse::SelfDescription { handshake, json } => (handshake, json),
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to read FlowRT self-description from `{}`: {message}",
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    };
    let hash = self_description_hash(json.as_bytes());
    if handshake.self_description_hash != hash {
        anyhow::bail!(
            "runtime socket `{}` self-description hash `{}` does not match served self-description `{}`",
            socket.display(),
            handshake.self_description_hash,
            hash
        );
    }
    let self_description = serde_json::from_str(&json).with_context(|| {
        format!(
            "failed to parse FlowRT self-description served by `{}`",
            socket.display()
        )
    })?;
    Ok((self_description, hash, socket))
}

fn select_live_self_description_socket(socket: Option<&Path>) -> Result<PathBuf> {
    if let Some(socket) = socket {
        return Ok(socket.to_path_buf());
    }
    let mut matches = Vec::new();
    for socket in discover_cli_runtime_sockets()? {
        let Ok(flowrt::IntrospectionResponse::SelfDescription { .. }) =
            flowrt::request_self_description_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT)
        else {
            continue;
        };
        matches.push(socket);
    }
    match matches.len() {
        0 => anyhow::bail!(
            "no live FlowRT process exposes self-description; pass `--socket <path>` or `--image <path>`"
        ),
        1 => Ok(matches.remove(0)),
        _ => {
            let sockets = matches
                .iter()
                .map(|socket| socket.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "multiple live FlowRT processes expose self-description: {sockets}; pass `--socket <path>` to choose one"
            )
        }
    }
}

fn open_echo_observer(
    socket: &Path,
    channel_spec: &EchoChannelSpec,
    self_description_hash: &str,
) -> Result<Option<std::os::unix::net::UnixStream>> {
    let (stream, response) = flowrt::observe_channel_stream_with_timeout(
        socket,
        &channel_spec.name,
        LOCAL_INTROSPECTION_TIMEOUT,
    )
    .with_context(|| {
        format!(
            "failed to observe channel `{}` via `{}`",
            channel_spec.name,
            socket.display()
        )
    })?;
    match response {
        flowrt::IntrospectionResponse::ObserveReady { handshake, .. } => {
            ensure_handshake_hash(&handshake, self_description_hash, socket)?;
            Ok(Some(stream))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to observe FlowRT channel `{}` via `{}`: {message}",
                channel_spec.name,
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    }
}

pub(crate) fn echo_channel_follow(
    target: &EchoTarget,
    socket: Option<&Path>,
    interval: Duration,
    options: EchoFormatOptions,
    output: &mut dyn Write,
) -> Result<()> {
    echo_channel_follow_for_polls(target, socket, interval, options, usize::MAX, output)
}

pub(crate) fn echo_channel_follow_for_polls(
    target: &EchoTarget,
    socket: Option<&Path>,
    interval: Duration,
    options: EchoFormatOptions,
    max_polls: usize,
    output: &mut dyn Write,
) -> Result<()> {
    let (self_description, self_description_hash, socket) = load_echo_context(target, socket)?;
    let channel_spec = find_echo_channel(&self_description, &target.channel)?;
    let _observe = open_echo_observer(&socket, &channel_spec, &self_description_hash)?;
    let mut last_snapshot_key = None;

    for index in 0..max_polls {
        let snapshot = request_echo_snapshot(&socket, &channel_spec, &self_description_hash)?;
        let snapshot_key = EchoSnapshotKey::from(&snapshot);
        if last_snapshot_key.as_ref() != Some(&snapshot_key) {
            writeln!(
                output,
                "{}",
                format_echo_snapshot(&channel_spec, &snapshot, options)?
            )
            .context("failed to write echo output")?;
            output.flush().context("failed to flush echo output")?;
            last_snapshot_key = Some(snapshot_key);
        }
        if index + 1 < max_polls {
            std::thread::sleep(interval);
        }
    }

    Ok(())
}

pub(crate) fn echo_channels_follow(
    selection: &EchoSelection,
    socket: Option<&Path>,
    interval: Duration,
    options: EchoFormatOptions,
    output: &mut dyn Write,
) -> Result<()> {
    echo_channels_follow_for_polls(selection, socket, interval, options, usize::MAX, output)
}

pub(crate) fn echo_channels_follow_for_polls(
    selection: &EchoSelection,
    socket: Option<&Path>,
    interval: Duration,
    options: EchoFormatOptions,
    max_polls: usize,
    output: &mut dyn Write,
) -> Result<()> {
    let (self_description, self_description_hash, socket) =
        load_echo_selection_context(selection, socket)?;
    let channel_specs = echo_channel_specs(&self_description, &selection.channels)?;
    let _observes = channel_specs
        .iter()
        .map(|channel_spec| open_echo_observer(&socket, channel_spec, &self_description_hash))
        .collect::<Result<Vec<_>>>()?;
    let mut last_snapshot_keys = BTreeMap::<String, EchoSnapshotKey>::new();

    for index in 0..max_polls {
        for channel_spec in &channel_specs {
            let snapshot = request_echo_snapshot(&socket, channel_spec, &self_description_hash)?;
            let snapshot_key = EchoSnapshotKey::from(&snapshot);
            if last_snapshot_keys.get(&channel_spec.name) != Some(&snapshot_key) {
                writeln!(
                    output,
                    "channel={} {}",
                    channel_spec.name,
                    format_echo_snapshot(channel_spec, &snapshot, options)?
                )
                .context("failed to write echo output")?;
                output.flush().context("failed to flush echo output")?;
                last_snapshot_keys.insert(channel_spec.name.clone(), snapshot_key);
            }
        }
        if index + 1 < max_polls {
            std::thread::sleep(interval);
        }
    }

    Ok(())
}

pub(crate) fn select_echo_socket(
    socket: Option<&Path>,
    self_description_hash: &str,
) -> Result<PathBuf> {
    let socket = match socket {
        Some(socket) => {
            ensure_socket_matches_self_description_hash(socket, self_description_hash)?;
            socket.to_path_buf()
        }
        None => {
            let sockets = discover_cli_runtime_sockets()?;
            select_matching_runtime_socket(self_description_hash, sockets)?
        }
    };
    Ok(socket)
}

fn request_echo_snapshot(
    socket: &Path,
    channel_spec: &EchoChannelSpec,
    self_description_hash: &str,
) -> Result<flowrt::introspection::IntrospectionChannelSnapshot> {
    let response = flowrt::request_channel_snapshot_with_timeout(
        socket,
        &channel_spec.name,
        LOCAL_INTROSPECTION_TIMEOUT,
    )
    .with_context(|| {
        format!(
            "failed to request channel snapshot from `{}`",
            socket.display()
        )
    })?;

    let (handshake, snapshot) = match response {
        flowrt::IntrospectionResponse::ChannelSnapshot { handshake, channel } => {
            (handshake, channel)
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to read channel snapshot `{}` from `{}`: {message}",
                channel_spec.name,
                socket.display()
            );
        }
        flowrt::IntrospectionResponse::Status { .. } => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected status response",
                socket.display()
            );
        }
        flowrt::IntrospectionResponse::SelfDescription { .. }
        | flowrt::IntrospectionResponse::ObserveReady { .. }
        | flowrt::IntrospectionResponse::OperationValue { .. }
        | flowrt::IntrospectionResponse::BoundaryPublish { .. } => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected introspection response",
                socket.display()
            );
        }
        flowrt::IntrospectionResponse::RecorderValue { .. }
        | flowrt::IntrospectionResponse::RecorderEvents { .. } => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected recorder response",
                socket.display()
            );
        }
        flowrt::IntrospectionResponse::ParamList { .. }
        | flowrt::IntrospectionResponse::ParamValue { .. } => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected parameter response",
                socket.display()
            );
        }
    };

    if handshake.self_description_hash != self_description_hash {
        anyhow::bail!(
            "runtime socket `{}` self-description hash `{}` does not match static self-description `{}`",
            socket.display(),
            handshake.self_description_hash,
            self_description_hash
        );
    }

    Ok(snapshot)
}

fn wait_for_echo_snapshot(
    socket: &Path,
    channel_spec: &EchoChannelSpec,
    self_description_hash: &str,
    timeout: Duration,
    interval: Duration,
) -> Result<flowrt::introspection::IntrospectionChannelSnapshot> {
    let started = std::time::Instant::now();
    loop {
        let snapshot = request_echo_snapshot(socket, channel_spec, self_description_hash)?;
        if snapshot
            .payload
            .as_ref()
            .is_some_and(|payload| !payload.is_empty())
        {
            return Ok(snapshot);
        }
        if started.elapsed() >= timeout {
            return Ok(snapshot);
        }
        std::thread::sleep(interval);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EchoSnapshotKey {
    published_count: u64,
    published_at_ms: Option<u64>,
    payload: Option<Vec<u8>>,
}

impl From<&flowrt::introspection::IntrospectionChannelSnapshot> for EchoSnapshotKey {
    fn from(snapshot: &flowrt::introspection::IntrospectionChannelSnapshot) -> Self {
        Self {
            published_count: snapshot.published_count,
            published_at_ms: snapshot.published_at_ms,
            payload: snapshot.payload.clone(),
        }
    }
}

fn ensure_socket_matches_self_description_hash(
    socket: &Path,
    self_description_hash: &str,
) -> Result<()> {
    let response = match flowrt::request_status_with_timeout(socket, LOCAL_INTROSPECTION_TIMEOUT) {
        Ok(response) => response,
        Err(error) => {
            return remove_discoverable_stale_runtime_socket(
                socket,
                "request status",
                error.into(),
            );
        }
    };
    let flowrt::IntrospectionResponse::Status { handshake, .. } = response else {
        anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        );
    };
    if handshake.self_description_hash != self_description_hash {
        anyhow::bail!(
            "runtime socket `{}` self-description hash `{}` does not match static self-description `{}`",
            socket.display(),
            handshake.self_description_hash,
            self_description_hash
        );
    }
    Ok(())
}

pub(crate) fn find_echo_channel(
    self_description: &SelfDescription,
    channel_name: &str,
) -> Result<EchoChannelSpec> {
    let mut matches = Vec::new();
    for graph in &self_description.graphs {
        for channel in &graph.channels {
            let name = echo_channel_name(channel);
            if name == channel_name || channel.from == channel_name || channel.to == channel_name {
                let prefer_frame = channel.backend == "zenoh";
                matches.push(EchoChannelSpec {
                    name,
                    message_type: channel.message_type.clone(),
                    payload_shape: echo_payload_shape(
                        &self_description.message_abi,
                        &self_description.message_frames,
                        &channel.message_type,
                        prefer_frame,
                    )?,
                });
            }
        }
        for boundary in &graph.boundary_endpoints {
            if boundary.direction == "output"
                && (boundary.name == channel_name || boundary.endpoint == channel_name)
            {
                matches.push(EchoChannelSpec {
                    name: boundary.name.clone(),
                    message_type: boundary.message_type.clone(),
                    payload_shape: echo_payload_shape(
                        &self_description.message_abi,
                        &self_description.message_frames,
                        &boundary.message_type,
                        true,
                    )?,
                });
            }
        }
    }

    match matches.len() {
        0 => anyhow::bail!(
            "FlowRT self-description does not contain channel or boundary output `{channel_name}`"
        ),
        1 => Ok(matches.remove(0)),
        _ => anyhow::bail!(
            "FlowRT self-description contains multiple channels named `{channel_name}`"
        ),
    }
}

fn echo_channel_specs(
    self_description: &SelfDescription,
    channels: &[String],
) -> Result<Vec<EchoChannelSpec>> {
    if channels.is_empty() {
        anyhow::bail!("flowrt echo requires at least one channel");
    }
    channels
        .iter()
        .map(|channel| find_echo_channel(self_description, channel))
        .collect()
}

fn echo_channel_name(channel: &SelfDescriptionChannel) -> String {
    format!("{}_to_{}", channel.from, channel.to)
}

fn echo_payload_shape(
    messages: &[SelfDescriptionMessageAbi],
    frames: &[SelfDescriptionMessageFrame],
    message_type: &str,
    prefer_frame: bool,
) -> Result<EchoPayloadShape> {
    if prefer_frame
        && let Some(frame) = crate::frame_json::message_frame_layout(frames, message_type)?
    {
        return Ok(EchoPayloadShape::CanonicalFrame {
            header_size_bytes: frame.header_size_bytes,
            max_size_bytes: frame.max_size_bytes,
            variable: frame.variable,
            messages: messages.to_vec(),
            fields: frame.fields.clone(),
        });
    }
    if let Some(message) = message_abi_layout(messages, message_type)? {
        return Ok(EchoPayloadShape::FixedAbi {
            size_bytes: message.size_bytes,
            messages: messages.to_vec(),
            fields: message.fields.clone(),
            descriptor: is_standard_frame_descriptor_layout(message),
        });
    }
    if let Some(frame) = crate::frame_json::message_frame_layout(frames, message_type)? {
        return Ok(EchoPayloadShape::CanonicalFrame {
            header_size_bytes: frame.header_size_bytes,
            max_size_bytes: frame.max_size_bytes,
            variable: frame.variable,
            messages: messages.to_vec(),
            fields: frame.fields.clone(),
        });
    }
    anyhow::bail!(
        "FlowRT self-description does not contain Message ABI or frame layout for `{message_type}`"
    )
}

pub(crate) fn message_abi_layout<'a>(
    messages: &'a [SelfDescriptionMessageAbi],
    message_type: &str,
) -> Result<Option<&'a SelfDescriptionMessageAbi>> {
    let mut layouts = messages
        .iter()
        .filter(|message| message.type_name == message_type)
        .collect::<Vec<_>>();
    match layouts.len() {
        0 => Ok(None),
        1 => Ok(Some(layouts.remove(0))),
        _ => anyhow::bail!(
            "FlowRT self-description contains multiple Message ABI layouts for `{message_type}`"
        ),
    }
}

const FRAME_DESCRIPTOR_FIELDS: &[(&str, &str, usize, usize)] = &[
    ("resource_id_hash", "u64", 0, 8),
    ("slot", "u32", 8, 4),
    ("generation", "u64", 16, 8),
    ("size_bytes", "u64", 24, 8),
    ("timestamp_unix_ns", "u64", 32, 8),
    ("width", "u32", 40, 4),
    ("height", "u32", 44, 4),
    ("stride_bytes", "u32", 48, 4),
    ("format_id", "u32", 52, 4),
    ("encoding_id", "u32", 56, 4),
    ("flags", "u32", 60, 4),
];

fn is_standard_frame_descriptor_layout(message: &SelfDescriptionMessageAbi) -> bool {
    message.size_bytes == 64
        && message.fields.len() == FRAME_DESCRIPTOR_FIELDS.len()
        && FRAME_DESCRIPTOR_FIELDS
            .iter()
            .zip(message.fields.iter())
            .all(|((name, ty, offset, size), field)| {
                field.name == *name
                    && field.ty == *ty
                    && field.offset_bytes == *offset
                    && field.size_bytes == *size
            })
}

#[derive(Debug, Clone, Copy)]
struct FrameDescriptorEcho {
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

fn read_u32_le(payload: &[u8], offset: usize) -> Result<u32> {
    let end = offset + 4;
    let bytes: [u8; 4] = payload[offset..end]
        .try_into()
        .context("failed to read u32 frame descriptor field")?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64_le(payload: &[u8], offset: usize) -> Result<u64> {
    let end = offset + 8;
    let bytes: [u8; 8] = payload[offset..end]
        .try_into()
        .context("failed to read u64 frame descriptor field")?;
    Ok(u64::from_le_bytes(bytes))
}

pub(crate) fn select_matching_runtime_socket(
    self_description_hash: &str,
    sockets: Vec<PathBuf>,
) -> Result<PathBuf> {
    let mut matches = Vec::new();
    for socket in sockets {
        let Ok(flowrt::IntrospectionResponse::Status { handshake, .. }) =
            flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT)
        else {
            continue;
        };
        if handshake.self_description_hash == self_description_hash {
            matches.push(socket);
        }
    }

    match matches.len() {
        0 => anyhow::bail!(
            "no live FlowRT process matches self-description hash `{self_description_hash}`; pass `--socket <path>` if the process uses a non-discoverable socket"
        ),
        1 => Ok(matches.remove(0)),
        _ => {
            let sockets = matches
                .iter()
                .map(|socket| socket.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "multiple live FlowRT processes match self-description hash `{self_description_hash}`: {sockets}; pass `--socket <path>` to choose one"
            )
        }
    }
}

fn format_echo_snapshot(
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

fn echo_payload_shape_label(shape: &EchoPayloadShape) -> String {
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

pub(crate) fn params_list(image: &Path, socket: Option<&Path>) -> Result<String> {
    let (_self_description, self_description_hash) = load_self_description_with_hash(image)?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let response = flowrt::request_param_list_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT)
        .with_context(|| {
            format!(
                "failed to request parameter list from `{}`",
                socket.display()
            )
        })?;
    let params = match response {
        flowrt::IntrospectionResponse::ParamList { handshake, params } => {
            ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
            params
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to list FlowRT parameters from `{}`: {message}",
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    };
    if params.is_empty() {
        return Ok("no FlowRT parameters".to_string());
    }
    Ok(params
        .iter()
        .map(format_param_status)
        .collect::<Vec<_>>()
        .join("\n"))
}

pub(crate) fn params_get(image: &Path, name: &str, socket: Option<&Path>) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    ensure_param_declared(&self_description, name)?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let response =
        flowrt::request_param_get_with_timeout(&socket, name, LOCAL_INTROSPECTION_TIMEOUT)
            .with_context(|| {
                format!(
                    "failed to request parameter `{name}` from `{}`",
                    socket.display()
                )
            })?;
    let param = match response {
        flowrt::IntrospectionResponse::ParamValue { handshake, param } => {
            ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
            param
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to get FlowRT parameter `{name}` from `{}`: {message}",
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    };
    Ok(format_param_status(&param))
}

pub(crate) fn params_set(
    image: &Path,
    name: &str,
    raw_value: &str,
    socket: Option<&Path>,
) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    params_set_with_target(
        &self_description,
        &self_description_hash,
        &socket,
        name,
        raw_value,
    )
}

pub(crate) struct ParamSetBatchResult {
    pub(crate) output: String,
    pub(crate) has_errors: bool,
}

#[derive(Debug)]
struct ParamSetFileEntry {
    name: String,
    raw_value: String,
}

#[derive(Debug, serde::Deserialize)]
struct ParamSetFileArrayEntry {
    name: String,
    value: serde_json::Value,
}

pub(crate) fn params_set_from_file(
    image: &Path,
    file: &Path,
    socket: Option<&Path>,
) -> Result<ParamSetBatchResult> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let entries = load_param_set_file(file)?;
    params_set_batch(entries, |name, raw_value| {
        params_set_with_target(
            &self_description,
            &self_description_hash,
            &socket,
            name,
            raw_value,
        )
    })
}

fn params_set_with_target(
    self_description: &SelfDescription,
    self_description_hash: &str,
    socket: &Path,
    name: &str,
    raw_value: &str,
) -> Result<String> {
    ensure_param_declared(self_description, name)?;
    let value = serde_json::from_str::<serde_json::Value>(raw_value).with_context(|| {
        format!("FlowRT parameter values must be valid JSON; got `{raw_value}`")
    })?;
    let response =
        flowrt::request_param_set_with_timeout(socket, name, value, LOCAL_INTROSPECTION_TIMEOUT)
            .with_context(|| {
                format!(
                    "failed to set parameter `{name}` via `{}`",
                    socket.display()
                )
            })?;
    let param = match response {
        flowrt::IntrospectionResponse::ParamValue { handshake, param } => {
            ensure_handshake_hash(&handshake, self_description_hash, socket)?;
            param
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to set FlowRT parameter `{name}` via `{}`: {message}",
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    };
    Ok(format_param_status(&param))
}

fn load_param_set_file(file: &Path) -> Result<Vec<ParamSetFileEntry>> {
    let raw = fs::read_to_string(file)
        .with_context(|| format!("failed to read params file `{}`", file.display()))?;
    let value = serde_json::from_str::<serde_json::Value>(&raw)
        .with_context(|| format!("params file `{}` must be valid JSON", file.display()))?;
    parse_param_set_entries(file, value)
}

fn parse_param_set_entries(
    file: &Path,
    value: serde_json::Value,
) -> Result<Vec<ParamSetFileEntry>> {
    match value {
        serde_json::Value::Object(map) => Ok(map
            .into_iter()
            .map(|(name, value)| ParamSetFileEntry {
                name,
                raw_value: json_inline(&value),
            })
            .collect()),
        serde_json::Value::Array(items) => items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let entry: ParamSetFileArrayEntry =
                    serde_json::from_value(item).with_context(|| {
                        format!(
                            "params file `{}` entry {} must be an object with `name` and `value`",
                            file.display(),
                            index + 1
                        )
                    })?;
                Ok(ParamSetFileEntry {
                    name: entry.name,
                    raw_value: json_inline(&entry.value),
                })
            })
            .collect(),
        _ => anyhow::bail!(
            "params file `{}` must be a JSON object or array",
            file.display()
        ),
    }
}

fn params_set_batch<F>(entries: Vec<ParamSetFileEntry>, mut apply: F) -> Result<ParamSetBatchResult>
where
    F: FnMut(&str, &str) -> Result<String>,
{
    let mut lines = Vec::new();
    let mut ok_count = 0usize;
    let mut error_count = 0usize;

    for entry in entries {
        match apply(&entry.name, &entry.raw_value) {
            Ok(status) => {
                ok_count += 1;
                lines.push(format!("{}: ok: {}", entry.name, status));
            }
            Err(error) => {
                error_count += 1;
                lines.push(format!("{}: error: {}", entry.name, error));
            }
        }
    }

    lines.push(format!("summary: ok={ok_count} error={error_count}"));

    Ok(ParamSetBatchResult {
        output: lines.join("\n"),
        has_errors: error_count > 0,
    })
}

fn declared_param<'a>(
    self_description: &'a SelfDescription,
    name: &str,
) -> Option<(&'a SelfDescriptionInstance, &'a SelfDescriptionParam)> {
    for graph in &self_description.graphs {
        for instance in &graph.instances {
            for param in &instance.params {
                if format!("{}.{}", instance.name, param.name) == name {
                    return Some((instance, param));
                }
            }
        }
    }
    None
}

fn ensure_param_declared(self_description: &SelfDescription, name: &str) -> Result<()> {
    let Some((_instance, param)) = declared_param(self_description, name) else {
        anyhow::bail!("FlowRT self-description does not contain parameter `{name}`")
    };
    if param.ty.is_empty() || param.update.is_empty() {
        anyhow::bail!("FlowRT self-description parameter `{name}` has an incomplete schema")
    }
    Ok(())
}

pub(crate) fn ensure_handshake_hash(
    handshake: &flowrt::IntrospectionHandshake,
    self_description_hash: &str,
    socket: &Path,
) -> Result<()> {
    if handshake.self_description_hash == self_description_hash {
        Ok(())
    } else {
        anyhow::bail!(
            "runtime socket `{}` self-description hash `{}` does not match static self-description `{}`",
            socket.display(),
            handshake.self_description_hash,
            self_description_hash
        )
    }
}

fn format_param_status(param: &flowrt::IntrospectionParamStatus) -> String {
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

fn format_operation_status(
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

fn format_diagnostic_status(diagnostic: &flowrt::IntrospectionDiagnostic, socket: &Path) -> String {
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

pub(crate) fn operation_list(image: Option<&Path>, socket: Option<&Path>) -> Result<String> {
    let self_description = match image {
        Some(image) => load_self_description(image)?,
        None => {
            let (self_description, _hash, _socket) = load_echo_context_from_live_socket(socket)?;
            self_description
        }
    };
    Ok(operation_topology_summary(&self_description))
}

fn operation_topology_summary(self_description: &SelfDescription) -> String {
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

fn format_operation_endpoint(operation: &SelfDescriptionOperationEndpoint) -> String {
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

pub(crate) fn operation_status_summary(
    socket: Option<&Path>,
    name: Option<&str>,
) -> Result<String> {
    let sockets = match socket {
        Some(socket) => vec![socket.to_path_buf()],
        None => discover_cli_runtime_sockets()?,
    };
    operation_status_summary_for_sockets(name, sockets)
}

pub(crate) fn operation_status_summary_for_sockets(
    name: Option<&str>,
    sockets: Vec<PathBuf>,
) -> Result<String> {
    let mut lines = Vec::new();
    for socket in sockets {
        match flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(flowrt::IntrospectionResponse::Status { status, .. }) => {
                for operation in status.operations {
                    if name.is_none_or(|name| operation.name == name) {
                        lines.push(format_operation_status(&operation, Some(&socket)));
                    }
                }
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                lines.push(format!("stale socket={} error={message}", socket.display()));
            }
            Ok(_) => {
                lines.push(format!(
                    "stale socket={} error=unexpected introspection response",
                    socket.display()
                ));
            }
            Err(error) => {
                lines.push(format!("stale socket={} error={error}", socket.display()));
            }
        }
    }
    if lines.is_empty() {
        if let Some(name) = name {
            Ok(format!("no live FlowRT operation matches `{name}`"))
        } else {
            Ok("no live FlowRT operations".to_string())
        }
    } else {
        Ok(lines.join("\n"))
    }
}

pub(crate) fn operation_cancel(operation_id: &str, socket: Option<&Path>) -> Result<String> {
    if let Some(socket) = socket {
        return operation_cancel_on_socket(operation_id, socket);
    }
    let sockets = discover_cli_runtime_sockets()?;
    operation_cancel_for_sockets(operation_id, sockets)
}

pub(crate) fn operation_cancel_for_sockets(
    operation_id: &str,
    sockets: Vec<PathBuf>,
) -> Result<String> {
    let mut candidates = Vec::new();
    let mut errors = Vec::new();
    for socket in sockets {
        match flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(flowrt::IntrospectionResponse::Status { status, .. }) => {
                if status.operations.iter().any(|operation| {
                    operation
                        .current_operation_ids
                        .iter()
                        .any(|id| id == operation_id)
                }) {
                    candidates.push(socket);
                }
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                errors.push(format!("{}: {message}", socket.display()));
            }
            Ok(_) => {
                errors.push(format!(
                    "{}: unexpected introspection response",
                    socket.display()
                ));
            }
            Err(error) => {
                errors.push(format!("{}: {error}", socket.display()));
            }
        }
    }
    match candidates.len() {
        0 => {
            if errors.is_empty() {
                anyhow::bail!("no live FlowRT process reports operation `{operation_id}`")
            }
            anyhow::bail!(
                "no live FlowRT process reports operation `{}`; status errors: {}",
                operation_id,
                errors.join("; ")
            )
        }
        1 => {
            let socket = candidates.remove(0);
            operation_cancel_on_socket(operation_id, &socket)
        }
        _ => {
            let sockets = candidates
                .iter()
                .map(|socket| socket.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "multiple live FlowRT processes report operation `{operation_id}`: {sockets}; pass `--socket <path>` to choose one"
            )
        }
    }
}

fn operation_cancel_on_socket(operation_id: &str, socket: &Path) -> Result<String> {
    match flowrt::request_operation_cancel_with_timeout(
        socket,
        operation_id,
        LOCAL_INTROSPECTION_TIMEOUT,
    ) {
        Ok(flowrt::IntrospectionResponse::OperationValue { operation, .. }) => Ok(format!(
            "operation_id={} {}",
            operation_id,
            format_operation_status(&operation, Some(socket))
        )),
        Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
            anyhow::bail!(
                "failed to cancel FlowRT operation `{}` on `{}`: {}",
                operation_id,
                socket.display(),
                message
            )
        }
        Ok(_) => {
            anyhow::bail!(
                "failed to cancel FlowRT operation `{}` on `{}`: unexpected introspection response",
                operation_id,
                socket.display()
            )
        }
        Err(error) => {
            anyhow::bail!(
                "failed to cancel FlowRT operation `{}` on `{}`: {}",
                operation_id,
                socket.display(),
                error
            )
        }
    }
}

fn json_inline(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

pub(crate) fn live_status_summary(live_only: bool) -> Result<String> {
    let sockets = discover_cli_runtime_sockets()?;
    live_status_summary_for_sockets(sockets, live_only)
}

#[derive(Debug, Serialize)]
struct LiveStatusJsonEntry {
    socket: String,
    live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    handshake: Option<flowrt::IntrospectionHandshake>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<flowrt::IntrospectionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub(crate) fn live_status_json(live_only: bool) -> Result<String> {
    let sockets = discover_cli_runtime_sockets()?;
    live_status_json_for_sockets(sockets, live_only)
}

pub(crate) fn live_status_json_for_sockets(
    sockets: Vec<PathBuf>,
    live_only: bool,
) -> Result<String> {
    let mut entries = Vec::new();
    for socket in sockets {
        match flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(flowrt::IntrospectionResponse::Status { handshake, status }) => {
                entries.push(LiveStatusJsonEntry {
                    socket: socket.display().to_string(),
                    live: true,
                    handshake: Some(handshake),
                    status: Some(status),
                    error: None,
                });
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) if !live_only => {
                entries.push(LiveStatusJsonEntry {
                    socket: socket.display().to_string(),
                    live: false,
                    handshake: None,
                    status: None,
                    error: Some(message),
                });
            }
            Ok(_) if !live_only => {
                entries.push(LiveStatusJsonEntry {
                    socket: socket.display().to_string(),
                    live: false,
                    handshake: None,
                    status: None,
                    error: Some("unexpected introspection response".to_string()),
                });
            }
            Err(error) if !live_only => {
                entries.push(LiveStatusJsonEntry {
                    socket: socket.display().to_string(),
                    live: false,
                    handshake: None,
                    status: None,
                    error: Some(error.to_string()),
                });
            }
            _ => {}
        }
    }
    serde_json::to_string_pretty(&entries).context("序列化 live status JSON 失败")
}

pub(crate) fn live_status_summary_for_sockets(
    sockets: Vec<PathBuf>,
    live_only: bool,
) -> Result<String> {
    let mut lines = Vec::new();
    for socket in sockets {
        match flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(flowrt::IntrospectionResponse::Status { handshake, status }) => {
                // 尝试从同一 socket 获取 self-description，用于把 live status 和静态合同关联。
                let enrichment =
                    load_self_description_enrichment(&socket, &handshake.self_description_hash);
                let recorder = status.recorder.clone();

                let active_observers = status
                    .channels
                    .iter()
                    .map(|channel| channel.active_observers)
                    .sum::<u64>();
                let dropped_samples = status
                    .channels
                    .iter()
                    .map(|channel| channel.dropped_samples)
                    .sum::<u64>();
                let temporary_overlay = enrichment.artifact.temporary_overlay.is_some();
                lines.push(format!(
                    "pid={} package={} process={} runtime={} selfdesc={} ticks={} clock_source={} tick_time_ms={} clock_unit={} clock_field={} channels={} inputs={} routes={} observers={} dropped_samples={} artifact_mode={} temporary_island={} test_only={} temporary_overlay={} socket={}",
                    handshake.pid,
                    handshake.package,
                    handshake.process,
                    handshake.runtime,
                    handshake.self_description_hash,
                    status.tick_count,
                    status.clock.source,
                    option_u64(status.clock.tick_time_ms),
                    status.clock.unit,
                    status.clock.field,
                    status.channels.len(),
                    status.inputs.len(),
                    status.routes.len(),
                    active_observers,
                    dropped_samples,
                    enrichment.artifact.mode,
                    enrichment.artifact.temporary_island,
                    enrichment.artifact.test_only,
                    temporary_overlay,
                    socket.display()
                ));
                for graph in &enrichment.graphs {
                    lines.push(format!(
                        "graph={} mode={} boundary_endpoints={} socket={}",
                        graph.name,
                        graph.mode,
                        graph.boundary_endpoint_count,
                        socket.display()
                    ));
                }
                for boundary in &enrichment.boundary_endpoints {
                    lines.push(format!(
                        "boundary_endpoint={} direction={} endpoint={} type={} graph={} mode={} socket={}",
                        boundary.name,
                        boundary.direction,
                        boundary.endpoint,
                        boundary.message_type,
                        boundary.graph,
                        boundary.graph_mode,
                        socket.display()
                    ));
                }
                for channel in &status.channels {
                    lines.push(format!(
                        "channel={} type={} published_count={} last_payload_len={} observers={} dropped_samples={} socket={}",
                        channel.name,
                        channel.message_type,
                        channel.published_count,
                        option_usize(channel.last_payload_len),
                        channel.active_observers,
                        channel.dropped_samples,
                        socket.display()
                    ));
                }
                for route in &status.routes {
                    let thread_affinity = enrichment
                        .route_thread_affinity
                        .get(&route_affinity_key(&route.from, &route.to))
                        .map(String::as_str)
                        .unwrap_or("none");
                    lines.push(format!(
                        "route={} from={} to={} type={} backend={} thread_affinity={} selected_reason={} published_count={} dropped_samples={} backpressure={} overflow={} last_publish_ms={} last_error={} socket={}",
                        route.name,
                        route.from,
                        route.to,
                        route.message_type,
                        route.backend,
                        thread_affinity,
                        empty_as_none(&route.selected_reason),
                        route.published_count,
                        route.dropped_samples,
                        route.backpressure_count,
                        route.overflow_count,
                        option_u64(route.last_publish_ms),
                        option_str(route.last_error.as_deref()),
                        socket.display()
                    ));
                }
                for input in &status.inputs {
                    lines.push(format!(
                        "input={} task={} channel={} type={} present={} stale={} last_revision={} last_read_ms={} updated_unix_ms={} dropped_samples={} backpressure={} overflow={} socket={}",
                        input_display_name(input),
                        input.task,
                        input.channel,
                        input.message_type,
                        input.present,
                        input.stale,
                        option_u64(input.last_revision),
                        option_u64(input.last_read_ms),
                        option_u64(input.updated_unix_ms),
                        input.dropped_samples,
                        input.backpressure_count,
                        input.overflow_count,
                        socket.display()
                    ));
                }
                for param in &status.params {
                    lines.push(format!(
                        "param={} socket={}",
                        format_param_status(param),
                        socket.display()
                    ));
                }
                for process in status.processes {
                    let readiness_info = process
                        .readiness_wait
                        .as_deref()
                        .map(|wait| format!(" readiness_wait={wait}"))
                        .unwrap_or_default();
                    let resource_info = process
                        .resource_placement
                        .as_ref()
                        .and_then(|placement| serde_json::to_string(placement).ok())
                        .map(|placement| format!(" resource_placement={placement}"))
                        .unwrap_or_default();
                    lines.push(format!(
                        "supervisor_process={} state={} pid={} restarts={} ticks={} last_seen_ms={} tick_stale={} exit_code={}{}{} socket={}",
                        process.name,
                        process.state,
                        option_u32(process.pid),
                        process.restart_count,
                        option_u64(process.tick_count),
                        option_u64(process.last_seen_unix_ms),
                        process.tick_stale,
                        option_i32(process.exit_code),
                        readiness_info,
                        resource_info,
                        socket.display()
                    ));
                }
                for service in status.services {
                    let (client_inst, server_inst) = enrichment
                        .service_endpoints
                        .get(service.name.as_str())
                        .map(|ep| (ep.client_instance.as_str(), ep.server_instance.as_str()))
                        .unwrap_or(("", ""));
                    if client_inst.is_empty() {
                        lines.push(format!(
                            "service={} ready={} in_flight={} queued={} total_requests={} timeout={} busy={} unavailable={} late_drop={} socket={}",
                            service.name,
                            service.ready,
                            service.in_flight,
                            service.queued,
                            service.total_requests,
                            service.timeout_count,
                            service.busy_count,
                            service.unavailable_count,
                            service.late_drop_count,
                            socket.display()
                        ));
                    } else {
                        lines.push(format!(
                            "service={} client_instance={} server_instance={} ready={} in_flight={} queued={} total_requests={} timeout={} busy={} unavailable={} late_drop={} socket={}",
                            service.name,
                            client_inst,
                            server_inst,
                            service.ready,
                            service.in_flight,
                            service.queued,
                            service.total_requests,
                            service.timeout_count,
                            service.busy_count,
                            service.unavailable_count,
                            service.late_drop_count,
                            socket.display()
                        ));
                    }
                }
                for operation in status.operations {
                    lines.push(format_operation_status(&operation, Some(&socket)));
                }
                for resource in &status.resources {
                    lines.push(format!(
                        "resource={} capability={} access={} state={} required={} readiness={} health={} on_failure={} contract_status={} satisfied={} provider={} provider_scope={} provider_readiness_source={} provider_health_source={} source={} owner_process={} diagnostic={} suggestion={} last_error={} updated_unix_ms={} socket={}",
                        resource.name,
                        resource.capability,
                        option_str(resource.access.as_deref()),
                        resource.state,
                        resource.required,
                        option_str(resource.readiness.as_deref()),
                        option_str(resource.health.as_deref()),
                        option_str(resource.on_failure.as_deref()),
                        option_str(resource.contract_status.as_deref()),
                        option_bool(resource.satisfied),
                        option_str(resource.provider.as_deref()),
                        option_str(resource.provider_scope.as_deref()),
                        option_str(resource.provider_readiness_source.as_deref()),
                        option_str(resource.provider_health_source.as_deref()),
                        option_str(resource.source.as_deref()),
                        option_str(resource.owner_process.as_deref()),
                        option_str(resource.diagnostic.as_deref()),
                        option_str(resource.suggestion.as_deref()),
                        option_str(resource.last_error.as_deref()),
                        option_u64(resource.updated_unix_ms),
                        socket.display()
                    ));
                }
                for boundary in &status.io_boundaries {
                    lines.push(format!(
                        "io_boundary={} component={} ready={} healthy={} last_error={} updated_unix_ms={} socket={}",
                        boundary.name,
                        boundary.component,
                        boundary.ready,
                        boundary.healthy,
                        option_str(boundary.last_error.as_deref()),
                        option_u64(boundary.updated_unix_ms),
                        socket.display()
                    ));
                    for resource in &boundary.resources {
                        let descriptor_info = enrichment
                            .resource_descriptors
                            .get(&resource_descriptor_key(&boundary.name, &resource.name))
                            .map(format_descriptor_schema)
                            .unwrap_or_default();
                        lines.push(format!(
                            "io_boundary_resource={}.{} kind={} ready={} message={} last_error={} updated_unix_ms={}{} socket={}",
                            boundary.name,
                            resource.name,
                            resource.kind,
                            resource.ready,
                            option_str(resource.message.as_deref()),
                            option_str(resource.last_error.as_deref()),
                            option_u64(resource.updated_unix_ms),
                            descriptor_info,
                            socket.display()
                        ));
                    }
                }
                for task in &status.tasks {
                    let last_run = task
                        .last_run_ms
                        .map_or_else(|| "none".to_string(), |v| v.to_string());
                    let last_success = task
                        .last_success_ms
                        .map_or_else(|| "none".to_string(), |v| v.to_string());
                    let timing = if task.scheduled_time_ms.is_some()
                        || task.observed_time_ms.is_some()
                        || task.lateness_ms.is_some()
                        || task.missed_periods.is_some()
                        || task.overrun.is_some()
                    {
                        "runtime_observed"
                    } else {
                        "none"
                    };
                    lines.push(format!(
                        "task_health={} lane={} inflight={} scheduled_time_ms={} observed_time_ms={} lateness_ms={} missed_periods={} overrun={} timing={} deadline_missed={} stale_input={} backpressure={} overflow={} fairness_violations={} runs={} successes={} consecutive_failures={} last_run_ms={} last_success_ms={} socket={}",
                        task.name,
                        task.lane,
                        task.inflight,
                        option_u64(task.scheduled_time_ms),
                        option_u64(task.observed_time_ms),
                        option_u64(task.lateness_ms),
                        option_u64(task.missed_periods),
                        option_bool(task.overrun),
                        timing,
                        task.deadline_missed,
                        task.stale_input,
                        task.backpressure,
                        task.overflow,
                        task.fairness_violations,
                        task.run_count,
                        task.success_count,
                        task.consecutive_failures,
                        last_run,
                        last_success,
                        socket.display()
                    ));
                }
                for lane in &status.lanes {
                    lines.push(format!(
                        "lane_health={} queue_depth={} dispatched_count={} fairness_violations={} socket={}",
                        lane.name,
                        lane.queue_depth,
                        lane.dispatched_count,
                        lane.fairness_violations,
                        socket.display()
                    ));
                }
                for diagnostic in &status.diagnostics {
                    lines.push(format_diagnostic_status(diagnostic, &socket));
                }
                if recorder.enabled
                    || recorder.dropped_count != 0
                    || recorder.bytes_written != 0
                    || recorder.queued_events != 0
                {
                    let output = recorder.output.as_deref().unwrap_or("none");
                    lines.push(format!(
                        "recorder enabled={} output={} dropped_count={} bytes_written={} queued_events={} active_filters=[{}] socket={}",
                        recorder.enabled,
                        output,
                        recorder.dropped_count,
                        recorder.bytes_written,
                        recorder.queued_events,
                        recorder.active_filters.join(","),
                        socket.display()
                    ));
                }
            }
            Ok(flowrt::IntrospectionResponse::ChannelSnapshot { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected channel snapshot response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::SelfDescription { .. })
            | Ok(flowrt::IntrospectionResponse::ObserveReady { .. })
            | Ok(flowrt::IntrospectionResponse::OperationValue { .. })
            | Ok(flowrt::IntrospectionResponse::BoundaryPublish { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected introspection response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::ParamList { .. })
            | Ok(flowrt::IntrospectionResponse::ParamValue { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected parameter response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::RecorderValue { .. })
            | Ok(flowrt::IntrospectionResponse::RecorderEvents { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected recorder response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!("stale socket={} error={message}", socket.display()));
            }
            Err(error) => {
                if live_only {
                    continue;
                }
                lines.push(format!("stale socket={} error={error}", socket.display()));
            }
        }
    }
    if lines.is_empty() {
        Ok("no live FlowRT processes".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

/// service endpoint 关联信息（从 self-description 提取）。
struct ServiceEndpointAssoc {
    client_instance: String,
    server_instance: String,
}

/// graph 级静态模式摘要。
#[derive(Clone)]
struct GraphModeAssoc {
    name: String,
    mode: String,
    boundary_endpoint_count: usize,
}

/// island boundary endpoint 静态摘要。
#[derive(Clone)]
struct BoundaryEndpointAssoc {
    graph: String,
    graph_mode: String,
    name: String,
    direction: String,
    endpoint: String,
    message_type: String,
}

/// live status 输出的静态合同增强信息。
#[derive(Default)]
struct StatusEnrichment {
    artifact: flowrt_selfdesc::SelfDescriptionArtifact,
    graphs: Vec<GraphModeAssoc>,
    boundary_endpoints: Vec<BoundaryEndpointAssoc>,
    route_thread_affinity: BTreeMap<String, String>,
    service_endpoints: BTreeMap<String, ServiceEndpointAssoc>,
    resource_descriptors: BTreeMap<String, SelfDescriptionResourceDescriptor>,
}

/// 从 runtime socket 请求 self-description，构建 service/resource 关联映射。
///
/// 如果 self-description 请求失败（如 socket 不支持），返回空 map，不报错。
fn load_self_description_enrichment(socket: &Path, expected_hash: &str) -> StatusEnrichment {
    let Ok(response) =
        flowrt::request_self_description_with_timeout(socket, LOCAL_INTROSPECTION_TIMEOUT)
    else {
        return StatusEnrichment::default();
    };
    let flowrt::IntrospectionResponse::SelfDescription { handshake, json } = response else {
        return StatusEnrichment::default();
    };
    if handshake.self_description_hash != expected_hash
        || self_description_hash(json.as_bytes()) != expected_hash
    {
        return StatusEnrichment::default();
    }
    let Ok(sd) = serde_json::from_str::<SelfDescription>(&json) else {
        return StatusEnrichment::default();
    };
    let mut enrichment = StatusEnrichment {
        artifact: sd.artifact.clone(),
        ..StatusEnrichment::default()
    };
    for graph in &sd.graphs {
        enrichment.graphs.push(GraphModeAssoc {
            name: graph.name.clone(),
            mode: graph.mode.clone(),
            boundary_endpoint_count: graph.boundary_endpoints.len(),
        });
        for boundary in &graph.boundary_endpoints {
            enrichment.boundary_endpoints.push(BoundaryEndpointAssoc {
                graph: graph.name.clone(),
                graph_mode: graph.mode.clone(),
                name: boundary.name.clone(),
                direction: boundary.direction.clone(),
                endpoint: boundary.endpoint.clone(),
                message_type: boundary.message_type.clone(),
            });
        }
        for channel in &graph.channels {
            if channel.thread_affinity.is_empty() {
                continue;
            }
            enrichment.route_thread_affinity.insert(
                route_affinity_key(&channel.from, &channel.to),
                channel.thread_affinity.clone(),
            );
        }
        for ep in &graph.services {
            if !ep.client_instance.is_empty() && !ep.server_instance.is_empty() {
                enrichment.service_endpoints.insert(
                    ep.name.clone(),
                    ServiceEndpointAssoc {
                        client_instance: ep.client_instance.clone(),
                        server_instance: ep.server_instance.clone(),
                    },
                );
            }
        }
        let component_by_instance = graph
            .instances
            .iter()
            .map(|instance| (instance.name.as_str(), instance.component.as_str()))
            .collect::<BTreeMap<_, _>>();
        let component_types = sd
            .component_types
            .iter()
            .map(|component| (component.name.as_str(), component))
            .collect::<BTreeMap<_, _>>();
        for (instance, component_name) in component_by_instance {
            let Some(component) = component_types.get(component_name) else {
                continue;
            };
            for resource in &component.resources {
                let Some(descriptor) = &resource.descriptor else {
                    continue;
                };
                enrichment.resource_descriptors.insert(
                    resource_descriptor_key(instance, &resource.name),
                    descriptor.clone(),
                );
            }
        }
    }
    enrichment
}

fn route_affinity_key(from: &str, to: &str) -> String {
    format!("{from}->{to}")
}

fn resource_descriptor_key(boundary: &str, resource: &str) -> String {
    format!("{boundary}.{resource}")
}

fn format_descriptor_schema(descriptor: &SelfDescriptionResourceDescriptor) -> String {
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

fn option_u32(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn option_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn option_usize(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn option_i32(value: Option<i32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn option_bool(value: Option<bool>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn option_str(value: Option<&str>) -> String {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or("none")
        .to_string()
}

fn empty_as_none(value: &str) -> String {
    if value.is_empty() {
        "none".to_string()
    } else {
        value.to_string()
    }
}

fn input_display_name(input: &flowrt::IntrospectionInputStatus) -> String {
    if input.task.is_empty() {
        input.input.clone()
    } else if input.input.is_empty() {
        input.task.clone()
    } else {
        format!("{}.{}", input.task, input.input)
    }
}

pub(crate) fn live_hz_summary(
    channel: Option<&str>,
    socket: Option<&Path>,
    window_ms: u64,
) -> Result<String> {
    let sockets = match socket {
        Some(socket) => vec![socket.to_path_buf()],
        None => discover_cli_runtime_sockets()?,
    };
    live_hz_summary_for_sockets(channel, sockets, Duration::from_millis(window_ms))
}

pub(crate) fn live_hz_summary_for_sockets(
    channel: Option<&str>,
    sockets: Vec<PathBuf>,
    window: Duration,
) -> Result<String> {
    if sockets.is_empty() {
        return Ok("no live FlowRT processes".to_string());
    }

    let mut first = Vec::new();
    let mut lines = Vec::new();
    for socket in &sockets {
        match flowrt::request_status_with_timeout(socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(response) => {
                if let Some(status) = hz_status_or_stale(socket, response, &mut lines) {
                    first.push((socket.clone(), status));
                }
            }
            Err(error) => lines.push(format!("stale socket={} error={error}", socket.display())),
        }
    }
    if first.is_empty() {
        return Ok(lines.join("\n"));
    }
    let started = Instant::now();
    std::thread::sleep(window);
    let elapsed = started.elapsed();

    for (socket, first_status) in first {
        let second_status =
            match flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
                Ok(response) => {
                    let Some(status) = hz_status_or_stale(&socket, response, &mut lines) else {
                        continue;
                    };
                    status
                }
                Err(error) => {
                    lines.push(format!("stale socket={} error={error}", socket.display()));
                    continue;
                }
            };
        let summary = format_hz_summary_from_status_pair(&first_status, &second_status, elapsed)?;
        for line in summary.lines() {
            if channel.is_none_or(|selected| hz_summary_line_matches_channel(line, selected)) {
                lines.push(format!("{line} socket={}", socket.display()));
            }
        }
    }

    if lines.is_empty() {
        match channel {
            Some(channel) => Ok(format!("no live FlowRT channel matched `{channel}`")),
            None => Ok("no live FlowRT channels".to_string()),
        }
    } else {
        Ok(lines.join("\n"))
    }
}

fn hz_status_or_stale(
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

fn hz_summary_line_matches_channel(line: &str, channel: &str) -> bool {
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

/// 从镜像文件计算 self-description hash，用于远程 discovery 匹配。
pub(crate) fn self_description_hash_for_image(image: &Path) -> Result<String> {
    let (_self_description, hash) = load_self_description_with_hash(image)?;
    Ok(hash)
}

// ── 远程参数控制面（zenoh） ──────────────────────────────────────────────

/// zenoh 远程 runtime 发现结果。
#[derive(Debug)]
pub(crate) struct RemoteRuntimeEntry {
    pub(crate) key_expr: String,
    pub(crate) pid: u32,
    pub(crate) package: String,
    pub(crate) process: String,
    pub(crate) runtime: String,
    pub(crate) self_description_hash: String,
}

impl std::fmt::Display for RemoteRuntimeEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "pid={} package={} process={} runtime={} selfdesc={} key={}",
            self.pid,
            self.package,
            self.process,
            self.runtime,
            self.self_description_hash,
            self.key_expr
        )
    }
}

/// 解析 `flowrt/params/{package}/{selfdesc_hash}/{pid}` 格式的 key expression。
pub(crate) fn parse_remote_params_key_expr(key: &str) -> Option<(&str, &str, &str)> {
    let rest = key.strip_prefix("flowrt/params/")?;
    let (package, rest) = rest.split_once('/')?;
    let (hash, pid) = rest.split_once('/')?;
    if package.is_empty() || hash.is_empty() || pid.is_empty() || pid.contains('/') {
        return None;
    }
    Some((package, hash, pid))
}

/// 打开用于远程参数控制面的 zenoh session。
fn open_zenoh_params_session() -> Result<zenoh::Session> {
    let zenoh_config = flowrt::zenoh::config_from_environment().map_err(|error| {
        anyhow::anyhow!("failed to configure zenoh session for params discovery: {error}")
    })?;
    zenoh::open(zenoh_config).wait().map_err(|error| {
        anyhow::anyhow!("failed to open zenoh session for params discovery: {error:?}")
    })
}

/// 通过 zenoh 扫描所有远程 params 端点，返回匹配 `self_description_hash` 的 runtime。
///
/// 复用调用方提供的 session，避免每次 discovery 重复创建 zenoh 连接。
pub(crate) fn discover_remote_params_runtimes(
    session: &zenoh::Session,
    self_description_hash: &str,
    timeout_ms: u64,
) -> Result<Vec<RemoteRuntimeEntry>> {
    let request = flowrt::IntrospectionRequest::ParamList;
    let payload = serde_json::to_vec(&request)
        .map_err(|error| anyhow::anyhow!("failed to encode params discovery request: {error}"))?;
    let timeout = Duration::from_millis(timeout_ms);

    let receiver = session
        .get("flowrt/params/**")
        .with(zenoh::handlers::FifoChannel::new(64))
        .payload(zenoh::bytes::ZBytes::from(payload))
        .timeout(timeout)
        .wait()
        .map_err(|error| {
            anyhow::anyhow!("failed to send zenoh params discovery query: {error:?}")
        })?;

    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();

    while let Ok(Some(reply)) = receiver.recv_timeout(timeout) {
        let Ok(sample) = reply.result() else {
            continue;
        };
        let key = sample.key_expr().to_string();
        let Some((package, hash, pid_str)) = parse_remote_params_key_expr(&key) else {
            continue;
        };
        if hash != self_description_hash {
            continue;
        }
        if !seen.insert(key.clone()) {
            continue;
        }
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        // 克隆借用的字段，避免 move key 后 use-after-move。
        let entry_hash = hash.to_string();
        let entry_package_hint = package.to_string();
        let raw = sample.payload().to_bytes().to_vec();
        let Ok(response) = serde_json::from_slice::<flowrt::IntrospectionResponse>(&raw) else {
            continue;
        };
        let handshake = match &response {
            flowrt::IntrospectionResponse::ParamList { handshake, .. } => handshake,
            flowrt::IntrospectionResponse::Error { handshake, .. } => handshake,
            _ => continue,
        };
        let entry_package = if entry_package_hint.is_empty() {
            handshake.package.clone()
        } else {
            entry_package_hint
        };
        entries.push(RemoteRuntimeEntry {
            key_expr: key,
            pid,
            package: entry_package,
            process: handshake.process.clone(),
            runtime: handshake.runtime.clone(),
            self_description_hash: entry_hash,
        });
    }

    Ok(entries)
}

/// 从远程 runtime 列表中选择唯一匹配项；多个匹配时要求用户显式选择。
pub(crate) fn select_remote_runtime(
    entries: Vec<RemoteRuntimeEntry>,
    self_description_hash: &str,
) -> Result<RemoteRuntimeEntry> {
    match entries.len() {
        0 => anyhow::bail!(
            "no remote FlowRT runtime matches self-description hash `{self_description_hash}`; \
             check that the runtime is running and the zenoh network is reachable"
        ),
        1 => Ok(entries.into_iter().next().expect("non-empty")),
        _ => {
            let listing = entries
                .iter()
                .enumerate()
                .map(|(i, entry)| format!("  [{}] {}", i + 1, entry))
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::bail!(
                "multiple remote FlowRT runtimes match self-description hash \
                 `{self_description_hash}`; pass `--runtime <key_expr>` to choose one:\n{listing}"
            )
        }
    }
}

/// 请求远程 runtime 参数列表。
pub(crate) fn remote_params_list(
    self_description_hash: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_params_session()?;
    let runtime = select_remote_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_param_list(&session, &runtime.key_expr, timeout_ms)
        .map_err(|error| {
            anyhow::anyhow!("failed to list remote params from `{}`: {error}", runtime)
        })?;
    let params = match response {
        flowrt::IntrospectionResponse::ParamList { handshake, params } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            params
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("failed to list remote params from `{runtime}`: {message}");
        }
        _ => {
            anyhow::bail!("remote runtime `{runtime}` returned unexpected response");
        }
    };
    if params.is_empty() {
        return Ok("no FlowRT parameters".to_string());
    }
    Ok(params
        .iter()
        .map(format_param_status)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// 请求远程 runtime 单个参数状态。
pub(crate) fn remote_params_get(
    self_description_hash: &str,
    name: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_params_session()?;
    let runtime = select_remote_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_param_get(&session, &runtime.key_expr, name, timeout_ms)
        .map_err(|error| {
            anyhow::anyhow!("failed to get remote param `{name}` from `{runtime}`: {error}")
        })?;
    match response {
        flowrt::IntrospectionResponse::ParamValue { handshake, param } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            Ok(format_param_status(&param))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("failed to get remote param `{name}` from `{runtime}`: {message}");
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

/// 请求远程 runtime 设置参数 pending 值。
pub(crate) fn remote_params_set(
    self_description_hash: &str,
    name: &str,
    raw_value: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_params_session()?;
    let runtime = select_remote_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    remote_params_set_with_target(
        &session,
        self_description_hash,
        &runtime,
        name,
        raw_value,
        timeout_ms,
    )
}

pub(crate) fn remote_params_set_from_file(
    self_description_hash: &str,
    file: &Path,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<ParamSetBatchResult> {
    let entries = load_param_set_file(file)?;
    let session = open_zenoh_params_session()?;
    let runtime = select_remote_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    params_set_batch(entries, |name, raw_value| {
        remote_params_set_with_target(
            &session,
            self_description_hash,
            &runtime,
            name,
            raw_value,
            timeout_ms,
        )
    })
}

fn remote_params_set_with_target(
    session: &zenoh::Session,
    self_description_hash: &str,
    runtime: &RemoteRuntimeEntry,
    name: &str,
    raw_value: &str,
    timeout_ms: u64,
) -> Result<String> {
    let value = serde_json::from_str::<serde_json::Value>(raw_value).with_context(|| {
        format!("FlowRT parameter values must be valid JSON; got `{raw_value}`")
    })?;
    let response =
        flowrt::request_remote_param_set(session, &runtime.key_expr, name, value, timeout_ms)
            .map_err(|error| {
                anyhow::anyhow!("failed to set remote param `{name}` via `{runtime}`: {error}")
            })?;
    match response {
        flowrt::IntrospectionResponse::ParamValue { handshake, param } => {
            ensure_remote_handshake(&handshake, self_description_hash, runtime)?;
            eprintln!("target: {runtime}");
            Ok(format_param_status(&param))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("failed to set remote param `{name}` via `{runtime}`: {message}");
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

fn select_remote_runtime_for_request(
    session: &zenoh::Session,
    self_description_hash: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<RemoteRuntimeEntry> {
    if let Some(key_expr) = runtime_key_expr {
        return remote_runtime_entry_from_key_expr(
            session,
            key_expr,
            self_description_hash,
            timeout_ms,
        );
    }
    let entries = discover_remote_params_runtimes(session, self_description_hash, timeout_ms)?;
    select_remote_runtime(entries, self_description_hash)
}

fn remote_runtime_entry_from_key_expr(
    session: &zenoh::Session,
    key_expr: &str,
    self_description_hash: &str,
    timeout_ms: u64,
) -> Result<RemoteRuntimeEntry> {
    let Some((package, hash, pid_str)) = parse_remote_params_key_expr(key_expr) else {
        anyhow::bail!(
            "invalid remote FlowRT runtime key expression `{key_expr}`; expected `flowrt/params/<package>/<selfdesc_hash>/<pid>`"
        );
    };
    if hash != self_description_hash {
        anyhow::bail!(
            "remote FlowRT runtime key expression `{key_expr}` uses self-description hash `{hash}`, expected `{self_description_hash}`"
        );
    }
    let pid = pid_str.parse::<u32>().with_context(|| {
        format!(
            "remote FlowRT runtime key expression `{key_expr}` contains invalid pid `{pid_str}`"
        )
    })?;
    let response = flowrt::request_remote_param_list(session, key_expr, timeout_ms)
        .map_err(|error| anyhow::anyhow!("failed to query remote runtime `{key_expr}`: {error}"))?;
    let handshake = match response {
        flowrt::IntrospectionResponse::ParamList { handshake, .. }
        | flowrt::IntrospectionResponse::Error { handshake, .. } => handshake,
        _ => anyhow::bail!("remote runtime `{key_expr}` returned unexpected response"),
    };
    if handshake.self_description_hash != self_description_hash {
        anyhow::bail!(
            "remote runtime `{key_expr}` self-description hash `{}` does not match expected `{self_description_hash}`",
            handshake.self_description_hash
        );
    }
    Ok(RemoteRuntimeEntry {
        key_expr: key_expr.to_string(),
        pid,
        package: package.to_string(),
        process: handshake.process,
        runtime: handshake.runtime,
        self_description_hash: hash.to_string(),
    })
}

fn ensure_remote_handshake(
    handshake: &flowrt::IntrospectionHandshake,
    expected_hash: &str,
    runtime: &RemoteRuntimeEntry,
) -> Result<()> {
    if handshake.self_description_hash == expected_hash {
        Ok(())
    } else {
        anyhow::bail!(
            "remote runtime `{runtime}` self-description hash `{}` does not match expected `{expected_hash}`",
            handshake.self_description_hash
        )
    }
}
