use super::model::IntrospectionParamStatus;

#[derive(Debug, Clone)]
pub(super) struct ParamState {
    pub(super) ty: String,
    pub(super) update: String,
    pub(super) current: serde_json::Value,
    pub(super) pending: Option<serde_json::Value>,
    pub(super) apply_state: String,
    pub(super) last_reject_reason: Option<String>,
    pub(super) updated_unix_ms: Option<u64>,
    pub(super) min: Option<serde_json::Value>,
    pub(super) max: Option<serde_json::Value>,
    pub(super) choices: Vec<serde_json::Value>,
}

pub(super) fn param_status(name: &str, param: &ParamState) -> IntrospectionParamStatus {
    IntrospectionParamStatus {
        name: name.to_string(),
        ty: param.ty.clone(),
        update: param.update.clone(),
        current: param.current.clone(),
        pending: param.pending.clone(),
        apply_state: param.apply_state.clone(),
        last_reject_reason: param.last_reject_reason.clone(),
        updated_unix_ms: param.updated_unix_ms,
        min: param.min.clone(),
        max: param.max.clone(),
        choices: param.choices.clone(),
    }
}

pub(super) fn validate_param_json_value(
    name: &str,
    param: &ParamState,
    value: &serde_json::Value,
) -> std::result::Result<(), String> {
    if !json_value_matches_param_type(&param.ty, value) {
        return Err(format!(
            "FlowRT parameter `{name}` expects `{}` value",
            param.ty
        ));
    }
    if let Some(min) = &param.min
        && compare_param_json_values(&param.ty, value, min).is_some_and(|ordering| ordering.is_lt())
    {
        return Err(format!("FlowRT parameter `{name}` is below minimum"));
    }
    if let Some(max) = &param.max
        && compare_param_json_values(&param.ty, value, max).is_some_and(|ordering| ordering.is_gt())
    {
        return Err(format!("FlowRT parameter `{name}` is above maximum"));
    }
    if !param.choices.is_empty() && !param.choices.iter().any(|choice| choice == value) {
        return Err(format!(
            "FlowRT parameter `{name}` is not in declared enum choices"
        ));
    }
    Ok(())
}

fn json_value_matches_param_type(ty: &str, value: &serde_json::Value) -> bool {
    match ty {
        "bool" => value.is_boolean(),
        "string" => value.is_string(),
        "f32" | "f64" => value.is_number(),
        "u8" | "u16" | "u32" | "u64" => value.as_u64().is_some(),
        "i8" | "i16" | "i32" | "i64" => value.as_i64().is_some(),
        "array" => value.is_array(),
        "table" => value.is_object(),
        _ => false,
    }
}

fn compare_param_json_values(
    ty: &str,
    left: &serde_json::Value,
    right: &serde_json::Value,
) -> Option<std::cmp::Ordering> {
    match ty {
        "u8" | "u16" | "u32" | "u64" => {
            return Some(left.as_u64()?.cmp(&right.as_u64()?));
        }
        "i8" | "i16" | "i32" | "i64" => {
            return Some(left.as_i64()?.cmp(&right.as_i64()?));
        }
        _ => {}
    }
    match (left, right) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            left.as_f64()?.partial_cmp(&right.as_f64()?)
        }
        (serde_json::Value::String(left), serde_json::Value::String(right)) => {
            Some(left.cmp(right))
        }
        _ => None,
    }
}
