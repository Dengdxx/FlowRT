use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml::{Table, Value};

use crate::load_contract_from_rsdl;

use super::AppAddLanguage;
use super::types::PortSpec;

pub(super) fn add_component_tables(
    document: &mut Value,
    component_name: &str,
    language: AppAddLanguage,
    inputs: &[PortSpec],
    outputs: &[PortSpec],
    target_name: Option<&str>,
) -> Result<()> {
    let mut component = Table::new();
    component.insert(
        "language".to_string(),
        Value::String(language.as_rsdl().to_string()),
    );
    if !inputs.is_empty() {
        component.insert("input".to_string(), Value::Array(port_strings(inputs)));
    }
    if !outputs.is_empty() {
        component.insert("output".to_string(), Value::Array(port_strings(outputs)));
    }
    table_entry_mut(document, "component")?
        .insert(component_name.to_string(), Value::Table(component));

    let mut instance = Table::new();
    instance.insert(
        "component".to_string(),
        Value::String(component_name.to_string()),
    );
    instance.insert("process".to_string(), Value::String("main".to_string()));
    if let Some(target_name) = target_name {
        instance.insert("target".to_string(), Value::String(target_name.to_string()));
    }

    let mut task = Table::new();
    task.insert("trigger".to_string(), Value::String("periodic".to_string()));
    task.insert("period_ms".to_string(), Value::Integer(100));
    if !outputs.is_empty() {
        task.insert(
            "output".to_string(),
            Value::Array(
                outputs
                    .iter()
                    .map(|port| Value::String(port.name.clone()))
                    .collect(),
            ),
        );
    }
    instance.insert("task".to_string(), Value::Table(task));
    table_entry_mut(document, "instance")?
        .insert(component_name.to_string(), Value::Table(instance));
    Ok(())
}

fn port_strings(ports: &[PortSpec]) -> Vec<Value> {
    ports
        .iter()
        .map(|port| Value::String(format!("{}:{}", port.name, port.ty)))
        .collect()
}

pub(super) fn read_rsdl_source(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))
}

pub(super) fn parse_rsdl_value(path: &Path, source: &str) -> Result<Value> {
    toml::from_str(source).with_context(|| format!("failed to parse `{}` as TOML", path.display()))
}

pub(super) fn render_rsdl_value(value: Value) -> Result<String> {
    let mut output = toml::to_string_pretty(&value).context("failed to render RSDL TOML")?;
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

fn root_table_mut(value: &mut Value) -> Result<&mut Table> {
    value
        .as_table_mut()
        .context("RSDL root document must be a TOML table")
}

pub(super) fn table_entry_mut<'a>(document: &'a mut Value, name: &str) -> Result<&'a mut Table> {
    let table = root_table_mut(document)?;
    if !table.contains_key(name) {
        table.insert(name.to_string(), Value::Table(Table::new()));
    }
    table
        .get_mut(name)
        .and_then(Value::as_table_mut)
        .with_context(|| format!("RSDL `{name}` section must be a table"))
}

pub(super) fn nested_table_exists(document: &Value, section: &str, name: &str) -> bool {
    document
        .get(section)
        .and_then(Value::as_table)
        .is_some_and(|table| table.contains_key(name))
}

pub(super) fn ensure_workspace_modules_glob(document: &mut Value) -> Result<()> {
    let workspace = table_entry_mut(document, "workspace")?;
    if !workspace.contains_key("modules") {
        workspace.insert(
            "modules".to_string(),
            Value::Array(vec![Value::String("modules/*.rsdl".to_string())]),
        );
        return Ok(());
    }
    let modules = workspace
        .get_mut("modules")
        .and_then(Value::as_array_mut)
        .context("workspace.modules must be an array of strings")?;
    if modules
        .iter()
        .any(|value| value.as_str() == Some("modules/*.rsdl"))
    {
        return Ok(());
    }
    modules.push(Value::String("modules/*.rsdl".to_string()));
    Ok(())
}

pub(super) fn first_target_name(document: &Value) -> Option<String> {
    document
        .get("target")
        .and_then(Value::as_table)
        .and_then(|targets| targets.keys().next().cloned())
}

pub(super) fn ensure_target_runtime(document: &mut Value, language: AppAddLanguage) -> Result<()> {
    let Some(targets) = document.get_mut("target").and_then(Value::as_table_mut) else {
        return Ok(());
    };
    let language = language.as_rsdl();
    for (_, target) in targets.iter_mut() {
        let Some(target) = target.as_table_mut() else {
            continue;
        };
        let Some(runtime) = target.get_mut("runtime").and_then(Value::as_array_mut) else {
            continue;
        };
        if runtime.iter().any(|value| value.as_str() == Some(language)) {
            continue;
        }
        runtime.push(Value::String(language.to_string()));
        runtime.sort_by_key(runtime_rank);
    }
    Ok(())
}

fn runtime_rank(value: &Value) -> u8 {
    match value.as_str() {
        Some("c") => 0,
        Some("cpp") => 1,
        Some("rust") => 2,
        Some("external") => 3,
        _ => 255,
    }
}

pub(super) fn validate_rsdl_source(rsdl: &Path, source: &str) -> Result<()> {
    let temp_path = temp_rsdl_path(rsdl);
    let validation = (|| {
        fs::write(&temp_path, source)
            .with_context(|| format!("failed to write `{}`", temp_path.display()))?;
        load_contract_from_rsdl(&temp_path)
            .map(|_| ())
            .context("contract validation failed after add")
    })();
    let _ = fs::remove_file(&temp_path);
    validation
}

pub(super) fn write_validated_rsdl_replacement(rsdl: &Path, source: &str) -> Result<()> {
    let temp_path = temp_rsdl_path(rsdl);
    fs::write(&temp_path, source)
        .with_context(|| format!("failed to write `{}`", temp_path.display()))?;
    match load_contract_from_rsdl(&temp_path)
        .map(|_| ())
        .context("contract validation failed after add")
    {
        Ok(()) => {
            fs::rename(&temp_path, rsdl)
                .with_context(|| format!("failed to replace `{}`", rsdl.display()))?;
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(error)
        }
    }
}

fn temp_rsdl_path(rsdl: &Path) -> PathBuf {
    let file_name = rsdl
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("robot.rsdl");
    rsdl.with_file_name(format!(
        ".{file_name}.flowrt-add-{}.rsdl",
        std::process::id()
    ))
}

pub(super) fn write_new_file(path: &Path, content: &str) -> Result<()> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            use std::io::Write;
            file.write_all(content.as_bytes())
                .with_context(|| format!("failed to write `{}`", path.display()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            anyhow::bail!("refusing to overwrite existing file `{}`", path.display())
        }
        Err(error) => Err(error).with_context(|| format!("failed to create `{}`", path.display())),
    }
}
