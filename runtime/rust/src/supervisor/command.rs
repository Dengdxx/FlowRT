//! Supervisor 子进程可执行文件解析和 Command 构造。

use std::collections::BTreeMap;
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command};

use super::manifest::{
    DEFAULT_RESTART_POLICY, LaunchExternalProcess, LaunchExternalWorkingDir, LaunchProcess,
    ReadinessGate, default_failure_propagation,
};
use super::resource_placement::ResourcePlacement;
use super::zenoh::ZenohLaunchEnv;

pub fn app_executable_for_runtime(
    current_exe: &Path,
    runtime_kind: &str,
    rust_stem: &str,
    cpp_stem: &str,
    ros2_bridge_stem: &str,
) -> Result<PathBuf, String> {
    match runtime_kind {
        "rust" => rust_app_executable(current_exe, rust_stem),
        "cpp" => cpp_app_executable(current_exe, cpp_stem),
        "c" => cpp_app_executable(current_exe, cpp_stem),
        "ros2_bridge" => ros2_bridge_executable(current_exe, ros2_bridge_stem),
        "mixed" => Err("FlowRT mixed process groups are not launchable yet".to_string()),
        other => Err(format!("unknown FlowRT process runtime_kind `{other}`")),
    }
}

fn rust_app_executable(current_exe: &Path, stem: &str) -> Result<PathBuf, String> {
    let mut path = current_exe.to_path_buf();
    path.set_file_name(binary_name(stem));
    Ok(path)
}

fn cpp_app_executable(current_exe: &Path, stem: &str) -> Result<PathBuf, String> {
    let sibling = sibling_executable(current_exe, stem);
    if sibling.exists() {
        return Ok(sibling);
    }
    legacy_cmake_executable(current_exe, stem)
}

fn ros2_bridge_executable(current_exe: &Path, stem: &str) -> Result<PathBuf, String> {
    let sibling = sibling_executable(current_exe, stem);
    if sibling.exists() {
        return Ok(sibling);
    }
    legacy_cmake_executable(current_exe, stem)
}

fn sibling_executable(current_exe: &Path, stem: &str) -> PathBuf {
    let mut path = current_exe.to_path_buf();
    path.set_file_name(binary_name(stem));
    path
}

fn legacy_cmake_executable(current_exe: &Path, stem: &str) -> Result<PathBuf, String> {
    let build_dir = current_exe
        .parent()
        .and_then(|profile_dir| profile_dir.parent())
        .and_then(|target_dir| target_dir.parent())
        .ok_or_else(|| {
            format!(
                "failed to resolve FlowRT build directory from `{}`",
                current_exe.display()
            )
        })?;
    let mut path = build_dir.join("cmake");
    path.push(binary_name(stem));
    Ok(path)
}

pub(super) fn binary_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// external process 可执行文件解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalExecutableResolution {
    pub executable: PathBuf,
    pub package_root: PathBuf,
    pub workspace_root: PathBuf,
}

/// 解析 external package 的可执行文件路径。
pub fn external_app_executable(
    current_exe: &Path,
    external: &LaunchExternalProcess,
) -> Result<ExternalExecutableResolution, String> {
    validate_external_executable_fragment(external)?;
    let workspace_root = workspace_root_from_supervisor(current_exe)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    let mut searched = Vec::new();
    for root in external_package_roots(current_exe, external, &workspace_root) {
        let candidate = root.join(&external.executable);
        searched.push(candidate.clone());
        if candidate.exists() {
            let package_root = root.canonicalize().map_err(|error| {
                format!(
                    "failed to canonicalize external package `{}` root `{}`: {error}",
                    external.package,
                    root.display()
                )
            })?;
            let executable = candidate.canonicalize().map_err(|error| {
                format!(
                    "failed to canonicalize external package `{}` executable `{}`: {error}",
                    external.package,
                    candidate.display()
                )
            })?;
            if !executable.starts_with(&package_root) {
                return Err(format!(
                    "external package `{}` executable `{}` escapes package root `{}`",
                    external.package,
                    external.executable,
                    package_root.display()
                ));
            }
            return Ok(ExternalExecutableResolution {
                executable,
                package_root,
                workspace_root,
            });
        }
    }

    Err(format!(
        "external package `{}` executable `{}` was not found; searched: {}",
        external.package,
        external.executable,
        searched
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn validate_external_executable_fragment(external: &LaunchExternalProcess) -> Result<(), String> {
    let executable = Path::new(&external.executable);
    if external.executable.trim().is_empty() {
        return Err(format!(
            "external package `{}` executable must not be empty",
            external.package
        ));
    }
    if executable.is_absolute()
        || executable
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "external package `{}` executable `{}` must be package-relative without `.` or `..` components",
            external.package, external.executable
        ));
    }
    Ok(())
}

fn external_package_roots(
    current_exe: &Path,
    external: &LaunchExternalProcess,
    workspace_root: &Path,
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = &external.package_root {
        push_unique_path(&mut roots, root.clone());
    }
    if let Some(paths) = std::env::var_os("FLOWRT_EXTERNAL_PATH") {
        for entry in std::env::split_paths(&paths) {
            push_external_search_entry(&mut roots, entry, &external.package);
        }
    }
    push_unique_path(
        &mut roots,
        PathBuf::from("/opt/flowrt/external").join(&external.package),
    );
    push_unique_path(
        &mut roots,
        workspace_root.join("external").join(&external.package),
    );
    if let Some(supervisor_workspace) = workspace_root_from_supervisor(current_exe) {
        push_unique_path(
            &mut roots,
            supervisor_workspace
                .join("external")
                .join(&external.package),
        );
    }
    if let Ok(current_dir) = std::env::current_dir() {
        push_unique_path(
            &mut roots,
            current_dir.join("external").join(&external.package),
        );
    }
    roots
}

fn push_external_search_entry(roots: &mut Vec<PathBuf>, entry: PathBuf, package: &str) {
    push_unique_path(roots, entry.clone());
    push_unique_path(roots, entry.join(package));
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn workspace_root_from_supervisor(current_exe: &Path) -> Option<PathBuf> {
    current_exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(Path::to_path_buf)
}

// ---------------------------------------------------------------------------
// 进程启动
// ---------------------------------------------------------------------------

fn configure_supervised_command(command: &mut Command) {
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
}

/// 构造子进程 Command。
///
/// - 非 ros2_bridge runtime 进程传 `--process <name>`
/// - ros2_bridge runtime 进程设置 `RMW_IMPLEMENTATION=rmw_zenoh_cpp`
/// - 注入 zenoh 环境变量
pub fn build_process_command(
    app_exe: &Path,
    process: &LaunchProcess,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
) -> Command {
    let mut command = Command::new(app_exe);
    apply_process_env(&mut command, &process.env);
    if process.runtime_kind == "ros2_bridge" {
        configure_ros2_bridge_env(&mut command, &process.env);
    } else {
        command.arg("--process").arg(&process.name);
    }
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-steps").arg(run_ticks.to_string());
    }
    if let Some(env) = zenoh_env {
        command.env("FLOWRT_ZENOH_MODE", "peer");
        if !env.listen.is_empty() {
            command.env("FLOWRT_ZENOH_LISTEN", &env.listen);
        }
        if !env.connect.is_empty() {
            command.env("FLOWRT_ZENOH_CONNECT", &env.connect);
        }
        command.env("FLOWRT_ZENOH_NO_MULTICAST", "1");
    }
    configure_supervised_command(&mut command);
    command
}

/// 构造子进程 Command，并可为 generated runtime shell 注入终态 status snapshot 路径。
pub fn build_process_command_with_status_out(
    app_exe: &Path,
    process: &LaunchProcess,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
    status_dir: Option<&Path>,
) -> Command {
    let mut command = build_process_command(app_exe, process, run_ticks, zenoh_env);
    if let Some(status_dir) = status_dir {
        command.env(
            "FLOWRT_STATUS_OUT",
            status_dir.join(format!("{}.status.json", process.name)),
        );
    }
    command
}

/// 构造 external process Command。
pub fn build_external_process_command(
    app_exe: &Path,
    process: &LaunchProcess,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
    external: &LaunchExternalProcess,
    resolution: &ExternalExecutableResolution,
) -> Command {
    let mut command = Command::new(app_exe);
    apply_process_env(&mut command, &process.env);
    command.args(&external.args);
    command.current_dir(match external.working_dir {
        LaunchExternalWorkingDir::Package => &resolution.package_root,
        LaunchExternalWorkingDir::Workspace => &resolution.workspace_root,
    });
    command.env("FLOWRT_PROCESS", &process.name);
    command.env("FLOWRT_BACKEND", &process.backend);
    command.env("FLOWRT_EXTERNAL_PACKAGE", &external.package);
    command.env("FLOWRT_EXTERNAL_EXECUTABLE", &external.executable);
    command.env("FLOWRT_EXTERNAL_PACKAGE_ROOT", &resolution.package_root);
    command.env("FLOWRT_WORKSPACE_ROOT", &resolution.workspace_root);
    if let Some(run_ticks) = run_ticks {
        command.env("FLOWRT_RUN_STEPS", run_ticks.to_string());
    }
    if let Some(env) = zenoh_env {
        command.env("FLOWRT_ZENOH_MODE", "peer");
        if !env.listen.is_empty() {
            command.env("FLOWRT_ZENOH_LISTEN", &env.listen);
        }
        if !env.connect.is_empty() {
            command.env("FLOWRT_ZENOH_CONNECT", &env.connect);
        }
        command.env("FLOWRT_ZENOH_NO_MULTICAST", "1");
    }
    configure_supervised_command(&mut command);
    command
}

fn apply_process_env(command: &mut Command, env: &BTreeMap<String, String>) {
    for (key, value) in env {
        command.env(key, value);
    }
}

fn configure_ros2_bridge_env(command: &mut Command, process_env: &BTreeMap<String, String>) {
    command.env("RMW_IMPLEMENTATION", "rmw_zenoh_cpp");
    if let Some(paths) = ros2_bridge_library_path(process_env) {
        command.env("LD_LIBRARY_PATH", paths);
    }
}

fn ros2_bridge_library_path(process_env: &BTreeMap<String, String>) -> Option<OsString> {
    let prefixes = ros2_prefixes(process_env);
    let existing = env_value(process_env, "LD_LIBRARY_PATH");
    let existing_paths = existing
        .as_ref()
        .map(|raw| std::env::split_paths(raw).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut paths = Vec::new();

    for prefix in &prefixes {
        push_unique_path(&mut paths, prefix.join("opt/zenoh_cpp_vendor/lib"));
    }
    for path in &existing_paths {
        if prefixes.iter().any(|prefix| path.starts_with(prefix)) {
            push_unique_path(&mut paths, path.clone());
        }
    }
    for prefix in &prefixes {
        push_unique_path(&mut paths, prefix.join("lib"));
    }
    for path in existing_paths {
        if prefixes.iter().any(|prefix| path.starts_with(prefix))
            || is_flowrt_private_library_path(&path, process_env)
        {
            continue;
        }
        push_unique_path(&mut paths, path);
    }

    if paths.is_empty() {
        return None;
    }
    std::env::join_paths(paths).ok()
}

fn ros2_prefixes(process_env: &BTreeMap<String, String>) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    for key in ["AMENT_PREFIX_PATH", "COLCON_PREFIX_PATH"] {
        if let Some(raw) = env_value(process_env, key) {
            for path in std::env::split_paths(&raw) {
                push_unique_path(&mut prefixes, path);
            }
        }
    }
    prefixes
}

fn is_flowrt_private_library_path(path: &Path, process_env: &BTreeMap<String, String>) -> bool {
    if path.starts_with("/opt/flowrt") {
        return true;
    }
    ["FLOWRT_PRIVATE_PREFIX", "FLOWRT_CPP_RUNTIME_DIR"]
        .into_iter()
        .filter_map(|key| env_value(process_env, key))
        .flat_map(|raw| std::env::split_paths(&raw).collect::<Vec<_>>())
        .any(|prefix| path.starts_with(prefix))
}

fn env_value(process_env: &BTreeMap<String, String>, key: &str) -> Option<OsString> {
    process_env
        .get(key)
        .map(|value| OsString::from(value.as_str()))
        .or_else(|| std::env::var_os(key))
}

pub(super) fn build_launch_process_command(
    app_exe: &Path,
    process: &LaunchProcess,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
    external_resolution: Option<&ExternalExecutableResolution>,
    status_dir: Option<&Path>,
) -> Result<Command, String> {
    if process.runtime_kind == "external" {
        let external = process.external.as_ref().ok_or_else(|| {
            format!(
                "external process `{}` is missing launch manifest external metadata",
                process.name
            )
        })?;
        let resolution = external_resolution.ok_or_else(|| {
            format!(
                "external process `{}` package `{}` executable `{}` was not resolved",
                process.name, external.package, external.executable
            )
        })?;
        Ok(build_external_process_command(
            app_exe, process, run_ticks, zenoh_env, external, resolution,
        ))
    } else {
        Ok(build_process_command_with_status_out(
            app_exe, process, run_ticks, zenoh_env, status_dir,
        ))
    }
}

/// 启动子进程。
pub fn spawn_flowrt_process(
    app_exe: &Path,
    process_name: &str,
    runtime_kind: &str,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
) -> Result<Child, String> {
    let process = LaunchProcess {
        name: process_name.to_string(),
        backend: "inproc".to_string(),
        target: None,
        runtimes: Vec::new(),
        runtime_kind: runtime_kind.to_string(),
        external: None,
        depends_on: Vec::new(),
        env: BTreeMap::new(),
        restart: DEFAULT_RESTART_POLICY,
        failure: default_failure_propagation(),
        readiness: ReadinessGate::ProcessStarted,
        startup_delay_ms: 0,
        instances: Vec::new(),
        tasks: Vec::new(),
        resource_placement: ResourcePlacement::default(),
        io_boundaries: Vec::new(),
    };
    build_process_command(app_exe, &process, run_ticks, zenoh_env)
        .spawn()
        .map_err(|error| format!("failed to start FlowRT process `{process_name}`: {error}"))
}

pub(super) fn spawn_launch_process(
    app_exe: &Path,
    process: &LaunchProcess,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
    external_resolution: Option<&ExternalExecutableResolution>,
    status_dir: Option<&Path>,
) -> Result<Child, String> {
    build_launch_process_command(
        app_exe,
        process,
        run_ticks,
        zenoh_env,
        external_resolution,
        status_dir,
    )?
    .spawn()
    .map_err(|error| {
        format!(
            "failed to start FlowRT process `{}` executable `{}`: {error}",
            process.name,
            app_exe.display()
        )
    })
}
