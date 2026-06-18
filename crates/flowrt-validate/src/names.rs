use std::collections::BTreeSet;

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

    let mut type_generated_symbols = BTreeSet::new();
    for ty in &ir.types {
        validate_name("type", "type name", &ty.name, NameStyle::PascalCase, errors);
        validate_name(
            "type",
            "type generated name",
            &ty.generated_name,
            NameStyle::GeneratedSymbol,
            errors,
        );
        validate_canonical_generated_name(
            "type",
            &ty.qualified_name,
            ty.module.as_deref(),
            &ty.name,
            &ty.generated_name,
            errors,
        );
        if !type_generated_symbols.insert(ty.generated_name.as_str()) {
            errors.push(ValidationError::new(format!(
                "contract has duplicate type generated symbol `{}`",
                ty.generated_name
            )));
        }
        for field in &ty.fields {
            validate_name(
                "field",
                "field name",
                &field.name,
                NameStyle::SnakeCaseIdentifier,
                errors,
            );
        }
    }

    let mut component_generated_symbols = BTreeSet::new();
    for component in &ir.components {
        validate_name(
            "component",
            "component name",
            &component.name,
            NameStyle::SnakeCase,
            errors,
        );
        validate_name(
            "component",
            "component generated name",
            &component.generated_name,
            NameStyle::GeneratedSymbol,
            errors,
        );
        validate_canonical_generated_name(
            "component",
            &component.qualified_name,
            component.module.as_deref(),
            &component.name,
            &component.generated_name,
            errors,
        );
        if !component_generated_symbols.insert(component.generated_name.as_str()) {
            errors.push(ValidationError::new(format!(
                "contract has duplicate component generated symbol `{}`",
                component.generated_name
            )));
        }
        for port in component.inputs.iter().chain(component.outputs.iter()) {
            validate_name(
                "port",
                "port name",
                &port.name,
                NameStyle::SnakeCaseIdentifier,
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
                NameStyle::SnakeCaseIdentifier,
                errors,
            );
        }
        for port in component
            .operation_clients
            .iter()
            .chain(component.operation_servers.iter())
        {
            validate_name(
                "operation",
                "operation port name",
                &port.name,
                NameStyle::SnakeCaseIdentifier,
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
                NameStyle::SnakeCaseIdentifier,
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
                NameStyle::SnakeCaseIdentifier,
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

fn validate_canonical_generated_name(
    kind: &str,
    qualified_name: &str,
    module: Option<&str>,
    name: &str,
    generated_name: &str,
    errors: &mut Vec<ValidationError>,
) {
    let expected = flowrt_ir::canonical_generated_symbol(module, name);
    if generated_name != expected {
        errors.push(ValidationError::new(format!(
            "{kind} `{qualified_name}` generated name `{generated_name}` does not match canonical `{expected}`"
        )));
    }
}

#[derive(Debug, Clone, Copy)]
enum NameStyle {
    SnakeCase,
    /// snake_case 且会被 codegen 直接 emit 为 Rust/C++ 标识符的名称（字段、端口、
    /// instance、task）。除 snake_case 外，还必须不与任一目标语言的保留关键字冲突，
    /// 否则生成 shell 会出现 `in:`、`class;` 之类无法编译的标识符。
    SnakeCaseIdentifier,
    PascalCase,
    GeneratedSymbol,
}

impl NameStyle {
    fn label(self) -> &'static str {
        match self {
            NameStyle::SnakeCase | NameStyle::SnakeCaseIdentifier => "snake_case",
            NameStyle::PascalCase => "PascalCase",
            NameStyle::GeneratedSymbol => "a non-empty generated identifier",
        }
    }

    fn accepts(self, name: &str) -> bool {
        match self {
            NameStyle::SnakeCase | NameStyle::SnakeCaseIdentifier => is_snake_case(name),
            NameStyle::PascalCase => is_pascal_case(name),
            NameStyle::GeneratedSymbol => is_generated_symbol(name),
        }
    }

    /// 该 name style 是否要求名称同时是合法的跨语言标识符（拒绝保留关键字）。
    fn requires_identifier(self) -> bool {
        matches!(self, NameStyle::SnakeCaseIdentifier)
    }
}

fn validate_name(
    entity_kind: &'static str,
    label: &'static str,
    name: &str,
    style: NameStyle,
    errors: &mut Vec<ValidationError>,
) {
    let style_accepts = style.accepts(name);
    if !style_accepts {
        errors.push(ValidationError::new(format!(
            "{label} `{name}` must be {}",
            style.label()
        )));
    }
    if style_accepts && style.requires_identifier() && is_reserved_cross_language_keyword(name) {
        errors.push(ValidationError::new(format!(
            "{label} `{name}` collides with a reserved Rust/C++ keyword"
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

fn is_reserved_cross_language_keyword(name: &str) -> bool {
    matches!(
        name,
        "abstract"
            | "alignas"
            | "alignof"
            | "and"
            | "and_eq"
            | "as"
            | "asm"
            | "async"
            | "atomic_cancel"
            | "atomic_commit"
            | "atomic_noexcept"
            | "auto"
            | "await"
            | "become"
            | "bitand"
            | "bitor"
            | "bool"
            | "box"
            | "break"
            | "case"
            | "catch"
            | "char"
            | "char16_t"
            | "char32_t"
            | "char8_t"
            | "class"
            | "co_await"
            | "co_return"
            | "co_yield"
            | "compl"
            | "concept"
            | "const"
            | "const_cast"
            | "consteval"
            | "constexpr"
            | "constinit"
            | "continue"
            | "crate"
            | "decltype"
            | "default"
            | "delete"
            | "do"
            | "double"
            | "dyn"
            | "dynamic_cast"
            | "else"
            | "enum"
            | "explicit"
            | "export"
            | "extern"
            | "false"
            | "final"
            | "float"
            | "fn"
            | "for"
            | "friend"
            | "gen"
            | "goto"
            | "if"
            | "impl"
            | "import"
            | "in"
            | "inline"
            | "int"
            | "let"
            | "long"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "module"
            | "move"
            | "mut"
            | "mutable"
            | "namespace"
            | "new"
            | "noexcept"
            | "not"
            | "not_eq"
            | "nullptr"
            | "operator"
            | "or"
            | "or_eq"
            | "override"
            | "priv"
            | "private"
            | "protected"
            | "pub"
            | "public"
            | "ref"
            | "register"
            | "reinterpret_cast"
            | "requires"
            | "return"
            | "self"
            | "short"
            | "signed"
            | "sizeof"
            | "static"
            | "static_assert"
            | "static_cast"
            | "struct"
            | "super"
            | "switch"
            | "template"
            | "this"
            | "thread_local"
            | "throw"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typedef"
            | "typeid"
            | "typename"
            | "typeof"
            | "union"
            | "unsafe"
            | "unsigned"
            | "unsized"
            | "use"
            | "using"
            | "virtual"
            | "void"
            | "volatile"
            | "wchar_t"
            | "where"
            | "while"
            | "xor"
            | "xor_eq"
            | "yield"
    )
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

fn is_generated_symbol(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
