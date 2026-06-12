use std::path::Path;

use anyhow::{Context, Result};
use flowrt_selfdesc::SelfDescription;

use crate::frame_json::encode_boundary_json as encode_message_json;
use crate::introspection::{
    LOCAL_INTROSPECTION_TIMEOUT, ensure_handshake_hash, load_echo_context_from_live_socket,
    load_self_description_with_hash, select_echo_socket,
};

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
            let spec = find_boundary_publish_endpoint(&self_description, endpoint)?;
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
    let spec = find_boundary_publish_endpoint(&self_description, endpoint)?;
    let payload = encode_boundary_json(&self_description, &spec, json)?;
    publish_boundary_payload(
        &socket,
        &self_description_hash,
        &spec,
        payload,
        published_at_ms,
    )
}

#[derive(Debug, Clone)]
struct BoundaryPublishSpec {
    name: String,
    message_type: String,
}

fn find_boundary_publish_endpoint(
    self_description: &SelfDescription,
    endpoint: &str,
) -> Result<BoundaryPublishSpec> {
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
            "FlowRT self-description is not island mode; flowrt pub only writes island boundary input"
        );
    }

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
            anyhow::bail!("FlowRT self-description does not contain boundary endpoint `{endpoint}`")
        }
        1 => {
            let boundary = matches.remove(0);
            match boundary.direction.as_str() {
                "input" => {}
                "output" => anyhow::bail!(
                    "FlowRT boundary endpoint `{endpoint}` is a boundary output; flowrt pub only writes boundary input"
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
