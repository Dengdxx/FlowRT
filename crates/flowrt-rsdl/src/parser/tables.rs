use std::collections::BTreeMap;

use toml::Value;
use toml::value::Table;

use crate::ast::*;
use crate::{Result, RsdlError};

use super::schema::validate_known_fields;
use super::values::optional_bool;
use super::values::{
    expect_string, expect_string_array, expect_table_value, optional_i32, optional_param_table,
    optional_port_array, optional_service_port_array, optional_string, optional_string_array,
    optional_string_table, optional_u32, optional_u32_array, optional_u64, required_string,
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
    let mut empty = false;
    for (field_name, value) in table {
        if field_name == "empty" {
            if let Some(flag) = value.as_bool() {
                empty = flag;
                continue;
            }
        }
        let ty = expect_string(&format!("type.{name}"), field_name, value)?;
        fields.push(RawField {
            name: field_name.clone(),
            ty,
        });
    }
    Ok(RawType { empty, fields })
}

pub(super) fn parse_component(name: &str, table: &Table) -> Result<RawComponent> {
    let context = format!("component.{name}");
    validate_known_fields(
        table,
        &context,
        &[
            "language",
            "kind",
            "concurrency",
            "build",
            "input",
            "output",
            "service_client",
            "service_server",
            "operation_client",
            "operation_server",
            "params",
            "io_side_effect",
            "io_readiness",
            "io_health",
            "io_shutdown",
            "resource",
        ],
    )?;

    Ok(RawComponent {
        language: required_string(table, &context, "language")?,
        kind: optional_string(table, &context, "kind")?,
        concurrency: optional_declared_concurrency(
            table,
            &context,
            "concurrency",
            "component concurrency",
        )?,
        build: optional_component_build(table, &context)?,
        input: optional_port_array(table, &context, "input")?,
        output: optional_port_array(table, &context, "output")?,
        service_clients: optional_service_port_array(table, &context, "service_client")?,
        service_servers: optional_service_port_array(table, &context, "service_server")?,
        operation_clients: optional_operation_port_table(table, &context, "operation_client")?,
        operation_servers: optional_operation_port_table(table, &context, "operation_server")?,
        params: optional_param_table(table, &context, "params")?,
        io_side_effect: optional_string_array(table, &context, "io_side_effect")?,
        io_readiness: optional_string(table, &context, "io_readiness")?,
        io_health: optional_string(table, &context, "io_health")?,
        io_shutdown: optional_string(table, &context, "io_shutdown")?,
        resources: optional_resource_table(table, &context)?,
    })
}

fn optional_component_build(table: &Table, context: &str) -> Result<RawComponentBuild> {
    let Some(value) = table.get("build") else {
        return Ok(RawComponentBuild::default());
    };
    let build_table = expect_table_value(context, "build", value)?;
    let build_context = format!("{context}.build");
    validate_known_fields(build_table, &build_context, &["pkg_config"])?;
    Ok(RawComponentBuild {
        pkg_config: optional_string_array(build_table, &build_context, "pkg_config")?,
    })
}

fn optional_resource_table(table: &Table, context: &str) -> Result<Vec<RawResourceRequirement>> {
    let Some(value) = table.get("resource") else {
        return Ok(Vec::new());
    };
    let table = expect_table_value(context, "resource", value)?;
    let mut resources = Vec::with_capacity(table.len());
    for (name, value) in table {
        let resource_table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: format!("{context}.resource"),
                field: name.clone(),
                expected: "table",
            })?;
        let resource_context = format!("{context}.resource.{name}");
        validate_known_fields(
            resource_table,
            &resource_context,
            &[
                "capability",
                "access",
                "required",
                "readiness",
                "health",
                "on_failure",
                "descriptor",
            ],
        )?;
        resources.push(RawResourceRequirement {
            name: name.clone(),
            capability: required_string(resource_table, &resource_context, "capability")?,
            access: optional_string(resource_table, &resource_context, "access")?,
            required: if resource_table.contains_key("required") {
                optional_bool(resource_table, &resource_context, "required")?
            } else {
                true
            },
            readiness: optional_string(resource_table, &resource_context, "readiness")?,
            health: optional_string(resource_table, &resource_context, "health")?,
            on_failure: optional_string(resource_table, &resource_context, "on_failure")?,
            descriptor: optional_resource_descriptor(resource_table, &resource_context)?,
        });
    }
    Ok(resources)
}

fn optional_resource_descriptor(
    table: &Table,
    context: &str,
) -> Result<Option<RawResourceDescriptor>> {
    let Some(value) = table.get("descriptor") else {
        return Ok(None);
    };
    let descriptor_table = expect_table_value(context, "descriptor", value)?;
    let descriptor_context = format!("{context}.descriptor");
    validate_known_fields(
        descriptor_table,
        &descriptor_context,
        &[
            "kind",
            "port",
            "format",
            "encoding",
            "metadata",
            "record_payload",
        ],
    )?;
    Ok(Some(RawResourceDescriptor {
        kind: required_string(descriptor_table, &descriptor_context, "kind")?,
        port: optional_string(descriptor_table, &descriptor_context, "port")?,
        format: required_string(descriptor_table, &descriptor_context, "format")?,
        encoding: optional_string(descriptor_table, &descriptor_context, "encoding")?,
        metadata: optional_string_table(descriptor_table, &descriptor_context, "metadata")?,
        record_payload: if descriptor_table.contains_key("record_payload") {
            optional_bool(descriptor_table, &descriptor_context, "record_payload")?
        } else {
            false
        },
    }))
}

fn optional_operation_port_table(
    table: &Table,
    context: &str,
    field: &'static str,
) -> Result<Vec<RawOperationPort>> {
    let Some(value) = table.get(field) else {
        return Ok(Vec::new());
    };
    let table = expect_table_value(context, field, value)?;
    let mut ports = Vec::with_capacity(table.len());
    for (name, value) in table {
        let port_table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: format!("{context}.{field}"),
                field: name.clone(),
                expected: "table",
            })?;
        let port_context = format!("{context}.{field}.{name}");
        validate_known_fields(port_table, &port_context, &["goal", "feedback", "result"])?;
        ports.push(RawOperationPort {
            name: name.clone(),
            goal: required_string(port_table, &port_context, "goal")?,
            feedback: required_string(port_table, &port_context, "feedback")?,
            result: required_string(port_table, &port_context, "result")?,
        });
    }
    Ok(ports)
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
            "concurrency",
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
        concurrency: optional_declared_concurrency(
            table,
            &context,
            "concurrency",
            "task concurrency",
        )?,
        readiness: optional_string(table, &context, "readiness")?,
        period_ms: optional_u64(table, &context, "period_ms")?,
        deadline_ms: optional_u64(table, &context, "deadline_ms")?,
        lane: optional_string(table, &context, "lane")?,
        priority: optional_u32(table, &context, "priority")?,
        input: optional_string_array(table, &context, "input")?,
        output: optional_string_array(table, &context, "output")?,
    })
}

fn optional_declared_concurrency(
    table: &Table,
    context: &str,
    field: &'static str,
    kind: &'static str,
) -> Result<Option<String>> {
    let value = optional_string(table, context, field)?;
    if let Some(concurrency) = &value {
        validate_declared_concurrency(&format!("{context}.{field}"), kind, concurrency)?;
    }
    Ok(value)
}

fn validate_declared_concurrency(context: &str, kind: &str, value: &str) -> Result<()> {
    match value {
        "exclusive" | "parallel" => Ok(()),
        _ => Err(RsdlError::InvalidValue {
            context: context.to_string(),
            message: format!("{kind} must be `exclusive` or `parallel`"),
        }),
    }
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
                "readiness",
                "startup_delay_ms",
                "env",
                "cpu_affinity",
                "nice",
                "rt_policy",
                "rt_priority",
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
            readiness: optional_string(table, &context, "readiness")?,
            startup_delay_ms: optional_u64(table, &context, "startup_delay_ms")?,
            env: optional_string_table(table, &context, "env")?,
            cpu_affinity: optional_u32_array(table, &context, "cpu_affinity")?,
            nice: optional_i32(table, &context, "nice")?,
            rt_policy: optional_string(table, &context, "rt_policy")?,
            rt_priority: optional_u32(table, &context, "rt_priority")?,
        });
    }
    Ok(parsed)
}

pub(super) fn parse_external_processes(root: &Table) -> Result<Vec<RawExternalProcess>> {
    let Some(process_value) = root.get("external_process") else {
        return Ok(Vec::new());
    };
    let processes = process_value
        .as_array()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "document".to_string(),
            field: "external_process".to_string(),
            expected: "array of tables",
        })?;

    let mut parsed = Vec::with_capacity(processes.len());
    for (index, value) in processes.iter().enumerate() {
        let context = format!("external_process[{index}]");
        let table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: "document".to_string(),
                field: "external_process".to_string(),
                expected: "array of tables",
            })?;
        validate_known_fields(
            table,
            &context,
            &[
                "process",
                "package",
                "executable",
                "args",
                "working_dir",
                "health",
                "required_backends",
            ],
        )?;
        parsed.push(RawExternalProcess {
            process: required_string(table, &context, "process")?,
            package: required_string(table, &context, "package")?,
            executable: required_string(table, &context, "executable")?,
            args: optional_string_array(table, &context, "args")?,
            working_dir: optional_string(table, &context, "working_dir")?,
            health: optional_string(table, &context, "health")?,
            required_backends: optional_string_array(table, &context, "required_backends")?,
        });
    }
    Ok(parsed)
}

pub(super) fn parse_resource_providers(root: &Table) -> Result<Vec<RawResourceProvider>> {
    let Some(resource_value) = root.get("resource") else {
        return Ok(Vec::new());
    };
    let resource_table = resource_value
        .as_table()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "document".to_string(),
            field: "resource".to_string(),
            expected: "table",
        })?;
    validate_known_fields(resource_table, "resource", &["provider"])?;
    let Some(provider_value) = resource_table.get("provider") else {
        return Ok(Vec::new());
    };
    let providers = provider_value
        .as_array()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "resource".to_string(),
            field: "provider".to_string(),
            expected: "array of tables",
        })?;

    let mut parsed = Vec::with_capacity(providers.len());
    for (index, value) in providers.iter().enumerate() {
        let context = format!("resource.provider[{index}]");
        let table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: "resource".to_string(),
                field: "provider".to_string(),
                expected: "array of tables",
            })?;
        validate_known_fields(
            table,
            &context,
            &[
                "name",
                "capabilities",
                "scope",
                "target",
                "process",
                "external_package",
                "health_source",
                "readiness_source",
            ],
        )?;
        parsed.push(RawResourceProvider {
            name: required_string(table, &context, "name")?,
            capabilities: optional_string_array(table, &context, "capabilities")?,
            scope: required_string(table, &context, "scope")?,
            target: optional_string(table, &context, "target")?,
            process: optional_string(table, &context, "process")?,
            external_package: optional_string(table, &context, "external_package")?,
            health_source: required_string(table, &context, "health_source")?,
            readiness_source: required_string(table, &context, "readiness_source")?,
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
    validate_known_fields(bind_table, "bind", &["dataflow", "service", "operation"])?;
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
    validate_known_fields(bind_table, "bind", &["dataflow", "service", "operation"])?;
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
        validate_known_fields(
            table,
            &context,
            &[
                "client",
                "server",
                "backend",
                "timeout_ms",
                "queue_depth",
                "overflow",
                "lane",
                "max_in_flight",
            ],
        )?;
        parsed.push(RawServiceBind {
            client: required_string(table, &context, "client")?,
            server: required_string(table, &context, "server")?,
            backend: optional_string(table, &context, "backend")?,
            timeout_ms: optional_u64(table, &context, "timeout_ms")?,
            queue_depth: optional_u32(table, &context, "queue_depth")?,
            overflow: optional_string(table, &context, "overflow")?,
            lane: optional_string(table, &context, "lane")?,
            max_in_flight: optional_u32(table, &context, "max_in_flight")?,
        });
    }
    Ok(parsed)
}

pub(super) fn parse_operation_binds(root: &Table) -> Result<Vec<RawOperationBind>> {
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
    validate_known_fields(bind_table, "bind", &["dataflow", "service", "operation"])?;
    let Some(operation_value) = bind_table.get("operation") else {
        return Ok(Vec::new());
    };
    let binds = operation_value
        .as_array()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "bind".to_string(),
            field: "operation".to_string(),
            expected: "array of tables",
        })?;

    let mut parsed = Vec::with_capacity(binds.len());
    for (index, value) in binds.iter().enumerate() {
        let context = format!("bind.operation[{index}]");
        let table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: "bind".to_string(),
                field: "operation".to_string(),
                expected: "array of tables",
            })?;
        validate_known_fields(
            table,
            &context,
            &[
                "client",
                "server",
                "backend",
                "timeout_ms",
                "concurrency",
                "preempt",
                "queue_depth",
                "max_in_flight",
                "feedback",
                "result_retention_ms",
            ],
        )?;
        parsed.push(RawOperationBind {
            client: required_string(table, &context, "client")?,
            server: required_string(table, &context, "server")?,
            backend: optional_string(table, &context, "backend")?,
            timeout_ms: optional_u64(table, &context, "timeout_ms")?,
            concurrency: optional_string(table, &context, "concurrency")?,
            preempt: optional_string(table, &context, "preempt")?,
            queue_depth: optional_u32(table, &context, "queue_depth")?,
            max_in_flight: optional_u32(table, &context, "max_in_flight")?,
            feedback: optional_string(table, &context, "feedback")?,
            result_retention_ms: optional_u64(table, &context, "result_retention_ms")?,
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

pub(super) fn parse_boundary_endpoints(
    root: &Table,
    direction: &'static str,
) -> Result<Vec<RawBoundaryEndpoint>> {
    let Some(boundary_value) = root.get("boundary") else {
        return Ok(Vec::new());
    };
    let boundary_table = boundary_value
        .as_table()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "document".to_string(),
            field: "boundary".to_string(),
            expected: "table",
        })?;
    validate_known_fields(boundary_table, "boundary", &["input", "output"])?;
    let Some(endpoint_value) = boundary_table.get(direction) else {
        return Ok(Vec::new());
    };
    let endpoints = endpoint_value
        .as_array()
        .ok_or_else(|| RsdlError::InvalidFieldType {
            context: "boundary".to_string(),
            field: direction.to_string(),
            expected: "array of tables",
        })?;

    let mut parsed = Vec::with_capacity(endpoints.len());
    let mut names = std::collections::BTreeSet::new();
    for (index, value) in endpoints.iter().enumerate() {
        let context = format!("boundary.{direction}[{index}]");
        let table = value
            .as_table()
            .ok_or_else(|| RsdlError::InvalidFieldType {
                context: "boundary".to_string(),
                field: direction.to_string(),
                expected: "array of tables",
            })?;
        validate_known_fields(table, &context, &["name", "port", "type"])?;
        let endpoint = RawBoundaryEndpoint {
            name: required_string(table, &context, "name")?,
            port: required_string(table, &context, "port")?,
            ty: required_string(table, &context, "type")?,
        };
        if !names.insert(endpoint.name.clone()) {
            return Err(RsdlError::DuplicateSymbol {
                kind: if direction == "input" {
                    "boundary.input"
                } else {
                    "boundary.output"
                },
                name: endpoint.name,
            });
        }
        parsed.push(endpoint);
    }
    Ok(parsed)
}

pub(super) fn parse_profile(name: &str, table: &Table) -> Result<RawProfile> {
    let context = format!("profile.{name}");
    validate_known_fields(
        table,
        &context,
        &[
            "mode",
            "backend",
            "worker_threads",
            "default_overflow",
            "default_stale_policy",
            "max_age_ms",
        ],
    )?;

    Ok(RawProfile {
        mode: parse_graph_mode(table, &context)?,
        backend: optional_string(table, &context, "backend")?,
        worker_threads: optional_u32(table, &context, "worker_threads")?,
        default_overflow: optional_string(table, &context, "default_overflow")?,
        default_stale_policy: optional_string(table, &context, "default_stale_policy")?,
        max_age_ms: optional_u64(table, &context, "max_age_ms")?,
    })
}

fn parse_graph_mode(table: &Table, context: &str) -> Result<RawGraphMode> {
    let Some(mode) = optional_string(table, context, "mode")? else {
        return Ok(RawGraphMode::Strict);
    };
    match mode.as_str() {
        "strict" => Ok(RawGraphMode::Strict),
        "island" => Ok(RawGraphMode::Island),
        _ => Err(RsdlError::InvalidValue {
            context: format!("{context}.mode"),
            message: "profile mode must be `strict` or `island`".to_string(),
        }),
    }
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
