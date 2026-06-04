use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{IrError, Result};

/// 结构化消息类型表达式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeExpr {
    Primitive {
        name: PrimitiveType,
    },
    Named {
        name: String,
    },
    Array {
        element: Box<TypeExpr>,
        len: usize,
    },
    VarBytes {
        max_len: u32,
    },
    VarString {
        max_len: u32,
        encoding: StringEncoding,
    },
    VarSequence {
        element: Box<TypeExpr>,
        max_len: u32,
    },
}

/// Message ABI v0.1 支持的 fixed-size primitive 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveType {
    Bool,
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
}

/// 未来 Variable Frame ABI 中有界变长字符串的编码。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StringEncoding {
    Utf8,
}

impl TypeExpr {
    /// 返回类型表达式的 canonical RSDL 表示，用于诊断信息和生成器边界检查。
    pub fn canonical_syntax(&self) -> String {
        match self {
            TypeExpr::Primitive { name } => primitive_syntax(*name).to_string(),
            TypeExpr::Named { name } => name.clone(),
            TypeExpr::Array { element, len } => {
                format!("[{}; {}]", element.canonical_syntax(), len)
            }
            TypeExpr::VarBytes { max_len } => format!("bytes<max={max_len}>"),
            TypeExpr::VarString {
                max_len,
                encoding: StringEncoding::Utf8,
            } => {
                format!("string<max={max_len}>")
            }
            TypeExpr::VarSequence { element, max_len } => {
                format!("sequence<{},max={max_len}>", element.canonical_syntax())
            }
        }
    }

    /// 返回该类型当前需要的未来 ABI；Message ABI v0.1 调用方必须据此拒绝。
    pub fn required_future_abi(&self) -> Option<&'static str> {
        match self {
            TypeExpr::Primitive { .. } | TypeExpr::Named { .. } => None,
            TypeExpr::Array { element, .. } => element.required_future_abi(),
            TypeExpr::VarBytes { .. }
            | TypeExpr::VarString { .. }
            | TypeExpr::VarSequence { .. } => Some("Variable Frame ABI"),
        }
    }
}

/// 解析 RSDL 中的类型表达式。
pub fn parse_type_expr(source: &str) -> Result<TypeExpr> {
    TypeExpr::from_str(source)
}

impl FromStr for TypeExpr {
    type Err = IrError;

    fn from_str(source: &str) -> Result<Self> {
        parse_expr(source.trim()).map_err(|message| IrError::InvalidTypeExpr {
            expr: source.to_string(),
            message,
        })
    }
}

fn parse_expr(source: &str) -> std::result::Result<TypeExpr, String> {
    if source.is_empty() {
        return Err("empty type expression".to_string());
    }

    if source.starts_with('[') {
        return parse_array(source);
    }

    if source == "bytes" {
        return Err("bytes type must use `bytes<max=N>` with max > 0".to_string());
    }
    if let Some(args) = generic_args(source, "bytes") {
        let max_len = parse_max_arg(args, "bytes")?;
        return Ok(TypeExpr::VarBytes { max_len });
    }

    if source == "string" {
        return Err("string type must use `string<max=N>` with max > 0".to_string());
    }
    if let Some(args) = generic_args(source, "string") {
        let max_len = parse_max_arg(args, "string")?;
        return Ok(TypeExpr::VarString {
            max_len,
            encoding: StringEncoding::Utf8,
        });
    }

    if source == "sequence" {
        return Err("sequence type must use `sequence<T,max=N>` with max > 0".to_string());
    }
    if let Some(args) = generic_args(source, "sequence") {
        return parse_sequence(args);
    }

    if let Some(primitive) = parse_primitive(source) {
        return Ok(TypeExpr::Primitive { name: primitive });
    }

    if is_identifier(source) {
        return Ok(TypeExpr::Named {
            name: source.to_string(),
        });
    }

    Err("expected primitive, named type, or fixed array".to_string())
}

fn generic_args<'a>(source: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = source.strip_prefix(keyword)?;
    if !rest.starts_with('<') || !rest.ends_with('>') {
        return None;
    }
    Some(&rest[1..rest.len() - 1])
}

fn parse_max_arg(args: &str, type_name: &str) -> std::result::Result<u32, String> {
    let (key, value) = args
        .trim()
        .split_once('=')
        .ok_or_else(|| format!("{type_name} type must declare `max=N`"))?;
    if key.trim() != "max" {
        return Err(format!("{type_name} type must declare `max=N`"));
    }
    let max_len = value
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("{type_name} max must be a positive integer"))?;
    if max_len == 0 {
        return Err(format!("{type_name} max must be greater than zero"));
    }
    Ok(max_len)
}

fn parse_sequence(args: &str) -> std::result::Result<TypeExpr, String> {
    let comma = find_top_level_comma(args)
        .ok_or_else(|| "sequence type must use `sequence<T,max=N>` syntax".to_string())?;
    let element = args[..comma].trim();
    let max = args[comma + 1..].trim();
    if element.is_empty() {
        return Err("sequence element type is missing".to_string());
    }
    let max_len = parse_max_arg(max, "sequence")?;
    Ok(TypeExpr::VarSequence {
        element: Box::new(parse_expr(element)?),
        max_len,
    })
}

fn parse_array(source: &str) -> std::result::Result<TypeExpr, String> {
    if !source.ends_with(']') {
        return Err("array type must end with `]`".to_string());
    }

    let inner = &source[1..source.len() - 1];
    let semicolon = find_top_level_semicolon(inner)
        .ok_or_else(|| "array type must use `[T; N]` syntax".to_string())?;
    let element = inner[..semicolon].trim();
    let len = inner[semicolon + 1..].trim();

    if len.is_empty() {
        return Err("array length is missing".to_string());
    }

    let len = len
        .parse::<usize>()
        .map_err(|_| "array length must be a positive integer".to_string())?;
    if len == 0 {
        return Err("array length must be greater than zero".to_string());
    }

    Ok(TypeExpr::Array {
        element: Box::new(parse_expr(element)?),
        len,
    })
}

fn find_top_level_comma(source: &str) -> Option<usize> {
    let mut square_depth = 0usize;
    let mut angle_depth = 0usize;
    for (index, byte) in source.bytes().enumerate() {
        match byte {
            b'[' => square_depth += 1,
            b']' => square_depth = square_depth.saturating_sub(1),
            b'<' => angle_depth += 1,
            b'>' => angle_depth = angle_depth.saturating_sub(1),
            b',' if square_depth == 0 && angle_depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn find_top_level_semicolon(source: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, byte) in source.bytes().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => return Some(index),
            _ => {}
        }
    }
    None
}

fn primitive_syntax(primitive: PrimitiveType) -> &'static str {
    match primitive {
        PrimitiveType::Bool => "bool",
        PrimitiveType::U8 => "u8",
        PrimitiveType::U16 => "u16",
        PrimitiveType::U32 => "u32",
        PrimitiveType::U64 => "u64",
        PrimitiveType::U128 => "u128",
        PrimitiveType::I8 => "i8",
        PrimitiveType::I16 => "i16",
        PrimitiveType::I32 => "i32",
        PrimitiveType::I64 => "i64",
        PrimitiveType::I128 => "i128",
        PrimitiveType::F32 => "f32",
        PrimitiveType::F64 => "f64",
    }
}

fn parse_primitive(source: &str) -> Option<PrimitiveType> {
    match source {
        "bool" => Some(PrimitiveType::Bool),
        "u8" => Some(PrimitiveType::U8),
        "u16" => Some(PrimitiveType::U16),
        "u32" => Some(PrimitiveType::U32),
        "u64" => Some(PrimitiveType::U64),
        "u128" => Some(PrimitiveType::U128),
        "i8" => Some(PrimitiveType::I8),
        "i16" => Some(PrimitiveType::I16),
        "i32" => Some(PrimitiveType::I32),
        "i64" => Some(PrimitiveType::I64),
        "i128" => Some(PrimitiveType::I128),
        "f32" => Some(PrimitiveType::F32),
        "f64" => Some(PrimitiveType::F64),
        _ => None,
    }
}

fn is_identifier(source: &str) -> bool {
    let mut chars = source.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_primitives_named_types_and_arrays() {
        assert_eq!(
            parse_type_expr("f32").unwrap(),
            TypeExpr::Primitive {
                name: PrimitiveType::F32
            }
        );
        assert_eq!(
            parse_type_expr("Odom").unwrap(),
            TypeExpr::Named {
                name: "Odom".to_string()
            }
        );
        assert_eq!(
            parse_type_expr("[[u8; 4]; 2]").unwrap(),
            TypeExpr::Array {
                element: Box::new(TypeExpr::Array {
                    element: Box::new(TypeExpr::Primitive {
                        name: PrimitiveType::U8,
                    }),
                    len: 4,
                }),
                len: 2,
            }
        );
    }

    #[test]
    fn parses_bounded_variable_type_expressions_for_future_abi() {
        assert_eq!(
            parse_type_expr("bytes<max=262144>").unwrap(),
            TypeExpr::VarBytes { max_len: 262144 }
        );
        assert_eq!(
            parse_type_expr("string<max=64>").unwrap(),
            TypeExpr::VarString {
                max_len: 64,
                encoding: StringEncoding::Utf8,
            }
        );
        assert_eq!(
            parse_type_expr("sequence<u32,max=16>").unwrap(),
            TypeExpr::VarSequence {
                element: Box::new(TypeExpr::Primitive {
                    name: PrimitiveType::U32,
                }),
                max_len: 16,
            }
        );
        assert_eq!(
            parse_type_expr("sequence<[u8; 4], max=8>").unwrap(),
            TypeExpr::VarSequence {
                element: Box::new(TypeExpr::Array {
                    element: Box::new(TypeExpr::Primitive {
                        name: PrimitiveType::U8,
                    }),
                    len: 4,
                }),
                max_len: 8,
            }
        );
        assert_eq!(
            parse_type_expr("sequence<u32,max=16>")
                .unwrap()
                .canonical_syntax(),
            "sequence<u32,max=16>"
        );
        assert_eq!(
            parse_type_expr("sequence<[u8; 4], max=8>")
                .unwrap()
                .canonical_syntax(),
            "sequence<[u8; 4],max=8>"
        );
    }

    #[test]
    fn rejects_dynamic_or_malformed_type_expressions() {
        assert!(parse_type_expr("[u8]").is_err());
        assert!(parse_type_expr("[u8; 0]").is_err());
        assert!(parse_type_expr("Vec<u8>").is_err());
    }

    #[test]
    fn rejects_variable_type_expressions_without_positive_max() {
        for source in [
            "bytes",
            "bytes<>",
            "bytes<max=0>",
            "bytes<len=8>",
            "string<>",
            "string<max=0>",
            "sequence<u32>",
            "sequence<u32,max=0>",
            "sequence<max=4>",
        ] {
            assert!(
                parse_type_expr(source).is_err(),
                "{source} should require max > 0"
            );
        }
    }
}
