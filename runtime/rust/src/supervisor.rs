//! Supervisor engine：进程编排、依赖排序、重启策略和健康监控。
//!
//! 本模块把 supervisor 核心逻辑从 codegen 字符串下沉为可独立测试的 runtime 深模块。
//! 生成物只保留 manifest 常量、binary name stems 和 self-description hash 的薄 glue。

pub mod resource_placement;

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use serde::Deserialize;

use crate::introspection::{IntrospectionIdentity, IntrospectionProcessStatus, IntrospectionState};

use self::resource_placement::{ResourceApplied, ResourcePlacement, ResourcePlacementStatus};

/// 子进程健康轮询间隔。
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// tick 无变化超过此阈值则标记 stale。
const TICK_STALE_AFTER_MS: u64 = 1_000;
/// readiness 等待轮询间隔。
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// readiness 等待超时时间。
const READINESS_TIMEOUT: Duration = Duration::from_secs(30);

/// readiness 等待参数，支持测试注入短超时。
#[derive(Debug, Clone)]
struct ReadinessConfig {
    timeout: Duration,
    poll_interval: Duration,
}

impl Default for ReadinessConfig {
    fn default() -> Self {
        Self {
            timeout: READINESS_TIMEOUT,
            poll_interval: READINESS_POLL_INTERVAL,
        }
    }
}

// ---------------------------------------------------------------------------
// launch manifest 反序列化结构
// ---------------------------------------------------------------------------

/// supervisor 消费的 launch manifest 顶层。
#[derive(Debug, Deserialize)]
pub struct LaunchManifest {
    pub graphs: Vec<LaunchGraph>,
}

/// manifest 中的 graph 节点。
#[derive(Debug, Deserialize)]
pub struct LaunchGraph {
    #[serde(default)]
    pub services: Vec<LaunchService>,
    pub processes: Vec<LaunchProcess>,
}

/// manifest 中单个 service bind 描述。
#[derive(Debug, Deserialize)]
pub struct LaunchService {
    pub name: String,
    pub server_instance: String,
}

/// manifest 中单个进程描述。
#[derive(Debug, Deserialize)]
pub struct LaunchProcess {
    pub name: String,
    pub backend: String,
    pub runtime_kind: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default = "default_restart_policy")]
    pub restart: RestartPolicy,
    #[serde(default = "default_failure_propagation")]
    pub failure: String,
    /// 进程 readiness gate 类型。
    #[serde(default)]
    pub readiness: ReadinessGate,
    /// readiness 通过后的额外启动延迟（ms），用于错峰启动。
    #[serde(default)]
    pub startup_delay_ms: u64,
    /// 属于该进程的 instance 名称。
    #[serde(default)]
    pub instances: Vec<String>,
    /// 进程资源提示（CPU affinity、nice、RT policy）。
    #[serde(default)]
    pub resource_placement: ResourcePlacement,
}

/// 进程 readiness gate 类型，决定 supervisor 何时认为进程就绪。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessGate {
    /// 进程已启动（PID 存在）即视为就绪。
    #[default]
    ProcessStarted,
    /// 进程的 runtime introspection 握手成功即视为就绪。
    RuntimeReady,
    /// 进程的 runtime introspection 握手成功且所有 service endpoint 就绪。
    ServiceReady,
}

fn default_restart_policy() -> RestartPolicy {
    DEFAULT_RESTART_POLICY
}

fn default_failure_propagation() -> String {
    "propagate".to_string()
}

// ---------------------------------------------------------------------------
// 重启策略
// ---------------------------------------------------------------------------

/// 重启策略类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicyKind {
    Never,
    OnFailure,
    Always,
}

/// 重启策略参数。
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct RestartPolicy {
    pub policy: RestartPolicyKind,
    pub max_restarts: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
}

/// 默认重启策略：失败时重启，最多 3 次，退避 100–1000ms。
pub const DEFAULT_RESTART_POLICY: RestartPolicy = RestartPolicy {
    policy: RestartPolicyKind::OnFailure,
    max_restarts: 3,
    initial_delay_ms: 100,
    max_delay_ms: 1_000,
};

impl RestartPolicy {
    /// 判断当前状态是否允许重启。
    pub fn can_restart(self, success: bool, restart_count: u32) -> bool {
        match self.policy {
            RestartPolicyKind::Never => false,
            RestartPolicyKind::OnFailure => !success && restart_count < self.max_restarts,
            RestartPolicyKind::Always => restart_count < self.max_restarts,
        }
    }

    /// 返回指定重启次数对应的退避延迟（ms），指数退避 + 上限截断。
    pub fn delay_ms_for(self, restart_count: u32) -> u64 {
        let shift = restart_count.min(63);
        let multiplier = 1_u64.checked_shl(shift).unwrap_or(u64::MAX);
        self.initial_delay_ms
            .saturating_mul(multiplier)
            .min(self.max_delay_ms)
    }
}

// ---------------------------------------------------------------------------
// Zenoh 自动 mesh 配置
// ---------------------------------------------------------------------------

/// Zenoh 启动环境变量。
#[derive(Debug, Clone)]
pub struct ZenohLaunchEnv {
    pub listen: String,
    pub connect: String,
}

/// 检查是否需要自动配置 zenoh（用户未显式设置相关环境变量时）。
pub fn should_auto_configure_zenoh() -> bool {
    std::env::var_os("FLOWRT_ZENOH_MODE").is_none()
        && std::env::var_os("FLOWRT_ZENOH_LISTEN").is_none()
        && std::env::var_os("FLOWRT_ZENOH_CONNECT").is_none()
}

/// 为 graph 中的 zenoh 进程生成 hub-and-spoke 拓扑配置。
///
/// 第一个 zenoh backend 进程作为 hub，监听随机端口；其余进程连接该 hub。
pub fn zenoh_launch_env_for_graph(
    processes: &[&LaunchProcess],
) -> Result<HashMap<String, ZenohLaunchEnv>, String> {
    let zenoh_processes: Vec<&LaunchProcess> = processes
        .iter()
        .filter(|p| p.backend == "zenoh")
        .copied()
        .collect();
    if zenoh_processes.is_empty() {
        return Ok(HashMap::new());
    }

    let hub = zenoh_processes[0];
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| {
        format!(
            "failed to reserve local zenoh port for `{}`: {error}",
            hub.name
        )
    })?;
    let port = listener
        .local_addr()
        .map_err(|error| {
            format!(
                "failed to read local zenoh port for `{}`: {error}",
                hub.name
            )
        })?
        .port();
    let hub_locator = format!("tcp/127.0.0.1:{port}");
    drop(listener);

    let mut env = HashMap::new();
    for process in zenoh_processes {
        let listen = if process.name == hub.name {
            hub_locator.clone()
        } else {
            String::new()
        };
        let connect = if process.name == hub.name {
            String::new()
        } else {
            hub_locator.clone()
        };
        env.insert(process.name.clone(), ZenohLaunchEnv { listen, connect });
    }
    Ok(env)
}

// ---------------------------------------------------------------------------
// 依赖排序
// ---------------------------------------------------------------------------

/// 判断进程的所有依赖是否已满足。
///
/// 对于 `process_started` readiness gate，依赖进程只需已启动（PID 存在）。
/// 对于 `runtime_ready` 和 `service_ready` gate，依赖进程必须已通过 readiness 检查。
pub fn process_dependencies_satisfied(
    process: &LaunchProcess,
    spawned_names: &BTreeSet<String>,
    ready_names: &BTreeSet<String>,
) -> bool {
    process.depends_on.iter().all(|dependency| {
        // 如果依赖进程已通过 readiness gate 则满足；
        // 否则如果依赖进程已启动且本进程的 readiness 是 process_started 也满足。
        ready_names.contains(dependency)
            || (process.readiness == ReadinessGate::ProcessStarted
                && spawned_names.contains(dependency))
    })
}

/// 等待子进程通过 readiness gate。
///
/// - `ProcessStarted`：进程已启动即通过，无需额外等待。
/// - `RuntimeReady`：轮询 introspection socket 直到握手成功或超时。
/// - `ServiceReady`：轮询 introspection socket 直到握手成功且所有 service endpoint
///   就绪或超时。
///
/// 超时返回错误，同时终止子进程。
fn wait_for_readiness(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    config: &ReadinessConfig,
) -> Result<(), String> {
    match child.readiness {
        ReadinessGate::ProcessStarted => Ok(()),
        ReadinessGate::RuntimeReady => {
            child.state = "waiting_readiness".to_string();
            record_child_health(supervisor_state, child, false);
            wait_for_runtime_ready(supervisor_state, child, config)
        }
        ReadinessGate::ServiceReady => {
            child.state = "waiting_readiness".to_string();
            record_child_health(supervisor_state, child, false);
            wait_for_service_ready(supervisor_state, child, config)
        }
    }
}

/// 等待子进程 runtime introspection 握手成功。
fn wait_for_runtime_ready(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    config: &ReadinessConfig,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + config.timeout;
    loop {
        // 检查子进程是否已退出。
        if let Some(status) = child.child.try_wait().map_err(|error| {
            format!(
                "failed to poll FlowRT process `{}` during readiness wait: {error}",
                child.name
            )
        })? {
            child.exit_code = status.code();
            child.finished = true;
            child.state = "readiness_failed".to_string();
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` exited during readiness wait (code: {})",
                child.name,
                child
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ));
        }

        // 检查是否超时。
        if std::time::Instant::now() >= deadline {
            let _ = child.child.kill();
            let _ = child.child.wait();
            child.finished = true;
            child.state = "readiness_timeout".to_string();
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` readiness timed out waiting for runtime_ready",
                child.name
            ));
        }

        // 轮询 introspection socket。
        match crate::request_status(&child.socket) {
            Ok(crate::IntrospectionResponse::Status { .. }) => return Ok(()),
            _ => std::thread::sleep(config.poll_interval),
        }
    }
}

/// 等待子进程 runtime introspection 握手成功且该进程预期承载的 service endpoint 就绪。
fn wait_for_service_ready(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    config: &ReadinessConfig,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + config.timeout;
    loop {
        // 检查子进程是否已退出。
        if let Some(status) = child.child.try_wait().map_err(|error| {
            format!(
                "failed to poll FlowRT process `{}` during readiness wait: {error}",
                child.name
            )
        })? {
            child.exit_code = status.code();
            child.finished = true;
            child.state = "readiness_failed".to_string();
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` exited during readiness wait (code: {})",
                child.name,
                child
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ));
        }

        // 检查是否超时。
        if std::time::Instant::now() >= deadline {
            let _ = child.child.kill();
            let _ = child.child.wait();
            child.finished = true;
            child.state = "readiness_timeout".to_string();
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` readiness timed out waiting for service_ready",
                child.name
            ));
        }

        // 轮询 introspection socket：需要握手成功且预期 service 全部就绪。
        match crate::request_status(&child.socket) {
            Ok(crate::IntrospectionResponse::Status { status, .. }) => {
                if expected_services_ready(&child.expected_services, &status.services) {
                    return Ok(());
                }
                // 有 service 但未全部就绪，继续等待。
                std::thread::sleep(config.poll_interval);
            }
            _ => std::thread::sleep(config.poll_interval),
        }
    }
}

/// 对进程列表做拓扑排序（BFS / Kahn 算法）。
///
/// 返回排序后的进程名列表；如果存在环或引用未声明的进程则返回错误。
pub fn resolve_dependency_order(processes: &[LaunchProcess]) -> Result<Vec<String>, String> {
    let all_names: BTreeSet<&str> = processes.iter().map(|p| p.name.as_str()).collect();

    // 校验依赖引用
    for process in processes {
        for dep in &process.depends_on {
            if !all_names.contains(dep.as_str()) {
                return Err(format!(
                    "process `{}` depends on unknown process `{}`",
                    process.name, dep
                ));
            }
            if dep == &process.name {
                return Err(format!("process `{}` depends on itself", process.name));
            }
        }
    }

    // Kahn 算法
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for process in processes {
        in_degree.entry(&process.name).or_insert(0);
        dependents.entry(&process.name).or_default();
        for dep in &process.depends_on {
            *in_degree.entry(&process.name).or_insert(0) += 1;
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(&process.name);
        }
    }

    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| *name)
        .collect();
    let mut sorted = Vec::new();

    while let Some(name) = queue.pop_front() {
        sorted.push(name.to_string());
        if let Some(deps) = dependents.get(name) {
            for &dep in deps {
                let deg = in_degree
                    .get_mut(dep)
                    .expect("dependents entry must exist in in_degree map");
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(dep);
                }
            }
        }
    }

    if sorted.len() != processes.len() {
        return Err("FlowRT process dependencies contain a cycle".to_string());
    }

    Ok(sorted)
}

// ---------------------------------------------------------------------------
// 可执行文件解析
// ---------------------------------------------------------------------------

/// 根据 runtime_kind 解析进程可执行文件路径。
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

fn binary_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

// ---------------------------------------------------------------------------
// 进程启动
// ---------------------------------------------------------------------------

/// 构造子进程 Command。
///
/// - 非 ros2_bridge runtime 进程传 `--process <name>`
/// - ros2_bridge runtime 进程设置 `RMW_IMPLEMENTATION=rmw_zenoh_cpp`
/// - 注入 zenoh 环境变量
pub fn build_process_command(
    app_exe: &Path,
    process_name: &str,
    runtime_kind: &str,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
) -> Command {
    let mut command = Command::new(app_exe);
    if runtime_kind == "ros2_bridge" {
        command.env("RMW_IMPLEMENTATION", "rmw_zenoh_cpp");
    } else {
        command.arg("--process").arg(process_name);
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
    command
}

/// 启动子进程。
pub fn spawn_flowrt_process(
    app_exe: &Path,
    process_name: &str,
    runtime_kind: &str,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
) -> Result<Child, String> {
    build_process_command(app_exe, process_name, runtime_kind, run_ticks, zenoh_env)
        .spawn()
        .map_err(|error| format!("failed to start FlowRT process `{process_name}`: {error}"))
}

// ---------------------------------------------------------------------------
// 失败传播
// ---------------------------------------------------------------------------

/// BFS 传播失败：终止所有传递依赖于 `failed_process` 且 failure 策略为 propagate 的进程。
///
/// 返回被终止的进程名列表（不含原始失败进程）。
pub fn collect_propagated_failures(
    children: &[PropagatableChild],
    failed_process: &str,
) -> Vec<String> {
    let mut terminated = Vec::new();
    let mut pending = VecDeque::new();
    pending.push_back(failed_process.to_string());

    while let Some(failed) = pending.pop_front() {
        for child in children {
            if child.finished {
                continue;
            }
            if child.dependencies.iter().any(|dep| dep == &failed) {
                terminated.push(child.name.clone());
                if child.failure == "propagate" {
                    pending.push_back(child.name.clone());
                }
            }
        }
    }

    terminated
}

/// 用于失败传播计算的子进程快照。
#[derive(Debug, Clone)]
pub struct PropagatableChild {
    pub name: String,
    pub dependencies: Vec<String>,
    pub failure: String,
    pub finished: bool,
}

// ---------------------------------------------------------------------------
// SupervisorConfig
// ---------------------------------------------------------------------------

/// 生成物传入的 supervisor 配置。
pub struct SupervisorConfig {
    /// 嵌入的 launch manifest JSON。
    pub manifest_json: &'static str,
    /// Rust 应用二进制名 stem（不含平台后缀）。
    pub rust_app_stem: &'static str,
    /// C++ 应用二进制名 stem。
    pub cpp_app_stem: &'static str,
    /// ROS2 bridge 二进制名 stem。
    pub ros2_bridge_stem: &'static str,
    /// 包名。
    pub package_name: &'static str,
    /// self-description hash（由生成物的 selfdesc 模块提供）。
    pub self_description_hash: fn() -> &'static str,
}

// ---------------------------------------------------------------------------
// SupervisedChild（运行时内部状态）
// ---------------------------------------------------------------------------

struct SupervisedChild {
    name: String,
    runtime_kind: String,
    app_exe: PathBuf,
    zenoh_env: Option<ZenohLaunchEnv>,
    dependencies: Vec<String>,
    restart_policy: RestartPolicy,
    failure: String,
    readiness: ReadinessGate,
    expected_services: Vec<String>,
    startup_delay_ms: u64,
    resource_placement: ResourcePlacement,
    resource_placement_status: ResourcePlacementStatus,
    child: Child,
    socket: PathBuf,
    finished: bool,
    restart_count: u32,
    next_restart_unix_ms: Option<u64>,
    last_seen_unix_ms: Option<u64>,
    last_tick_count: Option<u64>,
    last_tick_changed_unix_ms: u64,
    state: String,
    exit_code: Option<i32>,
}

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// Supervisor 主入口。
///
/// 解析 manifest，按依赖顺序启动进程，执行监控循环。
pub fn launch(config: &SupervisorConfig, run_ticks: Option<usize>) -> Result<(), String> {
    let manifest: LaunchManifest = serde_json::from_str(config.manifest_json)
        .map_err(|error| format!("failed to parse FlowRT launch manifest: {error}"))?;
    if manifest.graphs.is_empty() {
        return Err("FlowRT launch manifest does not contain a graph".to_string());
    }

    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;

    let supervisor_state = IntrospectionState::new();
    let _supervisor_status_server = crate::spawn_status_server(
        IntrospectionIdentity {
            self_description_hash: (config.self_description_hash)().to_string(),
            package: config.package_name.to_string(),
            process: "flowrt_supervisor".to_string(),
            runtime: "supervisor".to_string(),
        },
        supervisor_state.clone(),
    )
    .ok();

    let mut children = Vec::new();
    for graph in &manifest.graphs {
        let zenoh_env = if should_auto_configure_zenoh() {
            let refs: Vec<&LaunchProcess> = graph.processes.iter().collect();
            zenoh_launch_env_for_graph(&refs)?
        } else {
            HashMap::new()
        };
        let mut pending = graph.processes.iter().collect::<Vec<_>>();
        // ready_names 包含已通过 readiness gate 的进程名。
        // spawned_names 包含已启动（PID 存在）的进程名，用于依赖排序。
        let mut spawned_names = BTreeSet::new();
        let mut ready_names = BTreeSet::new();
        while !pending.is_empty() {
            let Some(index) = pending.iter().position(|process| {
                process_dependencies_satisfied(process, &spawned_names, &ready_names)
            }) else {
                return Err(
                    "FlowRT process dependencies contain a cycle or unknown process".to_string(),
                );
            };
            let process = pending.remove(index);
            let expected_services = expected_services_for_process(graph, process);
            let app_exe = app_executable_for_runtime(
                &current_exe,
                &process.runtime_kind,
                config.rust_app_stem,
                config.cpp_app_stem,
                config.ros2_bridge_stem,
            )?;
            let process_zenoh_env = zenoh_env.get(&process.name).cloned();
            let child = spawn_flowrt_process(
                &app_exe,
                &process.name,
                &process.runtime_kind,
                run_ticks,
                process_zenoh_env.as_ref(),
            )?;
            let resource_applied =
                apply_resource_placement_to_pid(&process.resource_placement, Some(child.id()));
            let socket = crate::runtime_socket_path_for_pid(child.id());
            let mut child = SupervisedChild {
                name: process.name.clone(),
                runtime_kind: process.runtime_kind.clone(),
                app_exe,
                zenoh_env: process_zenoh_env,
                dependencies: process.depends_on.clone(),
                restart_policy: process.restart,
                failure: process.failure.clone(),
                readiness: process.readiness,
                expected_services,
                startup_delay_ms: process.startup_delay_ms,
                resource_placement: process.resource_placement.clone(),
                resource_placement_status: ResourcePlacementStatus {
                    desired: process.resource_placement.clone(),
                    applied: resource_applied,
                },
                child,
                socket,
                finished: false,
                restart_count: 0,
                next_restart_unix_ms: None,
                last_seen_unix_ms: None,
                last_tick_count: None,
                last_tick_changed_unix_ms: unix_time_ms(),
                state: "starting".to_string(),
                exit_code: None,
            };
            record_child_health(&supervisor_state, &child, false);
            spawned_names.insert(process.name.clone());

            // 等待 readiness gate，然后再启动依赖此进程的后续进程。
            wait_for_readiness(&supervisor_state, &mut child, &ReadinessConfig::default())?;
            ready_names.insert(process.name.clone());

            // readiness 通过后执行错峰启动延迟。
            if child.startup_delay_ms > 0 {
                child.state = "delaying".to_string();
                record_child_health(&supervisor_state, &child, false);
                std::thread::sleep(Duration::from_millis(child.startup_delay_ms));
            }

            child.state = "running".to_string();
            record_child_health(&supervisor_state, &child, false);
            children.push(child);
        }
    }
    if children.is_empty() {
        return Err("FlowRT launch manifest does not contain process groups".to_string());
    }

    supervise_children(&supervisor_state, &mut children, run_ticks)?;

    let mut failures = Vec::new();
    for child in children {
        if child.state == "failed" {
            failures.push(format!(
                "{} exited with code {}",
                child.name,
                child
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

// ---------------------------------------------------------------------------
// 监控循环
// ---------------------------------------------------------------------------

fn supervise_children(
    supervisor_state: &IntrospectionState,
    children: &mut [SupervisedChild],
    run_ticks: Option<usize>,
) -> Result<(), String> {
    while children.iter().any(|child| !child.finished) {
        let mut failed_to_propagate = Vec::new();
        for child in children.iter_mut() {
            if child.finished {
                continue;
            }
            if let Some(next_restart_unix_ms) = child.next_restart_unix_ms {
                if unix_time_ms() >= next_restart_unix_ms {
                    restart_child(supervisor_state, child, run_ticks)?;
                } else {
                    record_child_health(supervisor_state, child, false);
                }
                continue;
            }
            if let Some(status) = child.child.try_wait().map_err(|error| {
                format!("failed to poll FlowRT process `{}`: {error}", child.name)
            })? {
                child.exit_code = status.code();
                if child
                    .restart_policy
                    .can_restart(status.success(), child.restart_count)
                {
                    child.state = "restarting".to_string();
                    child.next_restart_unix_ms = Some(
                        unix_time_ms()
                            .saturating_add(child.restart_policy.delay_ms_for(child.restart_count)),
                    );
                } else if status.success() {
                    child.finished = true;
                    child.state = "exited".to_string();
                } else {
                    child.finished = true;
                    child.state = "failed".to_string();
                    if child.failure == "propagate" {
                        failed_to_propagate.push(child.name.clone());
                    }
                }
                record_child_health(supervisor_state, child, false);
                continue;
            }
            refresh_child_health(supervisor_state, child);
        }
        for failed_process in failed_to_propagate {
            propagate_process_failure(supervisor_state, children, &failed_process);
        }
        std::thread::sleep(HEALTH_POLL_INTERVAL);
    }
    Ok(())
}

fn restart_child(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    run_ticks: Option<usize>,
) -> Result<(), String> {
    let restarted = spawn_flowrt_process(
        &child.app_exe,
        &child.name,
        &child.runtime_kind,
        run_ticks,
        child.zenoh_env.as_ref(),
    )?;
    let resource_applied =
        apply_resource_placement_to_pid(&child.resource_placement, Some(restarted.id()));
    child.child = restarted;
    child.socket = crate::runtime_socket_path_for_pid(child.child.id());
    child.restart_count = child.restart_count.saturating_add(1);
    child.next_restart_unix_ms = None;
    child.last_seen_unix_ms = None;
    child.last_tick_count = None;
    child.last_tick_changed_unix_ms = unix_time_ms();
    child.exit_code = None;
    child.state = "starting".to_string();
    child.resource_placement_status = ResourcePlacementStatus {
        desired: child.resource_placement.clone(),
        applied: resource_applied,
    };
    record_child_health(supervisor_state, child, false);
    Ok(())
}

fn apply_resource_placement_to_pid(
    placement: &ResourcePlacement,
    pid: Option<u32>,
) -> ResourceApplied {
    if resource_placement::has_placement(placement) {
        resource_placement::apply_to_pid(placement, pid)
    } else {
        ResourceApplied::default()
    }
}

fn expected_services_for_process(graph: &LaunchGraph, process: &LaunchProcess) -> Vec<String> {
    let instances = process
        .instances
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    graph
        .services
        .iter()
        .filter(|service| instances.contains(service.server_instance.as_str()))
        .map(|service| service.name.clone())
        .collect()
}

fn expected_services_ready(
    expected_services: &[String],
    live_services: &[crate::IntrospectionServiceStatus],
) -> bool {
    if expected_services.is_empty() {
        return true;
    }
    expected_services.iter().all(|expected| {
        live_services
            .iter()
            .any(|service| service.name == *expected && service.ready)
    })
}

fn propagate_process_failure(
    supervisor_state: &IntrospectionState,
    children: &mut [SupervisedChild],
    failed_process: &str,
) {
    let mut pending = vec![failed_process.to_string()];
    while let Some(failed) = pending.pop() {
        for child in children.iter_mut() {
            if child.finished
                || !child
                    .dependencies
                    .iter()
                    .any(|dependency| dependency == &failed)
            {
                continue;
            }
            let _ = child.child.kill();
            let _ = child.child.wait();
            child.finished = true;
            child.state = "failed".to_string();
            child.exit_code = None;
            record_child_health(supervisor_state, child, false);
            if child.failure == "propagate" {
                pending.push(child.name.clone());
            }
        }
    }
}

fn refresh_child_health(supervisor_state: &IntrospectionState, child: &mut SupervisedChild) {
    let now = unix_time_ms();
    match crate::request_status(&child.socket) {
        Ok(crate::IntrospectionResponse::Status { status, .. }) => {
            child.last_seen_unix_ms = Some(now);
            if child.last_tick_count != Some(status.tick_count) {
                child.last_tick_count = Some(status.tick_count);
                child.last_tick_changed_unix_ms = now;
            }
            let tick_stale =
                now.saturating_sub(child.last_tick_changed_unix_ms) > TICK_STALE_AFTER_MS;
            child.state = if tick_stale {
                "stale".to_string()
            } else {
                "running".to_string()
            };
            record_child_health(supervisor_state, child, tick_stale);
        }
        _ => {
            let tick_stale =
                now.saturating_sub(child.last_tick_changed_unix_ms) > TICK_STALE_AFTER_MS;
            if tick_stale {
                child.state = "stale".to_string();
            }
            record_child_health(supervisor_state, child, tick_stale);
        }
    }
}

fn record_child_health(
    supervisor_state: &IntrospectionState,
    child: &SupervisedChild,
    tick_stale: bool,
) {
    let readiness_wait = match child.state.as_str() {
        "waiting_readiness" => Some(readiness_gate_label(child.readiness)),
        _ => None,
    };
    let resource_placement = if resource_placement::has_placement(&child.resource_placement) {
        Some(child.resource_placement_status.clone())
    } else {
        None
    };
    supervisor_state.record_process_health(IntrospectionProcessStatus {
        name: child.name.clone(),
        state: child.state.clone(),
        pid: Some(child.child.id()),
        restart_count: child.restart_count,
        tick_count: child.last_tick_count,
        last_seen_unix_ms: child.last_seen_unix_ms,
        tick_stale,
        exit_code: child.exit_code,
        readiness_wait: readiness_wait.map(|s| s.to_string()),
        resource_placement,
    });
}

/// 返回 readiness gate 的可读标签。
fn readiness_gate_label(gate: ReadinessGate) -> &'static str {
    match gate {
        ReadinessGate::ProcessStarted => "process_started",
        ReadinessGate::RuntimeReady => "runtime_ready",
        ReadinessGate::ServiceReady => "service_ready",
    }
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    static ZENOH_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct EnvOverride {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvOverride {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: 测试调用方必须持有对应环境变量 mutex。
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvOverride {
        fn drop(&mut self) {
            // SAFETY: 测试调用方必须持有对应环境变量 mutex。
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    fn command_args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn command_env(command: &Command, key: &str) -> Option<String> {
        command
            .get_envs()
            .find(|(env_key, _)| env_key.to_string_lossy() == key)
            .and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
    }

    fn test_process(name: &str, depends_on: Vec<String>) -> LaunchProcess {
        LaunchProcess {
            name: name.into(),
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            depends_on,
            restart: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
            readiness: ReadinessGate::ProcessStarted,
            startup_delay_ms: 0,
            instances: vec![],
            resource_placement: ResourcePlacement::default(),
        }
    }

    fn temp_test_dir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "flowrt-supervisor-{name}-{}-{}",
            std::process::id(),
            unix_time_ms()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("test temp dir should be created");
        root
    }

    // -- RestartPolicy 测试 --

    #[test]
    fn restart_policy_never_never_restarts() {
        let policy = RestartPolicy {
            policy: RestartPolicyKind::Never,
            max_restarts: 5,
            initial_delay_ms: 100,
            max_delay_ms: 1000,
        };
        assert!(!policy.can_restart(false, 0));
        assert!(!policy.can_restart(true, 0));
    }

    #[test]
    fn restart_policy_on_failure_restarts_on_failure_only() {
        let policy = DEFAULT_RESTART_POLICY;
        assert!(policy.can_restart(false, 0));
        assert!(policy.can_restart(false, 2));
        assert!(!policy.can_restart(false, 3)); // max_restarts = 3
        assert!(!policy.can_restart(true, 0));
    }

    #[test]
    fn restart_policy_always_restarts_even_on_success() {
        let policy = RestartPolicy {
            policy: RestartPolicyKind::Always,
            max_restarts: 2,
            initial_delay_ms: 100,
            max_delay_ms: 1000,
        };
        assert!(policy.can_restart(true, 0));
        assert!(policy.can_restart(false, 1));
        assert!(!policy.can_restart(true, 2));
    }

    #[test]
    fn restart_delay_exponential_backoff() {
        let policy = RestartPolicy {
            policy: RestartPolicyKind::OnFailure,
            max_restarts: 10,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
        };
        assert_eq!(policy.delay_ms_for(0), 100); // 100 * 1
        assert_eq!(policy.delay_ms_for(1), 200); // 100 * 2
        assert_eq!(policy.delay_ms_for(2), 400); // 100 * 4
        assert_eq!(policy.delay_ms_for(3), 800); // 100 * 8
        assert_eq!(policy.delay_ms_for(4), 1600); // 100 * 16
        assert_eq!(policy.delay_ms_for(5), 3200); // 100 * 32
        assert_eq!(policy.delay_ms_for(6), 5000); // capped at max_delay_ms
    }

    #[test]
    fn restart_delay_saturates_at_max() {
        let policy = RestartPolicy {
            policy: RestartPolicyKind::OnFailure,
            max_restarts: 100,
            initial_delay_ms: 100,
            max_delay_ms: 1000,
        };
        // 100 * 2^63 would overflow, but saturating_mul handles it
        assert_eq!(policy.delay_ms_for(63), 1000);
        assert_eq!(policy.delay_ms_for(100), 1000);
    }

    #[test]
    fn process_command_uses_runtime_kind_not_process_name_for_ros2_bridge() {
        let command = build_process_command(
            Path::new("/tmp/flowrt_app"),
            "ros2_bridge",
            "rust",
            Some(5),
            None,
        );

        assert_eq!(
            command_args(&command),
            vec!["--process", "ros2_bridge", "--flowrt-run-steps", "5"]
        );
        assert_eq!(command_env(&command, "RMW_IMPLEMENTATION"), None);
    }

    #[test]
    fn ros2_bridge_runtime_command_sets_rmw_without_process_arg() {
        let command = build_process_command(
            Path::new("/tmp/flowrt_ros2_bridge"),
            "adapter",
            "ros2_bridge",
            Some(7),
            None,
        );

        assert_eq!(command_args(&command), vec!["--flowrt-run-steps", "7"]);
        assert_eq!(
            command_env(&command, "RMW_IMPLEMENTATION").as_deref(),
            Some("rmw_zenoh_cpp")
        );
    }

    #[test]
    fn cpp_runtime_executable_prefers_sibling_local_bin() {
        let root = temp_test_dir("cpp-sibling");
        let bin_dir = root.join("flowrt/build/bin/release");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let supervisor = bin_dir.join(binary_name("robot-flowrt-supervisor"));
        let cpp_app = bin_dir.join(binary_name("robot_cpp_app"));
        std::fs::write(&supervisor, "").unwrap();
        std::fs::write(&cpp_app, "").unwrap();

        let resolved =
            app_executable_for_runtime(&supervisor, "cpp", "robot-flowrt-app", "robot_cpp_app", "")
                .unwrap();

        assert_eq!(resolved, cpp_app);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ros2_bridge_runtime_executable_prefers_sibling_local_bin() {
        let root = temp_test_dir("ros2-sibling");
        let bin_dir = root.join("flowrt/build/bin/release");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let supervisor = bin_dir.join(binary_name("robot-flowrt-supervisor"));
        let bridge = bin_dir.join(binary_name("robot_ros2_bridge"));
        std::fs::write(&supervisor, "").unwrap();
        std::fs::write(&bridge, "").unwrap();

        let resolved = app_executable_for_runtime(
            &supervisor,
            "ros2_bridge",
            "robot-flowrt-app",
            "",
            "robot_ros2_bridge",
        )
        .unwrap();

        assert_eq!(resolved, bridge);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cpp_runtime_executable_keeps_legacy_cmake_fallback() {
        let supervisor = Path::new("/tmp/app/flowrt/build/bin/release/robot-flowrt-supervisor");

        let resolved =
            app_executable_for_runtime(supervisor, "cpp", "robot-flowrt-app", "robot_cpp_app", "")
                .unwrap();

        assert_eq!(
            resolved,
            Path::new("/tmp/app/flowrt/build/cmake").join(binary_name("robot_cpp_app"))
        );
    }

    // -- 依赖排序测试 --

    #[test]
    fn resolve_dependency_order_no_deps() {
        let processes = vec![test_process("a", vec![]), test_process("b", vec![])];
        let order = resolve_dependency_order(&processes).unwrap();
        assert_eq!(order.len(), 2);
        assert!(order.contains(&"a".to_string()));
        assert!(order.contains(&"b".to_string()));
    }

    #[test]
    fn resolve_dependency_order_linear_chain() {
        let processes = vec![
            test_process("a", vec![]),
            test_process("b", vec!["a".into()]),
            test_process("c", vec!["b".into()]),
        ];
        let order = resolve_dependency_order(&processes).unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn resolve_dependency_order_diamond() {
        let processes = vec![
            test_process("a", vec![]),
            test_process("b", vec!["a".into()]),
            test_process("c", vec!["a".into()]),
            test_process("d", vec!["b".into(), "c".into()]),
        ];
        let order = resolve_dependency_order(&processes).unwrap();
        let a = order.iter().position(|n| n == "a").unwrap();
        let b = order.iter().position(|n| n == "b").unwrap();
        let c = order.iter().position(|n| n == "c").unwrap();
        let d = order.iter().position(|n| n == "d").unwrap();
        assert!(a < b);
        assert!(a < c);
        assert!(b < d);
        assert!(c < d);
    }

    #[test]
    fn resolve_dependency_order_cycle_detected() {
        let processes = vec![
            test_process("a", vec!["b".into()]),
            test_process("b", vec!["a".into()]),
        ];
        let err = resolve_dependency_order(&processes).unwrap_err();
        assert!(err.contains("cycle"), "error should mention cycle: {err}");
    }

    #[test]
    fn resolve_dependency_order_unknown_dep_rejected() {
        let processes = vec![test_process("a", vec!["missing".into()])];
        let err = resolve_dependency_order(&processes).unwrap_err();
        assert!(err.contains("unknown process"));
    }

    #[test]
    fn resolve_dependency_order_self_dep_rejected() {
        let processes = vec![test_process("a", vec!["a".into()])];
        let err = resolve_dependency_order(&processes).unwrap_err();
        assert!(err.contains("depends on itself"));
    }

    // -- 失败传播测试 --

    #[test]
    fn propagate_failure_terminates_dependents() {
        let children = vec![
            PropagatableChild {
                name: "a".into(),
                dependencies: vec![],
                failure: "propagate".into(),
                finished: false,
            },
            PropagatableChild {
                name: "b".into(),
                dependencies: vec!["a".into()],
                failure: "propagate".into(),
                finished: false,
            },
            PropagatableChild {
                name: "c".into(),
                dependencies: vec!["b".into()],
                failure: "propagate".into(),
                finished: false,
            },
        ];
        let terminated = collect_propagated_failures(&children, "a");
        assert!(terminated.contains(&"b".to_string()));
        assert!(terminated.contains(&"c".to_string()));
    }

    #[test]
    fn propagate_failure_stops_at_isolate() {
        let children = vec![
            PropagatableChild {
                name: "a".into(),
                dependencies: vec![],
                failure: "propagate".into(),
                finished: false,
            },
            PropagatableChild {
                name: "b".into(),
                dependencies: vec!["a".into()],
                failure: "isolate".into(),
                finished: false,
            },
            PropagatableChild {
                name: "c".into(),
                dependencies: vec!["b".into()],
                failure: "propagate".into(),
                finished: false,
            },
        ];
        let terminated = collect_propagated_failures(&children, "a");
        assert!(terminated.contains(&"b".to_string()));
        // c depends on b, but b is isolate so propagation stops
        assert!(!terminated.contains(&"c".to_string()));
    }

    #[test]
    fn propagate_failure_skips_finished() {
        let children = vec![
            PropagatableChild {
                name: "a".into(),
                dependencies: vec![],
                failure: "propagate".into(),
                finished: false,
            },
            PropagatableChild {
                name: "b".into(),
                dependencies: vec!["a".into()],
                failure: "propagate".into(),
                finished: true, // already finished
            },
            PropagatableChild {
                name: "c".into(),
                dependencies: vec!["b".into()],
                failure: "propagate".into(),
                finished: false,
            },
        ];
        let terminated = collect_propagated_failures(&children, "a");
        // b is already finished, so it's skipped; c depends on b but b wasn't terminated
        assert!(!terminated.contains(&"b".to_string()));
        assert!(!terminated.contains(&"c".to_string()));
    }

    // -- Zenoh 自动配置测试 --

    #[test]
    fn should_auto_configure_zenoh_when_no_env_vars() {
        let _lock = ZENOH_ENV_LOCK.lock().expect("zenoh env lock should work");
        let _mode = EnvOverride::set("FLOWRT_ZENOH_MODE", None);
        let _listen = EnvOverride::set("FLOWRT_ZENOH_LISTEN", None);
        let _connect = EnvOverride::set("FLOWRT_ZENOH_CONNECT", None);

        assert!(should_auto_configure_zenoh());
    }

    #[test]
    fn should_not_auto_configure_when_env_set() {
        let _lock = ZENOH_ENV_LOCK.lock().expect("zenoh env lock should work");
        let _mode = EnvOverride::set("FLOWRT_ZENOH_MODE", Some("peer"));

        assert!(!should_auto_configure_zenoh());
    }

    #[test]
    fn zenoh_launch_env_hub_and_spoke() {
        let mut hub = test_process("hub", vec![]);
        hub.backend = "zenoh".into();
        let mut spoke = test_process("spoke", vec!["hub".into()]);
        spoke.backend = "zenoh".into();
        let inproc_only = test_process("inproc_only", vec![]);
        let processes = [hub, spoke, inproc_only];
        let refs: Vec<&LaunchProcess> = processes.iter().collect();
        let env = zenoh_launch_env_for_graph(&refs).unwrap();

        assert_eq!(env.len(), 2); // only zenoh processes
        let hub_env = env.get("hub").unwrap();
        let spoke_env = env.get("spoke").unwrap();
        assert!(hub_env.listen.starts_with("tcp/127.0.0.1:"));
        assert!(hub_env.connect.is_empty());
        assert!(spoke_env.listen.is_empty());
        assert_eq!(spoke_env.connect, hub_env.listen);
    }

    #[test]
    fn zenoh_launch_env_empty_for_no_zenoh() {
        let processes = [test_process("a", vec![])];
        let refs: Vec<&LaunchProcess> = processes.iter().collect();
        let env = zenoh_launch_env_for_graph(&refs).unwrap();
        assert!(env.is_empty());
    }

    // -- 依赖满足判断测试 --

    #[test]
    fn dependencies_satisfied_when_all_met_process_started() {
        let process = test_process("c", vec!["a".into(), "b".into()]);
        let mut spawned = BTreeSet::new();
        spawned.insert("a".into());
        spawned.insert("b".into());
        let ready = BTreeSet::new();
        // process_started gate：依赖只需已启动。
        assert!(process_dependencies_satisfied(&process, &spawned, &ready));
    }

    #[test]
    fn dependencies_satisfied_when_all_met_runtime_ready() {
        let mut process = test_process("c", vec!["a".into(), "b".into()]);
        process.readiness = ReadinessGate::RuntimeReady;
        let mut spawned = BTreeSet::new();
        spawned.insert("a".into());
        spawned.insert("b".into());
        let mut ready = BTreeSet::new();
        ready.insert("a".into());
        ready.insert("b".into());
        assert!(process_dependencies_satisfied(&process, &spawned, &ready));
    }

    #[test]
    fn dependencies_not_satisfied_for_runtime_ready_when_dep_only_spawned() {
        let mut process = test_process("c", vec!["a".into()]);
        process.readiness = ReadinessGate::RuntimeReady;
        let mut spawned = BTreeSet::new();
        spawned.insert("a".into());
        let ready = BTreeSet::new();
        // runtime_ready gate：依赖必须已通过 readiness，仅 spawned 不够。
        assert!(!process_dependencies_satisfied(&process, &spawned, &ready));
    }

    #[test]
    fn dependencies_not_satisfied_when_missing() {
        let process = test_process("c", vec!["a".into(), "b".into()]);
        let mut spawned = BTreeSet::new();
        spawned.insert("a".into());
        let ready = BTreeSet::new();
        assert!(!process_dependencies_satisfied(&process, &spawned, &ready));
    }

    // -- manifest 反序列化测试 --

    #[test]
    fn manifest_deserialization_with_defaults() {
        let json = r#"{
            "graphs": [{
                "processes": [{
                    "name": "sensor",
                    "backend": "zenoh",
                    "runtime_kind": "rust"
                }]
            }]
        }"#;
        let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
        let process = &manifest.graphs[0].processes[0];
        assert_eq!(process.name, "sensor");
        assert!(process.depends_on.is_empty());
        assert_eq!(process.restart.policy, RestartPolicyKind::OnFailure);
        assert_eq!(process.restart.max_restarts, 3);
        assert_eq!(process.failure, "propagate");
        assert_eq!(process.readiness, ReadinessGate::ProcessStarted);
        assert_eq!(process.startup_delay_ms, 0);
        assert_eq!(process.resource_placement, ResourcePlacement::default());
    }

    #[test]
    fn manifest_deserialization_with_custom_policy() {
        let json = r#"{
            "graphs": [{
                "processes": [{
                    "name": "control",
                    "backend": "iox2",
                    "runtime_kind": "cpp",
                    "depends_on": ["sensor"],
                    "restart": {
                        "policy": "never",
                        "max_restarts": 0,
                        "initial_delay_ms": 0,
                        "max_delay_ms": 0
                    },
                    "failure": "isolate"
                }]
            }]
        }"#;
        let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
        let process = &manifest.graphs[0].processes[0];
        assert_eq!(process.depends_on, vec!["sensor"]);
        assert_eq!(process.restart.policy, RestartPolicyKind::Never);
        assert_eq!(process.failure, "isolate");
        assert_eq!(process.readiness, ReadinessGate::ProcessStarted);
        assert_eq!(process.resource_placement, ResourcePlacement::default());
    }

    #[test]
    fn manifest_deserialization_with_readiness_gate() {
        let json = r#"{
            "graphs": [{
                "processes": [{
                    "name": "control",
                    "backend": "inproc",
                    "runtime_kind": "rust",
                    "readiness": "runtime_ready",
                    "startup_delay_ms": 500
                }]
            }]
        }"#;
        let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
        let process = &manifest.graphs[0].processes[0];
        assert_eq!(process.readiness, ReadinessGate::RuntimeReady);
        assert_eq!(process.startup_delay_ms, 500);
    }

    #[test]
    fn manifest_deserialization_service_ready_gate() {
        let json = r#"{
            "graphs": [{
                "processes": [{
                    "name": "server",
                    "backend": "inproc",
                    "runtime_kind": "rust",
                    "readiness": "service_ready"
                }]
            }]
        }"#;
        let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
        let process = &manifest.graphs[0].processes[0];
        assert_eq!(process.readiness, ReadinessGate::ServiceReady);
    }

    #[test]
    fn manifest_deserialization_with_resource_placement() {
        let json = r#"{
            "graphs": [{
                "processes": [{
                    "name": "control",
                    "backend": "iox2",
                    "runtime_kind": "rust",
                    "resource_placement": {
                        "cpu_affinity": [0, 1],
                        "nice": -10,
                        "rt_policy": "fifo",
                        "rt_priority": 50
                    }
                }]
            }]
        }"#;
        let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
        let process = &manifest.graphs[0].processes[0];
        assert_eq!(process.resource_placement.cpu_affinity, vec![0, 1]);
        assert_eq!(process.resource_placement.nice, Some(-10));
        assert_eq!(
            process.resource_placement.rt_policy,
            Some(resource_placement::RtPolicy::Fifo)
        );
        assert_eq!(process.resource_placement.rt_priority, Some(50));
    }

    #[test]
    fn expected_services_for_process_uses_server_instances() {
        let graph = LaunchGraph {
            services: vec![
                LaunchService {
                    name: "client.plan".to_string(),
                    server_instance: "server".to_string(),
                },
                LaunchService {
                    name: "client.inspect".to_string(),
                    server_instance: "inspector".to_string(),
                },
            ],
            processes: vec![],
        };
        let mut process = test_process("server_proc", vec![]);
        process.instances = vec!["server".to_string()];

        assert_eq!(
            expected_services_for_process(&graph, &process),
            vec!["client.plan".to_string()]
        );
    }

    #[test]
    fn expected_services_ready_requires_each_expected_ready_endpoint() {
        let expected = vec!["client.plan".to_string(), "client.inspect".to_string()];
        let live = vec![
            service_status("client.plan", true),
            service_status("client.inspect", true),
        ];
        assert!(expected_services_ready(&expected, &live));

        let missing = vec![service_status("client.plan", true)];
        assert!(!expected_services_ready(&expected, &missing));

        let not_ready = vec![
            service_status("client.plan", true),
            service_status("client.inspect", false),
        ];
        assert!(!expected_services_ready(&expected, &not_ready));
    }

    #[test]
    fn expected_services_ready_falls_back_to_runtime_ready_when_no_expected_services() {
        assert!(expected_services_ready(&[], &[]));
    }

    // -- readiness timeout 和交互测试 --

    fn short_readiness_config() -> ReadinessConfig {
        ReadinessConfig {
            timeout: Duration::from_millis(200),
            poll_interval: Duration::from_millis(10),
        }
    }

    #[test]
    fn readiness_timeout_marks_state_and_returns_error() {
        // 启动一个不会建立 introspection socket 的子进程（sleep）。
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let real_child = cmd.spawn().unwrap();
        let socket = crate::runtime_socket_path_for_pid(real_child.id());
        let supervisor_state = IntrospectionState::new();

        let mut child = SupervisedChild {
            name: "test_timeout".into(),
            runtime_kind: "rust".into(),
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            restart_policy: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
            readiness: ReadinessGate::RuntimeReady,
            expected_services: vec![],
            startup_delay_ms: 0,
            resource_placement: ResourcePlacement::default(),
            resource_placement_status: ResourcePlacementStatus::default(),
            child: real_child,
            socket,
            finished: false,
            restart_count: 0,
            next_restart_unix_ms: None,
            last_seen_unix_ms: None,
            last_tick_count: None,
            last_tick_changed_unix_ms: unix_time_ms(),
            state: "starting".into(),
            exit_code: None,
        };

        let config = short_readiness_config();
        let result = wait_for_runtime_ready(&supervisor_state, &mut child, &config);

        assert!(result.is_err(), "should timeout");
        let err = result.unwrap_err();
        assert!(
            err.contains("timed out"),
            "error should mention timeout: {err}"
        );
        assert!(
            err.contains("test_timeout"),
            "error should name the process"
        );
        assert!(
            child.finished,
            "child should be marked finished after timeout"
        );
        assert_eq!(child.state, "readiness_timeout");
    }

    #[test]
    fn readiness_service_ready_timeout_marks_state() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let real_child = cmd.spawn().unwrap();
        let socket = crate::runtime_socket_path_for_pid(real_child.id());
        let supervisor_state = IntrospectionState::new();

        let mut child = SupervisedChild {
            name: "test_svc_timeout".into(),
            runtime_kind: "rust".into(),
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            restart_policy: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
            readiness: ReadinessGate::ServiceReady,
            expected_services: vec!["planner.plan".to_string()],
            startup_delay_ms: 0,
            resource_placement: ResourcePlacement::default(),
            resource_placement_status: ResourcePlacementStatus::default(),
            child: real_child,
            socket,
            finished: false,
            restart_count: 0,
            next_restart_unix_ms: None,
            last_seen_unix_ms: None,
            last_tick_count: None,
            last_tick_changed_unix_ms: unix_time_ms(),
            state: "starting".into(),
            exit_code: None,
        };

        let config = short_readiness_config();
        let result = wait_for_service_ready(&supervisor_state, &mut child, &config);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timed out"));
        assert_eq!(child.state, "readiness_timeout");
        assert!(child.finished);
    }

    #[test]
    fn readiness_process_exit_during_wait_marks_failed() {
        // 启动一个立即退出的进程。
        let mut cmd = Command::new("true");
        let real_child = cmd.spawn().unwrap();
        let socket = crate::runtime_socket_path_for_pid(real_child.id());
        let supervisor_state = IntrospectionState::new();

        let mut child = SupervisedChild {
            name: "test_exit".into(),
            runtime_kind: "rust".into(),
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            restart_policy: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
            readiness: ReadinessGate::RuntimeReady,
            expected_services: vec![],
            startup_delay_ms: 0,
            resource_placement: ResourcePlacement::default(),
            resource_placement_status: ResourcePlacementStatus::default(),
            child: real_child,
            socket,
            finished: false,
            restart_count: 0,
            next_restart_unix_ms: None,
            last_seen_unix_ms: None,
            last_tick_count: None,
            last_tick_changed_unix_ms: unix_time_ms(),
            state: "starting".into(),
            exit_code: None,
        };

        let config = short_readiness_config();
        let result = wait_for_runtime_ready(&supervisor_state, &mut child, &config);

        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("exited during readiness wait"),
            "error should mention process exit"
        );
        assert_eq!(child.state, "readiness_failed");
        assert!(child.finished);
        assert_eq!(child.exit_code, Some(0));
    }

    #[test]
    fn readiness_config_defaults_use_production_values() {
        let config = ReadinessConfig::default();
        assert_eq!(config.timeout, READINESS_TIMEOUT);
        assert_eq!(config.poll_interval, READINESS_POLL_INTERVAL);
    }

    #[test]
    fn readiness_wait_process_started_returns_immediately() {
        let supervisor_state = IntrospectionState::new();
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let real_child = cmd.spawn().unwrap();
        let socket = crate::runtime_socket_path_for_pid(real_child.id());

        let mut child = SupervisedChild {
            name: "test_started".into(),
            runtime_kind: "rust".into(),
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            restart_policy: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
            readiness: ReadinessGate::ProcessStarted,
            expected_services: vec![],
            startup_delay_ms: 0,
            resource_placement: ResourcePlacement::default(),
            resource_placement_status: ResourcePlacementStatus::default(),
            child: real_child,
            socket,
            finished: false,
            restart_count: 0,
            next_restart_unix_ms: None,
            last_seen_unix_ms: None,
            last_tick_count: None,
            last_tick_changed_unix_ms: unix_time_ms(),
            state: "starting".into(),
            exit_code: None,
        };

        let config = ReadinessConfig::default();
        let result = wait_for_readiness(&supervisor_state, &mut child, &config);
        assert!(result.is_ok(), "ProcessStarted should return immediately");
        // 子进程不应被 kill，仍在运行。
        assert!(!child.finished);
        // 清理
        let _ = child.child.kill();
        let _ = child.child.wait();
    }

    #[test]
    fn readiness_gate_label_returns_correct_string() {
        assert_eq!(
            readiness_gate_label(ReadinessGate::ProcessStarted),
            "process_started"
        );
        assert_eq!(
            readiness_gate_label(ReadinessGate::RuntimeReady),
            "runtime_ready"
        );
        assert_eq!(
            readiness_gate_label(ReadinessGate::ServiceReady),
            "service_ready"
        );
    }

    #[test]
    fn readiness_record_health_sets_wait_field() {
        let supervisor_state = IntrospectionState::new();
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let real_child = cmd.spawn().unwrap();
        let socket = crate::runtime_socket_path_for_pid(real_child.id());

        let mut child = SupervisedChild {
            name: "test_health".into(),
            runtime_kind: "rust".into(),
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            restart_policy: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
            readiness: ReadinessGate::ServiceReady,
            expected_services: vec!["planner.plan".to_string()],
            startup_delay_ms: 0,
            resource_placement: ResourcePlacement::default(),
            resource_placement_status: ResourcePlacementStatus::default(),
            child: real_child,
            socket,
            finished: false,
            restart_count: 0,
            next_restart_unix_ms: None,
            last_seen_unix_ms: None,
            last_tick_count: None,
            last_tick_changed_unix_ms: unix_time_ms(),
            state: "waiting_readiness".into(),
            exit_code: None,
        };
        record_child_health(&supervisor_state, &child, false);

        let status = supervisor_state.status();
        let proc_status = status
            .processes
            .iter()
            .find(|p| p.name == "test_health")
            .unwrap();
        assert_eq!(
            proc_status.readiness_wait,
            Some("service_ready".to_string())
        );
        assert_eq!(proc_status.state, "waiting_readiness");

        // 清理
        let _ = child.child.kill();
        let _ = child.child.wait();
    }

    fn service_status(name: &str, ready: bool) -> crate::IntrospectionServiceStatus {
        crate::IntrospectionServiceStatus {
            name: name.to_string(),
            ready,
            in_flight: 0,
            queued: 0,
            total_requests: 0,
            timeout_count: 0,
            busy_count: 0,
            unavailable_count: 0,
            late_drop_count: 0,
        }
    }
}
