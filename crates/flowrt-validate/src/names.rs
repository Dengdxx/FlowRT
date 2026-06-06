use flowrt_ir::ContractIr;

use crate::ValidationError;

pub(crate) fn validate_names(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    validate_name(
        "package",
        "package name",
        &ir.package.name,
        NameStyle::SnakeCase,
        errors,
    );

    for ty in &ir.types {
        validate_name("type", "type name", &ty.name, NameStyle::PascalCase, errors);
        for field in &ty.fields {
            validate_name(
                "field",
                "field name",
                &field.name,
                NameStyle::SnakeCase,
                errors,
            );
        }
    }

    for component in &ir.components {
        validate_name(
            "component",
            "component name",
            &component.name,
            NameStyle::SnakeCase,
            errors,
        );
        for port in component.inputs.iter().chain(component.outputs.iter()) {
            validate_name(
                "port",
                "port name",
                &port.name,
                NameStyle::SnakeCase,
                errors,
            );
        }
        for port in component
            .service_clients
            .iter()
            .chain(component.service_servers.iter())
        {
            validate_name(
                "service",
                "service port name",
                &port.name,
                NameStyle::SnakeCase,
                errors,
            );
        }
    }

    for profile in &ir.profiles {
        validate_name(
            "profile",
            "profile name",
            &profile.name,
            NameStyle::SnakeCase,
            errors,
        );
    }

    for target in &ir.targets {
        validate_name(
            "target",
            "target name",
            &target.name,
            NameStyle::SnakeCase,
            errors,
        );
    }

    for graph in &ir.graphs {
        validate_name(
            "graph",
            "graph name",
            &graph.name,
            NameStyle::SnakeCase,
            errors,
        );
        for instance in &graph.instances {
            validate_name(
                "instance",
                "instance name",
                &instance.name,
                NameStyle::SnakeCase,
                errors,
            );
            if let Some(process) = &instance.process {
                validate_name(
                    "process",
                    "process name",
                    process,
                    NameStyle::SnakeCase,
                    errors,
                );
            }
        }
        for process in &graph.processes {
            validate_name(
                "process",
                "process name",
                &process.name,
                NameStyle::SnakeCase,
                errors,
            );
            for dependency in &process.depends_on {
                validate_name(
                    "process",
                    "process dependency name",
                    dependency,
                    NameStyle::SnakeCase,
                    errors,
                );
            }
        }
        for task in &graph.tasks {
            validate_name(
                "task",
                "task name",
                &task.name,
                NameStyle::SnakeCase,
                errors,
            );
            if let Some(lane) = &task.lane {
                validate_name("lane", "lane name", lane, NameStyle::SnakeCase, errors);
            }
        }
        for service in &graph.services {
            if let Some(lane) = &service.policy.lane {
                validate_name(
                    "lane",
                    "service lane name",
                    lane,
                    NameStyle::SnakeCase,
                    errors,
                );
            }
        }
        for bridge in &graph.ros2_bridges {
            validate_name(
                "ROS2 bridge",
                "ROS2 bridge name",
                &bridge.name,
                NameStyle::SnakeCase,
                errors,
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum NameStyle {
    SnakeCase,
    PascalCase,
}

impl NameStyle {
    fn label(self) -> &'static str {
        match self {
            NameStyle::SnakeCase => "snake_case",
            NameStyle::PascalCase => "PascalCase",
        }
    }

    fn accepts(self, name: &str) -> bool {
        match self {
            NameStyle::SnakeCase => is_snake_case(name),
            NameStyle::PascalCase => is_pascal_case(name),
        }
    }
}

fn validate_name(
    entity_kind: &'static str,
    label: &'static str,
    name: &str,
    style: NameStyle,
    errors: &mut Vec<ValidationError>,
) {
    if !style.accepts(name) {
        errors.push(ValidationError::new(format!(
            "{label} `{name}` must be {}",
            style.label()
        )));
    }
    if name
        .get(.."flowrt".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("flowrt"))
    {
        errors.push(ValidationError::new(format!(
            "{entity_kind} name `{name}` uses reserved `flowrt` prefix"
        )));
    }
}

fn is_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }

    let mut previous_underscore = false;
    for ch in chars {
        match ch {
            '_' if !previous_underscore => previous_underscore = true,
            '_' => return false,
            'a'..='z' | '0'..='9' => previous_underscore = false,
            _ => return false,
        }
    }
    !previous_underscore
}

fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric()) && name.chars().any(|ch| ch.is_ascii_lowercase())
}
