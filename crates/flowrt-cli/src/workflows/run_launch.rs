use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildStep {
    CargoApp,
    CargoSupervisor,
    CmakeApp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunMode {
    CargoApp,
    CmakeApp,
}

pub(crate) fn build_steps(contract: &ContractIr, include_launcher: bool) -> Vec<BuildStep> {
    let mut steps = Vec::new();
    if has_component_language(contract, LanguageKind::Rust) {
        steps.push(BuildStep::CargoApp);
    }
    if has_cmake_app_components(contract) || has_ros2_bridge(contract) {
        steps.push(BuildStep::CmakeApp);
    }
    if include_launcher {
        steps.push(BuildStep::CargoSupervisor);
    }
    steps
}

pub(crate) fn run_mode(contract: &ContractIr) -> Option<RunMode> {
    match (
        has_component_language(contract, LanguageKind::Rust),
        has_cmake_app_components(contract),
    ) {
        (true, false) => Some(RunMode::CargoApp),
        (false, true) => Some(RunMode::CmakeApp),
        _ => None,
    }
}

pub(crate) fn run_mode_for_process(
    contract: &ContractIr,
    process: Option<&str>,
) -> Result<RunMode> {
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

pub(crate) fn process_runtime_flags(
    contract: &ContractIr,
    process: &str,
) -> Option<ProcessRuntimeFlags> {
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

pub(crate) fn has_component_language(contract: &ContractIr, language: LanguageKind) -> bool {
    contract
        .components
        .iter()
        .any(|component| component.language == language)
}

pub(crate) fn has_ros2_bridge(contract: &ContractIr) -> bool {
    contract
        .graphs
        .iter()
        .any(|graph| !graph.ros2_bridges.is_empty())
}

pub(crate) fn is_mixed_language_contract(contract: &ContractIr) -> bool {
    has_component_language(contract, LanguageKind::Rust) && has_cmake_app_components(contract)
}

pub(crate) fn ensure_direct_runtime_supported(contract: &ContractIr, command: &str) -> Result<()> {
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

    if let Some(boundary) = first_mixed_language_bind_with_unsupported_backend(contract) {
        anyhow::bail!(
            "mixed-language `{command}` cannot carry dataflow `{}` -> `{}` over backend `{}`; use backend `iox2` or `zenoh` for cross-language process boundaries",
            boundary.from,
            boundary.to,
            boundary.backend
        );
    }

    Ok(())
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessRuntimeFlags {
    pub(crate) cpp: bool,
    pub(crate) rust: bool,
}

impl ProcessRuntimeFlags {
    pub(crate) fn add(&mut self, language: LanguageKind) {
        match language {
            LanguageKind::C | LanguageKind::Cpp => self.cpp = true,
            LanguageKind::Rust => self.rust = true,
            LanguageKind::External => {}
        }
    }

    pub(crate) fn is_mixed(&self) -> bool {
        self.cpp && self.rust
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MixedProcessGroup {
    pub(crate) graph: String,
    pub(crate) process: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MixedLanguageBind {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) backend: String,
}

pub(crate) fn mixed_process_group(contract: &ContractIr) -> Option<MixedProcessGroup> {
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

pub(crate) fn selected_runtime_backend_name(contract: &ContractIr) -> &str {
    contract
        .profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| contract.profiles.first())
        .map(|profile| profile.backend.0.as_str())
        .unwrap_or("inproc")
}

pub(crate) fn first_mixed_language_bind_with_unsupported_backend(
    contract: &ContractIr,
) -> Option<MixedLanguageBind> {
    let component_languages = contract
        .components
        .iter()
        .map(|component| (component.name.as_str(), component.language))
        .collect::<BTreeMap<_, _>>();

    for graph in &contract.graphs {
        let instance_languages = graph
            .instances
            .iter()
            .filter_map(|instance| {
                component_languages
                    .get(instance.component.name.as_str())
                    .copied()
                    .map(|language| (instance.name.as_str(), language))
            })
            .collect::<BTreeMap<_, _>>();

        for bind in &graph.binds {
            let Some(from_language) = instance_languages
                .get(bind.from.instance.name.as_str())
                .copied()
            else {
                continue;
            };
            let Some(to_language) = instance_languages
                .get(bind.to.instance.name.as_str())
                .copied()
            else {
                continue;
            };
            if from_language == to_language || matches!(bind.backend.0.as_str(), "iox2" | "zenoh") {
                continue;
            }
            return Some(MixedLanguageBind {
                from: format!("{}.{}", bind.from.instance.name, bind.from.port),
                to: format!("{}.{}", bind.to.instance.name, bind.to.port),
                backend: bind.backend.0.clone(),
            });
        }
    }

    None
}

pub(crate) fn ensure_backend_runtime_supported(
    _contract: &ContractIr,
    _command: &str,
) -> Result<()> {
    Ok(())
}

pub(crate) fn run_workspace(
    contract: &ContractIr,
    out_dir: &Path,
    process: Option<&str>,
    run_ticks: Option<usize>,
    requested_build_mode: Option<BuildMode>,
    replay_source: Option<&Path>,
) -> Result<()> {
    ensure_direct_runtime_supported(contract, "run")?;
    ensure_backend_runtime_supported(contract, "run")?;
    ensure_run_process_boundaries_supported(contract, process)?;
    let build_info = load_build_info(out_dir, requested_build_mode, false)?;
    match run_mode_for_process(contract, process)
        .context("contract does not contain runnable components")?
    {
        RunMode::CargoApp => {
            let bin = executable_from_build_info(
                out_dir,
                build_info.executables.rust_app.as_ref(),
                "Rust app",
                "flowrt build",
            )?;
            if !bin.exists() {
                anyhow::bail!(
                    "app binary `{}` not found; run `flowrt build` first",
                    bin.display()
                );
            }
            run_binary(&bin, process, run_ticks, replay_source)?;
        }
        RunMode::CmakeApp => {
            let bin = executable_from_build_info(
                out_dir,
                build_info.executables.cpp_app.as_ref(),
                "C++ app",
                "flowrt build",
            )?;
            run_cmake_app(&bin, process, run_ticks, replay_source)?;
        }
    }
    Ok(())
}

pub(crate) fn launch_workspace(
    contract: &ContractIr,
    out_dir: &Path,
    run_ticks: Option<usize>,
    requested_build_mode: Option<BuildMode>,
) -> Result<()> {
    ensure_direct_runtime_supported(contract, "launch")?;
    ensure_backend_runtime_supported(contract, "launch")?;
    ensure_launch_process_boundaries_supported(contract)?;
    let build_info = load_build_info(out_dir, requested_build_mode, true)?;
    let supervisor = executable_from_build_info(
        out_dir,
        build_info.executables.supervisor.as_ref(),
        "FlowRT supervisor",
        "flowrt build --launcher",
    )?;
    if !supervisor.exists() {
        anyhow::bail!(
            "FlowRT supervisor `{}` not found; run `flowrt build --launcher` first",
            supervisor.display()
        );
    }
    run_supervisor_binary(&supervisor, run_ticks)?;
    Ok(())
}

pub(crate) fn ensure_launch_process_boundaries_supported(contract: &ContractIr) -> Result<()> {
    let backend = selected_runtime_backend_name(contract);
    if backend != "inproc" {
        return Ok(());
    }

    if let Some(boundary) = first_cross_process_bind(contract) {
        anyhow::bail!(
            "backend `inproc` cannot launch dataflow `{}` -> `{}` across process groups `{}` -> `{}`; use backend `iox2` or `zenoh`, or place both instances in the same RSDL process group",
            boundary.from,
            boundary.to,
            boundary.from_process,
            boundary.to_process
        );
    }

    Ok(())
}

pub(crate) fn ensure_run_process_boundaries_supported(
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
            "backend `inproc` cannot run --process `{}` because dataflow `{}` -> `{}` crosses process groups `{}` -> `{}`; use backend `iox2` or `zenoh`, run the whole inproc app, or place both instances in the same RSDL process group",
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
pub(crate) struct CrossProcessBind {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) from_process: String,
    pub(crate) to_process: String,
}

pub(crate) fn first_cross_process_bind(contract: &ContractIr) -> Option<CrossProcessBind> {
    first_cross_process_bind_matching(contract, |_| true)
}

pub(crate) fn first_cross_process_bind_for_process(
    contract: &ContractIr,
    process: &str,
) -> Option<CrossProcessBind> {
    first_cross_process_bind_matching(contract, |boundary| {
        boundary.from_process == process || boundary.to_process == process
    })
}

pub(crate) fn first_cross_process_bind_matching(
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
            if bind.backend.0 != "inproc" {
                continue;
            }
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

pub(crate) fn run_cmake_app(
    app: &Path,
    process: Option<&str>,
    run_ticks: Option<usize>,
    replay_source: Option<&Path>,
) -> Result<()> {
    if !app.exists() {
        anyhow::bail!(
            "C++ app executable `{}` not found; run `flowrt build` first",
            app.display()
        );
    }
    let mut command = ProcessCommand::new(app);
    if let Some(process) = process {
        command.arg("--process").arg(process);
    }
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-steps").arg(run_ticks.to_string());
    }
    // C++ runtime 无 MCAP 解析能力：把 MCAP 回放源规范化为 JSONL 时间线后再注入，由 C++ 生成
    // shell 经 flowrt::replay_driver_from_timeline_file 解析。Rust 路径仍直读 MCAP。
    let cpp_replay_timeline = match replay_source {
        Some(replay_source) => Some(cpp_prepare_replay_timeline(app, replay_source)?),
        None => None,
    };
    if let Some(timeline_path) = cpp_replay_timeline.as_deref() {
        command.env("FLOWRT_REPLAY_SOURCE", timeline_path);
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn C++ app `{}`", app.display()))?;
    if let Some(timeline_path) = cpp_replay_timeline.as_deref() {
        let _ = std::fs::remove_file(timeline_path);
    }
    if !status.success() {
        anyhow::bail!("C++ app invocation failed with status {status}");
    }
    Ok(())
}

/// 把 MCAP 回放源规范化为 C++ runtime 可解析的 JSONL 时间线，返回临时文件路径。
///
/// C++ runtime 不解析 MCAP；CLI 读取 MCAP（flowrt-record，单一 MCAP 解析点）后写出按时间升序的
/// JSONL 时间线，交由 C++ 生成 shell 装配回放驱动。读取或写入失败 fail-fast，不静默回退。
fn cpp_prepare_replay_timeline(app: &Path, replay_source: &Path) -> Result<std::path::PathBuf> {
    let entries = flowrt_record::read_replay_timeline_from_path(replay_source)
        .with_context(|| format!("failed to read replay source `{}`", replay_source.display()))?;
    let timeline_path = cpp_replay_timeline_path(app);
    flowrt_record::write_replay_timeline_jsonl_to_path(&timeline_path, &entries).with_context(
        || {
            format!(
                "failed to write replay timeline `{}`",
                timeline_path.display()
            )
        },
    )?;
    Ok(timeline_path)
}

/// 派生 C++ 回放 JSONL 时间线的临时文件路径（按 app 文件名与当前进程号区分）。
fn cpp_replay_timeline_path(app: &Path) -> std::path::PathBuf {
    let file_name = app
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("flowrt-cpp-app");
    std::env::temp_dir().join(format!(
        ".{file_name}.flowrt-replay.{}.jsonl",
        std::process::id()
    ))
}

pub(crate) fn run_binary(
    binary: &Path,
    process: Option<&str>,
    run_ticks: Option<usize>,
    replay_source: Option<&Path>,
) -> Result<()> {
    let mut command = ProcessCommand::new(binary);
    if let Some(process) = process {
        command.arg("--process").arg(process);
    }
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-steps").arg(run_ticks.to_string());
    }
    if let Some(replay_source) = replay_source {
        command.env("FLOWRT_REPLAY_SOURCE", replay_source);
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn `{}`", binary.display()))?;
    if !status.success() {
        anyhow::bail!("app invocation failed with status {status}");
    }
    Ok(())
}

pub(crate) fn run_supervisor_binary(binary: &Path, run_ticks: Option<usize>) -> Result<()> {
    let mut command = ProcessCommand::new(binary);
    inject_flowrt_launch_library_path(&mut command)?;
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-steps").arg(run_ticks.to_string());
    }
    let status = command
        .status()
        .with_context(|| format!("failed to spawn `{}`", binary.display()))?;
    if !status.success() {
        anyhow::bail!("FlowRT supervisor invocation failed with status {status}");
    }
    Ok(())
}

pub(crate) fn inject_flowrt_launch_library_path(command: &mut ProcessCommand) -> Result<()> {
    let Some(runtime_dir) = cpp_runtime_dir_for_generated_build()? else {
        return Ok(());
    };
    let paths = flowrt_runtime_library_paths(&runtime_dir, host_flowrt_platform());
    if paths.is_empty() {
        return Ok(());
    }
    command.env(
        "LD_LIBRARY_PATH",
        prepend_env_paths("LD_LIBRARY_PATH", &paths)?,
    );
    Ok(())
}

pub(crate) fn flowrt_runtime_library_paths(
    runtime_dir: &Path,
    platform: Option<&str>,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    push_existing_unique_path(&mut paths, runtime_dir.join("lib"));
    if let Some(prefix) = flowrt_private_prefix_from_cpp_runtime_dir(runtime_dir)
        .or_else(|| flowrt_private_prefix_from_target_sdk_dir(runtime_dir))
    {
        push_existing_unique_path(&mut paths, prefix.join("lib"));
        if let Some(platform) = platform {
            push_existing_unique_path(
                &mut paths,
                prefix.join("targets").join(platform).join("lib"),
            );
        }
    }
    paths
}

pub(crate) fn flowrt_private_prefix_from_target_sdk_dir(runtime_dir: &Path) -> Option<PathBuf> {
    if !runtime_dir.join("flowrt-target-sdk.toml").exists() {
        return None;
    }
    let targets = runtime_dir.parent()?;
    if targets.file_name()? != OsStr::new("targets") {
        return None;
    }
    Some(targets.parent()?.to_path_buf())
}

pub(crate) fn host_flowrt_platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-amd64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        _ => None,
    }
}

pub(crate) fn push_existing_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() && !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

pub(crate) fn prepend_env_paths(var_name: &str, paths: &[PathBuf]) -> Result<OsString> {
    let mut merged = paths.to_vec();
    if let Some(existing) = env::var_os(var_name) {
        merged.extend(env::split_paths(&existing));
    }
    env::join_paths(merged).with_context(|| format!("failed to build {var_name}"))
}
