use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use flowrt_codegen::{ArtifactBundle, emit_artifacts};
use flowrt_ir::{
    ContractIr, LanguageKind, hash_source, normalize_document, project_contract_to_profile,
};
use flowrt_validate::validate_contract;

#[derive(Debug, Parser)]
#[command(name = "flowrt")]
#[command(version)]
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

        /// 选择用于生成产物的 profile 名称。
        #[arg(long)]
        profile: Option<String>,
    },

    /// 准备并构建 FlowRT 管理的应用产物。
    Build {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// 选择用于生成产物的 profile 名称。
        #[arg(long)]
        profile: Option<String>,
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

        /// 选择用于生成和运行的 profile 名称。
        #[arg(long)]
        profile: Option<String>,
    },

    /// 准备、构建并运行生成的 process supervisor。
    Launch {
        /// .rsdl 文件路径。
        rsdl: PathBuf,

        /// FlowRT 管理产物输出目录。
        #[arg(long, default_value = "flowrt")]
        out_dir: PathBuf,

        /// 选择用于生成和启动的 profile 名称。
        #[arg(long)]
        profile: Option<String>,
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
        Command::Prepare {
            rsdl,
            out_dir,
            profile,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let _lock = WorkspaceLock::acquire(&out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir, profile.as_deref())?;
            println!(
                "prepared {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
        }
        Command::Build {
            rsdl,
            out_dir,
            profile,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let _lock = WorkspaceLock::acquire(&out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir, profile.as_deref())?;
            build_workspace(&prepared.selected_contract, &out_dir)?;
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
            profile,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let _lock = WorkspaceLock::acquire(&out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir, profile.as_deref())?;
            run_workspace(&prepared.selected_contract, &out_dir, process.as_deref())?;
            println!(
                "ran {} and {} artifact(s)",
                prepared.contract_path.display(),
                prepared.artifact_count
            );
        }
        Command::Launch {
            rsdl,
            out_dir,
            profile,
        } => {
            let out_dir = resolve_output_dir(&rsdl, &out_dir)?;
            let _lock = WorkspaceLock::acquire(&out_dir)?;
            let prepared = prepare_workspace(&rsdl, &out_dir, profile.as_deref())?;
            launch_workspace(&prepared.selected_contract, &out_dir)?;
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

#[derive(Debug)]
struct WorkspaceLock {
    path: PathBuf,
}

impl WorkspaceLock {
    fn acquire(out_dir: &Path) -> Result<Self> {
        fs::create_dir_all(out_dir)
            .with_context(|| format!("failed to create `{}`", out_dir.display()))?;
        let path = out_dir.join(".flowrt.lock");
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                writeln!(file, "pid={}", std::process::id())
                    .with_context(|| format!("failed to write `{}`", path.display()))?;
                Ok(Self { path })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                anyhow::bail!(
                    "FlowRT output directory `{}` is already in use by another flowrt command; retry after it finishes, or remove `{}` if no FlowRT command is running",
                    out_dir.display(),
                    path.display()
                )
            }
            Err(error) => {
                Err(error).with_context(|| format!("failed to create lock `{}`", path.display()))
            }
        }
    }
}

impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn normalize_contract_from_rsdl(path: &Path) -> Result<ContractIr> {
    let loaded = flowrt_rsdl::load_file(path)
        .with_context(|| format!("failed to load RSDL source `{}`", path.display()))?;
    let source_bundle = loaded.source_bundle_text();
    normalize_document(&loaded.document, hash_source(&source_bundle))
        .with_context(|| format!("failed to normalize `{}`", path.display()))
}

fn load_contract_from_rsdl(path: &Path) -> Result<ContractIr> {
    let contract = normalize_contract_from_rsdl(path)?;
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
    selected_contract: ContractIr,
}

fn prepare_workspace(
    rsdl: &Path,
    out_dir: &Path,
    profile: Option<&str>,
) -> Result<PreparedWorkspace> {
    let contract = normalize_contract_from_rsdl(rsdl)?;
    let selected_contract = project_contract_to_profile(&contract, profile)
        .with_context(|| format!("failed to select profile for `{}`", rsdl.display()))?;
    validate_contract(&selected_contract).context("contract validation failed")?;
    let contract_path = write_contract(&selected_contract, out_dir)?;
    let artifacts = emit_artifacts(&selected_contract).context("failed to prepare artifacts")?;
    let artifact_count = write_artifacts(&artifacts, out_dir)?;
    Ok(PreparedWorkspace {
        contract_path,
        artifact_count,
        selected_contract,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildStep {
    Cargo,
    Cmake,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchStep {
    Build(BuildStep),
    CargoSupervisor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    CargoApp,
    CmakeApp,
}

fn build_steps(contract: &ContractIr) -> Vec<BuildStep> {
    let mut steps = Vec::new();
    if has_component_language(contract, LanguageKind::Rust) {
        steps.push(BuildStep::Cargo);
    }
    if has_component_language(contract, LanguageKind::Cpp) {
        steps.push(BuildStep::Cmake);
    }
    steps
}

fn launch_steps(contract: &ContractIr) -> Vec<LaunchStep> {
    let mut steps = build_steps(contract)
        .into_iter()
        .map(LaunchStep::Build)
        .collect::<Vec<_>>();
    steps.push(LaunchStep::CargoSupervisor);
    steps
}

fn run_mode(contract: &ContractIr) -> Option<RunMode> {
    match (
        has_component_language(contract, LanguageKind::Rust),
        has_component_language(contract, LanguageKind::Cpp),
    ) {
        (true, false) => Some(RunMode::CargoApp),
        (false, true) => Some(RunMode::CmakeApp),
        _ => None,
    }
}

fn run_mode_for_process(contract: &ContractIr, process: Option<&str>) -> Result<RunMode> {
    if let Some(mode) = run_mode(contract) {
        return Ok(mode);
    }

    let Some(process) = process else {
        anyhow::bail!(
            "mixed-language `run` requires `--process <name>`; use `flowrt launch` to start every process group"
        );
    };

    let runtimes = process_runtime_flags(contract, process)
        .with_context(|| format!("unknown FlowRT process group `{process}`"))?;

    match (runtimes.rust, runtimes.cpp) {
        (true, false) => Ok(RunMode::CargoApp),
        (false, true) => Ok(RunMode::CmakeApp),
        (true, true) => anyhow::bail!(
            "mixed-language `run` cannot run process `{process}` because it contains both C++ and Rust components"
        ),
        (false, false) => {
            anyhow::bail!("FlowRT process group `{process}` has no runnable components")
        }
    }
}

fn process_runtime_flags(contract: &ContractIr, process: &str) -> Option<ProcessRuntimeFlags> {
    let component_languages = contract
        .components
        .iter()
        .map(|component| (component.name.as_str(), component.language))
        .collect::<BTreeMap<_, _>>();

    let mut runtimes = ProcessRuntimeFlags::default();
    let mut found = false;
    for graph in &contract.graphs {
        for instance in &graph.instances {
            let instance_process = instance.process.as_deref().unwrap_or("main");
            if instance_process != process {
                continue;
            }
            let Some(language) = component_languages
                .get(instance.component.name.as_str())
                .copied()
            else {
                continue;
            };
            runtimes.add(language);
            found = true;
        }
    }

    found.then_some(runtimes)
}

fn has_component_language(contract: &ContractIr, language: LanguageKind) -> bool {
    contract
        .components
        .iter()
        .any(|component| component.language == language)
}

fn is_mixed_language_contract(contract: &ContractIr) -> bool {
    has_component_language(contract, LanguageKind::Rust)
        && has_component_language(contract, LanguageKind::Cpp)
}

fn ensure_direct_runtime_supported(contract: &ContractIr, command: &str) -> Result<()> {
    if !is_mixed_language_contract(contract) {
        return Ok(());
    }

    if let Some(group) = mixed_process_group(contract) {
        anyhow::bail!(
            "mixed-language `{command}` cannot run graph `{}` process `{}` because it contains both C++ and Rust components; split them into language-specific RSDL process groups before using a cross-language backend",
            group.graph,
            group.process
        );
    }

    let backend = selected_runtime_backend_name(contract);
    if backend != "iox2" {
        anyhow::bail!(
            "mixed-language `{command}` requires backend `iox2`; selected backend `{backend}` cannot carry cross-language process boundaries"
        );
    }

    Ok(())
}

#[derive(Debug, Clone, Default)]
struct ProcessRuntimeFlags {
    cpp: bool,
    rust: bool,
}

impl ProcessRuntimeFlags {
    fn add(&mut self, language: LanguageKind) {
        match language {
            LanguageKind::Cpp => self.cpp = true,
            LanguageKind::Rust => self.rust = true,
        }
    }

    fn is_mixed(&self) -> bool {
        self.cpp && self.rust
    }
}

#[derive(Debug, Clone)]
struct MixedProcessGroup {
    graph: String,
    process: String,
}

fn mixed_process_group(contract: &ContractIr) -> Option<MixedProcessGroup> {
    let component_languages = contract
        .components
        .iter()
        .map(|component| (component.name.as_str(), component.language))
        .collect::<BTreeMap<_, _>>();

    for graph in &contract.graphs {
        let mut processes = BTreeMap::<String, ProcessRuntimeFlags>::new();
        for instance in &graph.instances {
            let Some(language) = component_languages
                .get(instance.component.name.as_str())
                .copied()
            else {
                continue;
            };
            processes
                .entry(
                    instance
                        .process
                        .clone()
                        .unwrap_or_else(|| "main".to_string()),
                )
                .or_default()
                .add(language);
        }

        if let Some((process, _)) = processes
            .into_iter()
            .find(|(_, runtimes)| runtimes.is_mixed())
        {
            return Some(MixedProcessGroup {
                graph: graph.name.clone(),
                process,
            });
        }
    }

    None
}

fn selected_runtime_backend_name(contract: &ContractIr) -> &str {
    contract
        .profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| contract.profiles.first())
        .map(|profile| profile.backend.0.as_str())
        .unwrap_or("inproc")
}

fn ensure_backend_runtime_supported(_contract: &ContractIr, _command: &str) -> Result<()> {
    Ok(())
}

fn build_workspace(contract: &ContractIr, out_dir: &Path) -> Result<()> {
    ensure_backend_runtime_supported(contract, "build")?;
    for step in build_steps(contract) {
        match step {
            BuildStep::Cargo => {
                let manifest = cargo_manifest_with_local_runtime_patch(out_dir)?;
                run_cargo("build", &manifest)?;
            }
            BuildStep::Cmake => {
                run_cmake_configure_and_build(out_dir)?;
            }
        }
    }
    Ok(())
}

fn run_workspace(contract: &ContractIr, out_dir: &Path, process: Option<&str>) -> Result<()> {
    ensure_direct_runtime_supported(contract, "run")?;
    ensure_backend_runtime_supported(contract, "run")?;
    ensure_run_process_boundaries_supported(contract, process)?;
    match run_mode_for_process(contract, process)
        .context("contract does not contain runnable components")?
    {
        RunMode::CargoApp => {
            let manifest = cargo_manifest_with_local_runtime_patch(out_dir)?;
            run_cargo_run(&manifest, &app_bin_name(contract), process)?;
        }
        RunMode::CmakeApp => {
            build_workspace(contract, out_dir)?;
            run_cmake_app(contract, out_dir, process)?;
        }
    }
    Ok(())
}

fn launch_workspace(contract: &ContractIr, out_dir: &Path) -> Result<()> {
    ensure_direct_runtime_supported(contract, "launch")?;
    ensure_backend_runtime_supported(contract, "launch")?;
    ensure_launch_process_boundaries_supported(contract)?;
    for step in launch_steps(contract) {
        match step {
            LaunchStep::Build(BuildStep::Cargo) => {
                let manifest = cargo_manifest_with_local_runtime_patch(out_dir)?;
                run_cargo("build", &manifest)?;
            }
            LaunchStep::Build(BuildStep::Cmake) => {
                run_cmake_configure_and_build(out_dir)?;
            }
            LaunchStep::CargoSupervisor => {
                let manifest = cargo_manifest_with_local_runtime_patch(out_dir)?;
                run_cargo_supervisor(&manifest, &supervisor_bin_name(contract))?;
            }
        }
    }
    Ok(())
}

fn ensure_launch_process_boundaries_supported(contract: &ContractIr) -> Result<()> {
    let backend = selected_runtime_backend_name(contract);
    if backend != "inproc" {
        return Ok(());
    }

    if let Some(boundary) = first_cross_process_bind(contract) {
        anyhow::bail!(
            "backend `inproc` cannot launch dataflow `{}` -> `{}` across process groups `{}` -> `{}`; use backend `iox2` or place both instances in the same RSDL process group",
            boundary.from,
            boundary.to,
            boundary.from_process,
            boundary.to_process
        );
    }

    Ok(())
}

fn ensure_run_process_boundaries_supported(
    contract: &ContractIr,
    process: Option<&str>,
) -> Result<()> {
    let backend = selected_runtime_backend_name(contract);
    if backend != "inproc" {
        return Ok(());
    }

    let Some(process) = process else {
        return Ok(());
    };

    if let Some(boundary) = first_cross_process_bind_for_process(contract, process) {
        anyhow::bail!(
            "backend `inproc` cannot run --process `{}` because dataflow `{}` -> `{}` crosses process groups `{}` -> `{}`; use backend `iox2`, run the whole inproc app, or place both instances in the same RSDL process group",
            process,
            boundary.from,
            boundary.to,
            boundary.from_process,
            boundary.to_process
        );
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct CrossProcessBind {
    from: String,
    to: String,
    from_process: String,
    to_process: String,
}

fn first_cross_process_bind(contract: &ContractIr) -> Option<CrossProcessBind> {
    first_cross_process_bind_matching(contract, |_| true)
}

fn first_cross_process_bind_for_process(
    contract: &ContractIr,
    process: &str,
) -> Option<CrossProcessBind> {
    first_cross_process_bind_matching(contract, |boundary| {
        boundary.from_process == process || boundary.to_process == process
    })
}

fn first_cross_process_bind_matching(
    contract: &ContractIr,
    matches: impl Fn(&CrossProcessBind) -> bool,
) -> Option<CrossProcessBind> {
    for graph in &contract.graphs {
        let processes = graph
            .instances
            .iter()
            .map(|instance| {
                (
                    instance.name.as_str(),
                    instance.process.as_deref().unwrap_or("main").to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>();

        for bind in &graph.binds {
            let from_process = processes.get(bind.from.instance.name.as_str())?;
            let to_process = processes.get(bind.to.instance.name.as_str())?;
            if from_process != to_process {
                let boundary = CrossProcessBind {
                    from: format!("{}.{}", bind.from.instance.name, bind.from.port),
                    to: format!("{}.{}", bind.to.instance.name, bind.to.port),
                    from_process: from_process.clone(),
                    to_process: to_process.clone(),
                };
                if matches(&boundary) {
                    return Some(boundary);
                }
            }
        }
    }

    None
}

fn cargo_manifest_with_local_runtime_patch(out_dir: &Path) -> Result<PathBuf> {
    let generated_manifest = out_dir.join("build").join("Cargo.toml");
    let generated = fs::read_to_string(&generated_manifest)
        .with_context(|| format!("failed to read `{}`", generated_manifest.display()))?;
    if generated.contains("[patch.crates-io]") || !manifest_declares_flowrt_dependency(&generated) {
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

fn manifest_declares_flowrt_dependency(manifest: &str) -> bool {
    manifest
        .lines()
        .any(|line| line.trim_start().starts_with("flowrt ="))
}

fn run_cmake_configure_and_build(out_dir: &Path) -> Result<()> {
    let source_dir = out_dir.join("build");
    let build_dir = source_dir.join("cmake");
    let runtime_dir = repo_root_dir()?.join("runtime/cpp");
    run_cmake_configure(&source_dir, &build_dir, &runtime_dir)?;
    run_cmake_build(&build_dir)
}

fn run_cmake_configure(source_dir: &Path, build_dir: &Path, runtime_dir: &Path) -> Result<()> {
    let status = ProcessCommand::new("cmake")
        .arg("-S")
        .arg(source_dir)
        .arg("-B")
        .arg(build_dir)
        .arg(format!(
            "-DFLOWRT_CPP_RUNTIME_DIR={}",
            runtime_dir.to_string_lossy()
        ))
        .status()
        .context("failed to spawn cmake configure")?;
    if !status.success() {
        anyhow::bail!("cmake configure failed with status {status}");
    }
    Ok(())
}

fn run_cmake_build(build_dir: &Path) -> Result<()> {
    let status = ProcessCommand::new("cmake")
        .arg("--build")
        .arg(build_dir)
        .status()
        .context("failed to spawn cmake build")?;
    if !status.success() {
        anyhow::bail!("cmake build failed with status {status}");
    }
    Ok(())
}

fn run_cmake_app(contract: &ContractIr, out_dir: &Path, process: Option<&str>) -> Result<()> {
    let app = cpp_app_executable_path(contract, out_dir);
    if !app.exists() {
        anyhow::bail!(
            "C++ app executable `{}` was not produced; implement `flowrt_user::build_app()` in `src/cpp/*.cpp` or set `FLOWRT_USER_CPP_SOURCES`",
            app.display()
        );
    }
    let mut command = ProcessCommand::new(&app);
    if let Some(process) = process {
        command.arg("--process").arg(process);
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn C++ app `{}`", app.display()))?;
    if !status.success() {
        anyhow::bail!("C++ app invocation failed with status {status}");
    }
    Ok(())
}

fn cpp_app_executable_path(contract: &ContractIr, out_dir: &Path) -> PathBuf {
    out_dir
        .join("build")
        .join("cmake")
        .join(cpp_app_executable_name(contract))
}

fn cpp_app_executable_name(contract: &ContractIr) -> String {
    format!(
        "{}_cpp_app{}",
        sanitize_package_name(&contract.package.name).replace('-', "_"),
        std::env::consts::EXE_SUFFIX
    )
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use flowrt_rsdl::parse_str;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn contract_from_source(source: &str) -> ContractIr {
        let raw = parse_str(source).unwrap();
        let contract = normalize_document(&raw, hash_source(source)).unwrap();
        validate_contract(&contract).unwrap();
        contract
    }

    fn temp_test_dir(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("flowrt-{test_name}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn build_plan_selects_cargo_for_rust_contract() {
        let contract = contract_from_source(
            r#"
[package]
name = "rust_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
        );

        assert_eq!(build_steps(&contract), vec![BuildStep::Cargo]);
    }

    #[test]
    fn build_plan_selects_cmake_for_cpp_contract() {
        let contract = contract_from_source(
            r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
        );

        assert_eq!(build_steps(&contract), vec![BuildStep::Cmake]);
    }

    #[test]
    fn launch_plan_builds_cpp_app_before_running_supervisor() {
        let contract = contract_from_source(
            r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
        );

        assert_eq!(
            launch_steps(&contract),
            vec![
                LaunchStep::Build(BuildStep::Cmake),
                LaunchStep::CargoSupervisor
            ]
        );
    }

    #[test]
    fn build_plan_selects_cargo_and_cmake_for_mixed_contract() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.cpp_worker]
language = "cpp"

[component.rust_worker]
language = "rust"
"#,
        );

        assert_eq!(
            build_steps(&contract),
            vec![BuildStep::Cargo, BuildStep::Cmake]
        );
    }

    #[test]
    fn run_mode_selects_cmake_app_only_for_cpp_only_contracts() {
        let cpp_contract = contract_from_source(
            r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
        );
        assert_eq!(run_mode(&cpp_contract), Some(RunMode::CmakeApp));

        let rust_contract = contract_from_source(
            r#"
[package]
name = "rust_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
        );
        assert_eq!(run_mode(&rust_contract), Some(RunMode::CargoApp));

        let mixed_contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.cpp_worker]
language = "cpp"

[component.rust_worker]
language = "rust"
"#,
        );
        assert_eq!(run_mode(&mixed_contract), None);
        assert!(is_mixed_language_contract(&mixed_contract));
        let error = ensure_direct_runtime_supported(&mixed_contract, "run").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("mixed-language `run` requires backend `iox2`")
        );
    }

    #[test]
    fn run_mode_selects_app_by_process_for_mixed_iox2_contracts() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        assert_eq!(
            run_mode_for_process(&contract, Some("rust_main")).unwrap(),
            RunMode::CargoApp
        );
        assert_eq!(
            run_mode_for_process(&contract, Some("cpp_main")).unwrap(),
            RunMode::CmakeApp
        );
        assert!(run_mode_for_process(&contract, None).is_err());
    }

    #[test]
    fn mixed_runtime_readiness_rejects_same_process_mixed_components() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        let error = ensure_direct_runtime_supported(&contract, "launch").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("process `main`"));
        assert!(message.contains("contains both C++ and Rust components"));
        assert!(message.contains("split them into language-specific RSDL process groups"));
    }

    #[test]
    fn mixed_runtime_readiness_rejects_inproc_cross_process_components() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        let error = ensure_direct_runtime_supported(&contract, "launch").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("mixed-language `launch` requires backend `iox2`"));
        assert!(message.contains("selected backend `inproc`"));
    }

    #[test]
    fn mixed_runtime_readiness_allows_iox2_cross_process_components() {
        let contract = contract_from_source(
            r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        ensure_direct_runtime_supported(&contract, "launch").unwrap();
    }

    #[test]
    fn launch_readiness_rejects_inproc_dataflow_across_process_groups() {
        let contract = contract_from_source(
            r#"
[package]
name = "split_rust_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "source_process"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "sink_process"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        let error = ensure_launch_process_boundaries_supported(&contract).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("backend `inproc`"));
        assert!(message.contains("source_process"));
        assert!(message.contains("sink_process"));
    }

    #[test]
    fn run_process_readiness_rejects_inproc_dataflow_across_process_groups() {
        let contract = contract_from_source(
            r#"
[package]
name = "split_rust_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "source_process"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "sink_process"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        let error =
            ensure_run_process_boundaries_supported(&contract, Some("sink_process")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("backend `inproc`"));
        assert!(message.contains("source_process"));
        assert!(message.contains("sink_process"));
        assert!(message.contains("run --process"));
        ensure_run_process_boundaries_supported(&contract, None).unwrap();
    }

    #[test]
    fn backend_runtime_readiness_allows_cpp_iox2_contracts() {
        let contract = contract_from_source(
            r#"
[package]
name = "cpp_iox2_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        ensure_backend_runtime_supported(&contract, "build").unwrap();
        ensure_backend_runtime_supported(&contract, "run").unwrap();
    }

    #[test]
    fn backend_runtime_readiness_allows_rust_iox2_contracts() {
        let contract = contract_from_source(
            r#"
[package]
name = "rust_iox2_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
        );

        ensure_backend_runtime_supported(&contract, "build").unwrap();
    }

    #[test]
    fn cli_exposes_installed_binary_metadata() {
        let command = Cli::command();

        assert_eq!(command.get_name(), "flowrt");
        assert_eq!(command.get_version(), Some(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn workspace_lock_rejects_concurrent_access_to_same_out_dir() {
        let root = temp_test_dir("workspace-lock");
        let out_dir = root.join("flowrt");

        let first = WorkspaceLock::acquire(&out_dir).expect("first lock should be acquired");
        let error =
            WorkspaceLock::acquire(&out_dir).expect_err("second lock for same out dir should fail");

        assert!(error.to_string().contains("already in use"));
        drop(first);
        WorkspaceLock::acquire(&out_dir).expect("lock should be released on drop");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cargo_manifest_patch_is_skipped_when_flowrt_dependency_is_absent() {
        let root = temp_test_dir("cargo-patch-skip");
        let build_dir = root.join("flowrt").join("build");
        std::fs::create_dir_all(&build_dir).unwrap();
        let manifest = build_dir.join("Cargo.toml");
        std::fs::write(
            &manifest,
            r#"[package]
name = "supervisor-only"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
        )
        .unwrap();

        let patched_manifest = cargo_manifest_with_local_runtime_patch(&root.join("flowrt"))
            .expect("manifest without flowrt dependency should still be accepted");
        let content = std::fs::read_to_string(&patched_manifest).unwrap();

        assert!(!content.contains("[patch.crates-io]"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prepare_workspace_projects_selected_profile_before_validation() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "main"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 1

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
        let rsdl_dir = temp_test_dir("prepare-profile");
        let rsdl_path = rsdl_dir.join("robot.rsdl");
        std::fs::create_dir_all(&rsdl_dir).unwrap();
        std::fs::write(&rsdl_path, source).unwrap();
        let out_dir = rsdl_dir.join("flowrt");

        assert!(load_contract_from_rsdl(&rsdl_path).is_err());
        let prepared = prepare_workspace(&rsdl_path, &out_dir, Some("iox2"))
            .expect("selected profile should prepare");
        let prepared_ir =
            ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
                .unwrap();

        assert_eq!(prepared_ir.profiles.len(), 1);
        assert_eq!(prepared_ir.profiles[0].name, "iox2");
        assert_eq!(prepared_ir.deployments.len(), 1);
        assert_eq!(prepared_ir.deployments[0].profile.name, "iox2");

        let _ = std::fs::remove_dir_all(&rsdl_dir);
    }
}
