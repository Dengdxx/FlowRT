use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    ComponentIr, ComponentKind, ContractIr, LanguageKind, ParamIr, ParamType, ParamUpdatePolicy,
    ParamValue, TypeExpr,
};

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
        if component.kind == ComponentKind::External && component.language != LanguageKind::External
        {
            errors.push(ValidationError::new(format!(
                "component `{}` uses kind `external` but language is not `external`",
                component.name
            )));
        }
        if component.language == LanguageKind::External && component.kind != ComponentKind::External
        {
            errors.push(ValidationError::new(format!(
                "component `{}` uses language `external` but kind is not `external`",
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

        let mut operation_clients = BTreeSet::new();
        for port in &component.operation_clients {
            if !operation_clients.insert(port.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "component `{}` has duplicate operation client `{}`",
                    component.name, port.name
                )));
            }
            validate_operation_port_types(
                component,
                "operation client",
                port,
                type_names,
                &types_by_name,
                errors,
            );
        }

        let mut operation_servers = BTreeSet::new();
        for port in &component.operation_servers {
            if !operation_servers.insert(port.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "component `{}` has duplicate operation server `{}`",
                    component.name, port.name
                )));
            }
            validate_operation_port_types(
                component,
                "operation server",
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

fn validate_operation_port_types(
    component: &ComponentIr,
    label: &'static str,
    port: &flowrt_ir::OperationPortIr,
    type_names: &BTreeSet<&str>,
    types_by_name: &BTreeMap<&str, &flowrt_ir::TypeIr>,
    errors: &mut Vec<ValidationError>,
) {
    for (role, ty) in [
        ("goal", &port.goal),
        ("feedback", &port.feedback),
        ("result", &port.result),
    ] {
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
    param: &ParamIr,
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
    let context = format!("component `{}` param `{}`", component.name, param.name);
    validate_param_value_matches_schema(&context, param, "default", &param.default, errors);
    if let Some(min) = &param.min {
        validate_param_value_matches_schema(&context, param, "min", min, errors);
    }
    if let Some(max) = &param.max {
        validate_param_value_matches_schema(&context, param, "max", max, errors);
    }
    for choice in &param.choices {
        validate_param_value_matches_schema(&context, param, "enum choice", choice, errors);
    }
    validate_param_value_constraints(&context, param, "default", &param.default, errors);
}

pub(crate) fn validate_param_value_matches_schema(
    context: &str,
    param: &ParamIr,
    label: &str,
    value: &ParamValue,
    errors: &mut Vec<ValidationError>,
) {
    if !param_type_accepts_value(param.ty, value) {
        errors.push(ValidationError::new(format!(
            "{context}{} has incompatible value kind `{}`; expected `{}`",
            label_prefix(label),
            flowrt_ir::param_value_kind(value),
            param_type_name(param.ty)
        )));
    }
    if let Some(reason) = param_value_range_error(param.ty, value) {
        errors.push(ValidationError::new(format!(
            "{context}{} {reason}",
            label_prefix(label)
        )));
    }
}

pub(crate) fn validate_param_value_constraints(
    context: &str,
    param: &ParamIr,
    label: &str,
    value: &ParamValue,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(min) = &param.min
        && compare_param_values(param.ty, value, min).is_some_and(|ordering| ordering.is_lt())
    {
        errors.push(ValidationError::new(format!(
            "{context}{} is below declared minimum",
            label_prefix(label)
        )));
    }
    if let Some(max) = &param.max
        && compare_param_values(param.ty, value, max).is_some_and(|ordering| ordering.is_gt())
    {
        errors.push(ValidationError::new(format!(
            "{context}{} is above declared maximum",
            label_prefix(label)
        )));
    }
    if !param.choices.is_empty() && !param.choices.iter().any(|choice| choice == value) {
        errors.push(ValidationError::new(format!(
            "{context}{} is not in declared enum choices",
            label_prefix(label)
        )));
    }
}

fn label_prefix(label: &str) -> String {
    if label.is_empty() {
        String::new()
    } else {
        format!(" {label}")
    }
}

fn param_type_is_hot_update_scalar(ty: ParamType) -> bool {
    !matches!(ty, ParamType::Array | ParamType::Table)
}

fn param_type_accepts_value(ty: ParamType, value: &ParamValue) -> bool {
    matches!(
        (ty, value),
        (ParamType::Bool, ParamValue::Bool(_))
            | (
                ParamType::U8
                    | ParamType::U16
                    | ParamType::U32
                    | ParamType::U64
                    | ParamType::I8
                    | ParamType::I16
                    | ParamType::I32
                    | ParamType::I64,
                ParamValue::Integer(_)
            )
            | (
                ParamType::F32 | ParamType::F64,
                ParamValue::Float(_) | ParamValue::Integer(_)
            )
            | (ParamType::String, ParamValue::String(_))
            | (ParamType::Array, ParamValue::Array(_))
            | (ParamType::Table, ParamValue::Table(_))
    )
}

fn param_value_range_error(ty: ParamType, value: &ParamValue) -> Option<&'static str> {
    match value {
        ParamValue::Float(value) if !value.is_finite() => return Some("must be finite"),
        ParamValue::Array(values) => {
            if values.iter().any(param_value_contains_non_finite_float) {
                return Some("contains non-finite float");
            }
        }
        ParamValue::Table(values) => {
            if values.values().any(param_value_contains_non_finite_float) {
                return Some("contains non-finite float");
            }
        }
        _ => {}
    }
    match (ty, value) {
        (ParamType::U8, ParamValue::Integer(value)) => {
            integer_range_error(*value, 0, u8::MAX as i64)
        }
        (ParamType::U16, ParamValue::Integer(value)) => {
            integer_range_error(*value, 0, u16::MAX as i64)
        }
        (ParamType::U32, ParamValue::Integer(value)) => {
            integer_range_error(*value, 0, u32::MAX as i64)
        }
        (ParamType::U64, ParamValue::Integer(value)) => integer_range_error(*value, 0, i64::MAX),
        (ParamType::I8, ParamValue::Integer(value)) => {
            integer_range_error(*value, i8::MIN as i64, i8::MAX as i64)
        }
        (ParamType::I16, ParamValue::Integer(value)) => {
            integer_range_error(*value, i16::MIN as i64, i16::MAX as i64)
        }
        (ParamType::I32, ParamValue::Integer(value)) => {
            integer_range_error(*value, i32::MIN as i64, i32::MAX as i64)
        }
        (ParamType::I64, ParamValue::Integer(_)) => None,
        (ParamType::F32, ParamValue::Float(value)) => {
            float_range_error(*value, Some(f32::MAX as f64))
        }
        (ParamType::F64, ParamValue::Float(value)) => float_range_error(*value, None),
        _ => None,
    }
}

fn param_value_contains_non_finite_float(value: &ParamValue) -> bool {
    match value {
        ParamValue::Float(value) => !value.is_finite(),
        ParamValue::Array(values) => values.iter().any(param_value_contains_non_finite_float),
        ParamValue::Table(values) => values.values().any(param_value_contains_non_finite_float),
        _ => false,
    }
}

fn integer_range_error(value: i64, min: i64, max: i64) -> Option<&'static str> {
    if value < min || value > max {
        Some("is outside declared type range")
    } else {
        None
    }
}

fn float_range_error(value: f64, max_abs: Option<f64>) -> Option<&'static str> {
    if !value.is_finite() {
        return Some("must be finite");
    }
    if max_abs.is_some_and(|max_abs| value.abs() > max_abs) {
        return Some("is outside declared type range");
    }
    None
}

fn compare_param_values(
    ty: ParamType,
    left: &ParamValue,
    right: &ParamValue,
) -> Option<std::cmp::Ordering> {
    match ty {
        ParamType::U8
        | ParamType::U16
        | ParamType::U32
        | ParamType::U64
        | ParamType::I8
        | ParamType::I16
        | ParamType::I32
        | ParamType::I64 => {
            if let (ParamValue::Integer(left), ParamValue::Integer(right)) = (left, right) {
                return Some(left.cmp(right));
            }
            return None;
        }
        _ => {}
    }
    match (left, right) {
        (ParamValue::Integer(left), ParamValue::Integer(right)) => Some(left.cmp(right)),
        (ParamValue::Float(left), ParamValue::Float(right)) => left.partial_cmp(right),
        (ParamValue::Float(left), ParamValue::Integer(right)) => {
            compare_integer_float(*right, *left).map(std::cmp::Ordering::reverse)
        }
        (ParamValue::Integer(left), ParamValue::Float(right)) => {
            compare_integer_float(*left, *right)
        }
        (ParamValue::String(left), ParamValue::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn compare_integer_float(integer: i64, float: f64) -> Option<std::cmp::Ordering> {
    if float.is_nan() {
        return None;
    }
    if float == f64::INFINITY {
        return Some(std::cmp::Ordering::Less);
    }
    if float == f64::NEG_INFINITY {
        return Some(std::cmp::Ordering::Greater);
    }
    if float < i64::MIN as f64 {
        return Some(std::cmp::Ordering::Greater);
    }
    if float > i64::MAX as f64 {
        return Some(std::cmp::Ordering::Less);
    }

    let truncated = float.trunc() as i64;
    match integer.cmp(&truncated) {
        std::cmp::Ordering::Equal => {
            let fraction = float.fract();
            if fraction == 0.0 {
                Some(std::cmp::Ordering::Equal)
            } else if fraction > 0.0 {
                Some(std::cmp::Ordering::Less)
            } else {
                Some(std::cmp::Ordering::Greater)
            }
        }
        ordering => Some(ordering),
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
