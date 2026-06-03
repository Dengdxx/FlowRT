use std::collections::BTreeMap;
use std::path::Path;

use toml::Value;
use toml::value::Table;

use crate::ast::*;
use crate::{Result, RsdlError};

/// 从磁盘解析一个 `.rsdl` 文件。
pub fn parse_file(path: impl AsRef<Path>) -> Result<RawDocument> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(|source| RsdlError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_str(&source)
}

/// 解析 RSDL v0.1 源文本。
pub fn parse_str(source: &str) -> Result<RawDocument> {
    let value: Value = source.parse()?;
    let root = value.as_table().ok_or_else(|| RsdlError::InvalidValue {
        context: "document".to_string(),
        message: "expected a TOML table document".to_string(),
    })?;

    let package_table = root
        .get("package")
        .and_then(Value::as_table)
        .ok_or(RsdlError::MissingPackage)?;

    Ok(RawDocument {
        package: parse_package(package_table)?,
        types: parse_named_tables(root, "type", parse_type)?,
        components: parse_named_tables(root, "component", parse_component)?,
        instances: parse_named_tables(root, "instance", parse_instance)?,
        binds: parse_binds(root)?,
        profiles: parse_named_tables(root, "profile", parse_profile)?,
        targets: parse_named_tables(root, "target", parse_target)?,
    })
}

fn parse_package(table: &Table) -> Result<RawPackage> {
    let mut imports = BTreeMap::new();
    if let Some(value) = table.get("imports") {
        let table = expect_table_value("package", "imports", value)?;
        for (kind, value) in table {
            imports.insert(
                kind.clone(),
                expect_string_array("package.imports", kind, value)?,
            );
        }
    }

    Ok(RawPackage {
        name: required_string(table, "package", "name")?,
        version: optional_string(table, "package", "version")?,
        rsdl_version: required_string(table, "package", "rsdl_version")?,
        imports,
    })
}

fn parse_type(name: &str, table: &Table) -> Result<RawType> {
    let mut fields = Vec::with_capacity(table.len());
    for (field_name, value) in table {
        let ty = expect_string(&format!("type.{name}"), field_name, value)?;
        fields.push(RawField {
            name: field_name.clone(),
            ty,
        });
    }
    Ok(RawType { fields })
}

fn parse_component(name: &str, table: &Table) -> Result<RawComponent> {
    Ok(RawComponent {
        language: required_string(table, &format!("component.{name}"), "language")?,
        kind: optional_string(table, &format!("component.{name}"), "kind")?,
        input: optional_port_array(table, &format!("component.{name}"), "input")?,
        output: optional_port_array(table, &format!("component.{name}"), "output")?,
        params: optional_param_table(table, &format!("component.{name}"), "params")?,
    })
}

fn parse_instance(name: &str, table: &Table) -> Result<RawInstance> {
    let task = table
        .get("task")
        .map(|value| {
            let table = expect_table_value(&format!("instance.{name}"), "task", value)?;
            parse_task(name, table)
        })
        .transpose()?;

    Ok(RawInstance {
        component: required_string(table, &format!("instance.{name}"), "component")?,
        process: optional_string(table, &format!("instance.{name}"), "process")?,
        target: optional_string(table, &format!("instance.{name}"), "target")?,
        params: optional_param_table(table, &format!("instance.{name}"), "params")?,
        task,
    })
}

fn parse_task(instance_name: &str, table: &Table) -> Result<RawTask> {
    let context = format!("instance.{instance_name}.task");
    Ok(RawTask {
        trigger: required_string(table, &context, "trigger")?,
        period_ms: optional_u64(table, &context, "period_ms")?,
        deadline_ms: optional_u64(table, &context, "deadline_ms")?,
        priority: optional_u32(table, &context, "priority")?,
        input: optional_string_array(table, &context, "input")?,
        output: optional_string_array(table, &context, "output")?,
    })
}

fn parse_binds(root: &Table) -> Result<Vec<RawDataflowBind>> {
    let Some(bind_value) = root.get("bind") else {
        return Ok(Vec::new());
    };
    let bind_table = bind_value
        .as_table()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "document".to_string(),
            field: "bind".to_string(),
            expected: "table",
        })?;
    let Some(dataflow_value) = bind_table.get("dataflow") else {
        return Ok(Vec::new());
    };
    let binds = dataflow_value
        .as_array()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "bind".to_string(),
            field: "dataflow".to_string(),
            expected: "array of tables",
        })?;

    let mut parsed = Vec::with_capacity(binds.len());
    for (index, value) in binds.iter().enumerate() {
        let context = format!("bind.dataflow[{index}]");
        let table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: "bind".to_string(),
                field: "dataflow".to_string(),
                expected: "array of tables",
            })?;
        parsed.push(RawDataflowBind {
            from: required_string(table, &context, "from")?,
            to: required_string(table, &context, "to")?,
            channel: required_string(table, &context, "channel")?,
            depth: optional_u32(table, &context, "depth")?,
            overflow: optional_string(table, &context, "overflow")?,
            stale_policy: optional_string(table, &context, "stale_policy")?,
            max_age_ms: optional_u64(table, &context, "max_age_ms")?,
        });
    }
    Ok(parsed)
}

fn parse_profile(_name: &str, table: &Table) -> Result<RawProfile> {
    Ok(RawProfile {
        backend: optional_string(table, "profile", "backend")?,
        default_overflow: optional_string(table, "profile", "default_overflow")?,
        default_stale_policy: optional_string(table, "profile", "default_stale_policy")?,
        max_age_ms: optional_u64(table, "profile", "max_age_ms")?,
    })
}

fn parse_target(_name: &str, table: &Table) -> Result<RawTarget> {
    Ok(RawTarget {
        platform: optional_string(table, "target", "platform")?,
        runtime: optional_string_array(table, "target", "runtime")?,
        backends: optional_string_array(table, "target", "backends")?,
    })
}

fn parse_named_tables<T>(
    root: &Table,
    section: &'static str,
    parse_one: fn(&str, &Table) -> Result<T>,
) -> Result<BTreeMap<String, T>> {
    let Some(section_value) = root.get(section) else {
        return Ok(BTreeMap::new());
    };
    let section_table = section_value
        .as_table()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "document".to_string(),
            field: section.to_string(),
            expected: "table",
        })?;

    let mut result = BTreeMap::new();
    for (name, value) in section_table {
        let table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: section.to_string(),
                field: name.clone(),
                expected: "table",
            })?;
        result.insert(name.clone(), parse_one(name, table)?);
    }
    Ok(result)
}

fn optional_param_table(
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

fn convert_value(value: &Value) -> RawValue {
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

fn optional_port_array(table: &Table, context: &str, field: &'static str) -> Result<Vec<RawPort>> {
    optional_string_array(table, context, field)?
        .into_iter()
        .map(|descriptor| parse_port_descriptor(&descriptor))
        .collect()
}

fn parse_port_descriptor(descriptor: &str) -> Result<RawPort> {
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

fn required_string(table: &Table, context: &str, field: &'static str) -> Result<String> {
    let value = table.get(field).ok_or_else(|| RsdlError::MissingField {
        context: context.to_string(),
        field,
    })?;
    expect_string(context, field, value)
}

fn optional_string(table: &Table, context: &str, field: &'static str) -> Result<Option<String>> {
    table
        .get(field)
        .map(|value| expect_string(context, field, value))
        .transpose()
}

fn expect_string(context: &str, field: &str, value: &Value) -> Result<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: context.to_string(),
            field: field.to_string(),
            expected: "string",
        })
}

fn optional_string_array(table: &Table, context: &str, field: &'static str) -> Result<Vec<String>> {
    let Some(value) = table.get(field) else {
        return Ok(Vec::new());
    };
    expect_string_array(context, field, value)
}

fn expect_string_array(context: &str, field: &str, value: &Value) -> Result<Vec<String>> {
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

fn optional_u32(table: &Table, context: &str, field: &'static str) -> Result<Option<u32>> {
    optional_u64(table, context, field)?
        .map(|value| {
            u32::try_from(value).map_err(|_| RsdlError::InvalidValue {
                context: context.to_string(),
                message: format!("`{field}` is too large for u32"),
            })
        })
        .transpose()
}

fn optional_u64(table: &Table, context: &str, field: &'static str) -> Result<Option<u64>> {
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

fn expect_table_value<'a>(context: &str, field: &str, value: &'a Value) -> Result<&'a Table> {
    value.as_table().ok_or_else(|| RsdlError::InvalidFieldType {
        context: context.to_string(),
        field: field.to_string(),
        expected: "table",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_rsdl_document() {
        let source = r#"
[package]
name = "robot_demo"
version = "0.1.0"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.imu_sim]
language = "rust"
output = ["imu:Imu"]

[instance.imu_sim]
component = "imu_sim"
process = "main"
target = "linux"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[target.linux]
platform = "linux-x86_64"
runtime = ["rust"]
backends = ["inproc"]
"#;

        let document = parse_str(source).expect("document should parse");
        assert_eq!(document.package.name, "robot_demo");
        assert_eq!(document.types["Imu"].fields[0].name, "timestamp");
        assert_eq!(document.components["imu_sim"].output[0].name, "imu");
        assert_eq!(
            document.instances["imu_sim"].task.as_ref().unwrap().trigger,
            "periodic"
        );
    }

    #[test]
    fn rejects_invalid_port_descriptor() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.bad]
language = "rust"
input = ["odom"]
"#;

        let error = parse_str(source).expect_err("invalid port descriptor should fail");
        assert!(matches!(error, RsdlError::InvalidPortDescriptor { .. }));
    }
}
