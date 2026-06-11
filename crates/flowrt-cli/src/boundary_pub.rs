use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use flowrt_selfdesc::{SelfDescription, SelfDescriptionFieldAbi, SelfDescriptionMessageAbi};
use serde_json::Value;

use crate::introspection::{
    LOCAL_INTROSPECTION_TIMEOUT, ensure_handshake_hash, load_echo_context_from_live_socket,
    load_self_description_with_hash, message_abi_layout, select_echo_socket,
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
    let value = serde_json::from_str::<Value>(raw_json)
        .with_context(|| format!("flowrt pub --json must be valid JSON; got `{raw_json}`"))?;
    let Some(message) = message_abi_layout(&self_description.message_abi, &spec.message_type)?
    else {
        if self_description
            .message_frames
            .iter()
            .any(|frame| frame.type_name == spec.message_type)
        {
            anyhow::bail!(
                "flowrt pub currently supports fixed Message ABI JSON only; boundary input `{}` type `{}` uses canonical frame layout",
                spec.name,
                spec.message_type
            );
        }
        anyhow::bail!(
            "FlowRT self-description does not contain Message ABI layout for boundary input `{}` type `{}`",
            spec.name,
            spec.message_type
        );
    };
    encode_fixed_message_json(&self_description.message_abi, message, &value).with_context(|| {
        format!(
            "failed to encode boundary input `{}` as `{}`",
            spec.name, spec.message_type
        )
    })
}

fn encode_fixed_message_json(
    messages: &[SelfDescriptionMessageAbi],
    message: &SelfDescriptionMessageAbi,
    value: &Value,
) -> Result<Vec<u8>> {
    if message.fields.is_empty() {
        if message.size_bytes == 0
            && (value.is_null() || value.as_object().is_some_and(|object| object.is_empty()))
        {
            return Ok(Vec::new());
        }
        anyhow::bail!(
            "Message ABI layout for `{}` has no fields; JSON encoding requires field metadata",
            message.type_name
        );
    }

    let mut payload = vec![0u8; message.size_bytes];
    match value {
        Value::Object(object) => {
            let expected = message
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<BTreeSet<_>>();
            for key in object.keys() {
                if !expected.contains(key.as_str()) {
                    anyhow::bail!("unknown field `{key}` for `{}`", message.type_name);
                }
            }
            for field in &message.fields {
                let field_value = object.get(&field.name).with_context(|| {
                    format!("missing field `{}` for `{}`", field.name, message.type_name)
                })?;
                encode_fixed_field(messages, &mut payload, field, field_value)?;
            }
        }
        _ if message.fields.len() == 1 => {
            encode_fixed_field(messages, &mut payload, &message.fields[0], value)?;
        }
        _ => anyhow::bail!(
            "JSON for `{}` must be an object with fields: {}",
            message.type_name,
            message
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
    Ok(payload)
}

fn encode_fixed_field(
    messages: &[SelfDescriptionMessageAbi],
    payload: &mut [u8],
    field: &SelfDescriptionFieldAbi,
    value: &Value,
) -> Result<()> {
    let start = field.offset_bytes;
    let end = start
        .checked_add(field.size_bytes)
        .with_context(|| format!("field `{}` byte range overflows usize", field.name))?;
    if start > payload.len() || end > payload.len() {
        anyhow::bail!(
            "field `{}` range {}..{} exceeds payload length {}",
            field.name,
            start,
            end,
            payload.len()
        );
    }
    let bytes = encode_fixed_value(messages, &field.ty, field.size_bytes, value, &field.name)?;
    payload[start..end].copy_from_slice(&bytes);
    Ok(())
}

fn encode_fixed_value(
    messages: &[SelfDescriptionMessageAbi],
    ty: &str,
    size_bytes: usize,
    value: &Value,
    path: &str,
) -> Result<Vec<u8>> {
    let ty = ty.trim();
    if let Some((element, len)) = parse_boundary_fixed_array_type(ty)? {
        let values = value
            .as_array()
            .with_context(|| format!("field `{path}` expects JSON array for `{ty}`"))?;
        if values.len() != len {
            anyhow::bail!(
                "field `{path}` expects array length {len} for `{ty}`, got {}",
                values.len()
            );
        }
        let element_size = boundary_fixed_wire_size(messages, element)?.with_context(|| {
            format!("field `{path}` uses unsupported fixed array element type `{element}`")
        })?;
        let expected = element_size
            .checked_mul(len)
            .with_context(|| format!("field `{path}` fixed array byte length overflows usize"))?;
        if expected != size_bytes {
            anyhow::bail!(
                "field `{path}` type `{ty}` expects {expected} bytes but self-description declares {size_bytes} bytes"
            );
        }
        let mut output = Vec::with_capacity(size_bytes);
        for (index, element_value) in values.iter().enumerate() {
            output.extend(encode_fixed_value(
                messages,
                element,
                element_size,
                element_value,
                &format!("{path}[{index}]"),
            )?);
        }
        return Ok(output);
    }

    if let Some(expected) = boundary_primitive_size(ty) {
        if expected != size_bytes {
            anyhow::bail!(
                "field `{path}` type `{ty}` expects {expected} bytes but self-description declares {size_bytes} bytes"
            );
        }
        return encode_primitive_value(ty, value, path);
    }

    if let Some(nested) = message_abi_layout(messages, ty)? {
        if nested.size_bytes != size_bytes {
            anyhow::bail!(
                "field `{path}` type `{ty}` expects {} bytes but self-description declares {size_bytes} bytes",
                nested.size_bytes
            );
        }
        return encode_fixed_message_json(messages, nested, value)
            .with_context(|| format!("field `{path}` expects nested `{ty}` object"));
    }

    anyhow::bail!("field `{path}` has unsupported fixed ABI type `{ty}`")
}

fn parse_boundary_fixed_array_type(ty: &str) -> Result<Option<(&str, usize)>> {
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

fn boundary_fixed_wire_size(
    messages: &[SelfDescriptionMessageAbi],
    ty: &str,
) -> Result<Option<usize>> {
    let ty = ty.trim();
    if let Some(size) = boundary_primitive_size(ty) {
        return Ok(Some(size));
    }
    if let Some((element, len)) = parse_boundary_fixed_array_type(ty)? {
        let Some(element_size) = boundary_fixed_wire_size(messages, element)? else {
            return Ok(None);
        };
        return Ok(Some(element_size.checked_mul(len).with_context(|| {
            format!("fixed array `{ty}` byte length overflows usize")
        })?));
    }
    if let Some(message) = message_abi_layout(messages, ty)? {
        return Ok(Some(message.size_bytes));
    }
    Ok(None)
}

fn boundary_primitive_size(ty: &str) -> Option<usize> {
    Some(match ty {
        "bool" | "u8" | "i8" => 1,
        "u16" | "i16" => 2,
        "u32" | "i32" | "f32" => 4,
        "u64" | "i64" | "f64" => 8,
        "u128" | "i128" => 16,
        _ => return None,
    })
}

fn encode_primitive_value(ty: &str, value: &Value, path: &str) -> Result<Vec<u8>> {
    Ok(match ty {
        "bool" => {
            vec![u8::from(value.as_bool().with_context(|| {
                format!("field `{path}` expects JSON boolean")
            })?)]
        }
        "u8" => u8::try_from(json_unsigned(value, path, u8::MAX as u128)?)?
            .to_le_bytes()
            .to_vec(),
        "u16" => u16::try_from(json_unsigned(value, path, u16::MAX as u128)?)?
            .to_le_bytes()
            .to_vec(),
        "u32" => u32::try_from(json_unsigned(value, path, u32::MAX as u128)?)?
            .to_le_bytes()
            .to_vec(),
        "u64" => u64::try_from(json_unsigned(value, path, u64::MAX as u128)?)?
            .to_le_bytes()
            .to_vec(),
        "u128" => json_unsigned(value, path, u128::MAX)?
            .to_le_bytes()
            .to_vec(),
        "i8" => i8::try_from(json_signed(value, path, i8::MIN as i128, i8::MAX as i128)?)?
            .to_le_bytes()
            .to_vec(),
        "i16" => i16::try_from(json_signed(
            value,
            path,
            i16::MIN as i128,
            i16::MAX as i128,
        )?)?
        .to_le_bytes()
        .to_vec(),
        "i32" => i32::try_from(json_signed(
            value,
            path,
            i32::MIN as i128,
            i32::MAX as i128,
        )?)?
        .to_le_bytes()
        .to_vec(),
        "i64" => i64::try_from(json_signed(
            value,
            path,
            i64::MIN as i128,
            i64::MAX as i128,
        )?)?
        .to_le_bytes()
        .to_vec(),
        "i128" => json_signed(value, path, i128::MIN, i128::MAX)?
            .to_le_bytes()
            .to_vec(),
        "f32" => {
            let value = json_float(value, path)? as f32;
            if !value.is_finite() {
                anyhow::bail!("field `{path}` expects finite f32 value");
            }
            value.to_le_bytes().to_vec()
        }
        "f64" => json_float(value, path)?.to_le_bytes().to_vec(),
        _ => anyhow::bail!("field `{path}` has unsupported fixed ABI type `{ty}`"),
    })
}

fn json_unsigned(value: &Value, path: &str, max: u128) -> Result<u128> {
    let parsed = match value {
        Value::Number(number) => number
            .as_u64()
            .map(u128::from)
            .with_context(|| format!("field `{path}` expects unsigned integer"))?,
        Value::String(raw) => raw
            .parse::<u128>()
            .with_context(|| format!("field `{path}` expects unsigned integer string"))?,
        _ => anyhow::bail!("field `{path}` expects unsigned integer"),
    };
    if parsed > max {
        anyhow::bail!("field `{path}` unsigned integer {parsed} exceeds max {max}");
    }
    Ok(parsed)
}

fn json_signed(value: &Value, path: &str, min: i128, max: i128) -> Result<i128> {
    let parsed = match value {
        Value::Number(number) => number
            .as_i64()
            .map(i128::from)
            .with_context(|| format!("field `{path}` expects signed integer"))?,
        Value::String(raw) => raw
            .parse::<i128>()
            .with_context(|| format!("field `{path}` expects signed integer string"))?,
        _ => anyhow::bail!("field `{path}` expects signed integer"),
    };
    if parsed < min || parsed > max {
        anyhow::bail!("field `{path}` signed integer {parsed} outside {min}..={max}");
    }
    Ok(parsed)
}

fn json_float(value: &Value, path: &str) -> Result<f64> {
    let value = value
        .as_f64()
        .with_context(|| format!("field `{path}` expects JSON number"))?;
    if !value.is_finite() {
        anyhow::bail!("field `{path}` expects finite floating-point value");
    }
    Ok(value)
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
