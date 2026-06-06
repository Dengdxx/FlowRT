use std::collections::BTreeMap;

use toml::Value;
use toml::value::Table;

use crate::ast::*;
use crate::{Result, RsdlError};

use super::schema::validate_known_fields;
use super::values::{
    expect_string, expect_string_array, expect_table_value, optional_param_table,
    optional_port_array, optional_service_port_array, optional_string, optional_string_array,
    optional_u32, optional_u64, required_string,
};

pub(super) fn parse_named_tables<T>(
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

pub(super) fn parse_package(table: &Table) -> Result<RawPackage> {
    validate_known_fields(
        table,
        "package",
        &["name", "version", "rsdl_version", "imports"],
    )?;

    let mut imports = BTreeMap::new();
    if let Some(value) = table.get("imports") {
        let table = expect_table_value("package", "imports", value)?;
        validate_known_fields(
            table,
            "package.imports",
            &["types", "components", "graphs", "profiles", "targets"],
        )?;
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

pub(super) fn parse_workspace(table: &Table) -> Result<RawWorkspace> {
    validate_known_fields(table, "workspace", &["modules", "compositions"])?;
    Ok(RawWorkspace {
        modules: optional_string_array(table, "workspace", "modules")?,
        compositions: optional_string_array(table, "workspace", "compositions")?,
    })
}

pub(super) fn parse_module(table: &Table) -> Result<RawModule> {
    validate_known_fields(table, "module", &["name"])?;
    Ok(RawModule {
        name: required_string(table, "module", "name")?,
    })
}

pub(super) fn parse_type(name: &str, table: &Table) -> Result<RawType> {
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

pub(super) fn parse_component(name: &str, table: &Table) -> Result<RawComponent> {
    let context = format!("component.{name}");
    validate_known_fields(
        table,
        &context,
        &[
            "language",
            "kind",
            "input",
            "output",
            "service_client",
            "service_server",
            "params",
        ],
    )?;

    Ok(RawComponent {
        language: required_string(table, &context, "language")?,
        kind: optional_string(table, &context, "kind")?,
        input: optional_port_array(table, &context, "input")?,
        output: optional_port_array(table, &context, "output")?,
        service_clients: optional_service_port_array(table, &context, "service_client")?,
        service_servers: optional_service_port_array(table, &context, "service_server")?,
        params: optional_param_table(table, &context, "params")?,
    })
}

pub(super) fn parse_instance(name: &str, table: &Table) -> Result<RawInstance> {
    let context = format!("instance.{name}");
    validate_known_fields(
        table,
        &context,
        &["component", "process", "target", "params", "task"],
    )?;

    let tasks = table
        .get("task")
        .map(|value| parse_tasks(name, value))
        .transpose()?
        .unwrap_or_default();

    Ok(RawInstance {
        component: required_string(table, &context, "component")?,
        process: optional_string(table, &context, "process")?,
        target: optional_string(table, &context, "target")?,
        params: optional_param_table(table, &context, "params")?,
        tasks,
    })
}

fn parse_tasks(instance_name: &str, value: &Value) -> Result<Vec<RawTask>> {
    if let Some(table) = value.as_table() {
        return Ok(vec![parse_task(instance_name, table)?]);
    }

    let Some(tasks) = value.as_array() else {
        return Err(RsdlError::InvalidFieldType {
            context: format!("instance.{instance_name}"),
            field: "task".to_string(),
            expected: "table or array of tables",
        });
    };

    tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let table = task.as_table().ok_or_else(|| RsdlError::InvalidFieldType {
                context: format!("instance.{instance_name}.task[{index}]"),
                field: "task".to_string(),
                expected: "table",
            })?;
            let task = parse_task(instance_name, table)?;
            if task.name.is_none() {
                return Err(RsdlError::MissingField {
                    context: format!("instance.{instance_name}.task[{index}]"),
                    field: "name",
                });
            }
            Ok(task)
        })
        .collect()
}

fn parse_task(instance_name: &str, table: &Table) -> Result<RawTask> {
    let context = format!("instance.{instance_name}.task");
    validate_known_fields(
        table,
        &context,
        &[
            "name",
            "trigger",
            "readiness",
            "period_ms",
            "deadline_ms",
            "lane",
            "priority",
            "input",
            "output",
        ],
    )?;

    Ok(RawTask {
        name: optional_string(table, &context, "name")?,
        trigger: required_string(table, &context, "trigger")?,
        readiness: optional_string(table, &context, "readiness")?,
        period_ms: optional_u64(table, &context, "period_ms")?,
        deadline_ms: optional_u64(table, &context, "deadline_ms")?,
        lane: optional_string(table, &context, "lane")?,
        priority: optional_u32(table, &context, "priority")?,
        input: optional_string_array(table, &context, "input")?,
        output: optional_string_array(table, &context, "output")?,
    })
}

pub(super) fn parse_processes(root: &Table) -> Result<Vec<RawProcess>> {
    let Some(process_value) = root.get("process") else {
        return Ok(Vec::new());
    };
    let processes = process_value
        .as_array()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "document".to_string(),
            field: "process".to_string(),
            expected: "array of tables",
        })?;

    let mut parsed = Vec::with_capacity(processes.len());
    for (index, value) in processes.iter().enumerate() {
        let context = format!("process[{index}]");
        let table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: "document".to_string(),
                field: "process".to_string(),
                expected: "array of tables",
            })?;
        validate_known_fields(
            table,
            &context,
            &[
                "name",
                "depends_on",
                "restart",
                "max_restarts",
                "initial_delay_ms",
                "max_delay_ms",
                "failure",
            ],
        )?;
        parsed.push(RawProcess {
            name: required_string(table, &context, "name")?,
            depends_on: optional_string_array(table, &context, "depends_on")?,
            restart: optional_string(table, &context, "restart")?,
            max_restarts: optional_u32(table, &context, "max_restarts")?,
            initial_delay_ms: optional_u64(table, &context, "initial_delay_ms")?,
            max_delay_ms: optional_u64(table, &context, "max_delay_ms")?,
            failure: optional_string(table, &context, "failure")?,
        });
    }
    Ok(parsed)
}

pub(super) fn parse_binds(root: &Table) -> Result<Vec<RawDataflowBind>> {
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
    validate_known_fields(bind_table, "bind", &["dataflow", "service"])?;
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
        validate_known_fields(
            table,
            &context,
            &[
                "from",
                "to",
                "backend",
                "channel",
                "depth",
                "overflow",
                "stale_policy",
                "max_age_ms",
            ],
        )?;
        parsed.push(RawDataflowBind {
            from: required_string(table, &context, "from")?,
            to: required_string(table, &context, "to")?,
            backend: optional_string(table, &context, "backend")?,
            channel: required_string(table, &context, "channel")?,
            depth: optional_u32(table, &context, "depth")?,
            overflow: optional_string(table, &context, "overflow")?,
            stale_policy: optional_string(table, &context, "stale_policy")?,
            max_age_ms: optional_u64(table, &context, "max_age_ms")?,
        });
    }
    Ok(parsed)
}

pub(super) fn parse_service_binds(root: &Table) -> Result<Vec<RawServiceBind>> {
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
    validate_known_fields(bind_table, "bind", &["dataflow", "service"])?;
    let Some(service_value) = bind_table.get("service") else {
        return Ok(Vec::new());
    };
    let binds = service_value
        .as_array()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "bind".to_string(),
            field: "service".to_string(),
            expected: "array of tables",
        })?;

    let mut parsed = Vec::with_capacity(binds.len());
    for (index, value) in binds.iter().enumerate() {
        let context = format!("bind.service[{index}]");
        let table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: "bind".to_string(),
                field: "service".to_string(),
                expected: "array of tables",
            })?;
        validate_known_fields(table, &context, &["client", "server"])?;
        parsed.push(RawServiceBind {
            client: required_string(table, &context, "client")?,
            server: required_string(table, &context, "server")?,
        });
    }
    Ok(parsed)
}

pub(super) fn parse_ros2_bridges(root: &Table) -> Result<Vec<RawRos2Bridge>> {
    let Some(bridge_value) = root.get("bridge") else {
        return Ok(Vec::new());
    };
    let bridge_table = bridge_value
        .as_table()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "document".to_string(),
            field: "bridge".to_string(),
            expected: "table",
        })?;
    validate_known_fields(bridge_table, "bridge", &["ros2"])?;
    let Some(ros2_value) = bridge_table.get("ros2") else {
        return Ok(Vec::new());
    };
    let bridges = ros2_value
        .as_array()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "bridge".to_string(),
            field: "ros2".to_string(),
            expected: "array of tables",
        })?;

    let mut parsed = Vec::with_capacity(bridges.len());
    for (index, value) in bridges.iter().enumerate() {
        let context = format!("bridge.ros2[{index}]");
        let table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: "bridge".to_string(),
                field: "ros2".to_string(),
                expected: "array of tables",
            })?;
        validate_known_fields(
            table,
            &context,
            &["flowrt", "ros2_topic", "ros2_type", "direction", "field"],
        )?;
        parsed.push(RawRos2Bridge {
            flowrt: required_string(table, &context, "flowrt")?,
            ros2_topic: required_string(table, &context, "ros2_topic")?,
            ros2_type: required_string(table, &context, "ros2_type")?,
            direction: required_string(table, &context, "direction")?,
            field: optional_string(table, &context, "field")?,
        });
    }
    Ok(parsed)
}

pub(super) fn parse_profile(name: &str, table: &Table) -> Result<RawProfile> {
    let context = format!("profile.{name}");
    validate_known_fields(
        table,
        &context,
        &[
            "backend",
            "worker_threads",
            "default_overflow",
            "default_stale_policy",
            "max_age_ms",
        ],
    )?;

    Ok(RawProfile {
        backend: optional_string(table, &context, "backend")?,
        worker_threads: optional_u32(table, &context, "worker_threads")?,
        default_overflow: optional_string(table, &context, "default_overflow")?,
        default_stale_policy: optional_string(table, &context, "default_stale_policy")?,
        max_age_ms: optional_u64(table, &context, "max_age_ms")?,
    })
}

pub(super) fn parse_target(name: &str, table: &Table) -> Result<RawTarget> {
    let context = format!("target.{name}");
    validate_known_fields(table, &context, &["platform", "runtime", "backends"])?;

    Ok(RawTarget {
        platform: optional_string(table, &context, "platform")?,
        runtime: optional_string_array(table, &context, "runtime")?,
        backends: optional_string_array(table, &context, "backends")?,
    })
}
