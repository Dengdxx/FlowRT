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

use crate::frame_json::{decode_message_json, encode_boundary_json};

pub(crate) use flowrt_selfdesc::self_description_hash;

mod display;
pub(crate) use display::*;

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
        | flowrt::IntrospectionResponse::OperationStarted { .. }
        | flowrt::IntrospectionResponse::OperationResult { .. }
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

pub(crate) fn operation_start(
    image: &Path,
    name: &str,
    raw_json: &str,
    socket: Option<&Path>,
    timeout_ms: Option<u64>,
) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    let operation = find_operation_endpoint(&self_description, name)?;
    let payload = encode_boundary_json(&self_description, name, &operation.goal_type, raw_json)?;
    let socket = select_echo_socket(socket, &self_description_hash)?;
    match flowrt::request_operation_start_with_timeout(
        &socket,
        name,
        payload,
        timeout_ms,
        Some("flowrt.cli".to_string()),
        LOCAL_INTROSPECTION_TIMEOUT,
    ) {
        Ok(flowrt::IntrospectionResponse::OperationStarted { handshake, started }) => {
            ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
            Ok(format!(
                "operation_id={} {}",
                started.operation_id,
                format_operation_status(&started.operation, Some(&socket))
            ))
        }
        Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
            anyhow::bail!(
                "failed to start FlowRT operation `{}` on `{}`: {}",
                name,
                socket.display(),
                message
            )
        }
        Ok(_) => {
            anyhow::bail!(
                "failed to start FlowRT operation `{}` on `{}`: unexpected introspection response",
                name,
                socket.display()
            )
        }
        Err(error) => {
            anyhow::bail!(
                "failed to start FlowRT operation `{}` on `{}`: {}",
                name,
                socket.display(),
                error
            )
        }
    }
}

pub(crate) fn remote_operation_start(
    image: &Path,
    name: &str,
    raw_json: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: Option<u64>,
) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    let operation = find_operation_endpoint(&self_description, name)?;
    let payload = encode_boundary_json(&self_description, name, &operation.goal_type, raw_json)?;
    let request_timeout_ms = 5000;
    let session = open_zenoh_operation_session()?;
    let runtime = select_remote_operation_runtime_for_request(
        &session,
        &self_description_hash,
        runtime_key_expr,
        request_timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_start(
        &session,
        &runtime.key_expr,
        name,
        payload,
        timeout_ms,
        Some("flowrt.cli".to_string()),
        request_timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!("failed to start remote operation `{name}` via `{runtime}`: {error}")
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationStarted { handshake, started } => {
            ensure_remote_handshake(&handshake, &self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            Ok(format!(
                "operation_id={} {}",
                started.operation_id,
                format_operation_status(&started.operation, None)
            ))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("failed to start remote operation `{name}` via `{runtime}`: {message}");
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

fn find_operation_endpoint<'a>(
    self_description: &'a SelfDescription,
    name: &str,
) -> Result<&'a SelfDescriptionOperationEndpoint> {
    let mut matches = self_description
        .graphs
        .iter()
        .flat_map(|graph| graph.operations.iter())
        .filter(|operation| operation.name == name)
        .collect::<Vec<_>>();
    match matches.len() {
        0 => anyhow::bail!("FlowRT self-description does not contain Operation `{name}`"),
        1 => Ok(matches.remove(0)),
        _ => anyhow::bail!("FlowRT self-description contains multiple Operations named `{name}`"),
    }
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
    if let Some(operation_id) = name
        && looks_like_operation_id(operation_id)
    {
        return operation_status_by_id_for_sockets(operation_id, sockets);
    }

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

fn operation_status_by_id_for_sockets(operation_id: &str, sockets: Vec<PathBuf>) -> Result<String> {
    let mut lines = Vec::new();
    let mut errors = Vec::new();
    for socket in sockets {
        match flowrt::request_operation_status_with_timeout(
            &socket,
            operation_id,
            LOCAL_INTROSPECTION_TIMEOUT,
        ) {
            Ok(flowrt::IntrospectionResponse::OperationValue { operation, .. }) => {
                lines.push(format!(
                    "operation_id={} {}",
                    operation_id,
                    format_operation_status(&operation, Some(&socket))
                ));
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
    if lines.is_empty() {
        if errors.is_empty() {
            Ok(format!("no live FlowRT operation matches `{operation_id}`"))
        } else {
            Ok(format!(
                "no live FlowRT operation matches `{}`; status errors: {}",
                operation_id,
                errors.join("; ")
            ))
        }
    } else {
        Ok(lines.join("\n"))
    }
}

fn looks_like_operation_id(value: &str) -> bool {
    let mut parts = value.split(':');
    let Some(operation_key) = parts.next() else {
        return false;
    };
    let Some(client_id) = parts.next() else {
        return false;
    };
    let Some(sequence) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && [operation_key, client_id, sequence]
            .iter()
            .all(|part| !part.is_empty() && part.parse::<u64>().is_ok())
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

pub(crate) fn operation_result(
    image: &Path,
    operation_id: &str,
    socket: Option<&Path>,
    remote: bool,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    if remote {
        return remote_operation_result(
            &self_description,
            &self_description_hash,
            operation_id,
            runtime_key_expr,
            timeout_ms,
        );
    }

    let socket = select_echo_socket(socket, &self_description_hash)?;
    match flowrt::request_operation_result_with_timeout(
        &socket,
        operation_id,
        LOCAL_INTROSPECTION_TIMEOUT,
    ) {
        Ok(flowrt::IntrospectionResponse::OperationResult { handshake, result }) => {
            ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
            format_operation_result(&self_description, &result)
        }
        Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
            anyhow::bail!(
                "failed to get FlowRT operation result `{}` from `{}`: {}",
                operation_id,
                socket.display(),
                message
            )
        }
        Ok(_) => {
            anyhow::bail!(
                "failed to get FlowRT operation result `{}` from `{}`: unexpected introspection response",
                operation_id,
                socket.display()
            )
        }
        Err(error) => {
            anyhow::bail!(
                "failed to get FlowRT operation result `{}` from `{}`: {}",
                operation_id,
                socket.display(),
                error
            )
        }
    }
}

fn format_operation_result(
    self_description: &SelfDescription,
    result: &flowrt::IntrospectionOperationResult,
) -> Result<String> {
    let mut fields = vec![
        format!("operation_id={}", result.operation_id),
        format!("operation={}", result.operation),
        format!("state={}", result.state),
    ];
    if let Some(error) = &result.error {
        fields.push(format!("error={error}"));
    } else if let Some(payload) = &result.payload {
        let operation = find_operation_endpoint(self_description, &result.operation)?;
        let value = decode_message_json(self_description, &operation.result_type, payload)?;
        fields.push(format!("result={value}"));
    } else if let Some(value) = &result.result {
        fields.push(format!("result={value}"));
    }
    Ok(fields.join(" "))
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
                // static self-description 只提供合同关联信息；live status 仍是运行态事实源。
                let static_facts =
                    load_static_self_description_facts(&socket, &handshake.self_description_hash);
                let recorder = status.recorder.clone();

                let live_counts = live_status_counts(&status);
                let critical_instances = if status.critical_instances.is_empty() {
                    "none".to_string()
                } else {
                    status.critical_instances.join(",")
                };
                let temporary_overlay = static_facts.artifact.temporary_overlay.is_some();
                lines.push(format!(
                    "pid={} package={} process={} runtime={} selfdesc={} static_selfdesc={} ticks={} clock_source={} tick_time_ms={} clock_unit={} clock_field={} channels={} inputs={} routes={} graph_health={} graph_critical_health={} critical_instances={} observers={} dropped_samples={} artifact_mode={} temporary_island={} test_only={} temporary_overlay={} socket={}",
                    handshake.pid,
                    handshake.package,
                    handshake.process,
                    handshake.runtime,
                    handshake.self_description_hash,
                    static_facts.load_state_label(),
                    status.tick_count,
                    status.clock.source,
                    option_u64(status.clock.tick_time_ms),
                    status.clock.unit,
                    status.clock.field,
                    status.channels.len(),
                    status.inputs.len(),
                    status.routes.len(),
                    status.graph_health,
                    status.graph_critical_health,
                    critical_instances,
                    live_counts.active_observers,
                    live_counts.dropped_samples,
                    static_facts.artifact.mode,
                    static_facts.artifact.temporary_island,
                    static_facts.artifact.test_only,
                    temporary_overlay,
                    socket.display()
                ));
                for graph in &static_facts.graphs {
                    lines.push(format!(
                        "graph={} mode={} boundary_endpoints={} socket={}",
                        graph.name,
                        graph.mode,
                        graph.boundary_endpoint_count,
                        socket.display()
                    ));
                }
                for boundary in &static_facts.boundary_endpoints {
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
                    let static_thread_affinity = static_facts
                        .route_thread_affinity
                        .get(&route_affinity_key(&route.from, &route.to))
                        .map(String::as_str)
                        .unwrap_or("none");
                    lines.push(format!(
                        "route={} from={} to={} type={} backend={} thread_affinity={} static_thread_affinity={} selected_reason={} published_count={} dropped_samples={} backpressure={} overflow={} last_publish_ms={} last_error={} backend_health={} backend_recoverable={} backend_reconnect_attempt={} backend_next_retry_unix_ms={} backend_health_error={} socket={}",
                        route.name,
                        route.from,
                        route.to,
                        route.message_type,
                        route.backend,
                        static_thread_affinity,
                        static_thread_affinity,
                        empty_as_none(&route.selected_reason),
                        route.published_count,
                        route.dropped_samples,
                        route.backpressure_count,
                        route.overflow_count,
                        option_u64(route.last_publish_ms),
                        option_str(route.last_error.as_deref()),
                        empty_as_none(&route.backend_health_state),
                        route.backend_recoverable,
                        route.backend_reconnect_attempt,
                        option_u64(route.backend_next_retry_unix_ms),
                        option_str(route.backend_health_error.as_deref()),
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
                for instance in &status.instances {
                    lines.push(format!(
                        "instance={} lifecycle={} restart_count={} last_fault_reason={} last_fault_tick={} last_transition_tick={} socket={}",
                        instance.instance,
                        instance.lifecycle_state,
                        instance.restart_count,
                        option_str(instance.last_fault_reason.as_deref()),
                        option_u64(instance.last_fault_tick),
                        option_u64(instance.last_transition_tick),
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
                    let (client_inst, server_inst) = static_facts
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
                        let descriptor_info = static_facts
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
            | Ok(flowrt::IntrospectionResponse::OperationResult { .. })
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
            Ok(flowrt::IntrospectionResponse::OperationStarted { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected operation response",
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

/// live status 输出中来自 static self-description 的合同事实。
#[derive(Default)]
struct StaticSelfDescriptionFacts {
    loaded: bool,
    artifact: flowrt_selfdesc::SelfDescriptionArtifact,
    graphs: Vec<GraphModeAssoc>,
    boundary_endpoints: Vec<BoundaryEndpointAssoc>,
    route_thread_affinity: BTreeMap<String, String>,
    service_endpoints: BTreeMap<String, ServiceEndpointAssoc>,
    resource_descriptors: BTreeMap<String, SelfDescriptionResourceDescriptor>,
}

impl StaticSelfDescriptionFacts {
    fn load_state_label(&self) -> &'static str {
        if self.loaded { "loaded" } else { "unavailable" }
    }
}

struct LiveStatusCounts {
    active_observers: u64,
    dropped_samples: u64,
}

fn live_status_counts(status: &flowrt::IntrospectionStatus) -> LiveStatusCounts {
    LiveStatusCounts {
        active_observers: status
            .channels
            .iter()
            .map(|channel| channel.active_observers)
            .sum(),
        dropped_samples: status
            .channels
            .iter()
            .map(|channel| channel.dropped_samples)
            .sum(),
    }
}

/// 从 runtime socket 请求 static self-description，构建 service/resource 关联映射。
///
/// 如果 self-description 请求失败（如 socket 不支持），返回空 map，不报错。
fn load_static_self_description_facts(
    socket: &Path,
    expected_hash: &str,
) -> StaticSelfDescriptionFacts {
    let Ok(response) =
        flowrt::request_self_description_with_timeout(socket, LOCAL_INTROSPECTION_TIMEOUT)
    else {
        return StaticSelfDescriptionFacts::default();
    };
    let flowrt::IntrospectionResponse::SelfDescription { handshake, json } = response else {
        return StaticSelfDescriptionFacts::default();
    };
    if handshake.self_description_hash != expected_hash
        || self_description_hash(json.as_bytes()) != expected_hash
    {
        return StaticSelfDescriptionFacts::default();
    }
    let Ok(sd) = serde_json::from_str::<SelfDescription>(&json) else {
        return StaticSelfDescriptionFacts::default();
    };
    let mut static_facts = StaticSelfDescriptionFacts {
        loaded: true,
        artifact: sd.artifact.clone(),
        ..StaticSelfDescriptionFacts::default()
    };
    for graph in &sd.graphs {
        static_facts.graphs.push(GraphModeAssoc {
            name: graph.name.clone(),
            mode: graph.mode.clone(),
            boundary_endpoint_count: graph.boundary_endpoints.len(),
        });
        for boundary in &graph.boundary_endpoints {
            static_facts.boundary_endpoints.push(BoundaryEndpointAssoc {
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
            static_facts.route_thread_affinity.insert(
                route_affinity_key(&channel.from, &channel.to),
                channel.thread_affinity.clone(),
            );
        }
        for ep in &graph.services {
            if !ep.client_instance.is_empty() && !ep.server_instance.is_empty() {
                static_facts.service_endpoints.insert(
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
                static_facts.resource_descriptors.insert(
                    resource_descriptor_key(instance, &resource.name),
                    descriptor.clone(),
                );
            }
        }
    }
    static_facts
}

fn route_affinity_key(from: &str, to: &str) -> String {
    format!("{from}->{to}")
}

fn resource_descriptor_key(boundary: &str, resource: &str) -> String {
    format!("{boundary}.{resource}")
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

/// 解析 `flowrt/op/{package}/{selfdesc_hash}/{pid}` 格式的 key expression。
pub(crate) fn parse_remote_operation_key_expr(key: &str) -> Option<(&str, &str, &str)> {
    let rest = key.strip_prefix("flowrt/op/")?;
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

/// 打开用于远程 Operation 控制面的 zenoh session。
fn open_zenoh_operation_session() -> Result<zenoh::Session> {
    let zenoh_config = flowrt::zenoh::config_from_environment().map_err(|error| {
        anyhow::anyhow!("failed to configure zenoh session for operation discovery: {error}")
    })?;
    zenoh::open(zenoh_config).wait().map_err(|error| {
        anyhow::anyhow!("failed to open zenoh session for operation discovery: {error:?}")
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

/// 通过 zenoh 扫描所有远程 Operation 端点，返回匹配 `self_description_hash` 的 runtime。
pub(crate) fn discover_remote_operation_runtimes(
    session: &zenoh::Session,
    self_description_hash: &str,
    timeout_ms: u64,
) -> Result<Vec<RemoteRuntimeEntry>> {
    let request = flowrt::IntrospectionRequest::Status;
    let payload = serde_json::to_vec(&request).map_err(|error| {
        anyhow::anyhow!("failed to encode operation discovery request: {error}")
    })?;
    let timeout = Duration::from_millis(timeout_ms);

    let receiver = session
        .get("flowrt/op/**")
        .with(zenoh::handlers::FifoChannel::new(64))
        .payload(zenoh::bytes::ZBytes::from(payload))
        .timeout(timeout)
        .wait()
        .map_err(|error| {
            anyhow::anyhow!("failed to send zenoh operation discovery query: {error:?}")
        })?;

    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();

    while let Ok(Some(reply)) = receiver.recv_timeout(timeout) {
        let Ok(sample) = reply.result() else {
            continue;
        };
        let key = sample.key_expr().to_string();
        let Some((package, hash, pid_str)) = parse_remote_operation_key_expr(&key) else {
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
        let entry_hash = hash.to_string();
        let entry_package_hint = package.to_string();
        let raw = sample.payload().to_bytes().to_vec();
        let Ok(response) = serde_json::from_slice::<flowrt::IntrospectionResponse>(&raw) else {
            continue;
        };
        let handshake = match &response {
            flowrt::IntrospectionResponse::Status { handshake, .. }
            | flowrt::IntrospectionResponse::Error { handshake, .. } => handshake,
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
            anyhow::anyhow!("failed to list remote params from `{runtime}`: {error}")
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

/// 请求远程 runtime Operation invocation 状态。
pub(crate) fn remote_operation_status(
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    let runtime = select_remote_operation_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_status(
        &session,
        &runtime.key_expr,
        operation_id,
        timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!("failed to get remote operation `{operation_id}` from `{runtime}`: {error}")
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationValue {
            handshake,
            operation,
        } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            Ok(format!(
                "operation_id={} {}",
                operation_id,
                format_operation_status(&operation, None)
            ))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to get remote operation `{operation_id}` from `{runtime}`: {message}"
            );
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

/// 请求远程 runtime 取消 Operation invocation。
pub(crate) fn remote_operation_cancel(
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    let runtime = select_remote_operation_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_cancel(
        &session,
        &runtime.key_expr,
        operation_id,
        timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "failed to cancel remote operation `{operation_id}` via `{runtime}`: {error}"
        )
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationValue {
            handshake,
            operation,
        } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            Ok(format!(
                "operation_id={} {}",
                operation_id,
                format_operation_status(&operation, None)
            ))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to cancel remote operation `{operation_id}` via `{runtime}`: {message}"
            );
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

/// 请求远程 runtime Operation invocation result。
fn remote_operation_result(
    self_description: &SelfDescription,
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    let runtime = select_remote_operation_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_result(
        &session,
        &runtime.key_expr,
        operation_id,
        timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "failed to get remote operation result `{operation_id}` from `{runtime}`: {error}"
        )
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationResult { handshake, result } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            format_operation_result(self_description, &result)
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to get remote operation result `{operation_id}` from `{runtime}`: {message}"
            );
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
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

fn select_remote_operation_runtime_for_request(
    session: &zenoh::Session,
    self_description_hash: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<RemoteRuntimeEntry> {
    if let Some(key_expr) = runtime_key_expr {
        return remote_operation_runtime_entry_from_key_expr(
            session,
            key_expr,
            self_description_hash,
            timeout_ms,
        );
    }
    let entries = discover_remote_operation_runtimes(session, self_description_hash, timeout_ms)?;
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

fn remote_operation_runtime_entry_from_key_expr(
    session: &zenoh::Session,
    key_expr: &str,
    self_description_hash: &str,
    timeout_ms: u64,
) -> Result<RemoteRuntimeEntry> {
    let Some((package, hash, pid_str)) = parse_remote_operation_key_expr(key_expr) else {
        anyhow::bail!(
            "invalid remote FlowRT runtime key expression `{key_expr}`; expected `flowrt/op/<package>/<selfdesc_hash>/<pid>`"
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
    let response = flowrt::request_remote_operation_overview(session, key_expr, timeout_ms)
        .map_err(|error| anyhow::anyhow!("failed to query remote runtime `{key_expr}`: {error}"))?;
    let handshake = match response {
        flowrt::IntrospectionResponse::Status { handshake, .. }
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
