use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{ComponentIr, ComponentKind, ContractIr, ParamType, ParamUpdatePolicy, TypeExpr};

use crate::ValidationError;
use crate::types::{type_expr_contains_variable_data, validate_type_expr};

pub(crate) fn validate_components(
    ir: &ContractIr,
    type_names: &BTreeSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    let types_by_name = ir
        .types
        .iter()
        .map(|ty| (ty.qualified_name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();

    for component in &ir.components {
        if component.kind == ComponentKind::External {
            errors.push(ValidationError::new(format!(
                "component `{}` uses external process kind, which is not supported by Contract IR v0.1 runtime shell",
                component.name
            )));
        }

        let mut ports = BTreeSet::new();
        for port in component.inputs.iter().chain(component.outputs.iter()) {
            if !ports.insert(port.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "component `{}` has duplicate port `{}`",
                    component.name, port.name
                )));
            }
            validate_type_expr(
                &port.ty,
                type_names,
                &format!("component `{}` port `{}`", component.name, port.name),
                errors,
            );
            if !matches!(port.ty, TypeExpr::Named { .. })
                && type_expr_contains_variable_data(&port.ty, &types_by_name)
            {
                errors.push(ValidationError::new(format!(
                    "component `{}` port `{}` uses variable data directly; variable data must be declared as a top-level field of a named message type",
                    component.name, port.name
                )));
            }
        }

        let mut service_clients = BTreeSet::new();
        for port in &component.service_clients {
            if !service_clients.insert(port.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "component `{}` has duplicate service client `{}`",
                    component.name, port.name
                )));
            }
            validate_service_port_types(
                component,
                "service client",
                port,
                type_names,
                &types_by_name,
                errors,
            );
        }

        let mut service_servers = BTreeSet::new();
        for port in &component.service_servers {
            if !service_servers.insert(port.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "component `{}` has duplicate service server `{}`",
                    component.name, port.name
                )));
            }
            validate_service_port_types(
                component,
                "service server",
                port,
                type_names,
                &types_by_name,
                errors,
            );
        }

        let mut params = BTreeSet::new();
        for param in &component.params {
            if !params.insert(param.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "component `{}` has duplicate param `{}`",
                    component.name, param.name
                )));
            }
            validate_param_schema(component, param, errors);
        }
    }
}

fn validate_service_port_types(
    component: &ComponentIr,
    label: &'static str,
    port: &flowrt_ir::ServicePortIr,
    type_names: &BTreeSet<&str>,
    types_by_name: &BTreeMap<&str, &flowrt_ir::TypeIr>,
    errors: &mut Vec<ValidationError>,
) {
    for (role, ty) in [("request", &port.request), ("response", &port.response)] {
        validate_type_expr(
            ty,
            type_names,
            &format!(
                "component `{}` {label} `{}` {role}",
                component.name, port.name
            ),
            errors,
        );
        if !matches!(ty, TypeExpr::Named { .. })
            && type_expr_contains_variable_data(ty, types_by_name)
        {
            errors.push(ValidationError::new(format!(
                "component `{}` {label} `{}` {role} uses variable data directly; variable data must be declared as a top-level field of a named message type",
                component.name, port.name
            )));
        }
    }
}

fn validate_param_schema(
    component: &ComponentIr,
    param: &flowrt_ir::ParamIr,
    errors: &mut Vec<ValidationError>,
) {
    if param.update == ParamUpdatePolicy::OnTick && !param_type_is_hot_update_scalar(param.ty) {
        errors.push(ValidationError::new(format!(
            "component `{}` param `{}` uses `on_tick` update with non-scalar type `{}`",
            component.name,
            param.name,
            param_type_name(param.ty)
        )));
    }
    validate_param_value_matches_type(component, param, "default", &param.default, errors);
    if let Some(min) = &param.min {
        validate_param_value_matches_type(component, param, "min", min, errors);
    }
    if let Some(max) = &param.max {
        validate_param_value_matches_type(component, param, "max", max, errors);
    }
    for choice in &param.choices {
        validate_param_value_matches_type(component, param, "enum choice", choice, errors);
    }
    validate_param_value_constraints(component, param, "default", &param.default, errors);
}

fn validate_param_value_matches_type(
    component: &ComponentIr,
    param: &flowrt_ir::ParamIr,
    label: &str,
    value: &flowrt_ir::ParamValue,
    errors: &mut Vec<ValidationError>,
) {
    if param_type_accepts_value(param.ty, value) {
        return;
    }
    errors.push(ValidationError::new(format!(
        "component `{}` param `{}` {label} has incompatible value kind `{}`; expected `{}`",
        component.name,
        param.name,
        flowrt_ir::param_value_kind(value),
        param_type_name(param.ty)
    )));
}

fn validate_param_value_constraints(
    component: &ComponentIr,
    param: &flowrt_ir::ParamIr,
    label: &str,
    value: &flowrt_ir::ParamValue,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(min) = &param.min
        && compare_param_values(value, min).is_some_and(|ordering| ordering.is_lt())
    {
        errors.push(ValidationError::new(format!(
            "component `{}` param `{}` {label} is below declared minimum",
            component.name, param.name
        )));
    }
    if let Some(max) = &param.max
        && compare_param_values(value, max).is_some_and(|ordering| ordering.is_gt())
    {
        errors.push(ValidationError::new(format!(
            "component `{}` param `{}` {label} is above declared maximum",
            component.name, param.name
        )));
    }
    if !param.choices.is_empty() && !param.choices.iter().any(|choice| choice == value) {
        errors.push(ValidationError::new(format!(
            "component `{}` param `{}` {label} is not in declared enum choices",
            component.name, param.name
        )));
    }
}

fn param_type_is_hot_update_scalar(ty: ParamType) -> bool {
    !matches!(ty, ParamType::Array | ParamType::Table)
}

fn param_type_accepts_value(ty: ParamType, value: &flowrt_ir::ParamValue) -> bool {
    matches!(
        (ty, value),
        (ParamType::Bool, flowrt_ir::ParamValue::Bool(_))
            | (
                ParamType::U8
                    | ParamType::U16
                    | ParamType::U32
                    | ParamType::U64
                    | ParamType::I8
                    | ParamType::I16
                    | ParamType::I32
                    | ParamType::I64,
                flowrt_ir::ParamValue::Integer(_)
            )
            | (
                ParamType::F32 | ParamType::F64,
                flowrt_ir::ParamValue::Float(_) | flowrt_ir::ParamValue::Integer(_)
            )
            | (ParamType::String, flowrt_ir::ParamValue::String(_))
            | (ParamType::Array, flowrt_ir::ParamValue::Array(_))
            | (ParamType::Table, flowrt_ir::ParamValue::Table(_))
    )
}

fn compare_param_values(
    left: &flowrt_ir::ParamValue,
    right: &flowrt_ir::ParamValue,
) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (flowrt_ir::ParamValue::Integer(left), flowrt_ir::ParamValue::Integer(right)) => {
            Some(left.cmp(right))
        }
        (flowrt_ir::ParamValue::Float(left), flowrt_ir::ParamValue::Float(right)) => {
            left.partial_cmp(right)
        }
        (flowrt_ir::ParamValue::Float(left), flowrt_ir::ParamValue::Integer(right)) => {
            left.partial_cmp(&(*right as f64))
        }
        (flowrt_ir::ParamValue::Integer(left), flowrt_ir::ParamValue::Float(right)) => {
            (*left as f64).partial_cmp(right)
        }
        (flowrt_ir::ParamValue::String(left), flowrt_ir::ParamValue::String(right)) => {
            Some(left.cmp(right))
        }
        _ => None,
    }
}

fn param_type_name(ty: ParamType) -> &'static str {
    match ty {
        ParamType::Bool => "bool",
        ParamType::U8 => "u8",
        ParamType::U16 => "u16",
        ParamType::U32 => "u32",
        ParamType::U64 => "u64",
        ParamType::I8 => "i8",
        ParamType::I16 => "i16",
        ParamType::I32 => "i32",
        ParamType::I64 => "i64",
        ParamType::F32 => "f32",
        ParamType::F64 => "f64",
        ParamType::String => "string",
        ParamType::Array => "array",
        ParamType::Table => "table",
    }
}
