//! Supervisor engine：进程编排、依赖排序、重启策略和健康监控。
//!
//! 本模块把 supervisor 核心逻辑从 codegen 字符串下沉为可独立测试的 runtime 深模块。
//! 生成物只保留 manifest 常量、binary name stems 和 self-description hash 的薄 glue。

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use serde::Deserialize;

use crate::introspection::{IntrospectionIdentity, IntrospectionProcessStatus, IntrospectionState};

/// 子进程健康轮询间隔。
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// tick 无变化超过此阈值则标记 stale。
const TICK_STALE_AFTER_MS: u64 = 1_000;

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
    pub processes: Vec<LaunchProcess>,
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
pub fn process_dependencies_satisfied(
    process: &LaunchProcess,
    spawned_names: &BTreeSet<String>,
) -> bool {
    process
        .depends_on
        .iter()
        .all(|dependency| spawned_names.contains(dependency))
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
                let deg = in_degree.get_mut(dep).unwrap();
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

fn ros2_bridge_executable(current_exe: &Path, stem: &str) -> Result<PathBuf, String> {
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
        let mut spawned_names = BTreeSet::new();
        while !pending.is_empty() {
            let Some(index) = pending
                .iter()
                .position(|process| process_dependencies_satisfied(process, &spawned_names))
            else {
                return Err(
                    "FlowRT process dependencies contain a cycle or unknown process".to_string(),
                );
            };
            let process = pending.remove(index);
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
            let socket = crate::runtime_socket_path_for_pid(child.id());
            let child = SupervisedChild {
                name: process.name.clone(),
                runtime_kind: process.runtime_kind.clone(),
                app_exe,
                zenoh_env: process_zenoh_env,
                dependencies: process.depends_on.clone(),
                restart_policy: process.restart,
                failure: process.failure.clone(),
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
    child.child = restarted;
    child.socket = crate::runtime_socket_path_for_pid(child.child.id());
    child.restart_count = child.restart_count.saturating_add(1);
    child.next_restart_unix_ms = None;
    child.last_seen_unix_ms = None;
    child.last_tick_count = None;
    child.last_tick_changed_unix_ms = unix_time_ms();
    child.exit_code = None;
    child.state = "starting".to_string();
    record_child_health(supervisor_state, child, false);
    Ok(())
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
    supervisor_state.record_process_health(IntrospectionProcessStatus {
        name: child.name.clone(),
        state: child.state.clone(),
        pid: Some(child.child.id()),
        restart_count: child.restart_count,
        tick_count: child.last_tick_count,
        last_seen_unix_ms: child.last_seen_unix_ms,
        tick_stale,
        exit_code: child.exit_code,
    });
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
    use std::path::Path;

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

    // -- 依赖排序测试 --

    #[test]
    fn resolve_dependency_order_no_deps() {
        let processes = vec![
            LaunchProcess {
                name: "a".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec![],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
            LaunchProcess {
                name: "b".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec![],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
        ];
        let order = resolve_dependency_order(&processes).unwrap();
        assert_eq!(order.len(), 2);
        assert!(order.contains(&"a".to_string()));
        assert!(order.contains(&"b".to_string()));
    }

    #[test]
    fn resolve_dependency_order_linear_chain() {
        let processes = vec![
            LaunchProcess {
                name: "a".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec![],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
            LaunchProcess {
                name: "b".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec!["a".into()],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
            LaunchProcess {
                name: "c".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec!["b".into()],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
        ];
        let order = resolve_dependency_order(&processes).unwrap();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn resolve_dependency_order_diamond() {
        let processes = vec![
            LaunchProcess {
                name: "a".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec![],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
            LaunchProcess {
                name: "b".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec!["a".into()],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
            LaunchProcess {
                name: "c".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec!["a".into()],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
            LaunchProcess {
                name: "d".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec!["b".into(), "c".into()],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
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
            LaunchProcess {
                name: "a".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec!["b".into()],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
            LaunchProcess {
                name: "b".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec!["a".into()],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
        ];
        let err = resolve_dependency_order(&processes).unwrap_err();
        assert!(err.contains("cycle"), "error should mention cycle: {err}");
    }

    #[test]
    fn resolve_dependency_order_unknown_dep_rejected() {
        let processes = vec![LaunchProcess {
            name: "a".into(),
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            depends_on: vec!["missing".into()],
            restart: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
        }];
        let err = resolve_dependency_order(&processes).unwrap_err();
        assert!(err.contains("unknown process"));
    }

    #[test]
    fn resolve_dependency_order_self_dep_rejected() {
        let processes = vec![LaunchProcess {
            name: "a".into(),
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            depends_on: vec!["a".into()],
            restart: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
        }];
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
        // 清理环境变量以确保测试干净
        unsafe {
            std::env::remove_var("FLOWRT_ZENOH_MODE");
            std::env::remove_var("FLOWRT_ZENOH_LISTEN");
            std::env::remove_var("FLOWRT_ZENOH_CONNECT");
        }
        assert!(should_auto_configure_zenoh());
    }

    #[test]
    fn should_not_auto_configure_when_env_set() {
        unsafe {
            std::env::set_var("FLOWRT_ZENOH_MODE", "peer");
        }
        assert!(!should_auto_configure_zenoh());
        unsafe {
            std::env::remove_var("FLOWRT_ZENOH_MODE");
        }
    }

    #[test]
    fn zenoh_launch_env_hub_and_spoke() {
        let processes = [
            LaunchProcess {
                name: "hub".into(),
                backend: "zenoh".into(),
                runtime_kind: "rust".into(),
                depends_on: vec![],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
            LaunchProcess {
                name: "spoke".into(),
                backend: "zenoh".into(),
                runtime_kind: "rust".into(),
                depends_on: vec!["hub".into()],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
            LaunchProcess {
                name: "inproc_only".into(),
                backend: "inproc".into(),
                runtime_kind: "rust".into(),
                depends_on: vec![],
                restart: DEFAULT_RESTART_POLICY,
                failure: "propagate".into(),
            },
        ];
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
        let processes = [LaunchProcess {
            name: "a".into(),
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            depends_on: vec![],
            restart: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
        }];
        let refs: Vec<&LaunchProcess> = processes.iter().collect();
        let env = zenoh_launch_env_for_graph(&refs).unwrap();
        assert!(env.is_empty());
    }

    // -- 依赖满足判断测试 --

    #[test]
    fn dependencies_satisfied_when_all_met() {
        let process = LaunchProcess {
            name: "c".into(),
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            depends_on: vec!["a".into(), "b".into()],
            restart: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
        };
        let mut spawned = BTreeSet::new();
        spawned.insert("a".into());
        spawned.insert("b".into());
        assert!(process_dependencies_satisfied(&process, &spawned));
    }

    #[test]
    fn dependencies_not_satisfied_when_missing() {
        let process = LaunchProcess {
            name: "c".into(),
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            depends_on: vec!["a".into(), "b".into()],
            restart: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
        };
        let mut spawned = BTreeSet::new();
        spawned.insert("a".into());
        assert!(!process_dependencies_satisfied(&process, &spawned));
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
    }
}
