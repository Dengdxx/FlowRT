use std::collections::BTreeMap;

use toml::Value;
use toml::value::Table;

use crate::ast::*;
use crate::{Result, RsdlError};

pub(super) fn optional_param_table(
    table: &Table,
    context: &str,
    field: &'static str,
) -> Result<BTreeMap<String, RawValue>> {
    let Some(value) = table.get(field) else {
        return Ok(BTreeMap::new());
    };
    let table = expect_table_value(context, field, value)?;
    let mut params = BTreeMap::new();
    for (name, value) in table {
        params.insert(name.clone(), convert_value(value));
    }
    Ok(params)
}

pub(super) fn convert_value(value: &Value) -> RawValue {
    match value {
        Value::String(value) => RawValue::String(value.clone()),
        Value::Integer(value) => RawValue::Integer(*value),
        Value::Float(value) => RawValue::Float(*value),
        Value::Boolean(value) => RawValue::Bool(*value),
        Value::Array(values) => RawValue::Array(values.iter().map(convert_value).collect()),
        Value::Table(table) => RawValue::Table(
            table
                .iter()
                .map(|(key, value)| (key.clone(), convert_value(value)))
                .collect(),
        ),
        Value::Datetime(value) => RawValue::String(value.to_string()),
    }
}

pub(super) fn optional_port_array(
    table: &Table,
    context: &str,
    field: &'static str,
) -> Result<Vec<RawPort>> {
    optional_string_array(table, context, field)?
        .into_iter()
        .map(|descriptor| parse_port_descriptor(&descriptor))
        .collect()
}

pub(super) fn optional_service_port_array(
    table: &Table,
    context: &str,
    field: &'static str,
) -> Result<Vec<RawServicePort>> {
    optional_string_array(table, context, field)?
        .into_iter()
        .map(|descriptor| parse_service_port_descriptor(&descriptor))
        .collect()
}

pub(super) fn parse_port_descriptor(descriptor: &str) -> Result<RawPort> {
    let Some((name, ty)) = descriptor.split_once(':') else {
        return Err(RsdlError::InvalidPortDescriptor {
            descriptor: descriptor.to_string(),
        });
    };
    let name = name.trim();
    let ty = ty.trim();
    if name.is_empty() || ty.is_empty() {
        return Err(RsdlError::InvalidPortDescriptor {
            descriptor: descriptor.to_string(),
        });
    }
    Ok(RawPort {
        name: name.to_string(),
        ty: ty.to_string(),
    })
}

pub(super) fn parse_service_port_descriptor(descriptor: &str) -> Result<RawServicePort> {
    let Some((name, types)) = descriptor.split_once(':') else {
        return Err(RsdlError::InvalidValue {
            context: "service port descriptor".to_string(),
            message: format!(
                "`{descriptor}` must use `<port_name>:<request_type>-><response_type>`"
            ),
        });
    };
    let Some((request, response)) = types.split_once("->") else {
        return Err(RsdlError::InvalidValue {
            context: "service port descriptor".to_string(),
            message: format!(
                "`{descriptor}` must use `<port_name>:<request_type>-><response_type>`"
            ),
        });
    };
    let name = name.trim();
    let request = request.trim();
    let response = response.trim();
    if name.is_empty() || request.is_empty() || response.is_empty() {
        return Err(RsdlError::InvalidValue {
            context: "service port descriptor".to_string(),
            message: format!(
                "`{descriptor}` must use `<port_name>:<request_type>-><response_type>`"
            ),
        });
    }
    Ok(RawServicePort {
        name: name.to_string(),
        request: request.to_string(),
        response: response.to_string(),
    })
}

pub(super) fn required_string(table: &Table, context: &str, field: &'static str) -> Result<String> {
    let value = table.get(field).ok_or_else(|| RsdlError::MissingField {
        context: context.to_string(),
        field,
    })?;
    expect_string(context, field, value)
}

pub(super) fn optional_string(
    table: &Table,
    context: &str,
    field: &'static str,
) -> Result<Option<String>> {
    table
        .get(field)
        .map(|value| expect_string(context, field, value))
        .transpose()
}

pub(super) fn expect_string(context: &str, field: &str, value: &Value) -> Result<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: context.to_string(),
            field: field.to_string(),
            expected: "string",
        })
}

pub(super) fn optional_string_array(
    table: &Table,
    context: &str,
    field: &'static str,
) -> Result<Vec<String>> {
    let Some(value) = table.get(field) else {
        return Ok(Vec::new());
    };
    expect_string_array(context, field, value)
}

pub(super) fn expect_string_array(
    context: &str,
    field: &str,
    value: &Value,
) -> Result<Vec<String>> {
    let values = value
        .as_array()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: context.to_string(),
            field: field.to_string(),
            expected: "array of strings",
        })?;
    values
        .iter()
        .map(|value| expect_string(context, field, value))
        .collect()
}

pub(super) fn optional_u32(
    table: &Table,
    context: &str,
    field: &'static str,
) -> Result<Option<u32>> {
    optional_u64(table, context, field)?
        .map(|value| {
            u32::try_from(value).map_err(|_| RsdlError::InvalidValue {
                context: context.to_string(),
                message: format!("`{field}` is too large for u32"),
            })
        })
        .transpose()
}

pub(super) fn optional_u64(
    table: &Table,
    context: &str,
    field: &'static str,
) -> Result<Option<u64>> {
    let Some(value) = table.get(field) else {
        return Ok(None);
    };
    let integer = value
        .as_integer()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: context.to_string(),
            field: field.to_string(),
            expected: "non-negative integer",
        })?;
    let value = u64::try_from(integer).map_err(|_| RsdlError::InvalidValue {
        context: context.to_string(),
        message: format!("`{field}` must be non-negative"),
    })?;
    Ok(Some(value))
}

pub(super) fn expect_table_value<'a>(
    context: &str,
    field: &str,
    value: &'a Value,
) -> Result<&'a Table> {
    value.as_table().ok_or_else(|| RsdlError::InvalidFieldType {
        context: context.to_string(),
        field: field.to_string(),
        expected: "table",
    })
}
