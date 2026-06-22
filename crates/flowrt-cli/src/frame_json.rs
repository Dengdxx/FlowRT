use std::collections::BTreeSet;

use anyhow::{Context, Result};
use flowrt_selfdesc::{
    SelfDescription, SelfDescriptionFieldAbi, SelfDescriptionFrameField, SelfDescriptionMessageAbi,
    SelfDescriptionMessageFrame,
};
use serde_json::{Map, Value};

use crate::introspection::message_abi_layout;

pub(crate) fn encode_boundary_json(
    self_description: &SelfDescription,
    endpoint_name: &str,
    message_type: &str,
    raw_json: &str,
) -> Result<Vec<u8>> {
    let value = serde_json::from_str::<Value>(raw_json)
        .with_context(|| format!("flowrt pub --json must be valid JSON; got `{raw_json}`"))?;

    if let Some(frame) = message_frame_layout(&self_description.message_frames, message_type)? {
        return encode_frame_message_json(self_description, frame, &value).map_err(|err| {
            anyhow::anyhow!(
                "failed to encode boundary input `{endpoint_name}` as `{message_type}`: {err}"
            )
        });
    }

    if let Some(message) = message_abi_layout(&self_description.message_abi, message_type)? {
        return encode_fixed_message_json(&self_description.message_abi, message, &value).map_err(
            |err| {
                anyhow::anyhow!(
                    "failed to encode boundary input `{endpoint_name}` as `{message_type}`: {err}"
                )
            },
        );
    }

    anyhow::bail!(
        "FlowRT self-description does not contain Message ABI or frame layout for boundary input `{endpoint_name}` type `{message_type}`"
    );
}

pub(crate) fn decode_message_json(
    self_description: &SelfDescription,
    message_type: &str,
    payload: &[u8],
) -> Result<String> {
    if let Some(message) = message_abi_layout(&self_description.message_abi, message_type)? {
        let value = decode_fixed_message_json(&self_description.message_abi, message, payload)?;
        return serde_json::to_string(&value)
            .with_context(|| format!("failed to format `{message_type}` JSON"));
    }

    anyhow::bail!(
        "FlowRT self-description does not contain Message ABI layout for `{message_type}`"
    )
}

pub(crate) fn message_frame_layout<'a>(
    frames: &'a [SelfDescriptionMessageFrame],
    message_type: &str,
) -> Result<Option<&'a SelfDescriptionMessageFrame>> {
    let mut layouts = frames
        .iter()
        .filter(|message| message.type_name == message_type)
        .collect::<Vec<_>>();
    match layouts.len() {
        0 => Ok(None),
        1 => Ok(Some(layouts.remove(0))),
        _ => anyhow::bail!(
            "FlowRT self-description contains multiple Message frame layouts for `{message_type}`"
        ),
    }
}

fn encode_frame_message_json(
    self_description: &SelfDescription,
    frame: &SelfDescriptionMessageFrame,
    value: &Value,
) -> Result<Vec<u8>> {
    if frame.encoding != "canonical_frame_v1" {
        anyhow::bail!(
            "message frame `{}` uses unsupported encoding `{}`",
            frame.type_name,
            frame.encoding
        );
    }
    let object = expect_json_object(value, &frame.type_name)?;
    validate_expected_fields(
        frame.fields.iter().map(|field| field.name.as_str()),
        object.keys().map(String::as_str),
        &frame.type_name,
    )?;

    let mut header = vec![0u8; frame.header_size_bytes];
    let mut tail = Vec::new();
    for field in &frame.fields {
        let field_value = object
            .get(&field.name)
            .with_context(|| format!("missing field `{}` for `{}`", field.name, frame.type_name))?;
        encode_frame_field(self_description, &mut header, &mut tail, field, field_value)?;
    }

    let total = frame
        .header_size_bytes
        .checked_add(tail.len())
        .with_context(|| format!("message frame `{}` size overflows usize", frame.type_name))?;
    if let Some(max_size_bytes) = frame.max_size_bytes
        && total > max_size_bytes
    {
        anyhow::bail!(
            "message frame `{}` encoded size {} exceeds self-description max {}",
            frame.type_name,
            total,
            max_size_bytes
        );
    }
    header.extend_from_slice(&tail);
    Ok(header)
}

fn encode_frame_field(
    self_description: &SelfDescription,
    header: &mut [u8],
    tail: &mut Vec<u8>,
    field: &SelfDescriptionFrameField,
    value: &Value,
) -> Result<()> {
    let start = field.header_offset_bytes;
    let end = start
        .checked_add(field.header_size_bytes)
        .with_context(|| format!("field `{}` header range overflows usize", field.name))?;
    if start > header.len() || end > header.len() {
        anyhow::bail!(
            "field `{}` header range {}..{} exceeds frame header length {}",
            field.name,
            start,
            end,
            header.len()
        );
    }

    let bytes = encode_frame_field_value(self_description, tail, field, value)?;
    if bytes.len() != field.header_size_bytes {
        anyhow::bail!(
            "field `{}` type `{}` expects {} header bytes but encoder produced {} bytes",
            field.name,
            field.ty,
            field.header_size_bytes,
            bytes.len()
        );
    }
    header[start..end].copy_from_slice(&bytes);
    Ok(())
}

fn encode_frame_field_value(
    self_description: &SelfDescription,
    tail: &mut Vec<u8>,
    field: &SelfDescriptionFrameField,
    value: &Value,
) -> Result<Vec<u8>> {
    let ty = field.ty.trim();
    if ty == "string" {
        let text = value
            .as_str()
            .with_context(|| format!("field `{}` expects JSON string", field.name))?;
        return encode_tail_span(tail, field, text.as_bytes());
    }
    if ty == "bytes" {
        let bytes = decode_bytes_json(value, &field.name)?;
        return encode_tail_span(tail, field, &bytes);
    }
    if let Some(element_ty) = parse_sequence_type(ty)? {
        let values = value
            .as_array()
            .with_context(|| format!("field `{}` expects JSON array for `{ty}`", field.name))?;
        let mut block = Vec::new();
        if let Some(element_size) =
            boundary_fixed_wire_size(&self_description.message_abi, element_ty)?
        {
            let total_size = element_size.checked_mul(values.len()).with_context(|| {
                format!(
                    "field `{}` sequence byte length overflows usize",
                    field.name
                )
            })?;
            block.reserve(total_size);
            for (index, element_value) in values.iter().enumerate() {
                block.extend(encode_fixed_value(
                    &self_description.message_abi,
                    element_ty,
                    element_size,
                    element_value,
                    &format!("{}[{index}]", field.name),
                )?);
            }
            return encode_tail_span(tail, field, &block);
        }
        anyhow::bail!(
            "field `{}` sequence element type `{}` lacks fixed Message ABI metadata",
            field.name,
            element_ty
        );
    }

    if let Some(expected) = boundary_primitive_size(ty) {
        if expected != field.header_size_bytes {
            anyhow::bail!(
                "field `{}` type `{}` expects {expected} header bytes but self-description declares {} bytes",
                field.name,
                ty,
                field.header_size_bytes
            );
        }
        return encode_primitive_value(ty, value, &field.name);
    }

    if let Some(nested_frame) = message_frame_layout(&self_description.message_frames, ty)? {
        if nested_frame.header_size_bytes != field.header_size_bytes {
            anyhow::bail!(
                "field `{}` type `{}` expects {} header bytes but self-description declares {} bytes",
                field.name,
                ty,
                nested_frame.header_size_bytes,
                field.header_size_bytes
            );
        }
        return encode_frame_message_json(self_description, nested_frame, value)
            .with_context(|| format!("field `{}` expects nested `{ty}` object", field.name));
    }

    if let Some(nested) = message_abi_layout(&self_description.message_abi, ty)? {
        if nested.size_bytes != field.header_size_bytes {
            anyhow::bail!(
                "field `{}` type `{}` expects {} header bytes but self-description declares {} bytes",
                field.name,
                ty,
                nested.size_bytes,
                field.header_size_bytes
            );
        }
        return encode_fixed_message_json(&self_description.message_abi, nested, value)
            .with_context(|| format!("field `{}` expects nested `{ty}` object", field.name));
    }

    anyhow::bail!(
        "field `{}` has unsupported canonical frame type `{ty}`",
        field.name
    )
}

fn encode_tail_span(
    tail: &mut Vec<u8>,
    field: &SelfDescriptionFrameField,
    bytes: &[u8],
) -> Result<Vec<u8>> {
    if let Some(max) = field.tail_max_bytes
        && bytes.len() > max
    {
        anyhow::bail!(
            "field `{}` length {} exceeds self-description tail max {}",
            field.name,
            bytes.len(),
            max
        );
    }
    let span = flowrt::append_tail_block(tail, bytes).map_err(|err| {
        anyhow::anyhow!(
            "field `{}` failed to build variable tail span: {err}",
            field.name
        )
    })?;
    let mut header_bytes = vec![0u8; flowrt::VAR_SPAN_WIRE_SIZE];
    span.encode(&mut header_bytes).map_err(|err| {
        anyhow::anyhow!(
            "field `{}` failed to encode variable tail span: {err}",
            field.name
        )
    })?;
    Ok(header_bytes)
}

fn decode_bytes_json(value: &Value, path: &str) -> Result<Vec<u8>> {
    match value {
        Value::String(text) => decode_base64(text, path),
        Value::Array(values) => {
            let mut bytes = Vec::with_capacity(values.len());
            for (index, element) in values.iter().enumerate() {
                let element_path = format!("{path}[{index}]");
                let byte = u8::try_from(json_unsigned(element, &element_path, u8::MAX as u128)?)?;
                bytes.push(byte);
            }
            Ok(bytes)
        }
        _ => anyhow::bail!("field `{path}` expects base64 string or JSON byte array"),
    }
}

fn decode_base64(text: &str, path: &str) -> Result<Vec<u8>> {
    let mut cleaned = String::with_capacity(text.len());
    for ch in text.chars() {
        if !ch.is_ascii_whitespace() {
            cleaned.push(ch);
        }
    }
    if cleaned.is_empty() {
        return Ok(Vec::new());
    }
    if cleaned.len() % 4 != 0 {
        anyhow::bail!("field `{path}` contains invalid base64 length");
    }

    let mut output = Vec::with_capacity((cleaned.len() / 4) * 3);
    for chunk in cleaned.as_bytes().chunks_exact(4) {
        let mut values = [0u8; 4];
        let mut padding = 0usize;
        for (index, byte) in chunk.iter().copied().enumerate() {
            values[index] = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                b'=' => {
                    padding += 1;
                    0
                }
                _ => anyhow::bail!("field `{path}` contains invalid base64 character"),
            };
            if byte == b'=' && index < 2 {
                anyhow::bail!("field `{path}` contains invalid base64 padding");
            }
            if padding > 0 && byte != b'=' {
                anyhow::bail!("field `{path}` contains invalid base64 padding");
            }
        }
        let triple = ((values[0] as u32) << 18)
            | ((values[1] as u32) << 12)
            | ((values[2] as u32) << 6)
            | (values[3] as u32);
        output.push(((triple >> 16) & 0xff) as u8);
        if chunk[2] != b'=' {
            output.push(((triple >> 8) & 0xff) as u8);
        }
        if chunk[3] != b'=' {
            output.push((triple & 0xff) as u8);
        }
    }
    Ok(output)
}

fn expect_json_object<'a>(
    value: &'a Value,
    type_name: &str,
) -> Result<&'a serde_json::Map<String, Value>> {
    match value {
        Value::Object(object) => Ok(object),
        _ => anyhow::bail!("JSON for `{type_name}` must be an object"),
    }
}

fn validate_expected_fields<'a>(
    expected: impl Iterator<Item = &'a str>,
    actual: impl Iterator<Item = &'a str>,
    type_name: &str,
) -> Result<()> {
    let expected = expected.collect::<BTreeSet<_>>();
    for key in actual {
        if !expected.contains(key) {
            anyhow::bail!("unknown field `{key}` for `{type_name}`");
        }
    }
    Ok(())
}

pub(crate) fn encode_fixed_message_json(
    messages: &[SelfDescriptionMessageAbi],
    message: &SelfDescriptionMessageAbi,
    value: &Value,
) -> Result<Vec<u8>> {
    if message.fields.is_empty() {
        if message.empty
            && message.size_bytes == 0
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
            validate_expected_fields(
                message.fields.iter().map(|field| field.name.as_str()),
                object.keys().map(String::as_str),
                &message.type_name,
            )?;
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

fn decode_fixed_message_json(
    messages: &[SelfDescriptionMessageAbi],
    message: &SelfDescriptionMessageAbi,
    payload: &[u8],
) -> Result<Value> {
    if payload.len() != message.size_bytes {
        anyhow::bail!(
            "payload for `{}` has {} bytes; Message ABI expects {}",
            message.type_name,
            payload.len(),
            message.size_bytes
        );
    }
    if message.fields.is_empty() {
        if message.empty && message.size_bytes == 0 {
            return Ok(Value::Object(Map::new()));
        }
        anyhow::bail!(
            "Message ABI layout for `{}` has no fields; JSON decoding requires field metadata",
            message.type_name
        );
    }

    let mut object = Map::new();
    for field in &message.fields {
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
        object.insert(
            field.name.clone(),
            decode_fixed_value(messages, &field.ty, &payload[start..end], &field.name)?,
        );
    }
    Ok(Value::Object(object))
}

fn decode_fixed_value(
    messages: &[SelfDescriptionMessageAbi],
    ty: &str,
    payload: &[u8],
    path: &str,
) -> Result<Value> {
    let ty = ty.trim();
    if let Some((element, len)) = parse_boundary_fixed_array_type(ty)? {
        let element_size = boundary_fixed_wire_size(messages, element)?.with_context(|| {
            format!("field `{path}` uses unsupported fixed array element type `{element}`")
        })?;
        let expected = element_size
            .checked_mul(len)
            .with_context(|| format!("field `{path}` fixed array byte length overflows usize"))?;
        if payload.len() != expected {
            anyhow::bail!(
                "field `{path}` type `{ty}` expects {expected} bytes but payload has {}",
                payload.len()
            );
        }
        let values = payload
            .chunks_exact(element_size)
            .enumerate()
            .map(|(index, chunk)| {
                decode_fixed_value(messages, element, chunk, &format!("{path}[{index}]"))
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(Value::Array(values));
    }

    if let Some(expected) = boundary_primitive_size(ty) {
        if payload.len() != expected {
            anyhow::bail!(
                "field `{path}` type `{ty}` expects {expected} bytes but payload has {}",
                payload.len()
            );
        }
        return decode_primitive_value(ty, payload, path);
    }

    if let Some(nested) = message_abi_layout(messages, ty)? {
        return decode_fixed_message_json(messages, nested, payload)
            .with_context(|| format!("field `{path}` expects nested `{ty}` object"));
    }

    anyhow::bail!("field `{path}` has unsupported fixed ABI type `{ty}`")
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

pub(crate) fn encode_fixed_value(
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

pub(crate) fn boundary_fixed_wire_size(
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

pub(crate) fn boundary_primitive_size(ty: &str) -> Option<usize> {
    Some(match ty {
        "bool" | "u8" | "i8" => 1,
        "u16" | "i16" => 2,
        "u32" | "i32" | "f32" => 4,
        "u64" | "i64" | "f64" => 8,
        "u128" | "i128" => 16,
        _ => return None,
    })
}

pub(crate) fn encode_primitive_value(ty: &str, value: &Value, path: &str) -> Result<Vec<u8>> {
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

fn decode_primitive_value(ty: &str, payload: &[u8], path: &str) -> Result<Value> {
    Ok(match ty {
        "bool" => match payload[0] {
            0 => Value::Bool(false),
            1 => Value::Bool(true),
            other => anyhow::bail!("field `{path}` has invalid bool byte {other}"),
        },
        "u8" => Value::from(payload[0]),
        "u16" => Value::from(u16::from_le_bytes(payload.try_into()?)),
        "u32" => Value::from(u32::from_le_bytes(payload.try_into()?)),
        "u64" => Value::from(u64::from_le_bytes(payload.try_into()?)),
        "i8" => Value::from(i8::from_le_bytes(payload.try_into()?)),
        "i16" => Value::from(i16::from_le_bytes(payload.try_into()?)),
        "i32" => Value::from(i32::from_le_bytes(payload.try_into()?)),
        "i64" => Value::from(i64::from_le_bytes(payload.try_into()?)),
        "f32" => Value::from(f32::from_le_bytes(payload.try_into()?) as f64),
        "f64" => Value::from(f64::from_le_bytes(payload.try_into()?)),
        "u128" => Value::String(u128::from_le_bytes(payload.try_into()?).to_string()),
        "i128" => Value::String(i128::from_le_bytes(payload.try_into()?).to_string()),
        _ => anyhow::bail!("field `{path}` has unsupported primitive type `{ty}`"),
    })
}

pub(crate) fn json_unsigned(value: &Value, path: &str, max: u128) -> Result<u128> {
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

pub(crate) fn json_signed(value: &Value, path: &str, min: i128, max: i128) -> Result<i128> {
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

pub(crate) fn json_float(value: &Value, path: &str) -> Result<f64> {
    let value = value
        .as_f64()
        .with_context(|| format!("field `{path}` expects JSON number"))?;
    if !value.is_finite() {
        anyhow::bail!("field `{path}` expects finite floating-point value");
    }
    Ok(value)
}

fn parse_sequence_type(ty: &str) -> Result<Option<&str>> {
    let Some(inner) = ty
        .strip_prefix("sequence<")
        .and_then(|value| value.strip_suffix('>'))
    else {
        return Ok(None);
    };
    if inner.contains(",max=") {
        anyhow::bail!("legacy bounded sequence type `{ty}` is not supported");
    }
    Ok(Some(inner.trim()))
}
