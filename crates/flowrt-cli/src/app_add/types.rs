use std::collections::BTreeSet;

use anyhow::{Context, Result};
use flowrt_ir::{PrimitiveType, TypeExpr, parse_type_expr};

use super::names::{generated_type_name, validate_snake_case};

#[derive(Debug, Clone)]
pub(super) struct PortSpec {
    pub(super) name: String,
    pub(super) ty: String,
}

pub(super) fn parse_field_specs(fields: &[String]) -> Result<Vec<PortSpec>> {
    let specs = parse_colon_specs(fields, "field")?;
    let mut names = BTreeSet::new();
    for spec in &specs {
        validate_snake_case(&spec.name, "field name")?;
        validate_type_expr(&spec.ty)?;
        if !names.insert(spec.name.as_str()) {
            anyhow::bail!("field `{}` is declared more than once", spec.name);
        }
    }
    Ok(specs)
}

pub(super) fn parse_port_specs(values: &[String], kind: &str) -> Result<Vec<PortSpec>> {
    let specs = parse_colon_specs(values, kind)?;
    let mut names = BTreeSet::new();
    for spec in &specs {
        validate_snake_case(&spec.name, &format!("{kind} port name"))?;
        validate_type_expr(&spec.ty)?;
        if !names.insert(spec.name.as_str()) {
            anyhow::bail!("{kind} port `{}` is declared more than once", spec.name);
        }
    }
    Ok(specs)
}

fn parse_colon_specs(values: &[String], kind: &str) -> Result<Vec<PortSpec>> {
    values
        .iter()
        .map(|value| {
            let Some((name, ty)) = value.split_once(':') else {
                anyhow::bail!("{kind} spec must use `field:type`, got `{value}`");
            };
            let name = name.trim();
            let ty = ty.trim();
            if name.is_empty() || ty.is_empty() {
                anyhow::bail!("{kind} spec must use non-empty `field:type`, got `{value}`");
            }
            Ok(PortSpec {
                name: name.to_string(),
                ty: ty.to_string(),
            })
        })
        .collect()
}

fn validate_type_expr(source: &str) -> Result<()> {
    parse_type_expr(source)
        .map(|_| ())
        .with_context(|| format!("invalid type `{source}`"))
}

pub(super) fn named_rust_message_imports(
    inputs: &[PortSpec],
    outputs: &[PortSpec],
) -> Result<Vec<String>> {
    let mut imports = BTreeSet::new();
    for port in inputs.iter().chain(outputs.iter()) {
        collect_named_rust_type(&parse_type_expr(&port.ty)?, &mut imports);
    }
    Ok(imports.into_iter().collect())
}

fn collect_named_rust_type(expr: &TypeExpr, imports: &mut BTreeSet<String>) {
    match expr {
        TypeExpr::Named { name } => {
            imports.insert(generated_type_name(name));
        }
        TypeExpr::Array { element, .. } | TypeExpr::VarSequence { element } => {
            collect_named_rust_type(element, imports);
        }
        TypeExpr::Primitive { .. } | TypeExpr::VarBytes | TypeExpr::VarString { .. } => {}
    }
}

pub(super) fn parse_type(source: &str) -> Result<TypeExpr> {
    parse_type_expr(source).with_context(|| format!("invalid type `{source}`"))
}

pub(super) fn rust_type(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive { name } => rust_primitive(*name).to_string(),
        TypeExpr::Named { name } => generated_type_name(name),
        TypeExpr::Array { element, len } => format!("[{}; {len}]", rust_type(element)),
        TypeExpr::VarBytes => "Vec<u8>".to_string(),
        TypeExpr::VarString { .. } => "String".to_string(),
        TypeExpr::VarSequence { element } => format!("Vec<{}>", rust_type(element)),
    }
}

pub(super) fn cpp_type(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive { name } => cpp_primitive(*name).to_string(),
        TypeExpr::Named { name } => format!("flowrt_app::{}", generated_type_name(name)),
        TypeExpr::Array { element, len } => format!("std::array<{}, {len}>", cpp_type(element)),
        TypeExpr::VarBytes => "std::vector<std::uint8_t>".to_string(),
        TypeExpr::VarString { .. } => "std::string".to_string(),
        TypeExpr::VarSequence { element } => format!("std::vector<{}>", cpp_type(element)),
    }
}

fn rust_primitive(ty: PrimitiveType) -> &'static str {
    match ty {
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

fn cpp_primitive(ty: PrimitiveType) -> &'static str {
    match ty {
        PrimitiveType::Bool => "bool",
        PrimitiveType::U8 => "std::uint8_t",
        PrimitiveType::U16 => "std::uint16_t",
        PrimitiveType::U32 => "std::uint32_t",
        PrimitiveType::U64 => "std::uint64_t",
        PrimitiveType::U128 => "unsigned __int128",
        PrimitiveType::I8 => "std::int8_t",
        PrimitiveType::I16 => "std::int16_t",
        PrimitiveType::I32 => "std::int32_t",
        PrimitiveType::I64 => "std::int64_t",
        PrimitiveType::I128 => "__int128",
        PrimitiveType::F32 => "float",
        PrimitiveType::F64 => "double",
    }
}
