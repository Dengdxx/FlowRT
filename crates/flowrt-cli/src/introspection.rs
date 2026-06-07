use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use zenoh::Wait;

use flowrt_selfdesc::{
    SelfDescription, SelfDescriptionChannel, SelfDescriptionComponentType, SelfDescriptionFieldAbi,
    SelfDescriptionFrameField, SelfDescriptionInstance, SelfDescriptionMessageAbi,
    SelfDescriptionMessageFrame, SelfDescriptionParam, load_self_description as load_selfdesc,
    load_self_description_with_hash as load_selfdesc_with_hash,
};

pub(crate) use flowrt_selfdesc::self_description_hash;

pub(crate) fn load_self_description(path: &Path) -> Result<SelfDescription> {
    load_selfdesc(path).with_context(|| {
        format!(
            "failed to read FlowRT self-description from `{}`",
            path.display()
        )
    })
}

fn load_self_description_with_hash(path: &Path) -> Result<(SelfDescription, String)> {
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
    let mut output = format!(
        "package={} selfdesc={} source_hash={} graphs={} component_types={} instances={} tasks={} channels={} services={} messages={}",
        self_description.package.name,
        self_description.self_description_version,
        self_description.source_hash,
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
        total_services,
        self_description.message_abi.len()
    );

    // 按名称索引 component types。
    let component_types: BTreeMap<&str, &SelfDescriptionComponentType> = self_description
        .component_types
        .iter()
        .map(|ct| (ct.name.as_str(), ct))
        .collect();

    for graph in &self_description.graphs {
        output.push_str(&format!("\ngraph {}", graph.name));

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
        fields: Vec<SelfDescriptionFieldAbi>,
    },
    CanonicalFrame {
        header_size_bytes: usize,
        max_size_bytes: Option<usize>,
        variable: bool,
        fields: Vec<SelfDescriptionFrameField>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct EchoTarget {
    pub(crate) image: Option<PathBuf>,
    pub(crate) channel: String,
}

impl EchoTarget {
    pub(crate) fn from_cli(
        target: String,
        channel: Option<String>,
        image: Option<PathBuf>,
    ) -> Result<Self> {
        match (channel, image) {
            (Some(channel), None) => Ok(Self {
                image: Some(PathBuf::from(target)),
                channel,
            }),
            (Some(_), Some(_)) => anyhow::bail!(
                "flowrt echo accepts either `<image> <channel>` or `--image <path> <channel>`, not both"
            ),
            (None, image) => Ok(Self {
                image,
                channel: target,
            }),
        }
    }
}

pub(crate) fn echo_channel(target: &EchoTarget, socket: Option<&Path>) -> Result<String> {
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

    format_echo_snapshot(&channel_spec, &snapshot)
}

#[cfg(test)]
pub(crate) fn echo_channel_from_image(
    image: &Path,
    channel: &str,
    socket: Option<&Path>,
) -> Result<String> {
    echo_channel(
        &EchoTarget {
            image: Some(image.to_path_buf()),
            channel: channel.to_string(),
        },
        socket,
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
    format_echo_snapshot(&channel_spec, &snapshot)
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

fn load_echo_context_from_live_socket(
    socket: Option<&Path>,
) -> Result<(SelfDescription, String, PathBuf)> {
    let socket = select_live_self_description_socket(socket)?;
    let response = flowrt::request_self_description(&socket).with_context(|| {
        format!(
            "failed to request FlowRT self-description from `{}`",
            socket.display()
        )
    })?;
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
    for socket in
        flowrt::discover_runtime_sockets().context("failed to scan FlowRT runtime sockets")?
    {
        let Ok(flowrt::IntrospectionResponse::SelfDescription { .. }) =
            flowrt::request_self_description(&socket)
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
    let (stream, response) = flowrt::observe_channel_stream(socket, &channel_spec.name)
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
    output: &mut dyn Write,
) -> Result<()> {
    echo_channel_follow_for_polls(target, socket, interval, usize::MAX, output)
}

pub(crate) fn echo_channel_follow_for_polls(
    target: &EchoTarget,
    socket: Option<&Path>,
    interval: Duration,
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
                format_echo_snapshot(&channel_spec, &snapshot)?
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

fn select_echo_socket(socket: Option<&Path>, self_description_hash: &str) -> Result<PathBuf> {
    let socket = match socket {
        Some(socket) => {
            ensure_socket_matches_self_description_hash(socket, self_description_hash)?;
            socket.to_path_buf()
        }
        None => {
            let sockets = flowrt::discover_runtime_sockets()
                .context("failed to scan FlowRT runtime sockets")?;
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
    let response =
        flowrt::request_channel_snapshot(socket, &channel_spec.name).with_context(|| {
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
        | flowrt::IntrospectionResponse::ObserveReady { .. } => {
            anyhow::bail!(
                "runtime socket `{}` returned an unexpected introspection response",
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
    let response = flowrt::request_status(socket)
        .with_context(|| format!("failed to request status from `{}`", socket.display()))?;
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
                matches.push(EchoChannelSpec {
                    name,
                    message_type: channel.message_type.clone(),
                    payload_shape: echo_payload_shape(
                        &self_description.message_abi,
                        &self_description.message_frames,
                        &channel.message_type,
                    )?,
                });
            }
        }
    }

    match matches.len() {
        0 => anyhow::bail!("FlowRT self-description does not contain channel `{channel_name}`"),
        1 => Ok(matches.remove(0)),
        _ => anyhow::bail!(
            "FlowRT self-description contains multiple channels named `{channel_name}`"
        ),
    }
}

fn echo_channel_name(channel: &SelfDescriptionChannel) -> String {
    format!("{}_to_{}", channel.from, channel.to)
}

fn echo_payload_shape(
    messages: &[SelfDescriptionMessageAbi],
    frames: &[SelfDescriptionMessageFrame],
    message_type: &str,
) -> Result<EchoPayloadShape> {
    if let Some(message) = message_abi_layout(messages, message_type)? {
        return Ok(EchoPayloadShape::FixedAbi {
            size_bytes: message.size_bytes,
            fields: message.fields.clone(),
        });
    }
    let mut frame_matches = frames
        .iter()
        .filter(|message| message.type_name == message_type)
        .collect::<Vec<_>>();
    match frame_matches.len() {
        0 => anyhow::bail!(
            "FlowRT self-description does not contain Message ABI or frame layout for `{message_type}`"
        ),
        1 => {
            let frame = frame_matches.remove(0);
            Ok(EchoPayloadShape::CanonicalFrame {
                header_size_bytes: frame.header_size_bytes,
                max_size_bytes: frame.max_size_bytes,
                variable: frame.variable,
                fields: frame.fields.clone(),
            })
        }
        _ => anyhow::bail!(
            "FlowRT self-description contains multiple Message frame layouts for `{message_type}`"
        ),
    }
}

fn message_abi_layout<'a>(
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

pub(crate) fn select_matching_runtime_socket(
    self_description_hash: &str,
    sockets: Vec<PathBuf>,
) -> Result<PathBuf> {
    let mut matches = Vec::new();
    for socket in sockets {
        let Ok(flowrt::IntrospectionResponse::Status { handshake, .. }) =
            flowrt::request_status(&socket)
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
        EchoPayloadShape::FixedAbi { size_bytes, fields } => {
            if payload.len() != *size_bytes {
                anyhow::bail!(
                    "channel `{}` payload length {} does not match Message ABI size {} for `{}`",
                    channel.name,
                    payload.len(),
                    size_bytes,
                    channel.message_type
                );
            }
            let fields = flowrt_selfdesc::format_fixed_abi_fields(fields, payload)?;
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
            let fields = flowrt_selfdesc::format_frame_fields(fields, *header_size_bytes, payload)?;
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
        EchoPayloadShape::FixedAbi { size_bytes, .. } => format!("abi_size={size_bytes}"),
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
    let response = flowrt::request_param_list(&socket).with_context(|| {
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
    let response = flowrt::request_param_get(&socket, name).with_context(|| {
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
    ensure_param_declared(&self_description, name)?;
    let value = serde_json::from_str::<serde_json::Value>(raw_value).with_context(|| {
        format!("FlowRT parameter values must be valid JSON; got `{raw_value}`")
    })?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    let response = flowrt::request_param_set(&socket, name, value).with_context(|| {
        format!(
            "failed to set parameter `{name}` via `{}`",
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

fn ensure_handshake_hash(
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
    format!(
        "{} type={} update={} current={} pending={} min={} max={} choices={}",
        param.name,
        param.ty,
        param.update,
        json_inline(&param.current),
        pending,
        min,
        max,
        choices
    )
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

pub(crate) fn live_status_summary() -> Result<String> {
    let sockets =
        flowrt::discover_runtime_sockets().context("failed to scan FlowRT runtime sockets")?;
    live_status_summary_for_sockets(sockets)
}

pub(crate) fn live_status_summary_for_sockets(sockets: Vec<PathBuf>) -> Result<String> {
    let mut lines = Vec::new();
    for socket in sockets {
        match flowrt::request_status(&socket) {
            Ok(flowrt::IntrospectionResponse::Status { handshake, status }) => {
                // 尝试从同一 socket 获取 self-description，用于关联 service → instance。
                let service_endpoints =
                    load_service_endpoint_map(&socket, &handshake.self_description_hash);

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
                lines.push(format!(
                    "pid={} package={} process={} runtime={} selfdesc={} ticks={} channels={} observers={} dropped_samples={} socket={}",
                    handshake.pid,
                    handshake.package,
                    handshake.process,
                    handshake.runtime,
                    handshake.self_description_hash,
                    status.tick_count,
                    status.channels.len(),
                    active_observers,
                    dropped_samples,
                    socket.display()
                ));
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
                    let (client_inst, server_inst) = service_endpoints
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
                for task in &status.tasks {
                    let last_run = task
                        .last_run_ms
                        .map_or_else(|| "none".to_string(), |v| v.to_string());
                    let last_success = task
                        .last_success_ms
                        .map_or_else(|| "none".to_string(), |v| v.to_string());
                    lines.push(format!(
                        "task_health={} lane={} deadline_missed={} stale_input={} backpressure={} overflow={} fairness_violations={} runs={} successes={} consecutive_failures={} last_run_ms={} last_success_ms={} socket={}",
                        task.name,
                        task.lane,
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
            }
            Ok(flowrt::IntrospectionResponse::ChannelSnapshot { .. }) => {
                lines.push(format!(
                    "stale socket={} error=unexpected channel snapshot response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::SelfDescription { .. })
            | Ok(flowrt::IntrospectionResponse::ObserveReady { .. }) => {
                lines.push(format!(
                    "stale socket={} error=unexpected introspection response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::ParamList { .. })
            | Ok(flowrt::IntrospectionResponse::ParamValue { .. }) => {
                lines.push(format!(
                    "stale socket={} error=unexpected parameter response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                lines.push(format!("stale socket={} error={message}", socket.display()));
            }
            Err(error) => {
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

/// 从 runtime socket 请求 self-description，构建 service name → instance 关联映射。
///
/// 如果 self-description 请求失败（如 socket 不支持），返回空 map，不报错。
fn load_service_endpoint_map(
    socket: &Path,
    expected_hash: &str,
) -> BTreeMap<String, ServiceEndpointAssoc> {
    let Ok(response) = flowrt::request_self_description(socket) else {
        return BTreeMap::new();
    };
    let flowrt::IntrospectionResponse::SelfDescription { handshake, json } = response else {
        return BTreeMap::new();
    };
    if handshake.self_description_hash != expected_hash
        || self_description_hash(json.as_bytes()) != expected_hash
    {
        return BTreeMap::new();
    }
    let Ok(sd) = serde_json::from_str::<SelfDescription>(&json) else {
        return BTreeMap::new();
    };
    let mut map = BTreeMap::new();
    for graph in &sd.graphs {
        for ep in &graph.services {
            if !ep.client_instance.is_empty() && !ep.server_instance.is_empty() {
                map.insert(
                    ep.name.clone(),
                    ServiceEndpointAssoc {
                        client_instance: ep.client_instance.clone(),
                        server_instance: ep.server_instance.clone(),
                    },
                );
            }
        }
    }
    map
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

fn option_i32(value: Option<i32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string())
}

pub(crate) fn live_hz_summary(
    channel: Option<&str>,
    socket: Option<&Path>,
    window_ms: u64,
) -> Result<String> {
    let sockets = match socket {
        Some(socket) => vec![socket.to_path_buf()],
        None => {
            flowrt::discover_runtime_sockets().context("failed to scan FlowRT runtime sockets")?
        }
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
        match flowrt::request_status(socket) {
            Ok(response) => first.push((socket.clone(), response)),
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
        let second_status = match flowrt::request_status(&socket) {
            Ok(response) => response,
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
    let mut lines = Vec::new();
    for second_channel in &second_status.channels {
        let Some(first_channel) = first_channels.get(second_channel.name.as_str()) else {
            continue;
        };
        let delta = second_channel
            .published_count
            .saturating_sub(first_channel.published_count);
        let hz = delta as f64 / elapsed_secs;
        lines.push(format!(
            "pid={} package={} process={} channel={} type={} delta={} hz={:.2}",
            handshake.pid,
            handshake.package,
            handshake.process,
            second_channel.name,
            second_channel.message_type,
            delta,
            hz
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
    if package.is_empty() || hash.is_empty() || pid.is_empty() {
        return None;
    }
    Some((package, hash, pid))
}

/// 通过 zenoh 扫描所有远程 params 端点，返回匹配 `self_description_hash` 的 runtime。
pub(crate) fn discover_remote_params_runtimes(
    self_description_hash: &str,
    timeout_ms: u64,
) -> Result<Vec<RemoteRuntimeEntry>> {
    let zenoh_config = flowrt::zenoh::config_from_environment().map_err(|error| {
        anyhow::anyhow!("failed to configure zenoh session for params discovery: {error}")
    })?;
    let session = zenoh::open(zenoh_config).wait().map_err(|error| {
        anyhow::anyhow!("failed to open zenoh session for params discovery: {error:?}")
    })?;

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
                 `{self_description_hash}`; pass `--socket <key_expr>` to choose one:\n{listing}"
            )
        }
    }
}

/// 请求远程 runtime 参数列表。
pub(crate) fn remote_params_list(self_description_hash: &str, timeout_ms: u64) -> Result<String> {
    let entries = discover_remote_params_runtimes(self_description_hash, timeout_ms)?;
    let runtime = select_remote_runtime(entries, self_description_hash)?;
    let zenoh_config = flowrt::zenoh::config_from_environment()
        .map_err(|error| anyhow::anyhow!("failed to configure zenoh session: {error}"))?;
    let session = zenoh::open(zenoh_config)
        .wait()
        .map_err(|error| anyhow::anyhow!("failed to open zenoh session: {error:?}"))?;
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
    timeout_ms: u64,
) -> Result<String> {
    let entries = discover_remote_params_runtimes(self_description_hash, timeout_ms)?;
    let runtime = select_remote_runtime(entries, self_description_hash)?;
    let zenoh_config = flowrt::zenoh::config_from_environment()
        .map_err(|error| anyhow::anyhow!("failed to configure zenoh session: {error}"))?;
    let session = zenoh::open(zenoh_config)
        .wait()
        .map_err(|error| anyhow::anyhow!("failed to open zenoh session: {error:?}"))?;
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
    timeout_ms: u64,
) -> Result<String> {
    let value = serde_json::from_str::<serde_json::Value>(raw_value).with_context(|| {
        format!("FlowRT parameter values must be valid JSON; got `{raw_value}`")
    })?;
    let entries = discover_remote_params_runtimes(self_description_hash, timeout_ms)?;
    let runtime = select_remote_runtime(entries, self_description_hash)?;
    let zenoh_config = flowrt::zenoh::config_from_environment()
        .map_err(|error| anyhow::anyhow!("failed to configure zenoh session: {error}"))?;
    let session = zenoh::open(zenoh_config)
        .wait()
        .map_err(|error| anyhow::anyhow!("failed to open zenoh session: {error:?}"))?;
    let response =
        flowrt::request_remote_param_set(&session, &runtime.key_expr, name, value, timeout_ms)
            .map_err(|error| {
                anyhow::anyhow!("failed to set remote param `{name}` via `{runtime}`: {error}")
            })?;
    match response {
        flowrt::IntrospectionResponse::ParamValue { handshake, param } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            Ok(format_param_status(&param))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("failed to set remote param `{name}` via `{runtime}`: {message}");
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
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
