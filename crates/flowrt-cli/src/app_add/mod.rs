use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Subcommand, ValueEnum};
use toml::{Table, Value};

use crate::application_root_from_rsdl;

mod names;
mod rsdl;
mod types;

use names::{normalize_snake_name, validate_pascal_name};
use rsdl::{
    add_component_tables, ensure_target_runtime, ensure_workspace_modules_glob, first_target_name,
    nested_table_exists, parse_rsdl_value, read_rsdl_source, render_rsdl_value, table_entry_mut,
    write_new_file, write_validated_rsdl_replacement,
};
use types::{parse_field_specs, parse_port_specs};

#[derive(Debug, Subcommand)]
pub(crate) enum AddCommand {
    /// 向主 RSDL 追加 message type。
    Message {
        /// Message 名称，使用 PascalCase。
        name: String,

        /// 字段定义，格式 `field:type`。
        #[arg(required = true)]
        fields: Vec<String>,

        /// 显式 RSDL 路径；省略时从 flowrt.toml 发现。
        #[arg(long)]
        rsdl: Option<PathBuf>,
    },

    /// 创建 workspace module 文件并注册当前可解析的 module glob。
    Module {
        /// Module 名称，推荐 snake_case；PascalCase 会转为 snake_case。
        name: String,

        /// 显式 RSDL 路径；省略时从 flowrt.toml 发现。
        #[arg(long)]
        rsdl: Option<PathBuf>,
    },

    /// 向 RSDL 追加 Rust/C++/C native component、instance 和 task。
    Component {
        /// Component 名称，推荐 PascalCase；RSDL 中会规范化为 snake_case。
        name: String,

        /// 用户组件语言。
        #[arg(long = "lang", value_enum)]
        language: AppAddLanguage,

        /// 输入端口，格式 `name:Type`。初始 task 不激活 input，需用户补 topology。
        #[arg(long = "input")]
        inputs: Vec<String>,

        /// 输出端口，格式 `name:Type`。
        #[arg(long = "output")]
        outputs: Vec<String>,

        /// 显式 RSDL 路径；省略时从 flowrt.toml 发现。
        #[arg(long)]
        rsdl: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum AppAddLanguage {
    Rust,
    C,
    Cpp,
}

impl AppAddLanguage {
    pub(super) fn as_rsdl(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::C => "c",
            Self::Cpp => "cpp",
        }
    }

    fn as_output(self) -> &'static str {
        self.as_rsdl()
    }
}

#[derive(Debug)]
pub(crate) struct AddComponentSpec {
    pub(crate) name: String,
    pub(crate) language: AppAddLanguage,
    pub(crate) inputs: Vec<String>,
    pub(crate) outputs: Vec<String>,
}

pub(crate) fn add_message_to_rsdl(rsdl: &Path, name: &str, fields: &[String]) -> Result<String> {
    let message_name = validate_pascal_name(name, "message name")?;
    if fields.is_empty() {
        anyhow::bail!("message `{message_name}` must declare at least one field");
    }
    let field_specs = parse_field_specs(fields)?;
    let source = read_rsdl_source(rsdl)?;
    let mut document = parse_rsdl_value(rsdl, &source)?;
    if nested_table_exists(&document, "type", &message_name) {
        anyhow::bail!(
            "type `{message_name}` already exists in `{}`",
            rsdl.display()
        );
    }

    let mut message = Table::new();
    for field in field_specs {
        message.insert(field.name, Value::String(field.ty));
    }
    table_entry_mut(&mut document, "type")?.insert(message_name.clone(), Value::Table(message));

    let updated = render_rsdl_value(document)?;
    write_validated_rsdl_replacement(rsdl, &updated)?;
    Ok(format!(
        "added message `{message_name}` to {}",
        rsdl.display()
    ))
}

pub(crate) fn add_module_to_rsdl(rsdl: &Path, raw_name: &str) -> Result<String> {
    let module_name = normalize_snake_name(raw_name, "module name")?;
    let app_root = application_root_from_rsdl(rsdl)?;
    let module_path = app_root
        .join("rsdl")
        .join("modules")
        .join(format!("{module_name}.rsdl"));
    if module_path.exists() {
        anyhow::bail!(
            "refusing to overwrite existing file `{}`",
            module_path.display()
        );
    }

    let source = read_rsdl_source(rsdl)?;
    let mut document = parse_rsdl_value(rsdl, &source)?;
    ensure_workspace_modules_glob(&mut document)?;
    let updated = render_rsdl_value(document)?;

    if let Some(parent) = module_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    write_new_file(
        &module_path,
        &format!("[module]\nname = \"{module_name}\"\n"),
    )?;
    match rsdl::validate_rsdl_source(rsdl, &updated) {
        Ok(()) => {
            write_validated_rsdl_replacement(rsdl, &updated)?;
            Ok(format!(
                "added module `{module_name}` file={}",
                module_path.display()
            ))
        }
        Err(error) => {
            let _ = std::fs::remove_file(&module_path);
            Err(error)
        }
    }
}

pub(crate) fn add_component_to_rsdl(rsdl: &Path, spec: AddComponentSpec) -> Result<String> {
    let component_name = normalize_snake_name(&spec.name, "component name")?;
    let inputs = parse_port_specs(&spec.inputs, "input")?;
    let outputs = parse_port_specs(&spec.outputs, "output")?;
    let source = read_rsdl_source(rsdl)?;
    let mut document = parse_rsdl_value(rsdl, &source)?;
    if nested_table_exists(&document, "component", &component_name) {
        anyhow::bail!(
            "component `{component_name}` already exists in `{}`",
            rsdl.display()
        );
    }
    if nested_table_exists(&document, "instance", &component_name) {
        anyhow::bail!(
            "instance `{component_name}` already exists in `{}`",
            rsdl.display()
        );
    }

    let target_name = first_target_name(&document);
    add_component_tables(
        &mut document,
        &component_name,
        spec.language,
        &inputs,
        &outputs,
        target_name.as_deref(),
    )?;
    ensure_target_runtime(&mut document, spec.language)?;
    let updated_rsdl = render_rsdl_value(document)?;
    write_validated_rsdl_replacement(rsdl, &updated_rsdl)?;
    Ok(format!(
        "added component `{component_name}` language={} to {}; next run `flowrt prepare` or `flowrt explain` to inspect the user implementation interface",
        spec.language.as_output(),
        rsdl.display()
    ))
}
