use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use flowrt_codegen::{ArtifactBundle, emit_artifacts};
use flowrt_ir::{ContractIr, hash_source, normalize_document};
use flowrt_validate::validate_contract;

#[derive(Debug, Parser)]
#[command(name = "flowrt")]
#[command(about = "FlowRT 数据流契约工具链")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 解析、归一化并校验一个 RSDL 文件。
    Check {
        /// .rsdl 文件路径。
        rsdl: PathBuf,
    },

    /// 准备 FlowRT 管理的应用产物。
    Prepare {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,
    },

    /// 准备并构建 FlowRT 管理的应用 crate。
    Build {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,
    },

    /// 准备并运行 FlowRT 管理的应用 crate。
    Run {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// 只运行生成应用中的一个 RSDL process group。
        #[arg(long)]
        process: Option<String>,
    },

    /// 准备、构建并运行生成的 process supervisor。
    Launch {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,
    },

    /// 查看已落盘的 Contract IR JSON 文档摘要。
    Inspect {
        /// contract.ir.json 路径。
        ir: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Check { rsdl } => {
            let contract = load_contract_from_rsdl(&rsdl)?;
            println!("OK {}", summary(&contract));
        }
        Command::Prepare { rsdl, out_dir } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir)?;
            println!(
                "prepared {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
        }
        Command::Build { rsdl, out_dir } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir)?;
            let manifest = cargo_manifest_with_local_runtime_patch(&out_dir)?;
            run_cargo("build", &manifest)?;
            println!(
                "built {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
        }
        Command::Run {
            rsdl,
            out_dir,
            process,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir)?;
            let contract = load_contract_from_json(&prepared.contract_path)?;
            let manifest = cargo_manifest_with_local_runtime_patch(&out_dir)?;
            run_cargo_run(&manifest, &app_bin_name(&contract), process.as_deref())?;
            println!(
                "ran {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
        }
        Command::Launch { rsdl, out_dir } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir)?;
            let contract = load_contract_from_json(&prepared.contract_path)?;
            let manifest = cargo_manifest_with_local_runtime_patch(&out_dir)?;
            run_cargo("build", &manifest)?;
            run_cargo_supervisor(&manifest, &supervisor_bin_name(&contract))?;
            println!(
                "launched {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
        }
        Command::Inspect { ir } => {
            let contract = load_contract_from_json(&ir)?;
            println!("{}", summary(&contract));
        }
    }
    Ok(())
}

fn load_contract_from_rsdl(path: &Path) -> Result<ContractIr> {
    let loaded = flowrt_rsdl::load_file(path)
        .with_context(|| format!("failed to load RSDL source `{}`", path.display()))?;
    let source_bundle = loaded.source_bundle_text();
    let contract = normalize_document(&loaded.document, hash_source(&source_bundle))
        .with_context(|| format!("failed to normalize `{}`", path.display()))?;
    validate_contract(&contract).context("contract validation failed")?;
    Ok(contract)
}

fn load_contract_from_json(path: &Path) -> Result<ContractIr> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read Contract IR `{}`", path.display()))?;
    let contract = ContractIr::from_json_str(&source)
        .with_context(|| format!("failed to parse Contract IR `{}`", path.display()))?;
    validate_contract(&contract).context("contract validation failed")?;
    Ok(contract)
}

fn write_contract(contract: &ContractIr, out_dir: &Path) -> Result<PathBuf> {
    let contract_dir = out_dir.join("contract");
    fs::create_dir_all(&contract_dir)
        .with_context(|| format!("failed to create `{}`", contract_dir.display()))?;
    let output = contract_dir.join("contract.ir.json");
    fs::write(&output, contract.to_canonical_json()?)
        .with_context(|| format!("failed to write `{}`", output.display()))?;
    Ok(output)
}

struct PreparedWorkspace {
    contract_path: PathBuf,
    artifact_count: usize,
}

fn prepare_workspace(rsdl: &Path, out_dir: &Path) -> Result<PreparedWorkspace> {
    let contract = load_contract_from_rsdl(rsdl)?;
    let contract_path = write_contract(&contract, out_dir)?;
    let artifacts = emit_artifacts(&contract).context("failed to prepare artifacts")?;
    let artifact_count = write_artifacts(&artifacts, out_dir)?;
    Ok(PreparedWorkspace {
        contract_path,
        artifact_count,
    })
}

fn write_artifacts(bundle: &ArtifactBundle, out_dir: &Path) -> Result<usize> {
    for artifact in &bundle.artifacts {
        ensure_safe_relative_path(&artifact.relative_path)?;
        let output = out_dir.join(&artifact.relative_path);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }
        fs::write(&output, &artifact.content)
            .with_context(|| format!("failed to write `{}`", output.display()))?;
    }
    Ok(bundle.artifacts.len())
}

fn cargo_manifest_with_local_runtime_patch(out_dir: &Path) -> Result<PathBuf> {
    let generated_manifest = out_dir.join("build").join("Cargo.toml");
    let generated = fs::read_to_string(&generated_manifest)
        .with_context(|| format!("failed to read `{}`", generated_manifest.display()))?;
    if generated.contains("[patch.crates-io]") {
        return Ok(generated_manifest);
    }
    let repo_runtime = repo_root_dir()?.join("runtime/rust");
    let patched = format!(
        "{generated}\n[patch.crates-io]\nflowrt = {{ path = {} }}\n",
        toml_basic_string(&repo_runtime)
    );
    fs::write(&generated_manifest, patched)
        .with_context(|| format!("failed to write `{}`", generated_manifest.display()))?;
    Ok(generated_manifest)
}

fn run_cargo(subcommand: &str, manifest: &Path) -> Result<()> {
    let status = ProcessCommand::new("cargo")
        .arg(subcommand)
        .arg("--manifest-path")
        .arg(manifest)
        .status()
        .context("failed to spawn cargo")?;
    if !status.success() {
        anyhow::bail!("cargo invocation failed with status {status}");
    }
    Ok(())
}

fn run_cargo_run(manifest: &Path, app_bin: &str, process: Option<&str>) -> Result<()> {
    let mut command = ProcessCommand::new("cargo");
    command
        .arg("run")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--bin")
        .arg(app_bin);
    if let Some(process) = process {
        command.arg("--").arg("--process").arg(process);
    }
    let status = command.status().context("failed to spawn cargo")?;
    if !status.success() {
        anyhow::bail!("cargo invocation failed with status {status}");
    }
    Ok(())
}

fn run_cargo_supervisor(manifest: &Path, supervisor_bin: &str) -> Result<()> {
    let status = ProcessCommand::new("cargo")
        .arg("run")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--bin")
        .arg(supervisor_bin)
        .status()
        .context("failed to spawn cargo")?;
    if !status.success() {
        anyhow::bail!("cargo invocation failed with status {status}");
    }
    Ok(())
}

fn resolve_output_dir(rsdl: &Path, out_dir: &Path) -> Result<PathBuf> {
    if out_dir.is_absolute() {
        return Ok(out_dir.to_path_buf());
    }
    Ok(application_root_from_rsdl(rsdl)?.join(out_dir))
}

fn application_root_from_rsdl(rsdl: &Path) -> Result<PathBuf> {
    for ancestor in rsdl.ancestors() {
        if ancestor.file_name() == Some(OsStr::new("rsdl")) {
            return ancestor
                .parent()
                .map(Path::to_path_buf)
                .context("failed to resolve application root from `rsdl/` directory");
        }
    }
    rsdl.parent()
        .map(Path::to_path_buf)
        .context("failed to resolve application root from RSDL path")
}

fn repo_root_dir() -> Result<PathBuf> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::canonicalize(&repo_root).with_context(|| {
        format!(
            "failed to resolve repository root from `{}`",
            repo_root.display()
        )
    })
}

fn toml_basic_string(path: &Path) -> String {
    let escaped = path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('\"', "\\\"");
    format!("\"{escaped}\"")
}

fn supervisor_bin_name(contract: &ContractIr) -> String {
    format!(
        "{}-flowrt-supervisor",
        sanitize_package_name(&contract.package.name).replace('_', "-")
    )
}

fn app_bin_name(contract: &ContractIr) -> String {
    format!(
        "{}-flowrt-app",
        sanitize_package_name(&contract.package.name).replace('_', "-")
    )
}

fn sanitize_package_name(name: &str) -> String {
    let mut output = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.is_empty() {
        "flowrt-app".to_string()
    } else {
        output
    }
}

fn ensure_safe_relative_path(path: &Path) -> Result<()> {
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => anyhow::bail!("unsafe artifact path `{}`", path.display()),
        }
    }
    Ok(())
}

fn summary(contract: &ContractIr) -> String {
    let graph = contract.graphs.first();
    let instance_count = graph.map(|graph| graph.instances.len()).unwrap_or(0);
    let task_count = graph.map(|graph| graph.tasks.len()).unwrap_or(0);
    let bind_count = graph.map(|graph| graph.binds.len()).unwrap_or(0);
    format!(
        "package={} types={} components={} instances={} tasks={} binds={}",
        contract.package.name,
        contract.types.len(),
        contract.components.len(),
        instance_count,
        task_count,
        bind_count
    )
}
