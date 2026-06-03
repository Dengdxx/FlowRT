use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::{IrError, Result};

/// 结构化消息类型表达式。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeExpr {
    Primitive { name: PrimitiveType },
    Named { name: String },
    Array { element: Box<TypeExpr>, len: usize },
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
    fn rejects_dynamic_or_malformed_type_expressions() {
        assert!(parse_type_expr("[u8]").is_err());
        assert!(parse_type_expr("[u8; 0]").is_err());
        assert!(parse_type_expr("Vec<u8>").is_err());
    }
}
