//! Message ABI / variable frame 字段格式化。
//!
//! 提供 fixed-size ABI 字段和 variable frame 字段的解码与格式化，供 `flowrt echo` 等
//! CLI 命令复用。所有函数均为纯函数，不依赖 runtime 或 CLI 状态。

use thiserror::Error;

use crate::schema::{SelfDescriptionFieldAbi, SelfDescriptionFrameField};

/// 格式化错误。
#[derive(Debug, Error)]
pub enum FormatError {
    #[error("field `{name}` range {start}..{end} exceeds payload length {payload_len}")]
    FieldRangeOverflow {
        name: String,
        start: usize,
        end: usize,
        payload_len: usize,
    },
    #[error("canonical frame header size {header_size} exceeds payload length {payload_len}")]
    HeaderOverflow {
        header_size: usize,
        payload_len: usize,
    },
    #[error("field `{name}` header range {start}..{end} exceeds frame header length {header_len}")]
    FrameFieldHeaderOverflow {
        name: String,
        start: usize,
        end: usize,
        header_len: usize,
    },
    #[error("variable field `{name}` header expects 8-byte VarSpan but has {actual} bytes")]
    VarSpanSize { name: String, actual: usize },
    #[error("field `{name}` length {len} exceeds self-description tail max {max}")]
    TailMaxOverflow {
        name: String,
        len: usize,
        max: usize,
    },
    #[error("field `{name}` tail range {start}..{end} exceeds tail length {tail_len}")]
    TailRangeOverflow {
        name: String,
        start: usize,
        end: usize,
        tail_len: usize,
    },
    #[error("field `{name}` is not valid UTF-8: {source}")]
    Utf8 {
        name: String,
        source: std::str::Utf8Error,
    },
    #[error("unsupported field type `{ty}`")]
    UnsupportedType { ty: String },
    #[error("unsupported fixed wire type `{ty}`")]
    UnsupportedFixedWireType { ty: String },
    #[error("unsupported fixed array element type `{ty}`")]
    UnsupportedFixedArrayElement { ty: String },
    #[error("unsupported sequence element type `{ty}`")]
    UnsupportedSequenceElement { ty: String },
    #[error("primitive `{ty}` expects {expected} bytes but payload field has {actual} bytes")]
    PrimitiveSizeMismatch {
        ty: String,
        expected: usize,
        actual: usize,
    },
    #[error("fixed array `{ty}` expects {expected} bytes but payload field has {actual} bytes")]
    FixedArraySizeMismatch {
        ty: String,
        expected: usize,
        actual: usize,
    },
    #[error("field `{name}` byte length {len} is not divisible by element size {element_size}")]
    SequenceNotDivisible {
        name: String,
        len: usize,
        element_size: usize,
    },
    #[error(
        "channel `{channel}` payload length {actual} does not match Message ABI size {expected} for `{ty}`"
    )]
    PayloadSizeMismatch {
        channel: String,
        actual: usize,
        expected: usize,
        ty: String,
    },
    #[error(
        "channel `{channel}` payload length {actual} exceeds canonical frame max size {max} for `{ty}`"
    )]
    PayloadExceedsFrameMax {
        channel: String,
        actual: usize,
        max: usize,
        ty: String,
    },
    #[error("legacy bounded sequence type `{ty}` is not supported")]
    LegacyBoundedSequence { ty: String },
    #[error("invalid fixed array type `{ty}`")]
    InvalidFixedArrayType { ty: String },
    #[error("invalid fixed array length in `{ty}`")]
    InvalidFixedArrayLength { ty: String },
    #[error("u32 wire value must contain exactly 4 bytes")]
    U32WireSize,
    #[error("failed to format string field `{name}`: {source}")]
    StringFormat {
        name: String,
        source: serde_json::Error,
    },
}

/// 格式化 fixed-size ABI 字段，返回 `name=value,...` 字符串。
pub fn format_fixed_abi_fields(
    fields: &[SelfDescriptionFieldAbi],
    payload: &[u8],
) -> Result<String, FormatError> {
    let mut formatted = Vec::new();
    for field in fields {
        let start = field.offset_bytes;
        let end = start + field.size_bytes;
        if start > payload.len() || field.size_bytes > payload.len().saturating_sub(start) {
            return Err(FormatError::FieldRangeOverflow {
                name: field.name.clone(),
                start,
                end,
                payload_len: payload.len(),
            });
        }
        let bytes = &payload[start..end];
        formatted.push(format!(
            "{}={}",
            field.name,
            format_fixed_abi_value(&field.ty, bytes)?
        ));
    }
    Ok(formatted.join(","))
}

/// 格式化 variable frame 字段，返回 `name=value,...` 字符串。
pub fn format_frame_fields(
    fields: &[SelfDescriptionFrameField],
    header_size_bytes: usize,
    payload: &[u8],
) -> Result<String, FormatError> {
    if payload.len() < header_size_bytes {
        return Err(FormatError::HeaderOverflow {
            header_size: header_size_bytes,
            payload_len: payload.len(),
        });
    }
    let (header, tail) = payload.split_at(header_size_bytes);
    let mut formatted = Vec::new();
    for field in fields {
        let start = field.header_offset_bytes;
        let end = start + field.header_size_bytes;
        if start > header.len() || field.header_size_bytes > header.len().saturating_sub(start) {
            return Err(FormatError::FrameFieldHeaderOverflow {
                name: field.name.clone(),
                start,
                end,
                header_len: header.len(),
            });
        }
        let bytes = &header[start..end];
        let value = format_frame_field_value(field, bytes, tail)?;
        formatted.push(format!("{}={value}", field.name));
    }
    Ok(formatted.join(","))
}

fn format_frame_field_value(
    field: &SelfDescriptionFrameField,
    header_bytes: &[u8],
    tail: &[u8],
) -> Result<String, FormatError> {
    let ty = field.ty.trim();
    if ty == "string" {
        let block = frame_tail_block(field, header_bytes, tail)?;
        let text = std::str::from_utf8(block).map_err(|source| FormatError::Utf8 {
            name: field.name.clone(),
            source,
        })?;
        return serde_json::to_string(text).map_err(|source| FormatError::StringFormat {
            name: field.name.clone(),
            source,
        });
    }
    if ty == "bytes" {
        let block = frame_tail_block(field, header_bytes, tail)?;
        return Ok(format!("0x{}", hex_bytes(block)));
    }
    if let Some(element_ty) = parse_sequence_type(ty)? {
        let element_size = required_fixed_wire_size(element_ty)?;
        let block = frame_tail_block(field, header_bytes, tail)?;
        if block.len() % element_size != 0 {
            return Err(FormatError::SequenceNotDivisible {
                name: field.name.clone(),
                len: block.len(),
                element_size,
            });
        }
        let element_count = block.len() / element_size;
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
) -> Result<&'a [u8], FormatError> {
    if header_bytes.len() != 8 {
        return Err(FormatError::VarSpanSize {
            name: field.name.clone(),
            actual: header_bytes.len(),
        });
    }
    let offset = read_u32_le(&header_bytes[0..4])? as usize;
    let len = read_u32_le(&header_bytes[4..8])? as usize;
    if let Some(tail_max_bytes) = field.tail_max_bytes {
        if len > tail_max_bytes {
            return Err(FormatError::TailMaxOverflow {
                name: field.name.clone(),
                len,
                max: tail_max_bytes,
            });
        }
    }
    if offset > tail.len() || len > tail.len().saturating_sub(offset) {
        return Err(FormatError::TailRangeOverflow {
            name: field.name.clone(),
            start: offset,
            end: offset.saturating_add(len),
            tail_len: tail.len(),
        });
    }
    Ok(&tail[offset..offset + len])
}

fn read_u32_le(bytes: &[u8]) -> Result<u32, FormatError> {
    let array: [u8; 4] = bytes.try_into().map_err(|_| FormatError::U32WireSize)?;
    Ok(u32::from_le_bytes(array))
}

fn parse_sequence_type(ty: &str) -> Result<Option<&str>, FormatError> {
    let Some(inner) = ty
        .strip_prefix("sequence<")
        .and_then(|value| value.strip_suffix('>'))
    else {
        return Ok(None);
    };
    if inner.contains(",max=") {
        return Err(FormatError::LegacyBoundedSequence { ty: ty.to_string() });
    }
    Ok(Some(inner.trim()))
}

fn format_fixed_abi_value(ty: &str, bytes: &[u8]) -> Result<String, FormatError> {
    let ty = ty.trim();
    if let Some((element, len)) = parse_fixed_array_type(ty)? {
        let element_size = required_fixed_wire_size(element)?;
        if bytes.len() != element_size * len {
            return Err(FormatError::FixedArraySizeMismatch {
                ty: ty.to_string(),
                expected: element_size * len,
                actual: bytes.len(),
            });
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

fn required_fixed_wire_size(ty: &str) -> Result<usize, FormatError> {
    fixed_wire_size(ty)?.ok_or_else(|| FormatError::UnsupportedFixedWireType { ty: ty.to_string() })
}

fn fixed_wire_size(ty: &str) -> Result<Option<usize>, FormatError> {
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

fn parse_fixed_array_type(ty: &str) -> Result<Option<(&str, usize)>, FormatError> {
    let Some(inner) = ty
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    else {
        return Ok(None);
    };
    let Some((element, len)) = inner.split_once(';') else {
        return Err(FormatError::InvalidFixedArrayType { ty: ty.to_string() });
    };
    let len = len
        .trim()
        .parse::<usize>()
        .map_err(|_| FormatError::InvalidFixedArrayLength { ty: ty.to_string() })?;
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

fn format_primitive_value(ty: &str, bytes: &[u8]) -> Result<String, FormatError> {
    let expected =
        primitive_size(ty).ok_or_else(|| FormatError::UnsupportedType { ty: ty.to_string() })?;
    if bytes.len() != expected {
        return Err(FormatError::PrimitiveSizeMismatch {
            ty: ty.to_string(),
            expected,
            actual: bytes.len(),
        });
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

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_field(name: &str, ty: &str, offset: usize, size: usize) -> SelfDescriptionFieldAbi {
        SelfDescriptionFieldAbi {
            name: name.to_string(),
            ty: ty.to_string(),
            offset_bytes: offset,
            size_bytes: size,
            align_bytes: 0,
        }
    }

    fn frame_field(
        name: &str,
        ty: &str,
        header_offset: usize,
        header_size: usize,
        tail_max: Option<usize>,
    ) -> SelfDescriptionFrameField {
        SelfDescriptionFrameField {
            name: name.to_string(),
            ty: ty.to_string(),
            header_offset_bytes: header_offset,
            header_size_bytes: header_size,
            tail_max_bytes: tail_max,
        }
    }

    #[test]
    fn format_single_u32_field() {
        let fields = [fixed_field("value", "u32", 0, 4)];
        let payload = [0x01, 0x00, 0x00, 0x00];
        let result = format_fixed_abi_fields(&fields, &payload).unwrap();
        assert_eq!(result, "value=1");
    }

    #[test]
    fn format_multiple_primitive_fields() {
        let fields = [
            fixed_field("x", "f32", 0, 4),
            fixed_field("y", "f32", 4, 4),
            fixed_field("ts", "u64", 8, 8),
        ];
        let mut payload = Vec::new();
        payload.extend_from_slice(&1.0f32.to_le_bytes());
        payload.extend_from_slice(&2.5f32.to_le_bytes());
        payload.extend_from_slice(&42u64.to_le_bytes());
        let result = format_fixed_abi_fields(&fields, &payload).unwrap();
        assert!(result.contains("x=1.0"));
        assert!(result.contains("y=2.5"));
        assert!(result.contains("ts=42"));
    }

    #[test]
    fn format_bool_field() {
        let fields = [fixed_field("flag", "bool", 0, 1)];
        assert_eq!(
            format_fixed_abi_fields(&fields, &[0x01]).unwrap(),
            "flag=true"
        );
        assert_eq!(
            format_fixed_abi_fields(&fields, &[0x00]).unwrap(),
            "flag=false"
        );
    }

    #[test]
    fn format_fixed_array_field() {
        let fields = [fixed_field("vals", "[u32;3]", 0, 12)];
        let mut payload = Vec::new();
        payload.extend_from_slice(&10u32.to_le_bytes());
        payload.extend_from_slice(&20u32.to_le_bytes());
        payload.extend_from_slice(&30u32.to_le_bytes());
        let result = format_fixed_abi_fields(&fields, &payload).unwrap();
        assert_eq!(result, "vals=[10,20,30]");
    }

    #[test]
    fn field_range_overflow_returns_error() {
        let fields = [fixed_field("x", "u32", 0, 4)];
        let payload = [0u8; 2];
        let err = format_fixed_abi_fields(&fields, &payload).unwrap_err();
        assert!(matches!(err, FormatError::FieldRangeOverflow { .. }));
    }

    #[test]
    fn format_frame_string_field() {
        let fields = [frame_field("name", "string", 0, 8, Some(256))];
        // VarSpan: offset=0, len=5
        let mut payload = Vec::new();
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&5u32.to_le_bytes());
        payload.extend_from_slice(b"hello");
        let result = format_frame_fields(&fields, 8, &payload).unwrap();
        assert_eq!(result, "name=\"hello\"");
    }

    #[test]
    fn format_frame_bytes_field() {
        let fields = [frame_field("data", "bytes", 0, 8, Some(256))];
        let mut payload = Vec::new();
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&3u32.to_le_bytes());
        payload.extend_from_slice(&[0xde, 0xad, 0xbe]);
        let result = format_frame_fields(&fields, 8, &payload).unwrap();
        assert_eq!(result, "data=0xdeadbe");
    }

    #[test]
    fn format_frame_sequence_field() {
        let fields = [frame_field("vals", "sequence<u32>", 0, 8, Some(256))];
        let mut payload = Vec::new();
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&8u32.to_le_bytes());
        payload.extend_from_slice(&100u32.to_le_bytes());
        payload.extend_from_slice(&200u32.to_le_bytes());
        let result = format_frame_fields(&fields, 8, &payload).unwrap();
        assert_eq!(result, "vals=[100,200]");
    }

    #[test]
    fn header_overflow_returns_error() {
        let fields = [frame_field("x", "u32", 0, 4, None)];
        let payload = [0u8; 2];
        let err = format_frame_fields(&fields, 4, &payload).unwrap_err();
        assert!(matches!(err, FormatError::HeaderOverflow { .. }));
    }

    #[test]
    fn unsupported_type_returns_error() {
        let fields = [fixed_field("x", "unknown_type", 0, 4)];
        let payload = [0u8; 4];
        let err = format_fixed_abi_fields(&fields, &payload).unwrap_err();
        assert!(matches!(err, FormatError::UnsupportedType { .. }));
    }

    #[test]
    fn legacy_bounded_sequence_returns_error() {
        let fields = [frame_field("x", "sequence<u32,max=10>", 0, 8, None)];
        let mut payload = Vec::new();
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());
        let err = format_frame_fields(&fields, 8, &payload).unwrap_err();
        assert!(matches!(err, FormatError::LegacyBoundedSequence { .. }));
    }

    #[test]
    fn format_f64_field() {
        let fields = [fixed_field("val", "f64", 0, 8)];
        let payload = 3.14f64.to_le_bytes();
        let result = format_fixed_abi_fields(&fields, &payload).unwrap();
        assert_eq!(result, "val=3.14");
    }

    #[test]
    fn format_i8_field() {
        let fields = [fixed_field("val", "i8", 0, 1)];
        let result = format_fixed_abi_fields(&fields, &[0xff]).unwrap();
        assert_eq!(result, "val=-1");
    }

    #[test]
    fn format_empty_fields_returns_empty_string() {
        let result = format_fixed_abi_fields(&[], &[0u8; 4]).unwrap();
        assert_eq!(result, "");
    }
}
