use std::collections::BTreeMap;

use flowrt_rsdl::{RawComponent, RawValue};

use crate::{IrError, ParamIr, ParamType, ParamUpdatePolicy, ParamValue, ParamValueIr, Result};

pub(super) fn normalize_component_params(
    component_name: &str,
    raw: &RawComponent,
) -> Result<Vec<ParamIr>> {
    raw.params
        .iter()
        .map(|(name, value)| normalize_component_param(component_name, name, value))
        .collect()
}

fn normalize_component_param(
    component_name: &str,
    name: &str,
    value: &RawValue,
) -> Result<ParamIr> {
    if let Some(schema) = try_normalize_param_schema(component_name, name, value)? {
        return Ok(schema);
    }
    let default = convert_param_value(value);
    validate_param_value_supported(component_name, name, &default)?;
    Ok(ParamIr {
        name: name.to_string(),
        ty: infer_param_type(&default),
        default,
        update: ParamUpdatePolicy::Startup,
        min: None,
        max: None,
        choices: Vec::new(),
    })
}

fn try_normalize_param_schema(
    component_name: &str,
    name: &str,
    value: &RawValue,
) -> Result<Option<ParamIr>> {
    let RawValue::Table(table) = value else {
        return Ok(None);
    };
    if !table.contains_key("type") && !table.contains_key("default") {
        return Ok(None);
    }

    validate_param_schema_keys(component_name, name, table)?;
    let declared_ty = table
        .get("type")
        .and_then(raw_string)
        .map(|value| parse_param_type_schema(component_name, name, value))
        .transpose()?;
    let default = table
        .get("default")
        .map(convert_param_value)
        .unwrap_or_else(|| default_for_param_type(declared_ty.unwrap_or(ParamType::String)));
    let ty = declared_ty.unwrap_or_else(|| infer_param_type(&default));
    let update = table
        .get("update")
        .and_then(raw_string)
        .map(|value| parse_param_update_policy(component_name, name, value))
        .transpose()?
        .unwrap_or(ParamUpdatePolicy::Startup);
    let choices = table
        .get("enum")
        .map(convert_param_value)
        .and_then(|value| match value {
            ParamValue::Array(values) => Some(values),
            _ => None,
        })
        .unwrap_or_default();
    let min = table.get("min").map(convert_param_value);
    let max = table.get("max").map(convert_param_value);

    let schema = ParamIr {
        name: name.to_string(),
        ty,
        default,
        update,
        min,
        max,
        choices,
    };
    validate_param_schema_value(component_name, name, &schema, &schema.default)?;
    if let Some(min) = &schema.min {
        validate_param_schema_value(component_name, name, &schema, min)?;
    }
    if let Some(max) = &schema.max {
        validate_param_schema_value(component_name, name, &schema, max)?;
    }
    for choice in &schema.choices {
        validate_param_schema_value(component_name, name, &schema, choice)?;
        validate_param_value_constraints(component_name, name, &schema, choice)?;
    }
    validate_param_value_constraints(component_name, name, &schema, &schema.default)?;
    Ok(Some(schema))
}

pub(super) fn merge_instance_params(
    instance_name: &str,
    raw: &flowrt_rsdl::RawInstance,
    component: &BTreeMap<String, ParamIr>,
) -> Result<Vec<ParamValueIr>> {
    for (param, override_value) in &raw.params {
        let Some(schema) = component.get(param) else {
            return Err(IrError::UnknownParamOverride {
                instance: instance_name.to_string(),
                component: raw.component.clone(),
                param: param.clone(),
            });
        };
        validate_param_override_type(instance_name, &raw.component, param, schema, override_value)?;
    }

    let mut merged = component
        .iter()
        .map(|(name, param)| (name.clone(), param.default.clone()))
        .collect::<BTreeMap<_, _>>();
    for (name, value) in &raw.params {
        merged.insert(name.clone(), convert_param_value(value));
    }

    Ok(merged
        .iter()
        .map(|(name, value)| ParamValueIr {
            name: name.clone(),
            value: value.clone(),
        })
        .collect())
}

fn validate_param_override_type(
    instance_name: &str,
    component_name: &str,
    param_name: &str,
    schema: &ParamIr,
    override_value: &RawValue,
) -> Result<()> {
    let override_value = convert_param_value(override_value);
    validate_param_value_supported(component_name, param_name, &override_value)?;
    if !param_value_compatible(&schema.default, &override_value) {
        Err(IrError::IncompatibleParamOverride {
            instance: instance_name.to_string(),
            component: component_name.to_string(),
            param: param_name.to_string(),
            expected: param_value_kind(&schema.default),
            actual: param_value_kind(&override_value),
        })
    } else {
        validate_param_value_constraints(component_name, param_name, schema, &override_value)
    }
}

/// 判断一个参数值是否可覆盖另一个参数值。
pub fn param_value_compatible(default_value: &ParamValue, override_value: &ParamValue) -> bool {
    match (default_value, override_value) {
        (ParamValue::Bool(_), ParamValue::Bool(_)) => true,
        (ParamValue::Integer(_), ParamValue::Integer(_)) => true,
        (ParamValue::Float(_), ParamValue::Float(_) | ParamValue::Integer(_)) => true,
        (ParamValue::String(_), ParamValue::String(_)) => true,
        (ParamValue::Array(default_values), ParamValue::Array(override_values)) => {
            array_param_compatible(default_values, override_values)
        }
        (ParamValue::Table(default_values), ParamValue::Table(override_values)) => override_values
            .iter()
            .all(|(name, value)| match default_values.get(name) {
                Some(default_value) => param_value_compatible(default_value, value),
                None => false,
            }),
        _ => false,
    }
}

fn array_param_compatible(default_values: &[ParamValue], override_values: &[ParamValue]) -> bool {
    if default_values.is_empty() {
        return true;
    }
    if override_values.is_empty() {
        return true;
    }
    let Some(default_sample) = default_values.first() else {
        return false;
    };
    override_values
        .iter()
        .all(|value| param_value_compatible(default_sample, value))
}

/// 返回参数值的类别名称。
pub fn param_value_kind(value: &ParamValue) -> &'static str {
    match value {
        ParamValue::Bool(_) => "bool",
        ParamValue::Integer(_) => "integer",
        ParamValue::Float(_) => "float",
        ParamValue::String(_) => "string",
        ParamValue::Array(_) => "array",
        ParamValue::Table(_) => "table",
    }
}

fn convert_param_value(value: &RawValue) -> ParamValue {
    match value {
        RawValue::Bool(value) => ParamValue::Bool(*value),
        RawValue::Integer(value) => ParamValue::Integer(*value),
        RawValue::Float(value) => ParamValue::Float(*value),
        RawValue::String(value) => ParamValue::String(value.clone()),
        RawValue::Array(values) => {
            ParamValue::Array(values.iter().map(convert_param_value).collect())
        }
        RawValue::Table(values) => ParamValue::Table(
            values
                .iter()
                .map(|(name, value)| (name.clone(), convert_param_value(value)))
                .collect(),
        ),
    }
}

/// 把反馈边 `init` 表（字段名→字面值）转成 `ParamValue::Table`，供 codegen 播种字面量。
pub(super) fn convert_param_value_table(table: &BTreeMap<String, RawValue>) -> ParamValue {
    ParamValue::Table(
        table
            .iter()
            .map(|(name, value)| (name.clone(), convert_param_value(value)))
            .collect(),
    )
}

fn raw_string(value: &RawValue) -> Option<&str> {
    match value {
        RawValue::String(value) => Some(value),
        _ => None,
    }
}

fn validate_param_schema_keys(
    component: &str,
    param: &str,
    table: &BTreeMap<String, RawValue>,
) -> Result<()> {
    for key in table.keys() {
        if !matches!(
            key.as_str(),
            "type" | "default" | "update" | "min" | "max" | "enum"
        ) {
            return Err(IrError::InvalidParamSchema {
                component: component.to_string(),
                param: param.to_string(),
                message: format!("unknown schema key `{key}`"),
            });
        }
    }
    Ok(())
}

fn validate_param_schema_value(
    component: &str,
    param: &str,
    schema: &ParamIr,
    value: &ParamValue,
) -> Result<()> {
    validate_param_value_supported(component, param, value)?;
    if param_type_accepts_value(schema.ty, value) {
        Ok(())
    } else {
        Err(IrError::InvalidParamSchema {
            component: component.to_string(),
            param: param.to_string(),
            message: format!(
                "value kind `{}` does not match declared type `{}`",
                param_value_kind(value),
                param_type_name(schema.ty)
            ),
        })
    }
}

fn validate_param_value_constraints(
    component: &str,
    param: &str,
    schema: &ParamIr,
    value: &ParamValue,
) -> Result<()> {
    validate_param_value_supported(component, param, value)?;
    validate_param_value_range(component, param, schema.ty, value)?;
    if let Some(min) = &schema.min
        && compare_param_values(value, min).is_some_and(|ordering| ordering.is_lt())
    {
        return Err(IrError::InvalidParamSchema {
            component: component.to_string(),
            param: param.to_string(),
            message: "value is below declared minimum".to_string(),
        });
    }
    if let Some(max) = &schema.max
        && compare_param_values(value, max).is_some_and(|ordering| ordering.is_gt())
    {
        return Err(IrError::InvalidParamSchema {
            component: component.to_string(),
            param: param.to_string(),
            message: "value is above declared maximum".to_string(),
        });
    }
    if !schema.choices.is_empty() && !schema.choices.iter().any(|choice| choice == value) {
        return Err(IrError::InvalidParamSchema {
            component: component.to_string(),
            param: param.to_string(),
            message: "value is not in declared enum choices".to_string(),
        });
    }
    Ok(())
}

fn validate_param_value_range(
    component: &str,
    param: &str,
    ty: ParamType,
    value: &ParamValue,
) -> Result<()> {
    let valid = match (ty, value) {
        (
            ParamType::U8
            | ParamType::U16
            | ParamType::U32
            | ParamType::U64
            | ParamType::I8
            | ParamType::I16
            | ParamType::I32
            | ParamType::I64,
            ParamValue::Integer(value),
        ) => integer_param_bounds(ty).is_some_and(|(min, max)| *value >= min && *value <= max),
        (ParamType::F32, ParamValue::Float(value)) => {
            *value >= f64::from(f32::MIN) && *value <= f64::from(f32::MAX)
        }
        (ParamType::F32, ParamValue::Integer(_)) => true,
        _ => true,
    };

    if valid {
        Ok(())
    } else {
        Err(IrError::InvalidParamSchema {
            component: component.to_string(),
            param: param.to_string(),
            message: format!("value is outside declared `{}` range", param_type_name(ty)),
        })
    }
}

fn integer_param_bounds(ty: ParamType) -> Option<(i64, i64)> {
    match ty {
        ParamType::U8 => Some((0, i64::from(u8::MAX))),
        ParamType::U16 => Some((0, i64::from(u16::MAX))),
        ParamType::U32 => Some((0, i64::from(u32::MAX))),
        // RSDL/TOML integer values are normalized as i64 today, so the
        // representable u64 subset is 0..=i64::MAX.
        ParamType::U64 => Some((0, i64::MAX)),
        ParamType::I8 => Some((i64::from(i8::MIN), i64::from(i8::MAX))),
        ParamType::I16 => Some((i64::from(i16::MIN), i64::from(i16::MAX))),
        ParamType::I32 => Some((i64::from(i32::MIN), i64::from(i32::MAX))),
        ParamType::I64 => Some((i64::MIN, i64::MAX)),
        _ => None,
    }
}

fn validate_param_value_supported(component: &str, param: &str, value: &ParamValue) -> Result<()> {
    match value {
        ParamValue::Float(value) if !value.is_finite() => Err(IrError::InvalidParamSchema {
            component: component.to_string(),
            param: param.to_string(),
            message: "float value must be finite".to_string(),
        }),
        ParamValue::Array(values) => {
            for value in values {
                validate_param_value_supported(component, param, value)?;
            }
            Ok(())
        }
        ParamValue::Table(values) => {
            for value in values.values() {
                validate_param_value_supported(component, param, value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
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

fn compare_param_values(left: &ParamValue, right: &ParamValue) -> Option<std::cmp::Ordering> {
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

fn parse_param_type_schema(component: &str, param: &str, value: &str) -> Result<ParamType> {
    match value {
        "bool" => Ok(ParamType::Bool),
        "u8" => Ok(ParamType::U8),
        "u16" => Ok(ParamType::U16),
        "u32" => Ok(ParamType::U32),
        "u64" => Ok(ParamType::U64),
        "i8" => Ok(ParamType::I8),
        "i16" => Ok(ParamType::I16),
        "i32" => Ok(ParamType::I32),
        "i64" => Ok(ParamType::I64),
        "f32" => Ok(ParamType::F32),
        "f64" => Ok(ParamType::F64),
        "string" => Ok(ParamType::String),
        "array" => Ok(ParamType::Array),
        "table" => Ok(ParamType::Table),
        _ => Err(IrError::InvalidParamSchema {
            component: component.to_string(),
            param: param.to_string(),
            message: format!("unknown parameter type `{value}`"),
        }),
    }
}

fn parse_param_update_policy(
    component: &str,
    param: &str,
    value: &str,
) -> Result<ParamUpdatePolicy> {
    match value {
        "startup" => Ok(ParamUpdatePolicy::Startup),
        "on_tick" => Ok(ParamUpdatePolicy::OnTick),
        _ => Err(IrError::InvalidEnum {
            context: format!("component.{component}.params.{param}.update"),
            kind: "parameter update policy",
            value: value.to_string(),
        }),
    }
}

fn default_for_param_type(ty: ParamType) -> ParamValue {
    match ty {
        ParamType::Bool => ParamValue::Bool(false),
        ParamType::U8
        | ParamType::U16
        | ParamType::U32
        | ParamType::U64
        | ParamType::I8
        | ParamType::I16
        | ParamType::I32
        | ParamType::I64 => ParamValue::Integer(0),
        ParamType::F32 | ParamType::F64 => ParamValue::Float(0.0),
        ParamType::String => ParamValue::String(String::new()),
        ParamType::Array => ParamValue::Array(Vec::new()),
        ParamType::Table => ParamValue::Table(BTreeMap::new()),
    }
}

fn infer_param_type(value: &ParamValue) -> ParamType {
    match value {
        ParamValue::Bool(_) => ParamType::Bool,
        ParamValue::Integer(_) => ParamType::I64,
        ParamValue::Float(_) => ParamType::F64,
        ParamValue::String(_) => ParamType::String,
        ParamValue::Array(_) => ParamType::Array,
        ParamValue::Table(_) => ParamType::Table,
    }
}
