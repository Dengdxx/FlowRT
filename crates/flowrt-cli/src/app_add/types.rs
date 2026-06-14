use std::collections::BTreeSet;

use anyhow::{Context, Result};
use flowrt_ir::parse_type_expr;

use super::names::validate_snake_case;

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
