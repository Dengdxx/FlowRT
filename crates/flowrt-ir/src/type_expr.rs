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
        max_len: Option<u32>,
    },
    VarString {
        encoding: StringEncoding,
        max_len: Option<u32>,
    },
    VarSequence {
        element: Box<TypeExpr>,
        max_len: Option<u32>,
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

/// Variable Frame ABI 中变长字符串的编码。
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
            TypeExpr::VarBytes { max_len } => match max_len {
                Some(n) => format!("bytes<max={n}>"),
                None => "bytes".to_string(),
            },
            TypeExpr::VarString {
                encoding: StringEncoding::Utf8,
                max_len,
            } => match max_len {
                Some(n) => format!("string<max={n}>"),
                None => "string".to_string(),
            },
            TypeExpr::VarSequence { element, max_len } => match max_len {
                Some(n) => format!("sequence<{},max={n}>", element.canonical_syntax()),
                None => format!("sequence<{}>", element.canonical_syntax()),
            },
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
        return Ok(TypeExpr::VarBytes { max_len: None });
    }
    if let Some(args) = generic_args(source, "bytes") {
        return Ok(TypeExpr::VarBytes {
            max_len: Some(parse_max_arg(args, "bytes")?),
        });
    }

    if source == "string" {
        return Ok(TypeExpr::VarString {
            encoding: StringEncoding::Utf8,
            max_len: None,
        });
    }
    if let Some(args) = generic_args(source, "string") {
        return Ok(TypeExpr::VarString {
            encoding: StringEncoding::Utf8,
            max_len: Some(parse_max_arg(args, "string")?),
        });
    }

    if source == "sequence" {
        return Err("sequence type must use `sequence<T>` syntax".to_string());
    }
    if let Some(args) = generic_args(source, "sequence") {
        return parse_sequence(args);
    }

    if let Some(primitive) = parse_primitive(source) {
        return Ok(TypeExpr::Primitive { name: primitive });
    }

    if is_qualified_identifier(source) {
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

fn parse_sequence(args: &str) -> std::result::Result<TypeExpr, String> {
    if let Some(comma) = find_top_level_comma(args) {
        let element = args[..comma].trim();
        if element.is_empty() {
            return Err("sequence element type is missing".to_string());
        }
        let max_len = parse_max_arg(args[comma + 1..].trim(), "sequence")?;
        return Ok(TypeExpr::VarSequence {
            element: Box::new(parse_expr(element)?),
            max_len: Some(max_len),
        });
    }
    let element = args.trim();
    if element.is_empty() {
        return Err("sequence element type is missing".to_string());
    }
    Ok(TypeExpr::VarSequence {
        element: Box::new(parse_expr(element)?),
        max_len: None,
    })
}

/// 解析 `max=N` 有界变长参数；N 必须是正整数。
fn parse_max_arg(args: &str, type_name: &str) -> std::result::Result<u32, String> {
    let (key, value) = args
        .split_once('=')
        .ok_or_else(|| format!("{type_name} bounded form must declare `max=N`"))?;
    if key.trim() != "max" {
        return Err(format!("{type_name} bounded form must declare `max=N`"));
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

fn is_qualified_identifier(source: &str) -> bool {
    let mut parts = source.split("::");
    let Some(first) = parts.next() else {
        return false;
    };
    if !is_identifier(first) {
        return false;
    }
    let mut count = 1usize;
    for part in parts {
        count += 1;
        if !is_identifier(part) {
            return false;
        }
    }
    count <= 2
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
    fn parses_unbounded_variable_type_expressions_for_variable_frame_abi() {
        assert_eq!(
            parse_type_expr("bytes").unwrap(),
            TypeExpr::VarBytes { max_len: None }
        );
        assert_eq!(
            parse_type_expr("string").unwrap(),
            TypeExpr::VarString {
                encoding: StringEncoding::Utf8,
                max_len: None,
            }
        );
        assert_eq!(
            parse_type_expr("sequence<u32>").unwrap(),
            TypeExpr::VarSequence {
                element: Box::new(TypeExpr::Primitive {
                    name: PrimitiveType::U32,
                }),
                max_len: None,
            }
        );
        assert_eq!(
            parse_type_expr("sequence<[u8; 4]>").unwrap(),
            TypeExpr::VarSequence {
                element: Box::new(TypeExpr::Array {
                    element: Box::new(TypeExpr::Primitive {
                        name: PrimitiveType::U8,
                    }),
                    len: 4,
                }),
                max_len: None,
            }
        );
        assert_eq!(
            parse_type_expr("sequence<u32>").unwrap().canonical_syntax(),
            "sequence<u32>"
        );
        assert_eq!(
            parse_type_expr("sequence<[u8; 4]>")
                .unwrap()
                .canonical_syntax(),
            "sequence<[u8; 4]>"
        );
    }

    #[test]
    fn rejects_dynamic_or_malformed_type_expressions() {
        assert!(parse_type_expr("[u8]").is_err());
        assert!(parse_type_expr("[u8; 0]").is_err());
        assert!(parse_type_expr("Vec<u8>").is_err());
    }

    #[test]
    fn accepts_bounded_variable_type_expressions() {
        assert_eq!(
            parse_type_expr("bytes<max=32>").unwrap(),
            TypeExpr::VarBytes { max_len: Some(32) }
        );
        assert_eq!(
            parse_type_expr("string<max=64>").unwrap(),
            TypeExpr::VarString {
                encoding: StringEncoding::Utf8,
                max_len: Some(64),
            }
        );
        assert_eq!(
            parse_type_expr("sequence<u32,max=8>").unwrap(),
            TypeExpr::VarSequence {
                element: Box::new(TypeExpr::Primitive {
                    name: PrimitiveType::U32,
                }),
                max_len: Some(8),
            }
        );
        // canonical round-trip
        for source in ["bytes<max=32>", "string<max=64>", "sequence<u32,max=8>"] {
            assert_eq!(
                parse_type_expr(source).unwrap().canonical_syntax(),
                source,
                "{source} must round-trip"
            );
        }
        // 嵌套元素无界仍可解析（有界性由 IR 谓词判定，非 parser 拒绝）
        assert_eq!(
            parse_type_expr("sequence<bytes,max=8>").unwrap(),
            TypeExpr::VarSequence {
                element: Box::new(TypeExpr::VarBytes { max_len: None }),
                max_len: Some(8),
            }
        );
    }

    #[test]
    fn rejects_malformed_bounded_variable_type_expressions() {
        for source in [
            "bytes<>",
            "bytes<max=0>",
            "bytes<len=8>",
            "string<>",
            "string<max=0>",
            "sequence<u32,max=0>",
            "sequence<max=4>",
        ] {
            assert!(
                parse_type_expr(source).is_err(),
                "{source} should reject malformed bounded variable syntax"
            );
        }
    }
}
