use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;

use flowrt_selfdesc::{
    SelfDescription, SelfDescriptionChannel, SelfDescriptionComponentType, SelfDescriptionFieldAbi,
    SelfDescriptionFrameField, SelfDescriptionInstance, SelfDescriptionMessageAbi,
    SelfDescriptionMessageFrame, SelfDescriptionOperationEndpoint, SelfDescriptionParam,
    load_self_description as load_selfdesc,
    load_self_description_with_hash as load_selfdesc_with_hash,
};

use crate::frame_json::{decode_message_json, encode_boundary_json};

pub(crate) use flowrt_selfdesc::self_description_hash;

mod display;
pub(crate) use display::*;
mod live;
pub(crate) use live::*;
mod remote;
pub(crate) use remote::*;

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
        | flowrt::IntrospectionResponse::OperationEvents { .. }
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

pub(crate) fn operation_list_json(image: Option<&Path>, socket: Option<&Path>) -> Result<String> {
    let self_description = match image {
        Some(image) => load_self_description(image)?,
        None => {
            let (self_description, _hash, _socket) = load_echo_context_from_live_socket(socket)?;
            self_description
        }
    };
    let operations = self_description
        .graphs
        .iter()
        .flat_map(|graph| graph.operations.iter())
        .collect::<Vec<_>>();
    operation_json(&serde_json::json!({
        "response": "operation_list",
        "package": self_description.package.name,
        "operations": operations,
    }))
}

#[derive(Debug, Serialize)]
struct OperationStatusJsonEntry {
    socket: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<flowrt::IntrospectionOperationStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn operation_json<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).context("序列化 Operation JSON 失败")
}

fn operation_json_value_with_target(
    response: &str,
    operation_id: Option<&str>,
    operation: &flowrt::IntrospectionOperationStatus,
    socket: Option<&Path>,
    runtime: Option<&RemoteRuntimeEntry>,
) -> Result<String> {
    let mut value = serde_json::json!({
        "response": response,
        "operation": operation,
    });
    if let Some(operation_id) = operation_id {
        value["operation_id"] = serde_json::Value::String(operation_id.to_string());
    }
    if let Some(socket) = socket {
        value["socket"] = serde_json::Value::String(socket.display().to_string());
    }
    if let Some(runtime) = runtime {
        value["runtime"] = serde_json::Value::String(runtime.to_string());
    }
    operation_json(&value)
}

fn operation_started_json_value(
    started: &flowrt::IntrospectionOperationStartStatus,
    socket: Option<&Path>,
    runtime: Option<&RemoteRuntimeEntry>,
) -> Result<String> {
    let mut value = serde_json::json!({
        "response": "operation_started",
        "operation_id": started.operation_id,
        "operation": started.operation,
    });
    if let Some(socket) = socket {
        value["socket"] = serde_json::Value::String(socket.display().to_string());
    }
    if let Some(runtime) = runtime {
        value["runtime"] = serde_json::Value::String(runtime.to_string());
    }
    operation_json(&value)
}

fn decoded_operation_payload_value(
    self_description: &SelfDescription,
    operation_name: &str,
    payload: &[u8],
    payload_kind: &str,
) -> Result<serde_json::Value> {
    let operation = find_operation_endpoint(self_description, operation_name)?;
    let message_type = match payload_kind {
        "progress" => &operation.feedback_type,
        "result" => &operation.result_type,
        _ => anyhow::bail!("unknown Operation payload kind `{payload_kind}`"),
    };
    let raw_json = decode_message_json(self_description, message_type, payload)?;
    serde_json::from_str(&raw_json).with_context(|| {
        format!("decoded Operation `{operation_name}` {payload_kind} payload is not JSON")
    })
}

fn operation_result_json_value(
    self_description: &SelfDescription,
    result: &flowrt::IntrospectionOperationResult,
) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(result).context("序列化 Operation result JSON 失败")?;
    if let Some(payload) = &result.payload {
        let decoded = decoded_operation_payload_value(
            self_description,
            &result.operation,
            payload,
            "result",
        )?;
        value["value"] = decoded;
    }
    Ok(value)
}

fn operation_event_json_value(
    self_description: &SelfDescription,
    event: &flowrt::IntrospectionOperationEvent,
) -> Result<serde_json::Value> {
    let mut value = serde_json::to_value(event).context("序列化 Operation event JSON 失败")?;
    if let Some(payload) = &event.payload {
        match event.kind.as_str() {
            "progress" | "result" => {
                let decoded = decoded_operation_payload_value(
                    self_description,
                    &event.operation,
                    payload,
                    &event.kind,
                )?;
                value["value"] = decoded;
            }
            _ => {}
        }
    }
    Ok(value)
}

fn operation_result_json_response(
    self_description: &SelfDescription,
    result: &flowrt::IntrospectionOperationResult,
    runtime: Option<&RemoteRuntimeEntry>,
) -> Result<String> {
    let mut value = serde_json::json!({
        "response": "operation_result",
        "result": operation_result_json_value(self_description, result)?,
    });
    if let Some(runtime) = runtime {
        value["runtime"] = serde_json::Value::String(runtime.to_string());
    }
    operation_json(&value)
}

fn operation_events_json_response(
    self_description: &SelfDescription,
    operation_id: &str,
    events: &[flowrt::IntrospectionOperationEvent],
    next_sequence: u64,
    terminal: bool,
    runtime: Option<&RemoteRuntimeEntry>,
) -> Result<String> {
    let events = events
        .iter()
        .map(|event| operation_event_json_value(self_description, event))
        .collect::<Result<Vec<_>>>()?;
    let mut value = serde_json::json!({
        "response": "operation_events",
        "operation_id": operation_id,
        "events": events,
        "next_sequence": next_sequence,
        "terminal": terminal,
    });
    if let Some(runtime) = runtime {
        value["runtime"] = serde_json::Value::String(runtime.to_string());
    }
    operation_json(&value)
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

pub(crate) fn operation_start_json(
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
            operation_started_json_value(&started, Some(&socket), None)
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

pub(crate) fn remote_operation_start_json(
    image: &Path,
    name: &str,
    raw_json: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: Option<u64>,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    remote_operation_start_json_with_session(
        &session,
        image,
        name,
        raw_json,
        runtime_key_expr,
        timeout_ms,
    )
}

pub(crate) fn remote_operation_start_json_with_session(
    session: &zenoh::Session,
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
    let runtime = select_remote_operation_runtime_for_request(
        session,
        &self_description_hash,
        runtime_key_expr,
        request_timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_start(
        session,
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
            operation_started_json_value(&started, None, Some(&runtime))
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

pub(crate) fn operation_status_json(socket: Option<&Path>, name: Option<&str>) -> Result<String> {
    let sockets = match socket {
        Some(socket) => vec![socket.to_path_buf()],
        None => discover_cli_runtime_sockets()?,
    };
    if let Some(operation_id) = name
        && looks_like_operation_id(operation_id)
    {
        return operation_status_by_id_json_for_sockets(operation_id, sockets);
    }

    let mut entries = Vec::new();
    for socket in sockets {
        match flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(flowrt::IntrospectionResponse::Status { status, .. }) => {
                for operation in status.operations {
                    if name.is_none_or(|name| operation.name == name) {
                        entries.push(OperationStatusJsonEntry {
                            socket: socket.display().to_string(),
                            operation_id: None,
                            operation: Some(operation),
                            error: None,
                        });
                    }
                }
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                entries.push(OperationStatusJsonEntry {
                    socket: socket.display().to_string(),
                    operation_id: None,
                    operation: None,
                    error: Some(message),
                });
            }
            Ok(_) => {
                entries.push(OperationStatusJsonEntry {
                    socket: socket.display().to_string(),
                    operation_id: None,
                    operation: None,
                    error: Some("unexpected introspection response".to_string()),
                });
            }
            Err(error) => {
                entries.push(OperationStatusJsonEntry {
                    socket: socket.display().to_string(),
                    operation_id: None,
                    operation: None,
                    error: Some(error.to_string()),
                });
            }
        }
    }
    operation_json(&serde_json::json!({
        "response": "operation_status",
        "name": name,
        "entries": entries,
    }))
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

fn operation_status_by_id_json_for_sockets(
    operation_id: &str,
    sockets: Vec<PathBuf>,
) -> Result<String> {
    let mut entries = Vec::new();
    for socket in sockets {
        match flowrt::request_operation_status_with_timeout(
            &socket,
            operation_id,
            LOCAL_INTROSPECTION_TIMEOUT,
        ) {
            Ok(flowrt::IntrospectionResponse::OperationValue { operation, .. }) => {
                entries.push(OperationStatusJsonEntry {
                    socket: socket.display().to_string(),
                    operation_id: Some(operation_id.to_string()),
                    operation: Some(operation),
                    error: None,
                });
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                entries.push(OperationStatusJsonEntry {
                    socket: socket.display().to_string(),
                    operation_id: Some(operation_id.to_string()),
                    operation: None,
                    error: Some(message),
                });
            }
            Ok(_) => {
                entries.push(OperationStatusJsonEntry {
                    socket: socket.display().to_string(),
                    operation_id: Some(operation_id.to_string()),
                    operation: None,
                    error: Some("unexpected introspection response".to_string()),
                });
            }
            Err(error) => {
                entries.push(OperationStatusJsonEntry {
                    socket: socket.display().to_string(),
                    operation_id: Some(operation_id.to_string()),
                    operation: None,
                    error: Some(error.to_string()),
                });
            }
        }
    }
    operation_json(&serde_json::json!({
        "response": "operation_status",
        "operation_id": operation_id,
        "entries": entries,
    }))
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

pub(crate) fn operation_cancel_json(operation_id: &str, socket: Option<&Path>) -> Result<String> {
    if let Some(socket) = socket {
        return operation_cancel_on_socket_json(operation_id, socket);
    }
    let sockets = discover_cli_runtime_sockets()?;
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
            operation_cancel_on_socket_json(operation_id, &socket)
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

fn operation_cancel_on_socket_json(operation_id: &str, socket: &Path) -> Result<String> {
    match flowrt::request_operation_cancel_with_timeout(
        socket,
        operation_id,
        LOCAL_INTROSPECTION_TIMEOUT,
    ) {
        Ok(flowrt::IntrospectionResponse::OperationValue { operation, .. }) => {
            operation_json_value_with_target(
                "operation_value",
                Some(operation_id),
                &operation,
                Some(socket),
                None,
            )
        }
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

pub(crate) fn operation_result_json(
    image: &Path,
    operation_id: &str,
    socket: Option<&Path>,
    remote: bool,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    if remote {
        return remote_operation_result_json(
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
            operation_result_json_response(&self_description, &result, None)
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

pub(crate) fn operation_follow(
    image: &Path,
    operation_id: &str,
    socket: Option<&Path>,
    remote: bool,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    if remote {
        return remote_operation_follow(
            &self_description,
            &self_description_hash,
            operation_id,
            runtime_key_expr,
            timeout_ms,
        );
    }

    let socket = select_echo_socket(socket, &self_description_hash)?;
    let mut cursor = 0;
    let mut lines = Vec::new();
    loop {
        let response = flowrt::request_operation_observe_with_timeout(
            &socket,
            operation_id,
            cursor,
            Some(64),
            LOCAL_INTROSPECTION_TIMEOUT,
        );
        match response {
            Ok(flowrt::IntrospectionResponse::OperationEvents {
                handshake,
                events,
                next_sequence,
                terminal,
                ..
            }) => {
                ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
                let event_count = events.len();
                for event in &events {
                    lines.push(format_operation_event(&self_description, event)?);
                }
                cursor = next_sequence;
                if terminal && event_count < 64 {
                    break;
                }
                if event_count == 0 {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                anyhow::bail!(
                    "failed to follow FlowRT operation `{}` from `{}`: {}",
                    operation_id,
                    socket.display(),
                    message
                )
            }
            Ok(_) => {
                anyhow::bail!(
                    "failed to follow FlowRT operation `{}` from `{}`: unexpected introspection response",
                    operation_id,
                    socket.display()
                )
            }
            Err(error) => {
                anyhow::bail!(
                    "failed to follow FlowRT operation `{}` from `{}`: {}",
                    operation_id,
                    socket.display(),
                    error
                )
            }
        }
    }
    Ok(lines.join("\n"))
}

pub(crate) fn operation_follow_json(
    image: &Path,
    operation_id: &str,
    socket: Option<&Path>,
    remote: bool,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
    if remote {
        return remote_operation_follow_json(
            &self_description,
            &self_description_hash,
            operation_id,
            runtime_key_expr,
            timeout_ms,
        );
    }

    let socket = select_echo_socket(socket, &self_description_hash)?;
    let mut cursor = 0;
    let mut all_events = Vec::new();
    let terminal = loop {
        let response = flowrt::request_operation_observe_with_timeout(
            &socket,
            operation_id,
            cursor,
            Some(64),
            LOCAL_INTROSPECTION_TIMEOUT,
        );
        match response {
            Ok(flowrt::IntrospectionResponse::OperationEvents {
                handshake,
                events,
                next_sequence,
                terminal: response_terminal,
                ..
            }) => {
                ensure_handshake_hash(&handshake, &self_description_hash, &socket)?;
                let event_count = events.len();
                all_events.extend(events);
                cursor = next_sequence;
                if response_terminal && event_count < 64 {
                    break response_terminal;
                }
                if event_count == 0 {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                anyhow::bail!(
                    "failed to follow FlowRT operation `{}` from `{}`: {}",
                    operation_id,
                    socket.display(),
                    message
                )
            }
            Ok(_) => {
                anyhow::bail!(
                    "failed to follow FlowRT operation `{}` from `{}`: unexpected introspection response",
                    operation_id,
                    socket.display()
                )
            }
            Err(error) => {
                anyhow::bail!(
                    "failed to follow FlowRT operation `{}` from `{}`: {}",
                    operation_id,
                    socket.display(),
                    error
                )
            }
        }
    };
    operation_events_json_response(
        &self_description,
        operation_id,
        &all_events,
        cursor,
        terminal,
        None,
    )
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

fn format_operation_event(
    self_description: &SelfDescription,
    event: &flowrt::IntrospectionOperationEvent,
) -> Result<String> {
    match event.kind.as_str() {
        "state" => Ok(format!(
            "operation_id={} state={}",
            event.operation_id,
            event.state.as_deref().unwrap_or("unknown")
        )),
        "progress" => {
            let operation = find_operation_endpoint(self_description, &event.operation)?;
            let payload = event.payload.as_deref().unwrap_or(&[]);
            let progress =
                decode_message_json(self_description, &operation.feedback_type, payload)?;
            Ok(format!(
                "operation_id={} progress_sequence={} progress={}",
                event.operation_id,
                event.progress_sequence.unwrap_or(0),
                progress
            ))
        }
        "result" => {
            if let Some(payload) = &event.payload {
                let operation = find_operation_endpoint(self_description, &event.operation)?;
                let result =
                    decode_message_json(self_description, &operation.result_type, payload)?;
                Ok(format!(
                    "operation_id={} state={} result={}",
                    event.operation_id,
                    event.state.as_deref().unwrap_or("succeeded"),
                    result
                ))
            } else {
                Ok(format!(
                    "operation_id={} state={}",
                    event.operation_id,
                    event.state.as_deref().unwrap_or("succeeded")
                ))
            }
        }
        "error" => Ok(format!(
            "operation_id={} state={} error={}",
            event.operation_id,
            event.state.as_deref().unwrap_or("failed"),
            event.message.as_deref().unwrap_or("handler error")
        )),
        kind => Ok(format!("operation_id={} event={kind}", event.operation_id)),
    }
}
