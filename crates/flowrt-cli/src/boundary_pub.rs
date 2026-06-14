use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use flowrt_selfdesc::SelfDescription;
use serde_json::Value;

use crate::frame_json::encode_boundary_json as encode_message_json;
use crate::introspection::{
    LOCAL_INTROSPECTION_TIMEOUT, ensure_handshake_hash, load_echo_context_from_live_socket,
    load_self_description_with_hash, select_echo_socket,
};

pub(crate) struct BoundaryPublishTarget {
    self_description: SelfDescription,
    self_description_hash: String,
    socket: std::path::PathBuf,
}

impl BoundaryPublishTarget {
    pub(crate) fn open_for_command(
        image: &Path,
        socket: Option<&Path>,
        command: &'static str,
    ) -> Result<Self> {
        let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
        ensure_island_boundary_publish_mode(&self_description, command)?;
        let socket = select_echo_socket(socket, &self_description_hash)?;
        Ok(Self {
            self_description,
            self_description_hash,
            socket,
        })
    }

    pub(crate) fn publish_for_command(
        &self,
        endpoint: &str,
        json: &str,
        published_at_ms: Option<u64>,
        command: &str,
    ) -> Result<String> {
        let spec = find_boundary_publish_endpoint(&self.self_description, endpoint, command)?;
        let payload = encode_boundary_json(&self.self_description, &spec, json)?;
        publish_boundary_payload(
            &self.socket,
            &self.self_description_hash,
            &spec,
            payload,
            published_at_ms,
        )
    }
}

pub(crate) fn boundary_publish(
    endpoint: &str,
    json: &str,
    image: Option<&Path>,
    socket: Option<&Path>,
    published_at_ms: Option<u64>,
) -> Result<String> {
    let (self_description, self_description_hash, socket) = match image {
        Some(image) => {
            let (self_description, self_description_hash) = load_self_description_with_hash(image)?;
            let spec = find_boundary_publish_endpoint(&self_description, endpoint, "flowrt pub")?;
            let payload = encode_boundary_json(&self_description, &spec, json)?;
            let socket = select_echo_socket(socket, &self_description_hash)?;
            return publish_boundary_payload(
                &socket,
                &self_description_hash,
                &spec,
                payload,
                published_at_ms,
            );
        }
        None => load_echo_context_from_live_socket(socket)?,
    };
    let spec = find_boundary_publish_endpoint(&self_description, endpoint, "flowrt pub")?;
    let payload = encode_boundary_json(&self_description, &spec, json)?;
    publish_boundary_payload(
        &socket,
        &self_description_hash,
        &spec,
        payload,
        published_at_ms,
    )
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BoundaryPubInput {
    pub json: String,
    pub source: BoundaryPubInputSource,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BoundaryPubInputSource {
    JsonArrayIndex(usize),
    JsonLine(usize),
    SingleJsonValue,
}

impl BoundaryPubInputSource {
    fn describe(&self, path: &Path) -> String {
        match self {
            Self::JsonArrayIndex(index) => {
                format!("{} array index {index}", path.display())
            }
            Self::JsonLine(line) => format!("{} line {line}", path.display()),
            Self::SingleJsonValue => path.display().to_string(),
        }
    }
}

pub(crate) fn boundary_publish_from_file(
    endpoint: &str,
    file: &Path,
    image: Option<&Path>,
    socket: Option<&Path>,
    published_at_ms: Option<u64>,
    freq_hz: Option<f64>,
) -> Result<String> {
    if is_jsonl_path(file) {
        return boundary_publish_from_jsonl_file(
            endpoint,
            file,
            image,
            socket,
            published_at_ms,
            freq_hz,
        );
    }

    match boundary_pub_inputs_from_json_document(file) {
        Ok(inputs) => boundary_publish_inputs(
            endpoint,
            file,
            image,
            socket,
            published_at_ms,
            freq_hz,
            inputs,
        ),
        Err(json_error) => boundary_publish_from_jsonl_file(
            endpoint,
            file,
            image,
            socket,
            published_at_ms,
            freq_hz,
        )
        .with_context(|| {
            format!(
                "failed to parse `{}` as JSON document: {json_error}",
                file.display()
            )
        }),
    }
}

fn boundary_publish_inputs(
    endpoint: &str,
    file: &Path,
    image: Option<&Path>,
    socket: Option<&Path>,
    published_at_ms: Option<u64>,
    freq_hz: Option<f64>,
    inputs: Vec<BoundaryPubInput>,
) -> Result<String> {
    if inputs.is_empty() {
        anyhow::bail!(
            "flowrt pub --file `{}` does not contain any JSON messages",
            file.display()
        );
    }
    let interval = freq_hz.map(|freq| Duration::from_secs_f64(1.0 / freq));
    let mut lines = Vec::with_capacity(inputs.len() + 1);
    let mut next_send = Instant::now();

    for input in inputs {
        pace_boundary_publish(interval, &mut next_send);
        lines.push(boundary_publish_one_input(
            endpoint,
            file,
            image,
            socket,
            published_at_ms,
            input,
        )?);
    }
    lines.push(format!(
        "summary: endpoint={endpoint} sent={} source={}",
        lines.len(),
        file.display()
    ));
    Ok(lines.join("\n"))
}

fn boundary_publish_from_jsonl_file(
    endpoint: &str,
    file: &Path,
    image: Option<&Path>,
    socket: Option<&Path>,
    published_at_ms: Option<u64>,
    freq_hz: Option<f64>,
) -> Result<String> {
    let input = File::open(file)
        .with_context(|| format!("failed to open boundary pub input `{}`", file.display()))?;
    let reader = BufReader::new(input);
    let interval = freq_hz.map(|freq| Duration::from_secs_f64(1.0 / freq));
    let mut next_send = Instant::now();
    let mut sent = 0usize;
    let mut lines = Vec::new();

    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.with_context(|| {
            format!(
                "failed to read boundary pub input `{}` line {line_number}",
                file.display()
            )
        })?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let input = boundary_pub_input_from_jsonl_line(file, trimmed, line_number)?;
        pace_boundary_publish(interval, &mut next_send);
        lines.push(boundary_publish_one_input(
            endpoint,
            file,
            image,
            socket,
            published_at_ms,
            input,
        )?);
        sent += 1;
    }
    if sent == 0 {
        anyhow::bail!(
            "flowrt pub --file `{}` does not contain any JSON messages",
            file.display()
        );
    }
    lines.push(format!(
        "summary: endpoint={endpoint} sent={sent} source={}",
        file.display()
    ));
    Ok(lines.join("\n"))
}

fn boundary_publish_one_input(
    endpoint: &str,
    file: &Path,
    image: Option<&Path>,
    socket: Option<&Path>,
    published_at_ms: Option<u64>,
    input: BoundaryPubInput,
) -> Result<String> {
    boundary_publish(endpoint, &input.json, image, socket, published_at_ms).with_context(|| {
        format!(
            "failed to publish boundary input `{endpoint}` from {}",
            input.source.describe(file)
        )
    })
}

fn pace_boundary_publish(interval: Option<Duration>, next_send: &mut Instant) {
    if let Some(interval) = interval {
        let now = Instant::now();
        if *next_send > now {
            std::thread::sleep(*next_send - now);
        }
        *next_send += interval;
    }
}

fn boundary_pub_inputs_from_json_document(file: &Path) -> Result<Vec<BoundaryPubInput>> {
    let input = File::open(file)
        .with_context(|| format!("failed to open boundary pub input `{}`", file.display()))?;
    let value = serde_json::from_reader::<_, Value>(input)
        .with_context(|| format!("failed to parse `{}` as JSON", file.display()))?;
    match value {
        Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                Ok(BoundaryPubInput {
                    json: serde_json::to_string(&value).with_context(|| {
                        format!(
                            "failed to serialize `{}` array index {index}",
                            file.display()
                        )
                    })?,
                    source: BoundaryPubInputSource::JsonArrayIndex(index),
                })
            })
            .collect(),
        other => Ok(vec![BoundaryPubInput {
            json: serde_json::to_string(&other)
                .with_context(|| format!("failed to serialize `{}`", file.display()))?,
            source: BoundaryPubInputSource::SingleJsonValue,
        }]),
    }
}

fn boundary_pub_input_from_jsonl_line(
    file: &Path,
    raw: &str,
    line_number: usize,
) -> Result<BoundaryPubInput> {
    let value = serde_json::from_str::<Value>(raw).with_context(|| {
        format!(
            "flowrt pub --file JSONL entry `{}` line {line_number} must be valid JSON",
            file.display()
        )
    })?;
    Ok(BoundaryPubInput {
        json: serde_json::to_string(&value).with_context(|| {
            format!(
                "failed to serialize boundary pub input `{}` line {line_number}",
                file.display()
            )
        })?,
        source: BoundaryPubInputSource::JsonLine(line_number),
    })
}

fn is_jsonl_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
}

#[derive(Debug, Clone)]
struct BoundaryPublishSpec {
    name: String,
    message_type: String,
}

pub(crate) fn ensure_boundary_publish_endpoint(
    self_description: &SelfDescription,
    endpoint: &str,
    command: &str,
) -> Result<()> {
    find_boundary_publish_endpoint(self_description, endpoint, command).map(|_| ())
}

fn find_boundary_publish_endpoint(
    self_description: &SelfDescription,
    endpoint: &str,
    command: &str,
) -> Result<BoundaryPublishSpec> {
    ensure_island_boundary_publish_mode(self_description, command)?;
    let mut matches = Vec::new();
    for graph in &self_description.graphs {
        for boundary in &graph.boundary_endpoints {
            if boundary.name == endpoint {
                matches.push(boundary);
            }
        }
    }
    match matches.len() {
        0 => {
            if self_description.graphs.iter().any(|graph| {
                graph
                    .channels
                    .iter()
                    .any(|channel| channel.from == endpoint || channel.to == endpoint)
            }) {
                anyhow::bail!(
                    "FlowRT endpoint `{endpoint}` is a dataflow channel endpoint; {command} only writes typed boundary input"
                );
            }
            if self_description.graphs.iter().any(|graph| {
                graph
                    .services
                    .iter()
                    .any(|service| service.name == endpoint)
            }) {
                anyhow::bail!(
                    "FlowRT endpoint `{endpoint}` is a service endpoint; {command} only writes typed boundary input"
                );
            }
            if self_description.graphs.iter().any(|graph| {
                graph
                    .operations
                    .iter()
                    .any(|operation| operation.name == endpoint)
            }) {
                anyhow::bail!(
                    "FlowRT endpoint `{endpoint}` is an operation endpoint; {command} only writes typed boundary input"
                );
            }
            anyhow::bail!(
                "unknown FlowRT boundary input `{endpoint}`; {command} only writes typed boundary input"
            )
        }
        1 => {
            let boundary = matches.remove(0);
            match boundary.direction.as_str() {
                "input" => {}
                "output" => anyhow::bail!(
                    "FlowRT boundary endpoint `{endpoint}` is a boundary output; {command} only writes boundary input"
                ),
                other => anyhow::bail!(
                    "FlowRT boundary endpoint `{endpoint}` has unsupported direction `{other}`"
                ),
            }
            if boundary.message_type.is_empty() {
                anyhow::bail!("FlowRT boundary endpoint `{endpoint}` has empty message_type");
            }
            Ok(BoundaryPublishSpec {
                name: boundary.name.clone(),
                message_type: boundary.message_type.clone(),
            })
        }
        _ => anyhow::bail!(
            "FlowRT self-description contains multiple boundary endpoints named `{endpoint}`"
        ),
    }
}

pub(crate) fn ensure_island_boundary_publish_mode(
    self_description: &SelfDescription,
    command: &str,
) -> Result<()> {
    let has_island_profile = self_description
        .profiles
        .iter()
        .any(|profile| profile.mode == "island");
    let has_island_graph = self_description
        .graphs
        .iter()
        .any(|graph| graph.mode == "island");
    if !has_island_profile && !has_island_graph {
        anyhow::bail!(
            "FlowRT self-description is not island mode; {command} only writes island boundary input. Use an island profile or temporary island overlay for offline validation"
        );
    }
    Ok(())
}

fn encode_boundary_json(
    self_description: &SelfDescription,
    spec: &BoundaryPublishSpec,
    raw_json: &str,
) -> Result<Vec<u8>> {
    encode_message_json(self_description, &spec.name, &spec.message_type, raw_json)
}

fn publish_boundary_payload(
    socket: &Path,
    self_description_hash: &str,
    spec: &BoundaryPublishSpec,
    payload: Vec<u8>,
    published_at_ms: Option<u64>,
) -> Result<String> {
    let payload_len = payload.len();
    let response = flowrt::request_boundary_publish_with_timeout(
        socket,
        &spec.name,
        payload,
        published_at_ms,
        LOCAL_INTROSPECTION_TIMEOUT,
    )
    .with_context(|| {
        format!(
            "failed to publish boundary input `{}` via `{}`",
            spec.name,
            socket.display()
        )
    })?;
    let boundary = match response {
        flowrt::IntrospectionResponse::BoundaryPublish {
            handshake,
            boundary,
        } => {
            ensure_handshake_hash(&handshake, self_description_hash, socket)?;
            boundary
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to publish boundary input `{}` via `{}`: {message}",
                spec.name,
                socket.display()
            );
        }
        _ => anyhow::bail!(
            "runtime socket `{}` returned an unexpected introspection response",
            socket.display()
        ),
    };
    if boundary.message_type != spec.message_type {
        anyhow::bail!(
            "runtime boundary input `{}` type `{}` does not match self-description type `{}`",
            boundary.endpoint,
            boundary.message_type,
            spec.message_type
        );
    }
    let published_at_ms = boundary
        .published_at_ms
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    Ok(format!(
        "boundary={} type={} revision={} published_at_ms={} payload_len={}",
        boundary.endpoint, boundary.message_type, boundary.revision, published_at_ms, payload_len
    ))
}
