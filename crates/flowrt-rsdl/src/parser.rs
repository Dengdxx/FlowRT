use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use toml::Value;
use toml::value::Table;

use crate::ast::*;
use crate::{Result, RsdlError};

#[derive(Debug)]
struct ParsedDocument {
    package: Option<RawPackage>,
    workspace: Option<RawWorkspace>,
    module: Option<RawModule>,
    types: BTreeMap<String, RawType>,
    components: BTreeMap<String, RawComponent>,
    instances: BTreeMap<String, RawInstance>,
    processes: Vec<RawProcess>,
    binds: Vec<RawDataflowBind>,
    ros2_bridges: Vec<RawRos2Bridge>,
    profiles: BTreeMap<String, RawProfile>,
    targets: BTreeMap<String, RawTarget>,
}

/// 从磁盘解析一个 `.rsdl` 文件。
pub fn parse_file(path: impl AsRef<Path>) -> Result<RawDocument> {
    Ok(load_file(path)?.document)
}

/// 从磁盘加载一个 `.rsdl` 文件，并展开 `[package.imports]`。
pub fn load_file(path: impl AsRef<Path>) -> Result<LoadedDocument> {
    let path = path.as_ref();
    let root_path = canonicalize_existing(path)?;
    let package_root = root_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut loaded_paths = std::collections::BTreeSet::new();
    let mut sources = Vec::new();
    let mut document = load_root_document(&root_path, &package_root, &mut sources)?;
    loaded_paths.insert(root_path.clone());
    let mut modules = Vec::new();
    let mut compositions = Vec::new();

    if document.workspace.is_some() {
        expand_workspace(
            &mut document,
            &root_path,
            &package_root,
            &mut loaded_paths,
            &mut sources,
            &mut modules,
            &mut compositions,
        )?;
        return Ok(LoadedDocument {
            document,
            sources,
            modules,
            compositions,
        });
    }

    expand_imports(
        &mut document,
        &root_path,
        &package_root,
        &mut loaded_paths,
        &mut sources,
    )?;

    Ok(LoadedDocument {
        document,
        sources,
        modules,
        compositions,
    })
}

fn load_root_document(
    path: &Path,
    package_root: &Path,
    sources: &mut Vec<LoadedSource>,
) -> Result<RawDocument> {
    let source = read_source(path)?;
    sources.push(LoadedSource {
        path: logical_source_path(path, package_root),
        content: source.clone(),
    });
    parsed_to_raw(parse_source(&source, true)?)
}

fn load_import_document(
    path: &Path,
    package_root: &Path,
    sources: &mut Vec<LoadedSource>,
) -> Result<ParsedDocument> {
    let source = read_source(path)?;
    sources.push(LoadedSource {
        path: logical_source_path(path, package_root),
        content: source.clone(),
    });
    parse_source(&source, false)
}

fn read_source(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|source| RsdlError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// 解析 RSDL v0.1 源文本。
pub fn parse_str(source: &str) -> Result<RawDocument> {
    parsed_to_raw(parse_source(source, true)?)
}

fn parse_source(source: &str, require_package: bool) -> Result<ParsedDocument> {
    let value: Value = source.parse()?;
    let root = value.as_table().ok_or_else(|| RsdlError::InvalidValue {
        context: "document".to_string(),
        message: "expected a TOML table document".to_string(),
    })?;
    validate_top_level_sections(root)?;

    let package = match root.get("package").and_then(Value::as_table) {
        Some(package_table) => Some(parse_package(package_table)?),
        None if require_package => return Err(RsdlError::MissingPackage),
        None => None,
    };

    Ok(ParsedDocument {
        package,
        workspace: root
            .get("workspace")
            .and_then(Value::as_table)
            .map(parse_workspace)
            .transpose()?,
        module: root
            .get("module")
            .and_then(Value::as_table)
            .map(parse_module)
            .transpose()?,
        types: parse_named_tables(root, "type", parse_type)?,
        components: parse_named_tables(root, "component", parse_component)?,
        instances: parse_named_tables(root, "instance", parse_instance)?,
        processes: parse_processes(root)?,
        binds: parse_binds(root)?,
        ros2_bridges: parse_ros2_bridges(root)?,
        profiles: parse_named_tables(root, "profile", parse_profile)?,
        targets: parse_named_tables(root, "target", parse_target)?,
    })
}

fn validate_top_level_sections(root: &Table) -> Result<()> {
    const ALLOWED_SECTIONS: &[&str] = &[
        "package",
        "workspace",
        "module",
        "type",
        "component",
        "instance",
        "process",
        "bind",
        "bridge",
        "profile",
        "target",
    ];

    for section in root.keys() {
        if !ALLOWED_SECTIONS.contains(&section.as_str()) {
            return Err(RsdlError::UnknownTopLevelSection {
                section: section.clone(),
            });
        }
    }
    Ok(())
}

fn validate_known_fields(table: &Table, context: &str, allowed_fields: &[&str]) -> Result<()> {
    for field in table.keys() {
        if !allowed_fields.contains(&field.as_str()) {
            return Err(RsdlError::UnknownField {
                context: context.to_string(),
                field: field.clone(),
            });
        }
    }
    Ok(())
}

fn parsed_to_raw(parsed: ParsedDocument) -> Result<RawDocument> {
    Ok(RawDocument {
        package: parsed.package.ok_or(RsdlError::MissingPackage)?,
        workspace: parsed.workspace,
        types: parsed.types,
        components: parsed.components,
        instances: parsed.instances,
        processes: parsed.processes,
        binds: parsed.binds,
        ros2_bridges: parsed.ros2_bridges,
        profiles: parsed.profiles,
        targets: parsed.targets,
    })
}

fn expand_workspace(
    document: &mut RawDocument,
    root_path: &Path,
    package_root: &Path,
    loaded_paths: &mut std::collections::BTreeSet<PathBuf>,
    sources: &mut Vec<LoadedSource>,
    modules: &mut Vec<RawModuleDocument>,
    compositions: &mut Vec<RawCompositionDocument>,
) -> Result<()> {
    let workspace = document
        .workspace
        .clone()
        .expect("caller checked workspace presence");
    let mut module_names = std::collections::BTreeSet::new();

    for pattern in &workspace.modules {
        for path in expand_import_pattern(root_path, pattern)? {
            let path = canonicalize_existing(&path)?;
            if !loaded_paths.insert(path.clone()) {
                continue;
            }
            let parsed = load_import_document(&path, package_root, sources)?;
            let module = parsed
                .module
                .clone()
                .ok_or_else(|| RsdlError::MissingModule {
                    path: logical_source_path(&path, package_root),
                })?;
            validate_module_document(&path, package_root, &module, &parsed)?;
            if !module_names.insert(module.name.clone()) {
                return Err(RsdlError::DuplicateModule {
                    module: module.name,
                });
            }
            modules.push(RawModuleDocument {
                module,
                types: parsed.types,
                components: parsed.components,
                source: logical_source_path(&path, package_root),
            });
        }
    }

    for pattern in &workspace.compositions {
        for path in expand_import_pattern(root_path, pattern)? {
            let path = canonicalize_existing(&path)?;
            if !loaded_paths.insert(path.clone()) {
                continue;
            }
            let parsed = load_import_document(&path, package_root, sources)?;
            if parsed.module.is_some() {
                return Err(RsdlError::UnexpectedModule {
                    path: logical_source_path(&path, package_root),
                });
            }
            let composition = RawCompositionDocument {
                instances: parsed.instances.clone(),
                processes: parsed.processes.clone(),
                binds: parsed.binds.clone(),
                ros2_bridges: parsed.ros2_bridges.clone(),
                profiles: parsed.profiles.clone(),
                targets: parsed.targets.clone(),
                source: logical_source_path(&path, package_root),
            };
            merge_composition_document(document, parsed)?;
            compositions.push(composition);
        }
    }

    modules.sort_by(|left, right| left.module.name.cmp(&right.module.name));
    compositions.sort_by(|left, right| left.source.cmp(&right.source));
    Ok(())
}

fn validate_module_document(
    path: &Path,
    package_root: &Path,
    module: &RawModule,
    parsed: &ParsedDocument,
) -> Result<()> {
    let invalid = [
        (!parsed.instances.is_empty(), "instance"),
        (!parsed.processes.is_empty(), "process"),
        (!parsed.binds.is_empty(), "bind"),
        (!parsed.ros2_bridges.is_empty(), "bridge"),
        (!parsed.profiles.is_empty(), "profile"),
        (!parsed.targets.is_empty(), "target"),
        (parsed.workspace.is_some(), "workspace"),
        (parsed.package.is_some(), "package"),
    ]
    .into_iter()
    .find_map(|(present, section)| present.then_some(section));

    if let Some(section) = invalid {
        return Err(RsdlError::InvalidModuleSection {
            path: logical_source_path(path, package_root),
            module: module.name.clone(),
            section: section.to_string(),
        });
    }

    Ok(())
}

fn merge_composition_document(
    document: &mut RawDocument,
    composition: ParsedDocument,
) -> Result<()> {
    merge_named_map("instance", &mut document.instances, composition.instances)?;
    document.processes.extend(composition.processes);
    document.binds.extend(composition.binds);
    document.ros2_bridges.extend(composition.ros2_bridges);
    merge_named_map("profile", &mut document.profiles, composition.profiles)?;
    merge_named_map("target", &mut document.targets, composition.targets)?;
    Ok(())
}

fn expand_imports(
    document: &mut RawDocument,
    importer: &Path,
    package_root: &Path,
    loaded_paths: &mut std::collections::BTreeSet<PathBuf>,
    sources: &mut Vec<LoadedSource>,
) -> Result<()> {
    let imports = document.package.imports.clone();
    for pattern in imports.values().flatten() {
        let matches = expand_import_pattern(importer, pattern)?;
        for path in matches {
            let path = canonicalize_existing(&path)?;
            if !loaded_paths.insert(path.clone()) {
                continue;
            }

            let imported = load_import_document(&path, package_root, sources)?;
            let nested_imports = imported
                .package
                .as_ref()
                .map(|package| package.imports.clone())
                .unwrap_or_default();
            merge_imported_document(document, imported)?;
            expand_nested_imports(
                document,
                &path,
                package_root,
                nested_imports,
                loaded_paths,
                sources,
            )?;
        }
    }
    Ok(())
}

fn expand_nested_imports(
    document: &mut RawDocument,
    importer: &Path,
    package_root: &Path,
    imports: BTreeMap<String, Vec<String>>,
    loaded_paths: &mut std::collections::BTreeSet<PathBuf>,
    sources: &mut Vec<LoadedSource>,
) -> Result<()> {
    for pattern in imports.values().flatten() {
        let matches = expand_import_pattern(importer, pattern)?;
        for path in matches {
            let path = canonicalize_existing(&path)?;
            if !loaded_paths.insert(path.clone()) {
                continue;
            }
            let imported = load_import_document(&path, package_root, sources)?;
            let nested_imports = imported
                .package
                .as_ref()
                .map(|package| package.imports.clone())
                .unwrap_or_default();
            merge_imported_document(document, imported)?;
            expand_nested_imports(
                document,
                &path,
                package_root,
                nested_imports,
                loaded_paths,
                sources,
            )?;
        }
    }
    Ok(())
}

fn merge_imported_document(document: &mut RawDocument, imported: ParsedDocument) -> Result<()> {
    merge_named_map("type", &mut document.types, imported.types)?;
    merge_named_map("component", &mut document.components, imported.components)?;
    merge_named_map("instance", &mut document.instances, imported.instances)?;
    document.processes.extend(imported.processes);
    document.binds.extend(imported.binds);
    document.ros2_bridges.extend(imported.ros2_bridges);
    merge_named_map("profile", &mut document.profiles, imported.profiles)?;
    merge_named_map("target", &mut document.targets, imported.targets)?;
    Ok(())
}

fn merge_named_map<T>(
    kind: &'static str,
    target: &mut BTreeMap<String, T>,
    imported: BTreeMap<String, T>,
) -> Result<()> {
    for (name, value) in imported {
        if target.contains_key(&name) {
            return Err(RsdlError::DuplicateSymbol { kind, name });
        }
        target.insert(name, value);
    }
    Ok(())
}

fn expand_import_pattern(importer: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let importer_dir = importer.parent().unwrap_or_else(|| Path::new("."));
    validate_relative_import_path(importer, pattern)?;
    let components = pattern.split('/').collect::<Vec<_>>();
    let mut matches = Vec::new();
    expand_import_components(importer_dir, importer, pattern, &components, &mut matches)?;
    matches.sort();
    matches.dedup();
    if matches.is_empty() {
        return Err(RsdlError::ImportPatternNoMatches {
            importer: importer.to_path_buf(),
            pattern: pattern.to_string(),
        });
    }
    Ok(matches)
}

fn expand_import_components(
    base: &Path,
    importer: &Path,
    pattern: &str,
    components: &[&str],
    matches: &mut Vec<PathBuf>,
) -> Result<()> {
    if !base.exists() {
        return Ok(());
    }

    let Some((component, rest)) = components.split_first() else {
        if base.extension() == Some(std::ffi::OsStr::new("rsdl")) {
            matches.push(base.to_path_buf());
        }
        return Ok(());
    };

    if component.contains('*') {
        let mut entries = std::fs::read_dir(base)
            .map_err(|source| RsdlError::Io {
                path: base.to_path_buf(),
                source,
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|source| RsdlError::Io {
                path: base.to_path_buf(),
                source,
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if wildcard_match(component, &name) {
                expand_import_components(&entry.path(), importer, pattern, rest, matches)?;
            }
        }
        return Ok(());
    }

    let next = base.join(component);
    if rest.is_empty() && !next.exists() {
        return Err(RsdlError::ImportPatternNoMatches {
            importer: importer.to_path_buf(),
            pattern: pattern.to_string(),
        });
    }
    expand_import_components(&next, importer, pattern, rest, matches)
}

fn validate_relative_import_path(importer: &Path, pattern: &str) -> Result<()> {
    let path = Path::new(pattern);
    if path.is_absolute() {
        return Err(RsdlError::InvalidImportPath {
            importer: importer.to_path_buf(),
            pattern: pattern.to_string(),
            message: "absolute paths are not allowed".to_string(),
        });
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(RsdlError::InvalidImportPath {
                    importer: importer.to_path_buf(),
                    pattern: pattern.to_string(),
                    message: "only normal relative path components are allowed".to_string(),
                });
            }
        }
    }
    Ok(())
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }

    let mut rest = value;
    if let Some(first) = parts.first()
        && !first.is_empty()
    {
        let Some(next) = rest.strip_prefix(first) else {
            return false;
        };
        rest = next;
    }

    for part in parts.iter().skip(1).take(parts.len().saturating_sub(2)) {
        if part.is_empty() {
            continue;
        }
        let Some(index) = rest.find(part) else {
            return false;
        };
        rest = &rest[index + part.len()..];
    }

    if let Some(last) = parts.last()
        && !last.is_empty()
    {
        return rest.ends_with(last);
    }
    true
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(path).map_err(|source| RsdlError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn logical_source_path(path: &Path, package_root: &Path) -> PathBuf {
    path.strip_prefix(package_root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| {
            path.file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| path.to_path_buf())
        })
}

fn parse_package(table: &Table) -> Result<RawPackage> {
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

fn parse_workspace(table: &Table) -> Result<RawWorkspace> {
    validate_known_fields(table, "workspace", &["modules", "compositions"])?;
    Ok(RawWorkspace {
        modules: optional_string_array(table, "workspace", "modules")?,
        compositions: optional_string_array(table, "workspace", "compositions")?,
    })
}

fn parse_module(table: &Table) -> Result<RawModule> {
    validate_known_fields(table, "module", &["name"])?;
    Ok(RawModule {
        name: required_string(table, "module", "name")?,
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
    let context = format!("component.{name}");
    validate_known_fields(
        table,
        &context,
        &["language", "kind", "input", "output", "params"],
    )?;

    Ok(RawComponent {
        language: required_string(table, &context, "language")?,
        kind: optional_string(table, &context, "kind")?,
        input: optional_port_array(table, &context, "input")?,
        output: optional_port_array(table, &context, "output")?,
        params: optional_param_table(table, &context, "params")?,
    })
}

fn parse_instance(name: &str, table: &Table) -> Result<RawInstance> {
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

fn parse_processes(root: &Table) -> Result<Vec<RawProcess>> {
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
    validate_known_fields(bind_table, "bind", &["dataflow"])?;
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

fn parse_ros2_bridges(root: &Table) -> Result<Vec<RawRos2Bridge>> {
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

fn parse_profile(name: &str, table: &Table) -> Result<RawProfile> {
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

fn parse_target(name: &str, table: &Table) -> Result<RawTarget> {
    let context = format!("target.{name}");
    validate_known_fields(table, &context, &["platform", "runtime", "backends"])?;

    Ok(RawTarget {
        platform: optional_string(table, &context, "platform")?,
        runtime: optional_string_array(table, &context, "runtime")?,
        backends: optional_string_array(table, &context, "backends")?,
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
worker_threads = 3
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
        assert_eq!(document.instances["imu_sim"].tasks[0].trigger, "periodic");
        assert_eq!(document.profiles["default"].worker_threads, Some(3));
    }

    #[test]
    fn parses_multiple_tasks_for_one_instance() {
        let source = r#"
[package]
name = "multi_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
input = ["in:u32"]
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
input = ["in"]
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 100
input = ["in"]
output = ["slow"]
"#;

        let document = parse_str(source).expect("document should parse");
        let tasks = &document.instances["worker"].tasks;

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name.as_deref(), Some("fast_loop"));
        assert_eq!(tasks[1].name.as_deref(), Some("slow_loop"));
        assert_eq!(tasks[0].output, vec!["fast"]);
        assert_eq!(tasks[1].output, vec!["slow"]);
    }

    #[test]
    fn parses_scheduler_v2_task_fields() {
        let source = r#"
[package]
name = "scheduler_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
input = ["in:u32"]
output = ["out:u32"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "on_message"
readiness = "all_ready"
lane = "worker_serial"
priority = 7
input = ["in"]
output = ["out"]
"#;

        let document = parse_str(source).expect("document should parse");
        let task = &document.instances["worker"].tasks[0];

        assert_eq!(task.readiness.as_deref(), Some("all_ready"));
        assert_eq!(task.lane.as_deref(), Some("worker_serial"));
        assert_eq!(task.priority, Some(7));
    }

    #[test]
    fn parses_process_orchestration_tables() {
        let source = r#"
[package]
name = "process_demo"
rsdl_version = "0.1"

[[process]]
name = "sensor_proc"
restart = "on_failure"
max_restarts = 5
initial_delay_ms = 50
max_delay_ms = 500
failure = "propagate"

[[process]]
name = "control_proc"
depends_on = ["sensor_proc"]
restart = "never"
failure = "isolate"
"#;

        let document = parse_str(source).expect("document should parse");

        assert_eq!(document.processes.len(), 2);
        assert_eq!(document.processes[0].name, "sensor_proc");
        assert_eq!(document.processes[0].depends_on, Vec::<String>::new());
        assert_eq!(document.processes[0].restart.as_deref(), Some("on_failure"));
        assert_eq!(document.processes[0].max_restarts, Some(5));
        assert_eq!(document.processes[0].initial_delay_ms, Some(50));
        assert_eq!(document.processes[0].max_delay_ms, Some(500));
        assert_eq!(document.processes[0].failure.as_deref(), Some("propagate"));
        assert_eq!(document.processes[1].name, "control_proc");
        assert_eq!(document.processes[1].depends_on, vec!["sensor_proc"]);
        assert_eq!(document.processes[1].restart.as_deref(), Some("never"));
        assert_eq!(document.processes[1].failure.as_deref(), Some("isolate"));
    }

    #[test]
    fn rejects_unnamed_task_in_task_array() {
        let source = r#"
[package]
name = "multi_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["fast:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
trigger = "periodic"
period_ms = 5
output = ["fast"]
"#;

        let error = parse_str(source).expect_err("task array entries must be named");
        assert!(error.to_string().contains("missing required field `name`"));
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

    #[test]
    fn rejects_unknown_top_level_sections() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[components.worker]
language = "rust"
"#;

        let error = parse_str(source).expect_err("unknown top-level section should fail");

        assert!(matches!(
            error,
            RsdlError::UnknownTopLevelSection { section } if section == "components"
        ));
    }

    #[test]
    fn rejects_unknown_fields_in_fixed_schema_tables() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
proces = "main"
"#;

        let error = parse_str(source).expect_err("unknown fixed-schema field should fail");

        assert!(matches!(
            error,
            RsdlError::UnknownField { context, field }
                if context == "instance.worker" && field == "proces"
        ));
    }

    #[test]
    fn parse_file_expands_package_imports() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("types")).unwrap();
        std::fs::create_dir_all(root.join("components")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/*.rsdl"]
components = ["components/estimator.rsdl"]

[instance.estimator]
component = "estimator"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("types").join("imu.rsdl"),
            r#"
[type.Imu]
timestamp = "u64"
ax = "f32"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("estimator.rsdl"),
            r#"
[component.estimator]
language = "rust"
input = ["imu:Imu"]
"#,
        )
        .unwrap();

        let document = parse_file(root.join("robot.rsdl")).unwrap();

        assert_eq!(document.package.name, "robot_demo");
        assert_eq!(document.package.imports["types"], vec!["types/*.rsdl"]);
        assert_eq!(document.types["Imu"].fields.len(), 2);
        assert_eq!(document.components["estimator"].input[0].ty, "Imu");
        assert_eq!(document.instances["estimator"].component, "estimator");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_expands_graph_fragment_imports() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("types")).unwrap();
        std::fs::create_dir_all(root.join("components")).unwrap();
        std::fs::create_dir_all(root.join("graphs")).unwrap();
        std::fs::create_dir_all(root.join("profiles")).unwrap();
        std::fs::create_dir_all(root.join("targets")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/*.rsdl"]
components = ["components/*.rsdl"]
graphs = ["graphs/*.rsdl"]
profiles = ["profiles/*.rsdl"]
targets = ["targets/*.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("types").join("messages.rsdl"),
            r#"
[type.Imu]
timestamp = "u64"

[type.Odom]
timestamp = "u64"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("imu_sim.rsdl"),
            r#"
[component.imu_sim]
language = "rust"
output = ["imu:Imu"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("estimator.rsdl"),
            r#"
[component.estimator]
language = "rust"
input = ["imu:Imu"]
output = ["odom:Odom"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("graphs").join("default.rsdl"),
            r#"
[instance.imu_sim]
component = "imu_sim"
process = "main"
target = "linux"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.estimator]
component = "estimator"
process = "main"
target = "linux"

[instance.estimator.task]
trigger = "on_message"
input = ["imu"]
output = ["odom"]
deadline_ms = 10

[[bind.dataflow]]
from = "imu_sim.imu"
to = "estimator.imu"
channel = "latest"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("profiles").join("default.rsdl"),
            r#"
[profile.default]
backend = "inproc"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("targets").join("linux.rsdl"),
            r#"
[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
        )
        .unwrap();

        let document = parse_file(root.join("robot.rsdl")).unwrap();

        assert_eq!(document.package.name, "robot_demo");
        assert_eq!(document.types["Imu"].fields[0].name, "timestamp");
        assert_eq!(document.components["imu_sim"].output[0].name, "imu");
        assert_eq!(document.instances["imu_sim"].component, "imu_sim");
        assert_eq!(
            document.instances["estimator"]
                .tasks
                .first()
                .unwrap()
                .trigger,
            "on_message"
        );
        assert_eq!(document.binds[0].from, "imu_sim.imu");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_rejects_import_patterns_without_matches() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/*.rsdl"]
"#,
        )
        .unwrap();

        let error = parse_file(root.join("robot.rsdl")).expect_err("missing import should fail");
        assert!(matches!(error, RsdlError::ImportPatternNoMatches { .. }));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_rejects_absolute_import_paths() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["/tmp/flowrt/secret.rsdl"]
"#,
        )
        .unwrap();

        let error =
            parse_file(root.join("robot.rsdl")).expect_err("absolute import path should fail");

        assert!(matches!(
            error,
            RsdlError::InvalidImportPath { pattern, .. }
                if pattern == "/tmp/flowrt/secret.rsdl"
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_rejects_parent_directory_import_paths() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["../shared/types.rsdl"]
"#,
        )
        .unwrap();

        let error =
            parse_file(root.join("robot.rsdl")).expect_err("parent import path should fail");

        assert!(matches!(
            error,
            RsdlError::InvalidImportPath { pattern, .. }
                if pattern == "../shared/types.rsdl"
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_file_expands_nested_imports_and_records_loaded_sources() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("components").join("common")).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
components = ["components/source.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("source.rsdl"),
            r#"
[package]
name = "source_fragment"
rsdl_version = "0.1"

[package.imports]
types = ["common/*.rsdl"]

[component.source]
language = "rust"
output = ["sample:Sample"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("common").join("sample.rsdl"),
            r#"
[type.Sample]
value = "u32"
"#,
        )
        .unwrap();

        let loaded = load_file(root.join("robot.rsdl")).unwrap();
        let source_paths = loaded
            .sources
            .iter()
            .map(|source| source.path.as_path())
            .collect::<Vec<_>>();

        assert!(loaded.document.types.contains_key("Sample"));
        assert_eq!(loaded.document.components["source"].output[0].ty, "Sample");
        assert_eq!(
            source_paths,
            vec![
                Path::new("robot.rsdl"),
                Path::new("components/source.rsdl"),
                Path::new("components/common/sample.rsdl"),
            ]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_rejects_duplicate_imported_symbols() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("types")).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/*.rsdl"]

[type.Imu]
timestamp = "u64"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("types").join("imu.rsdl"),
            r#"
[type.Imu]
timestamp = "u64"
"#,
        )
        .unwrap();

        let error = parse_file(root.join("robot.rsdl")).expect_err("duplicate type should fail");
        assert!(matches!(
            error,
            RsdlError::DuplicateSymbol { kind: "type", .. }
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_ros2_bridge_tables() {
        let document = parse_str(
            r#"
[package]
name = "ros2_bridge_demo"
rsdl_version = "0.1"

[type.TextFrame]
data = "string"

[component.source]
language = "rust"
output = ["text:TextFrame"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 10
output = ["text"]

[[bridge.ros2]]
flowrt = "source.text"
ros2_topic = "/flowrt/text"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"
field = "data"

[profile.default]
backend = "zenoh"
"#,
        )
        .unwrap();

        assert_eq!(document.ros2_bridges.len(), 1);
        assert_eq!(document.ros2_bridges[0].flowrt, "source.text");
        assert_eq!(document.ros2_bridges[0].ros2_topic, "/flowrt/text");
        assert_eq!(document.ros2_bridges[0].ros2_type, "std_msgs/msg/String");
        assert_eq!(document.ros2_bridges[0].direction, "flowrt_to_ros2");
        assert_eq!(document.ros2_bridges[0].field.as_deref(), Some("data"));
    }

    #[test]
    fn load_file_expands_workspace_modules_and_compositions() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("modules")).unwrap();
        std::fs::create_dir_all(root.join("composition")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "workspace_demo"
rsdl_version = "0.1"

[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules").join("perception.rsdl"),
            r#"
[module]
name = "perception"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.imu_sim]
language = "rust"
output = ["imu:Imu"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules").join("control.rsdl"),
            r#"
[module]
name = "control"

[type.Odom]
timestamp = "u64"

[component.estimator]
language = "rust"
input = ["imu:perception::Imu"]
output = ["odom:Odom"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("composition").join("default.rsdl"),
            r#"
[instance.imu_sim]
component = "perception::imu_sim"
process = "main"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.estimator]
component = "control::estimator"
process = "main"

[instance.estimator.task]
trigger = "on_message"
input = ["imu"]
output = ["odom"]

[[bind.dataflow]]
from = "imu_sim.imu"
to = "estimator.imu"
channel = "latest"
"#,
        )
        .unwrap();

        let loaded = load_file(root.join("robot.rsdl")).unwrap();

        assert_eq!(
            loaded.document.workspace.as_ref().unwrap().modules,
            vec!["modules/*.rsdl"]
        );
        assert_eq!(loaded.modules.len(), 2);
        assert_eq!(loaded.modules[0].module.name, "control");
        assert_eq!(loaded.modules[1].module.name, "perception");
        assert_eq!(loaded.modules[1].types["Imu"].fields[0].name, "timestamp");
        assert_eq!(
            loaded.modules[0].components["estimator"].input[0].ty,
            "perception::Imu"
        );
        assert_eq!(loaded.compositions.len(), 1);
        assert_eq!(
            loaded.document.instances["estimator"].component,
            "control::estimator"
        );
        assert_eq!(loaded.document.binds[0].from, "imu_sim.imu");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_file_rejects_instance_inside_workspace_module() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("modules")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "workspace_demo"
rsdl_version = "0.1"

[workspace]
modules = ["modules/perception.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules").join("perception.rsdl"),
            r#"
[module]
name = "perception"

[component.imu_sim]
language = "rust"

[instance.imu_sim]
component = "imu_sim"
"#,
        )
        .unwrap();

        let error = load_file(root.join("robot.rsdl")).expect_err("module instance should fail");

        assert!(matches!(
            error,
            RsdlError::InvalidModuleSection { module, section, .. }
                if module == "perception" && section == "instance"
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> std::path::PathBuf {
        let suffix = format!(
            "flowrt-rsdl-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(suffix)
    }
}
