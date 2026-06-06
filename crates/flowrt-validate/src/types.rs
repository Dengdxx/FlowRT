use std::collections::{BTreeMap, BTreeSet};

use flowrt_conformance::{AbiError, message_abi_expectations};
use flowrt_ir::{ContractIr, TypeExpr};

use crate::ValidationError;

pub(crate) fn validate_message_abi(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    if let Err(error) = message_abi_expectations(ir) {
        if matches!(
            error,
            AbiError::UnsupportedFutureType { .. } | AbiError::RecursiveType { .. }
        ) {
            return;
        }
        errors.push(ValidationError::new(format!(
            "message ABI v0.1 violation: {error}"
        )));
    }
}

pub(crate) fn validate_message_types(
    ir: &ContractIr,
    type_names: &BTreeSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    for ty in &ir.types {
        if ty.fields.is_empty() {
            errors.push(ValidationError::new(format!(
                "type `{}` must declare at least one field",
                ty.name
            )));
        }

        let mut fields = BTreeSet::new();
        for field in &ty.fields {
            if !fields.insert(field.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "type `{}` has duplicate field `{}`",
                    ty.name, field.name
                )));
            }
            validate_type_expr(
                &field.ty,
                type_names,
                &format!("type `{}` field `{}`", ty.name, field.name),
                errors,
            );
        }
    }
}

pub(crate) fn validate_variable_frame_shapes(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let types_by_name = ir
        .types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();

    for ty in &ir.types {
        for field in &ty.fields {
            let context = format!("type `{}` field `{}`", ty.name, field.name);
            match &field.ty {
                TypeExpr::VarBytes | TypeExpr::VarString { .. } => {}
                TypeExpr::VarSequence { element, .. } => {
                    if type_expr_contains_variable_data(element, &types_by_name) {
                        errors.push(ValidationError::new(format!(
                            "{context} has a variable-length sequence element; sequence elements must be fixed-size"
                        )));
                    }
                }
                expr if type_expr_contains_variable_data(expr, &types_by_name) => {
                    errors.push(ValidationError::new(format!(
                        "{context} nests variable data; variable data is only supported as a top-level message field"
                    )));
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn type_expr_contains_variable_data(
    expr: &TypeExpr,
    types_by_name: &BTreeMap<&str, &flowrt_ir::TypeIr>,
) -> bool {
    type_expr_contains_variable_data_inner(expr, types_by_name, &mut BTreeSet::new())
}

fn type_expr_contains_variable_data_inner(
    expr: &TypeExpr,
    types_by_name: &BTreeMap<&str, &flowrt_ir::TypeIr>,
    visiting: &mut BTreeSet<String>,
) -> bool {
    match expr {
        TypeExpr::Primitive { .. } => false,
        TypeExpr::VarBytes | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => true,
        TypeExpr::Array { element, .. } => {
            type_expr_contains_variable_data_inner(element, types_by_name, visiting)
        }
        TypeExpr::Named { name } => {
            if !visiting.insert(name.clone()) {
                return false;
            }
            let contains_variable = types_by_name.get(name.as_str()).is_some_and(|ty| {
                ty.fields.iter().any(|field| {
                    type_expr_contains_variable_data_inner(&field.ty, types_by_name, visiting)
                })
            });
            visiting.remove(name);
            contains_variable
        }
    }
}

pub(crate) fn validate_message_type_cycles(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let types_by_name = ir
        .types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    let mut visited = BTreeSet::new();
    let mut visiting = Vec::new();
    let mut recursive_types = BTreeSet::new();

    for ty in &ir.types {
        collect_recursive_message_types(
            &ty.name,
            &types_by_name,
            &mut visited,
            &mut visiting,
            &mut recursive_types,
        );
    }

    for type_name in recursive_types {
        errors.push(ValidationError::new(format!(
            "message ABI v0.1 violation: recursive message type `{type_name}` detected"
        )));
    }
}

fn collect_recursive_message_types(
    type_name: &str,
    types_by_name: &BTreeMap<&str, &flowrt_ir::TypeIr>,
    visited: &mut BTreeSet<String>,
    visiting: &mut Vec<String>,
    recursive_types: &mut BTreeSet<String>,
) {
    if let Some(cycle_start) = visiting.iter().position(|name| name == type_name) {
        recursive_types.extend(visiting[cycle_start..].iter().cloned());
        return;
    }
    if visited.contains(type_name) {
        return;
    }

    let Some(ty) = types_by_name.get(type_name) else {
        return;
    };
    visiting.push(type_name.to_string());
    for field in &ty.fields {
        collect_recursive_types_from_expr(
            &field.ty,
            types_by_name,
            visited,
            visiting,
            recursive_types,
        );
    }
    visiting.pop();
    visited.insert(type_name.to_string());
}

fn collect_recursive_types_from_expr(
    expr: &TypeExpr,
    types_by_name: &BTreeMap<&str, &flowrt_ir::TypeIr>,
    visited: &mut BTreeSet<String>,
    visiting: &mut Vec<String>,
    recursive_types: &mut BTreeSet<String>,
) {
    match expr {
        TypeExpr::Named { name } => {
            collect_recursive_message_types(name, types_by_name, visited, visiting, recursive_types)
        }
        TypeExpr::Array { element, .. } | TypeExpr::VarSequence { element, .. } => {
            collect_recursive_types_from_expr(
                element,
                types_by_name,
                visited,
                visiting,
                recursive_types,
            );
        }
        TypeExpr::Primitive { .. } | TypeExpr::VarBytes | TypeExpr::VarString { .. } => {}
    }
}

pub(crate) fn validate_type_expr(
    expr: &TypeExpr,
    type_names: &BTreeSet<&str>,
    context: &str,
    errors: &mut Vec<ValidationError>,
) {
    match expr {
        TypeExpr::Primitive { .. } => {}
        TypeExpr::Named { name } => {
            if !type_names.contains(name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "{context} references unknown type `{name}`"
                )));
            }
        }
        TypeExpr::Array { element, len } => {
            if *len == 0 {
                errors.push(ValidationError::new(format!(
                    "{context} has zero-length array"
                )));
            }
            validate_type_expr(element, type_names, context, errors);
        }
        TypeExpr::VarBytes | TypeExpr::VarString { .. } => {}
        TypeExpr::VarSequence { element } => {
            validate_type_expr(element, type_names, context, errors);
        }
    }
}
