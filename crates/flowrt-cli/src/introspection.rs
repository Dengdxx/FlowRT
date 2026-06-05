use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use object::{Object, ObjectSection};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const SELF_DESCRIPTION_SECTION: &str = ".flowrt.selfdesc";

#[derive(Debug, Deserialize)]
pub(crate) struct SelfDescription {
    pub(crate) self_description_version: String,
    pub(crate) source_hash: String,
    pub(crate) package: SelfDescriptionPackage,
    pub(crate) graphs: Vec<SelfDescriptionGraph>,
    pub(crate) message_abi: Vec<SelfDescriptionMessageAbi>,
    #[serde(default)]
    pub(crate) message_frames: Vec<SelfDescriptionMessageFrame>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelfDescriptionPackage {
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelfDescriptionGraph {
    pub(crate) name: String,
    pub(crate) instances: Vec<SelfDescriptionInstance>,
    pub(crate) tasks: Vec<SelfDescriptionTask>,
    pub(crate) channels: Vec<SelfDescriptionChannel>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelfDescriptionInstance {
    pub(crate) name: String,
    pub(crate) component: String,
    pub(crate) process: String,
    pub(crate) runtime: String,
    #[serde(default)]
    pub(crate) params: Vec<SelfDescriptionParam>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelfDescriptionParam {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) ty: String,
    pub(crate) update: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelfDescriptionTask {
    pub(crate) instance: String,
    pub(crate) trigger: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelfDescriptionChannel {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) message_type: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelfDescriptionMessageAbi {
    pub(crate) type_name: String,
    pub(crate) size_bytes: usize,
    #[serde(default)]
    pub(crate) fields: Vec<SelfDescriptionFieldAbi>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SelfDescriptionFieldAbi {
    pub(crate) name: String,
    #[serde(rename = "type", default)]
    pub(crate) ty: String,
    pub(crate) offset_bytes: usize,
    pub(crate) size_bytes: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SelfDescriptionMessageFrame {
    pub(crate) type_name: String,
    #[serde(default)]
    pub(crate) header_size_bytes: usize,
    pub(crate) max_size_bytes: usize,
    pub(crate) variable: bool,
    #[serde(default)]
    pub(crate) fields: Vec<SelfDescriptionFrameField>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SelfDescriptionFrameField {
    pub(crate) name: String,
    #[serde(rename = "type", default)]
    pub(crate) ty: String,
    pub(crate) header_offset_bytes: usize,
    pub(crate) header_size_bytes: usize,
    pub(crate) tail_max_bytes: Option<usize>,
}

pub(crate) fn load_self_description(path: &Path) -> Result<SelfDescription> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read FlowRT image `{}`", path.display()))?;
    let json = if path
        .file_name()
        .is_some_and(|name| name == OsStr::new("selfdesc.json"))
    {
        bytes
    } else {
        self_description_section_bytes(&bytes).with_context(|| {
            format!(
                "failed to read `{SELF_DESCRIPTION_SECTION}` section from `{}`",
                path.display()
            )
        })?
    };
    serde_json::from_slice(&json).with_context(|| {
        format!(
            "failed to parse FlowRT self-description from `{}`",
            path.display()
        )
    })
}

fn load_self_description_with_hash(path: &Path) -> Result<(SelfDescription, String)> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read FlowRT image `{}`", path.display()))?;
    let json = if path
        .file_name()
        .is_some_and(|name| name == OsStr::new("selfdesc.json"))
    {
        bytes
    } else {
        self_description_section_bytes(&bytes).with_context(|| {
            format!(
                "failed to read `{SELF_DESCRIPTION_SECTION}` section from `{}`",
                path.display()
            )
        })?
    };
    let hash = self_description_hash(&json);
    let self_description = serde_json::from_slice(&json).with_context(|| {
        format!(
            "failed to parse FlowRT self-description from `{}`",
            path.display()
        )
    })?;
    Ok((self_description, hash))
}

fn self_description_section_bytes(image: &[u8]) -> Result<Vec<u8>> {
    let object =
        object::File::parse(image).context("FlowRT image is not a supported object file")?;
    let section = object
        .section_by_name(SELF_DESCRIPTION_SECTION)
        .with_context(|| format!("FlowRT image does not contain `{SELF_DESCRIPTION_SECTION}`"))?;
    let data = section
        .data()
        .context("failed to decode FlowRT self-description section data")?;
    Ok(data.to_vec())
}

pub(crate) fn self_description_hash(json: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(json);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn self_description_summary(self_description: &SelfDescription) -> String {
    let mut output = format!(
        "package={} selfdesc={} source_hash={} graphs={} instances={} tasks={} channels={} messages={}",
        self_description.package.name,
        self_description.self_description_version,
        self_description.source_hash,
        self_description.graphs.len(),
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
        self_description.message_abi.len()
    );
    for graph in &self_description.graphs {
        output.push_str(&format!("\ngraph {}", graph.name));
        for task in &graph.tasks {
            output.push_str(&format!(
                "\ntask {} trigger={}",
                task.instance, task.trigger
            ));
        }
        for channel in &graph.channels {
            output.push_str(&format!(
                "\nchannel {} -> {} type={}",
                channel.from, channel.to, channel.message_type
            ));
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
    let mut lines = Vec::new();
    for graph in &self_description.graphs {
        lines.push(format!("graph {}", graph.name));
        for instance in &graph.instances {
            lines.push(format!(
                "{} process={} runtime={} component={}",
                instance.name, instance.process, instance.runtime, instance.component
            ));
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
        max_size_bytes: usize,
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
            let fields = format_fixed_abi_fields(fields, payload)?;
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
            if payload.len() > *max_size_bytes {
                anyhow::bail!(
                    "channel `{}` payload length {} exceeds canonical frame max size {} for `{}`",
                    channel.name,
                    payload.len(),
                    max_size_bytes,
                    channel.message_type
                );
            }
            let fields = format_frame_fields(fields, *header_size_bytes, payload)?;
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
            format!("frame_max_size={max_size_bytes} variable={variable}")
        }
    }
}

fn format_fixed_abi_fields(fields: &[SelfDescriptionFieldAbi], payload: &[u8]) -> Result<String> {
    let mut formatted = Vec::new();
    for field in fields {
        if field.offset_bytes > payload.len()
            || field.size_bytes > payload.len().saturating_sub(field.offset_bytes)
        {
            anyhow::bail!(
                "field `{}` range {}..{} exceeds payload length {}",
                field.name,
                field.offset_bytes,
                field.offset_bytes.saturating_add(field.size_bytes),
                payload.len()
            );
        }
        let bytes = &payload[field.offset_bytes..field.offset_bytes + field.size_bytes];
        formatted.push(format!(
            "{}={}",
            field.name,
            format_fixed_abi_value(&field.ty, bytes)?
        ));
    }
    Ok(formatted.join(","))
}

fn format_frame_fields(
    fields: &[SelfDescriptionFrameField],
    header_size_bytes: usize,
    payload: &[u8],
) -> Result<String> {
    if payload.len() < header_size_bytes {
        anyhow::bail!(
            "canonical frame header size {} exceeds payload length {}",
            header_size_bytes,
            payload.len()
        );
    }
    let (header, tail) = payload.split_at(header_size_bytes);
    let mut formatted = Vec::new();
    for field in fields {
        if field.header_offset_bytes > header.len()
            || field.header_size_bytes > header.len().saturating_sub(field.header_offset_bytes)
        {
            anyhow::bail!(
                "field `{}` header range {}..{} exceeds frame header length {}",
                field.name,
                field.header_offset_bytes,
                field
                    .header_offset_bytes
                    .saturating_add(field.header_size_bytes),
                header.len()
            );
        }
        let bytes =
            &header[field.header_offset_bytes..field.header_offset_bytes + field.header_size_bytes];
        let value = format_frame_field_value(field, bytes, tail)?;
        formatted.push(format!("{}={value}", field.name));
    }
    Ok(formatted.join(","))
}

fn format_frame_field_value(
    field: &SelfDescriptionFrameField,
    header_bytes: &[u8],
    tail: &[u8],
) -> Result<String> {
    let ty = field.ty.trim();
    if let Some(max_len) = parse_bounded_type_max(ty, "string")? {
        let block = frame_tail_block(field, header_bytes, tail, max_len)?;
        let text = std::str::from_utf8(block)
            .with_context(|| format!("field `{}` is not valid UTF-8", field.name))?;
        return serde_json::to_string(text)
            .with_context(|| format!("failed to format string field `{}`", field.name));
    }
    if let Some(max_len) = parse_bounded_type_max(ty, "bytes")? {
        let block = frame_tail_block(field, header_bytes, tail, max_len)?;
        return Ok(format!("0x{}", hex_bytes(block)));
    }
    if let Some((element_ty, max_len)) = parse_sequence_type(ty)? {
        let element_size = required_fixed_wire_size(element_ty)
            .with_context(|| format!("unsupported sequence element type `{element_ty}`"))?;
        let max_tail_bytes = element_size
            .checked_mul(max_len)
            .with_context(|| format!("sequence `{ty}` max length overflows"))?;
        let block = frame_tail_block(field, header_bytes, tail, max_tail_bytes)?;
        if block.len() % element_size != 0 {
            anyhow::bail!(
                "field `{}` byte length {} is not divisible by element size {}",
                field.name,
                block.len(),
                element_size
            );
        }
        let element_count = block.len() / element_size;
        if element_count > max_len {
            anyhow::bail!(
                "field `{}` contains {} sequence elements, exceeding max {}",
                field.name,
                element_count,
                max_len
            );
        }
        let mut values = Vec::with_capacity(element_count);
        for chunk in block.chunks_exact(element_size) {
            values.push(format_fixed_abi_value(element_ty, chunk)?);
        }
        return Ok(format!("[{}]", values.join(",")));
    }
    format_fixed_abi_value(ty, header_bytes)
}

fn frame_tail_block<'a>(
    field: &SelfDescriptionFrameField,
    header_bytes: &[u8],
    tail: &'a [u8],
    declared_max_len: usize,
) -> Result<&'a [u8]> {
    if header_bytes.len() != 8 {
        anyhow::bail!(
            "variable field `{}` header expects 8-byte VarSpan but has {} bytes",
            field.name,
            header_bytes.len()
        );
    }
    let offset = read_u32_le(&header_bytes[0..4])? as usize;
    let len = read_u32_le(&header_bytes[4..8])? as usize;
    if len > declared_max_len {
        anyhow::bail!(
            "field `{}` length {} exceeds declared max {}",
            field.name,
            len,
            declared_max_len
        );
    }
    if let Some(tail_max_bytes) = field.tail_max_bytes
        && len > tail_max_bytes
    {
        anyhow::bail!(
            "field `{}` length {} exceeds self-description tail max {}",
            field.name,
            len,
            tail_max_bytes
        );
    }
    if offset > tail.len() || len > tail.len().saturating_sub(offset) {
        anyhow::bail!(
            "field `{}` tail range {}..{} exceeds tail length {}",
            field.name,
            offset,
            offset.saturating_add(len),
            tail.len()
        );
    }
    Ok(&tail[offset..offset + len])
}

fn read_u32_le(bytes: &[u8]) -> Result<u32> {
    let array: [u8; 4] = bytes
        .try_into()
        .context("u32 wire value must contain exactly 4 bytes")?;
    Ok(u32::from_le_bytes(array))
}

fn parse_bounded_type_max(ty: &str, prefix: &str) -> Result<Option<usize>> {
    let Some(inner) = ty
        .strip_prefix(prefix)
        .and_then(|value| value.strip_prefix("<max="))
        .and_then(|value| value.strip_suffix('>'))
    else {
        return Ok(None);
    };
    let max = inner
        .trim()
        .parse::<usize>()
        .with_context(|| format!("invalid bounded type max in `{ty}`"))?;
    Ok(Some(max))
}

fn parse_sequence_type(ty: &str) -> Result<Option<(&str, usize)>> {
    let Some(inner) = ty
        .strip_prefix("sequence<")
        .and_then(|value| value.strip_suffix('>'))
    else {
        return Ok(None);
    };
    let Some((element, max_len)) = inner.rsplit_once(",max=") else {
        anyhow::bail!("invalid sequence type `{ty}`");
    };
    let max_len = max_len
        .trim()
        .parse::<usize>()
        .with_context(|| format!("invalid sequence max length in `{ty}`"))?;
    Ok(Some((element.trim(), max_len)))
}

fn format_fixed_abi_value(ty: &str, bytes: &[u8]) -> Result<String> {
    let ty = ty.trim();
    if let Some((element, len)) = parse_fixed_array_type(ty)? {
        let element_size = required_fixed_wire_size(element)
            .with_context(|| format!("unsupported fixed array element type `{element}`"))?;
        if bytes.len() != element_size * len {
            anyhow::bail!(
                "fixed array `{ty}` expects {} bytes but payload field has {} bytes",
                element_size * len,
                bytes.len()
            );
        }
        let mut values = Vec::with_capacity(len);
        for index in 0..len {
            let start = index * element_size;
            values.push(format_fixed_abi_value(
                element,
                &bytes[start..start + element_size],
            )?);
        }
        return Ok(format!("[{}]", values.join(",")));
    }
    format_primitive_value(ty, bytes)
}

fn required_fixed_wire_size(ty: &str) -> Result<usize> {
    fixed_wire_size(ty)?.with_context(|| format!("unsupported fixed wire type `{ty}`"))
}

fn fixed_wire_size(ty: &str) -> Result<Option<usize>> {
    let ty = ty.trim();
    if let Some(size) = primitive_size(ty) {
        return Ok(Some(size));
    }
    if let Some((element, len)) = parse_fixed_array_type(ty)? {
        let Some(element_size) = fixed_wire_size(element)? else {
            return Ok(None);
        };
        return Ok(element_size.checked_mul(len));
    }
    Ok(None)
}

fn parse_fixed_array_type(ty: &str) -> Result<Option<(&str, usize)>> {
    let Some(inner) = ty
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Ok(None);
    };
    let Some((element, len)) = inner.split_once(';') else {
        anyhow::bail!("invalid fixed array type `{ty}`");
    };
    let len = len
        .trim()
        .parse::<usize>()
        .with_context(|| format!("invalid fixed array length in `{ty}`"))?;
    Ok(Some((element.trim(), len)))
}

fn primitive_size(ty: &str) -> Option<usize> {
    Some(match ty {
        "bool" | "u8" | "i8" => 1,
        "u16" | "i16" => 2,
        "u32" | "i32" | "f32" => 4,
        "u64" | "i64" | "f64" => 8,
        "u128" | "i128" => 16,
        _ => return None,
    })
}

fn format_primitive_value(ty: &str, bytes: &[u8]) -> Result<String> {
    let expected = primitive_size(ty).with_context(|| format!("unsupported field type `{ty}`"))?;
    if bytes.len() != expected {
        anyhow::bail!(
            "primitive `{ty}` expects {expected} bytes but payload field has {} bytes",
            bytes.len()
        );
    }
    Ok(match ty {
        "bool" => (bytes[0] != 0).to_string(),
        "u8" => bytes[0].to_string(),
        "i8" => (bytes[0] as i8).to_string(),
        "u16" => u16::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "i16" => i16::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "u32" => u32::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "i32" => i32::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "u64" => u64::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "i64" => i64::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "u128" => u128::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "i128" => i128::from_le_bytes(bytes.try_into().unwrap()).to_string(),
        "f32" => format_float(f32::from_le_bytes(bytes.try_into().unwrap()) as f64),
        "f64" => format_float(f64::from_le_bytes(bytes.try_into().unwrap())),
        _ => unreachable!("primitive_size already accepted type"),
    })
}

fn format_float(value: f64) -> String {
    let mut formatted = value.to_string();
    if !formatted.contains('.') && !formatted.contains('e') && !formatted.contains('E') {
        formatted.push_str(".0");
    }
    formatted
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
                    lines.push(format!(
                        "supervisor_process={} state={} pid={} restarts={} ticks={} last_seen_ms={} tick_stale={} exit_code={} socket={}",
                        process.name,
                        process.state,
                        option_u32(process.pid),
                        process.restart_count,
                        option_u64(process.tick_count),
                        option_u64(process.last_seen_unix_ms),
                        process.tick_stale,
                        option_i32(process.exit_code),
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
