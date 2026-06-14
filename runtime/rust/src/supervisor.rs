//! Supervisor engine：进程编排、依赖排序、重启策略和健康监控。
//!
//! 本模块把 supervisor 核心逻辑从 codegen 字符串下沉为可独立测试的 runtime 深模块。
//! 生成物只保留 manifest 常量、binary name stems 和 self-description hash 的薄 glue。

pub mod resource_placement;

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::introspection::{IntrospectionIdentity, IntrospectionProcessStatus, IntrospectionState};
use crate::shutdown::ShutdownToken;

use self::resource_placement::{ResourceApplied, ResourcePlacement, ResourcePlacementStatus};

/// 子进程健康轮询间隔。
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// tick 无变化超过此阈值则标记 stale。
const TICK_STALE_AFTER_MS: u64 = 1_000;
/// readiness 等待轮询间隔。
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// readiness 等待超时时间。
const READINESS_TIMEOUT: Duration = Duration::from_secs(30);
/// supervisor 主动终止子进程时等待其自行退出的宽限时间。
const CHILD_TERMINATE_GRACE: Duration = Duration::from_millis(500);
/// 主动终止子进程时的轮询间隔。
const CHILD_TERMINATE_POLL_INTERVAL: Duration = Duration::from_millis(20);
/// 本机 zenoh 自动 mesh 使用 IANA dynamic/private port range。
const ZENOH_AUTO_PORT_BASE: u16 = 49_152;
const ZENOH_AUTO_PORT_COUNT: u16 = 16_384;
const SUPPORTED_LAUNCH_IR_VERSION: &str = "0.1";
const SUPPORTED_RESOURCE_CONTRACT_VERSION: &str = "0.1";

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
#[serde(deny_unknown_fields)]
pub struct LaunchManifest {
    pub package: String,
    pub ir_version: String,
    #[serde(default)]
    pub artifact: LaunchArtifact,
    pub profiles: Vec<String>,
    #[serde(default)]
    pub profile_modes: Vec<LaunchProfileMode>,
    pub targets: Vec<String>,
    pub graphs: Vec<LaunchGraph>,
}

/// manifest 中当前生成物的安全模式摘要。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchArtifact {
    #[serde(default = "default_graph_mode")]
    pub mode: String,
    #[serde(default)]
    pub temporary_island: bool,
    #[serde(default)]
    pub test_only: bool,
}

impl Default for LaunchArtifact {
    fn default() -> Self {
        Self {
            mode: default_graph_mode(),
            temporary_island: false,
            test_only: false,
        }
    }
}

/// manifest 中 profile 的 graph 完整性模式摘要。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchProfileMode {
    pub name: String,
    pub mode: String,
}

/// manifest 中的 graph 节点。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchGraph {
    pub name: String,
    #[serde(default = "default_graph_mode")]
    pub mode: String,
    #[serde(default)]
    pub scheduler: serde_json::Value,
    #[serde(default)]
    pub resource_contract: LaunchResourceContract,
    #[serde(default)]
    pub channels: Vec<serde_json::Value>,
    #[serde(default)]
    pub boundary_endpoints: Vec<LaunchBoundaryEndpoint>,
    #[serde(default)]
    pub services: Vec<LaunchService>,
    #[serde(default)]
    pub ros2_bridges: Vec<serde_json::Value>,
    #[serde(default)]
    pub instances: Vec<serde_json::Value>,
    #[serde(default)]
    pub tasks: Vec<serde_json::Value>,
    pub processes: Vec<LaunchProcess>,
}

fn default_graph_mode() -> String {
    "strict".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchResourceContract {
    #[serde(default = "default_resource_contract_version")]
    pub resource_contract_version: String,
    #[serde(default)]
    pub requirements: Vec<serde_json::Value>,
    #[serde(default)]
    pub providers: Vec<serde_json::Value>,
    #[serde(default)]
    pub satisfactions: Vec<serde_json::Value>,
}

impl Default for LaunchResourceContract {
    fn default() -> Self {
        Self {
            resource_contract_version: default_resource_contract_version(),
            requirements: Vec::new(),
            providers: Vec::new(),
            satisfactions: Vec::new(),
        }
    }
}

fn default_resource_contract_version() -> String {
    SUPPORTED_RESOURCE_CONTRACT_VERSION.to_string()
}

/// manifest 中的 island boundary endpoint 静态摘要。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchBoundaryEndpoint {
    pub name: String,
    #[serde(default)]
    pub canonical_id: String,
    pub direction: String,
    pub endpoint: String,
    pub instance: String,
    pub port: String,
    pub message_type: String,
}

/// manifest 中单个 service bind 描述。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchService {
    pub name: String,
    pub client: String,
    pub client_instance: String,
    pub client_port: String,
    pub server: String,
    pub server_instance: String,
    pub server_port: String,
    pub request: String,
    pub response: String,
    pub backend: String,
    pub timeout_ms: u64,
    pub queue_depth: u32,
    pub overflow: String,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub max_in_flight: u32,
}

/// manifest 中单个进程描述。
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchProcess {
    pub name: String,
    pub backend: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub runtimes: Vec<String>,
    pub runtime_kind: String,
    /// external process package/executable 元数据。
    #[serde(default)]
    pub external: Option<LaunchExternalProcess>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// 用户在 process orchestration 中声明的环境变量。
    #[serde(default)]
    pub env: BTreeMap<String, String>,
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
    #[serde(default)]
    pub tasks: Vec<serde_json::Value>,
    /// 进程资源提示（CPU affinity、nice、RT policy）。
    #[serde(default)]
    pub resource_placement: ResourcePlacement,
    /// 进程内 I/O boundary 静态摘要；当前 supervisor 只接收并透传到后续健康路径。
    #[serde(default)]
    pub io_boundaries: Vec<LaunchIoBoundary>,
}

/// manifest 中进程内 I/O boundary 描述。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchIoBoundary {
    pub instance: String,
    pub component: String,
    #[serde(default)]
    pub side_effects: Vec<String>,
    pub readiness: String,
    pub health: String,
    pub shutdown: String,
    #[serde(default)]
    pub resources: Vec<LaunchIoResource>,
}

/// manifest 中 I/O boundary 资源需求描述。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchIoResource {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub descriptor: Option<LaunchResourceDescriptor>,
}

/// manifest 中 I/O boundary 资源的 descriptor schema。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchResourceDescriptor {
    pub kind: String,
    pub port: String,
    pub format: String,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub record_payload: bool,
}

/// manifest 中 external process package/executable 描述。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchExternalProcess {
    pub package: String,
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_dir: LaunchExternalWorkingDir,
    #[serde(default)]
    pub health: LaunchExternalHealth,
    #[serde(default)]
    pub required_backends: Vec<String>,
    #[serde(default)]
    pub package_root: Option<PathBuf>,
}

/// external process 工作目录策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchExternalWorkingDir {
    #[default]
    Package,
    Workspace,
}

/// external process 健康检查策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchExternalHealth {
    ProcessStarted,
    #[default]
    RuntimeSocket,
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

fn effective_readiness(process: &LaunchProcess) -> ReadinessGate {
    match process.external.as_ref().map(|external| external.health) {
        Some(LaunchExternalHealth::ProcessStarted) => ReadinessGate::ProcessStarted,
        Some(LaunchExternalHealth::RuntimeSocket)
            if process.readiness == ReadinessGate::ProcessStarted =>
        {
            ReadinessGate::RuntimeReady
        }
        _ => process.readiness,
    }
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
#[serde(deny_unknown_fields)]
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

fn parse_launch_manifest(manifest_json: &str) -> Result<LaunchManifest, String> {
    let manifest: LaunchManifest = serde_json::from_str(manifest_json)
        .map_err(|error| format!("failed to parse FlowRT launch manifest: {error}"))?;
    if manifest.ir_version != SUPPORTED_LAUNCH_IR_VERSION {
        return Err(format!(
            "unsupported FlowRT launch manifest IR version `{}`; expected `{SUPPORTED_LAUNCH_IR_VERSION}`",
            manifest.ir_version
        ));
    }
    for graph in &manifest.graphs {
        if graph.resource_contract.resource_contract_version != SUPPORTED_RESOURCE_CONTRACT_VERSION
        {
            return Err(format!(
                "unsupported FlowRT resource contract version `{}`; expected `{SUPPORTED_RESOURCE_CONTRACT_VERSION}`",
                graph.resource_contract.resource_contract_version
            ));
        }
    }
    Ok(manifest)
}

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

/// Zenoh 启动计划。
///
/// `port_lease` 必须和该 graph 的子进程生命周期一起持有，避免同一台机器上多个
/// FlowRT supervisor 自动选择同一个本机 zenoh 端口。
#[derive(Debug)]
pub struct ZenohLaunchPlan {
    pub env: HashMap<String, ZenohLaunchEnv>,
    _port_lease: Option<ZenohPortLease>,
}

impl ZenohLaunchPlan {
    fn empty() -> Self {
        Self {
            env: HashMap::new(),
            _port_lease: None,
        }
    }
}

#[derive(Debug)]
struct ZenohPortLease {
    port: u16,
    file: File,
}

impl Drop for ZenohPortLease {
    fn drop(&mut self) {
        let _ = unlock_zenoh_port_file(&self.file);
    }
}

/// 检查是否需要自动配置 zenoh（用户未显式设置相关环境变量时）。
pub fn should_auto_configure_zenoh() -> bool {
    std::env::var_os("FLOWRT_ZENOH_MODE").is_none()
        && std::env::var_os("FLOWRT_ZENOH_LISTEN").is_none()
        && std::env::var_os("FLOWRT_ZENOH_CONNECT").is_none()
}

/// 为 graph 中的 zenoh 进程生成 hub-and-spoke 拓扑配置。
///
/// 第一个 zenoh backend 进程作为 hub，监听 FlowRT supervisor 持有租约的本机端口；
/// 其余进程连接该 hub。
pub fn zenoh_launch_env_for_graph(processes: &[&LaunchProcess]) -> Result<ZenohLaunchPlan, String> {
    let zenoh_processes: Vec<&LaunchProcess> = processes
        .iter()
        .filter(|p| p.backend == "zenoh")
        .copied()
        .collect();
    if zenoh_processes.is_empty() {
        return Ok(ZenohLaunchPlan::empty());
    }

    let hub = zenoh_processes[0];
    let port_lease = reserve_zenoh_port_lease(&hub.name)?;
    let hub_locator = format!("tcp/127.0.0.1:{}", port_lease.port);

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
    Ok(ZenohLaunchPlan {
        env,
        _port_lease: Some(port_lease),
    })
}

fn reserve_zenoh_port_lease(process_name: &str) -> Result<ZenohPortLease, String> {
    let seed = std::process::id() as u16;
    for offset in 0..ZENOH_AUTO_PORT_COUNT {
        let port = ZENOH_AUTO_PORT_BASE + seed.wrapping_add(offset) % ZENOH_AUTO_PORT_COUNT;
        match try_acquire_zenoh_port_lease(port) {
            Ok(Some(lease)) => return Ok(lease),
            Ok(None) => continue,
            Err(error) => {
                return Err(format!(
                    "failed to reserve local zenoh port lease for `{process_name}`: {error}"
                ));
            }
        }
    }
    Err(format!(
        "failed to reserve local zenoh port lease for `{process_name}`: no FlowRT auto ports are available"
    ))
}

fn try_acquire_zenoh_port_lease(port: u16) -> std::io::Result<Option<ZenohPortLease>> {
    let path = std::env::temp_dir().join(format!("flowrt.zenoh.{port}.lock"));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    if !try_lock_zenoh_port_file(&file)? {
        return Ok(None);
    }
    file.set_len(0)?;
    writeln!(file, "pid={}", std::process::id())?;
    Ok(Some(ZenohPortLease { port, file }))
}

#[cfg(unix)]
fn try_lock_zenoh_port_file(file: &File) -> std::io::Result<bool> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => Ok(false),
        _ => Err(error),
    }
}

#[cfg(unix)]
fn unlock_zenoh_port_file(file: &File) -> std::io::Result<()> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
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
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    abort_child_if_shutdown_requested(supervisor_state, child, shutdown)?;
    match child.readiness {
        ReadinessGate::ProcessStarted => Ok(()),
        ReadinessGate::RuntimeReady => {
            child.state = "waiting_readiness".to_string();
            record_child_health(supervisor_state, child, false);
            wait_for_runtime_ready(supervisor_state, child, config, shutdown)
        }
        ReadinessGate::ServiceReady => {
            child.state = "waiting_readiness".to_string();
            record_child_health(supervisor_state, child, false);
            wait_for_service_ready(supervisor_state, child, config, shutdown)
        }
    }
}

fn abort_child_if_shutdown_requested(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    if !shutdown.is_requested() {
        return Ok(());
    }
    child.terminate("shutdown");
    record_child_health(supervisor_state, child, false);
    Err(format!(
        "FlowRT supervisor shutdown requested while waiting for process `{}`",
        child.name
    ))
}

/// 等待子进程 runtime introspection 握手成功。
fn wait_for_runtime_ready(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    config: &ReadinessConfig,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + config.timeout;
    loop {
        abort_child_if_shutdown_requested(supervisor_state, child, shutdown)?;
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
            child.terminate("readiness_timeout");
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` readiness timed out waiting for runtime_ready",
                child.name
            ));
        }

        // 轮询 introspection socket；单次 socket 读写不能超过外层 readiness deadline。
        match crate::introspection::request_status_with_timeout(
            &child.socket,
            readiness_socket_timeout(deadline, config.poll_interval),
        ) {
            Ok(crate::IntrospectionResponse::Status { .. }) => return Ok(()),
            _ => sleep_or_abort_child(
                supervisor_state,
                child,
                config.poll_interval,
                shutdown,
                "readiness wait",
            )?,
        }
    }
}

/// 等待子进程 runtime introspection 握手成功且该进程预期承载的 service endpoint 就绪。
fn wait_for_service_ready(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    config: &ReadinessConfig,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + config.timeout;
    loop {
        abort_child_if_shutdown_requested(supervisor_state, child, shutdown)?;
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
            child.terminate("readiness_timeout");
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` readiness timed out waiting for service_ready",
                child.name
            ));
        }

        // 轮询 introspection socket：需要握手成功且预期 service 全部就绪。
        match crate::introspection::request_status_with_timeout(
            &child.socket,
            readiness_socket_timeout(deadline, config.poll_interval),
        ) {
            Ok(crate::IntrospectionResponse::Status { status, .. }) => {
                if expected_services_ready(&child.expected_services, &status.services) {
                    return Ok(());
                }
                // 有 service 但未全部就绪，继续等待。
                sleep_or_abort_child(
                    supervisor_state,
                    child,
                    config.poll_interval,
                    shutdown,
                    "readiness wait",
                )?;
            }
            _ => sleep_or_abort_child(
                supervisor_state,
                child,
                config.poll_interval,
                shutdown,
                "readiness wait",
            )?,
        }
    }
}

fn readiness_socket_timeout(deadline: Instant, poll_interval: Duration) -> Duration {
    let remaining = deadline.saturating_duration_since(Instant::now());
    let timeout = remaining.min(poll_interval);
    if timeout.is_zero() {
        Duration::from_millis(1)
    } else {
        timeout
    }
}

fn sleep_or_abort_child(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    duration: Duration,
    shutdown: &ShutdownToken,
    wait_context: &str,
) -> Result<(), String> {
    if duration.is_zero() {
        return abort_child_if_shutdown_requested(supervisor_state, child, shutdown);
    }
    let deadline = Instant::now() + duration;
    loop {
        abort_child_if_shutdown_requested(supervisor_state, child, shutdown)?;
        if let Some(status) = child.child.try_wait().map_err(|error| {
            format!(
                "failed to poll FlowRT process `{}` while waiting: {error}",
                child.name
            )
        })? {
            child.exit_code = status.code();
            child.finished = true;
            child.state = "readiness_failed".to_string();
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` exited during {wait_context} (code: {})",
                child.name,
                child
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(());
        }
        std::thread::sleep((deadline - now).min(Duration::from_millis(10)));
    }
}

fn wait_for_startup_delay(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    if child.startup_delay_ms == 0 {
        return Ok(());
    }
    child.state = "delaying".to_string();
    record_child_health(supervisor_state, child, false);
    sleep_or_abort_child(
        supervisor_state,
        child,
        Duration::from_millis(child.startup_delay_ms),
        shutdown,
        "startup delay",
    )
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

fn binary_name(stem: &str) -> String {
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

fn build_launch_process_command(
    app_exe: &Path,
    process: &LaunchProcess,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
    external_resolution: Option<&ExternalExecutableResolution>,
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
        Ok(build_process_command(
            app_exe, process, run_ticks, zenoh_env,
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

fn spawn_launch_process(
    app_exe: &Path,
    process: &LaunchProcess,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
    external_resolution: Option<&ExternalExecutableResolution>,
) -> Result<Child, String> {
    build_launch_process_command(app_exe, process, run_ticks, zenoh_env, external_resolution)?
        .spawn()
        .map_err(|error| {
            format!(
                "failed to start FlowRT process `{}` executable `{}`: {error}",
                process.name,
                app_exe.display()
            )
        })
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
    backend: String,
    runtime_kind: String,
    external: Option<LaunchExternalProcess>,
    external_resolution: Option<ExternalExecutableResolution>,
    app_exe: PathBuf,
    zenoh_env: Option<ZenohLaunchEnv>,
    dependencies: Vec<String>,
    env: BTreeMap<String, String>,
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

impl SupervisedChild {
    fn terminate(&mut self, state: &'static str) {
        if self.finished {
            return;
        }
        self.request_termination();
        let deadline = Instant::now() + CHILD_TERMINATE_GRACE;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.exit_code = status.code();
                    self.finished = true;
                    self.state = state.to_string();
                    return;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(CHILD_TERMINATE_POLL_INTERVAL);
                }
                Ok(None) => {
                    self.force_kill();
                    let status = self.child.wait().ok();
                    self.exit_code = status.and_then(|status| status.code());
                    self.finished = true;
                    self.state = state.to_string();
                    return;
                }
                Err(_) => {
                    self.finished = true;
                    self.state = state.to_string();
                    self.exit_code = None;
                    return;
                }
            }
        }
    }

    fn request_termination(&mut self) {
        #[cfg(unix)]
        {
            let pid = self.child.id();
            if pid <= i32::MAX as u32 {
                let pgid = -(pid as i32);
                // 子进程启动时已经 setpgid(0, 0)。若遇到旧进程或 setpgid 失败，
                // 后面的 Child::kill 回退仍会终止直接子进程。
                let signaled = unsafe { libc::kill(pgid, libc::SIGTERM) == 0 };
                if signaled {
                    return;
                }
            }
        }
        let _ = self.child.kill();
    }

    fn force_kill(&mut self) {
        #[cfg(unix)]
        {
            let pid = self.child.id();
            if pid <= i32::MAX as u32 {
                let pgid = -(pid as i32);
                let _ = unsafe { libc::kill(pgid, libc::SIGKILL) };
            }
        }
        let _ = self.child.kill();
    }
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        self.terminate("terminated");
    }
}

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// Supervisor 主入口。
///
/// 解析 manifest，按依赖顺序启动进程，执行监控循环。
pub fn launch(config: &SupervisorConfig, run_ticks: Option<usize>) -> Result<(), String> {
    let manifest = parse_launch_manifest(config.manifest_json)?;
    if manifest.graphs.is_empty() {
        return Err("FlowRT launch manifest does not contain a graph".to_string());
    }

    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let shutdown = crate::install_signal_shutdown_token();

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

    let mut zenoh_launch_plans = Vec::new();
    let mut children = Vec::new();
    for graph in &manifest.graphs {
        let zenoh_plan = if should_auto_configure_zenoh() {
            let refs: Vec<&LaunchProcess> = graph.processes.iter().collect();
            zenoh_launch_env_for_graph(&refs)?
        } else {
            ZenohLaunchPlan::empty()
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
            let external_resolution = if process.runtime_kind == "external" {
                let external = process.external.as_ref().ok_or_else(|| {
                    format!(
                        "external process `{}` is missing launch manifest external metadata",
                        process.name
                    )
                })?;
                Some(
                    external_app_executable(&current_exe, external).map_err(|error| {
                        format!(
                            "failed to resolve external process `{}`: {error}",
                            process.name
                        )
                    })?,
                )
            } else {
                None
            };
            let app_exe = if let Some(resolution) = &external_resolution {
                resolution.executable.clone()
            } else {
                app_executable_for_runtime(
                    &current_exe,
                    &process.runtime_kind,
                    config.rust_app_stem,
                    config.cpp_app_stem,
                    config.ros2_bridge_stem,
                )?
            };
            let process_zenoh_env = zenoh_plan.env.get(&process.name).cloned();
            let child = spawn_launch_process(
                &app_exe,
                process,
                run_ticks,
                process_zenoh_env.as_ref(),
                external_resolution.as_ref(),
            )?;
            let resource_applied =
                apply_resource_placement_to_pid(&process.resource_placement, Some(child.id()));
            let socket = crate::runtime_socket_path_for_pid(child.id());
            let mut child = SupervisedChild {
                name: process.name.clone(),
                backend: process.backend.clone(),
                runtime_kind: process.runtime_kind.clone(),
                external: process.external.clone(),
                external_resolution,
                app_exe,
                zenoh_env: process_zenoh_env,
                dependencies: process.depends_on.clone(),
                env: process.env.clone(),
                restart_policy: process.restart,
                failure: process.failure.clone(),
                readiness: effective_readiness(process),
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
            wait_for_readiness(
                &supervisor_state,
                &mut child,
                &ReadinessConfig::default(),
                &shutdown,
            )?;
            ready_names.insert(process.name.clone());

            // readiness 通过后执行错峰启动延迟。
            wait_for_startup_delay(&supervisor_state, &mut child, &shutdown)?;

            child.state = "running".to_string();
            record_child_health(&supervisor_state, &child, false);
            children.push(child);
        }
        zenoh_launch_plans.push(zenoh_plan);
    }
    if children.is_empty() {
        return Err("FlowRT launch manifest does not contain process groups".to_string());
    }

    supervise_children(&supervisor_state, &mut children, run_ticks, &shutdown)?;

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
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    while children.iter().any(|child| !child.finished) {
        if shutdown.is_requested() {
            terminate_active_children(supervisor_state, children, "shutdown");
            return Ok(());
        }
        let mut failed_to_propagate = Vec::new();
        for child in children.iter_mut() {
            if child.finished || child.next_restart_unix_ms.is_some() {
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
        if run_ticks_satisfied(children, run_ticks) {
            terminate_active_children(supervisor_state, children, "completed");
            return Ok(());
        }
        let (spawned_names, ready_names) = child_dependency_snapshot(children);
        for child in children.iter_mut() {
            if child.finished {
                continue;
            }
            if let Some(next_restart_unix_ms) = child.next_restart_unix_ms {
                if unix_time_ms() >= next_restart_unix_ms {
                    if child_dependencies_satisfied(child, &spawned_names, &ready_names) {
                        restart_child(supervisor_state, child, run_ticks, shutdown)?;
                    } else {
                        child.state = "waiting_dependencies".to_string();
                        child.next_restart_unix_ms = Some(
                            unix_time_ms().saturating_add(HEALTH_POLL_INTERVAL.as_millis() as u64),
                        );
                        record_child_health(supervisor_state, child, false);
                    }
                } else {
                    record_child_health(supervisor_state, child, false);
                }
                continue;
            }
        }
        std::thread::sleep(HEALTH_POLL_INTERVAL);
    }
    Ok(())
}

fn run_ticks_satisfied(children: &[SupervisedChild], run_ticks: Option<usize>) -> bool {
    let Some(run_ticks) = run_ticks else {
        return false;
    };
    children
        .iter()
        .filter(|child| !child.finished)
        .all(|child| {
            child
                .last_tick_count
                .is_some_and(|tick_count| tick_count >= run_ticks as u64)
        })
}

fn restart_child(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    run_ticks: Option<usize>,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    let restart_process = LaunchProcess {
        name: child.name.clone(),
        backend: child.backend.clone(),
        target: None,
        runtimes: Vec::new(),
        runtime_kind: child.runtime_kind.clone(),
        external: child.external.clone(),
        depends_on: child.dependencies.clone(),
        env: child.env.clone(),
        restart: child.restart_policy,
        failure: child.failure.clone(),
        readiness: child.readiness,
        startup_delay_ms: child.startup_delay_ms,
        instances: Vec::new(),
        tasks: Vec::new(),
        resource_placement: child.resource_placement.clone(),
        io_boundaries: Vec::new(),
    };
    let restarted = if child.runtime_kind == "external" {
        let external = child.external.as_ref().ok_or_else(|| {
            format!(
                "external process `{}` is missing restart metadata",
                child.name
            )
        })?;
        let resolution = child.external_resolution.as_ref().ok_or_else(|| {
            format!(
                "external process `{}` package `{}` executable `{}` lost resolved path",
                child.name, external.package, external.executable
            )
        })?;
        build_external_process_command(
            &child.app_exe,
            &restart_process,
            run_ticks,
            child.zenoh_env.as_ref(),
            external,
            resolution,
        )
        .spawn()
        .map_err(|error| {
            format!(
                "failed to restart FlowRT process `{}` executable `{}`: {error}",
                child.name,
                child.app_exe.display()
            )
        })?
    } else {
        build_process_command(
            &child.app_exe,
            &restart_process,
            run_ticks,
            child.zenoh_env.as_ref(),
        )
        .spawn()
        .map_err(|error| {
            format!(
                "failed to restart FlowRT process `{}` executable `{}`: {error}",
                child.name,
                child.app_exe.display()
            )
        })?
    };
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
    wait_for_readiness(
        supervisor_state,
        child,
        &ReadinessConfig::default(),
        shutdown,
    )?;
    wait_for_startup_delay(supervisor_state, child, shutdown)?;
    child.state = "running".to_string();
    record_child_health(supervisor_state, child, false);
    Ok(())
}

fn child_dependency_snapshot(children: &[SupervisedChild]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut spawned_names = BTreeSet::new();
    let mut ready_names = BTreeSet::new();
    for child in children {
        if child.finished || child.next_restart_unix_ms.is_some() {
            continue;
        }
        spawned_names.insert(child.name.clone());
        if matches!(child.state.as_str(), "running" | "stale") {
            ready_names.insert(child.name.clone());
        }
    }
    (spawned_names, ready_names)
}

fn child_dependencies_satisfied(
    child: &SupervisedChild,
    spawned_names: &BTreeSet<String>,
    ready_names: &BTreeSet<String>,
) -> bool {
    child.dependencies.iter().all(|dependency| {
        ready_names.contains(dependency)
            || (child.readiness == ReadinessGate::ProcessStarted
                && spawned_names.contains(dependency))
    })
}

fn terminate_active_children(
    supervisor_state: &IntrospectionState,
    children: &mut [SupervisedChild],
    state: &'static str,
) {
    for child in children.iter_mut().filter(|child| !child.finished) {
        child.terminate(state);
        record_child_health(supervisor_state, child, false);
    }
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
            child.terminate("failed");
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
    match crate::introspection::request_status_with_timeout(&child.socket, HEALTH_POLL_INTERVAL) {
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
    static EXTERNAL_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static ROS2_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    fn path_list(raw: &str) -> Vec<String> {
        std::env::split_paths(raw)
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    fn test_process(name: &str, depends_on: Vec<String>) -> LaunchProcess {
        LaunchProcess {
            name: name.into(),
            backend: "inproc".into(),
            target: None,
            runtimes: vec![],
            runtime_kind: "rust".into(),
            external: None,
            depends_on,
            env: BTreeMap::new(),
            restart: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
            readiness: ReadinessGate::ProcessStarted,
            startup_delay_ms: 0,
            instances: vec![],
            tasks: vec![],
            resource_placement: ResourcePlacement::default(),
            io_boundaries: vec![],
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

    fn supervised_child_for_test(name: &str, child: Child) -> SupervisedChild {
        let socket = crate::runtime_socket_path_for_pid(child.id());
        SupervisedChild {
            name: name.into(),
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            external: None,
            external_resolution: None,
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            env: BTreeMap::new(),
            restart_policy: DEFAULT_RESTART_POLICY,
            failure: "propagate".into(),
            readiness: ReadinessGate::ProcessStarted,
            expected_services: vec![],
            startup_delay_ms: 0,
            resource_placement: ResourcePlacement::default(),
            resource_placement_status: ResourcePlacementStatus::default(),
            child,
            socket,
            finished: false,
            restart_count: 0,
            next_restart_unix_ms: None,
            last_seen_unix_ms: None,
            last_tick_count: None,
            last_tick_changed_unix_ms: unix_time_ms(),
            state: "running".into(),
            exit_code: None,
        }
    }

    #[cfg(unix)]
    fn process_exists(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
        if result == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    #[cfg(unix)]
    fn assert_process_exits(pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if !process_exists(pid) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("process {pid} should have exited");
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

    #[cfg(unix)]
    #[test]
    fn supervised_child_drop_terminates_running_process() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let child = command.spawn().unwrap();
        let pid = child.id();
        let supervised = supervised_child_for_test("drop_cleanup", child);

        drop(supervised);

        assert_process_exits(pid);
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_shutdown_token_terminates_active_children() {
        let supervisor_state = IntrospectionState::new();
        let mut command = Command::new("sleep");
        command.arg("30");
        let child = command.spawn().unwrap();
        let pid = child.id();
        let mut children = vec![supervised_child_for_test("shutdown_cleanup", child)];
        let shutdown = ShutdownToken::new_for_test();
        shutdown.request();

        let result = supervise_children(&supervisor_state, &mut children, None, &shutdown);

        assert!(result.is_ok());
        assert!(children[0].finished);
        assert_eq!(children[0].state, "shutdown");
        assert_process_exits(pid);
    }

    #[cfg(unix)]
    #[test]
    fn restart_dependency_check_requires_ready_dependencies_for_runtime_ready() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let child = command.spawn().unwrap();
        let mut supervised = supervised_child_for_test("worker", child);
        supervised.readiness = ReadinessGate::RuntimeReady;
        supervised.dependencies = vec!["driver".to_string()];
        let mut spawned = BTreeSet::new();
        spawned.insert("driver".to_string());
        let ready = BTreeSet::new();

        assert!(!child_dependencies_satisfied(&supervised, &spawned, &ready));

        supervised.terminate("test_cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn restart_dependency_check_allows_spawned_dependency_for_process_started() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let child = command.spawn().unwrap();
        let mut supervised = supervised_child_for_test("worker", child);
        supervised.readiness = ReadinessGate::ProcessStarted;
        supervised.dependencies = vec!["driver".to_string()];
        let mut spawned = BTreeSet::new();
        spawned.insert("driver".to_string());
        let ready = BTreeSet::new();

        assert!(child_dependencies_satisfied(&supervised, &spawned, &ready));

        supervised.terminate("test_cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn startup_delay_fails_if_process_exits_after_readiness() {
        let child = Command::new("true").spawn().unwrap();
        let supervisor_state = IntrospectionState::new();
        let mut supervised = supervised_child_for_test("startup_delay_exit", child);
        supervised.startup_delay_ms = 50;

        let result = wait_for_startup_delay(
            &supervisor_state,
            &mut supervised,
            &ShutdownToken::new_for_test(),
        );

        assert!(result.is_err());
        assert!(supervised.finished);
        assert_eq!(supervised.state, "readiness_failed");
    }

    #[cfg(unix)]
    #[test]
    fn restart_waits_when_dependency_exits_in_same_monitor_iteration() {
        let driver = Command::new("true").spawn().unwrap();
        let worker = Command::new("true").spawn().unwrap();
        let mut children = vec![
            supervised_child_for_test("driver", driver),
            supervised_child_for_test("worker", worker),
        ];
        children[1].app_exe = PathBuf::from("true");
        children[1].readiness = ReadinessGate::RuntimeReady;
        children[1].dependencies = vec!["driver".to_string()];
        children[1].next_restart_unix_ms = Some(unix_time_ms());
        std::thread::sleep(Duration::from_millis(20));
        let supervisor_state = IntrospectionState::new();
        let shutdown = ShutdownToken::new_for_test();
        let shutdown_clone = shutdown.clone();
        let shutdown_thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            shutdown_clone.request();
        });

        let result = supervise_children(&supervisor_state, &mut children, None, &shutdown);

        shutdown_thread
            .join()
            .expect("shutdown thread should finish");
        assert!(
            result.is_ok(),
            "worker must not restart against an exited dependency: {result:?}"
        );
        assert_eq!(children[1].restart_count, 0);
        assert_eq!(children[1].state, "shutdown");
    }

    #[cfg(unix)]
    #[test]
    fn refresh_child_health_returns_when_socket_stalls() {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let mut supervised = supervised_child_for_test("stalled_socket", child);
        let socket = crate::runtime_socket_path_for_pid(supervised.child.id());
        if let Some(parent) = socket.parent() {
            std::fs::create_dir_all(parent).expect("socket parent should be created");
        }
        let _ = std::fs::remove_file(&socket);
        let listener =
            std::os::unix::net::UnixListener::bind(&socket).expect("test listener should bind");
        let handle = std::thread::spawn(move || {
            let (_stream, _addr) = listener.accept().expect("test listener should accept");
            std::thread::sleep(Duration::from_millis(250));
        });
        supervised.socket = socket.clone();
        let supervisor_state = IntrospectionState::new();

        let started = Instant::now();
        refresh_child_health(&supervisor_state, &mut supervised);

        assert!(
            started.elapsed() < Duration::from_millis(500),
            "refresh_child_health should not block the supervisor loop indefinitely"
        );
        assert!(!supervised.finished);
        supervised.terminate("test_cleanup");
        handle
            .join()
            .expect("stalled listener thread should finish");
        let _ = std::fs::remove_file(socket);
    }

    #[cfg(unix)]
    #[test]
    fn supervisor_terminates_active_children_when_run_ticks_reached() {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let mut supervised = supervised_child_for_test("bounded_run", child);
        supervised.last_tick_count = Some(3);
        let supervisor_state = IntrospectionState::new();

        let result = supervise_children(
            &supervisor_state,
            std::slice::from_mut(&mut supervised),
            Some(3),
            &ShutdownToken::new_for_test(),
        );

        assert!(
            result.is_ok(),
            "supervisor run limit should stop active children: {result:?}"
        );
        assert!(supervised.finished);
        assert_eq!(supervised.state, "completed");
    }

    #[test]
    fn process_command_uses_runtime_kind_not_process_name_for_ros2_bridge() {
        let mut process = test_process("ros2_bridge", vec![]);
        process.runtime_kind = "rust".into();
        let command = build_process_command(Path::new("/tmp/flowrt_app"), &process, Some(5), None);

        assert_eq!(
            command_args(&command),
            vec!["--process", "ros2_bridge", "--flowrt-run-steps", "5"]
        );
        assert_eq!(command_env(&command, "RMW_IMPLEMENTATION"), None);
    }

    #[test]
    fn ros2_bridge_runtime_command_sets_rmw_without_process_arg() {
        let mut process = test_process("adapter", vec![]);
        process.runtime_kind = "ros2_bridge".into();
        let command = build_process_command(
            Path::new("/tmp/flowrt_ros2_bridge"),
            &process,
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
    fn ros2_bridge_runtime_command_isolates_ros2_zenoh_library_path() {
        let _lock = ROS2_ENV_LOCK.lock().expect("ros2 env lock should work");
        let _ament = EnvOverride::set(
            "AMENT_PREFIX_PATH",
            Some("/opt/ros/lyrical:/home/robot/ros_overlay"),
        );
        let _ld = EnvOverride::set(
            "LD_LIBRARY_PATH",
            Some(
                "/opt/flowrt/0.8.3/lib:/opt/ros/lyrical/lib/aarch64-linux-gnu:/opt/ros/lyrical/lib:/custom/lib:/opt/flowrt/0.8.3/targets/linux-arm64/lib",
            ),
        );
        let _flowrt_prefix = EnvOverride::set("FLOWRT_PRIVATE_PREFIX", Some("/opt/flowrt/0.8.3"));

        let mut process = test_process("adapter", vec![]);
        process.runtime_kind = "ros2_bridge".into();
        let command =
            build_process_command(Path::new("/tmp/flowrt_ros2_bridge"), &process, None, None);
        let library_path = command_env(&command, "LD_LIBRARY_PATH")
            .expect("ros2 bridge command should set isolated LD_LIBRARY_PATH");
        let paths = path_list(&library_path);

        assert_eq!(paths[0], "/opt/ros/lyrical/opt/zenoh_cpp_vendor/lib");
        assert_eq!(paths[1], "/home/robot/ros_overlay/opt/zenoh_cpp_vendor/lib");
        assert!(paths.contains(&"/opt/ros/lyrical/lib/aarch64-linux-gnu".to_string()));
        assert!(paths.contains(&"/opt/ros/lyrical/lib".to_string()));
        assert!(paths.contains(&"/custom/lib".to_string()));
        assert!(
            !paths.iter().any(|path| path.starts_with("/opt/flowrt/")),
            "{paths:?}"
        );
    }

    #[test]
    fn process_command_applies_manifest_env() {
        let mut process = test_process("control", vec![]);
        process.env.insert("APP_MODE".into(), "control".into());

        let command = build_process_command(Path::new("/tmp/flowrt_app"), &process, None, None);

        assert_eq!(
            command_env(&command, "APP_MODE").as_deref(),
            Some("control")
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
    fn c_runtime_executable_uses_cmake_app_binary() {
        let root = temp_test_dir("c-runtime-sibling");
        let bin_dir = root.join("flowrt/build/bin/release");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let supervisor = bin_dir.join(binary_name("robot-flowrt-supervisor"));
        let cpp_app = bin_dir.join(binary_name("robot_cpp_app"));
        std::fs::write(&supervisor, "").unwrap();
        std::fs::write(&cpp_app, "").unwrap();

        let resolved =
            app_executable_for_runtime(&supervisor, "c", "robot-flowrt-app", "robot_cpp_app", "")
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
    fn external_runtime_executable_resolves_from_external_path() {
        let _lock = EXTERNAL_ENV_LOCK
            .lock()
            .expect("external env lock should work");
        let root = temp_test_dir("external-path");
        let package_root = root.join("packages/camera_driver");
        let executable = package_root.join("bin/camera-node");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, "").unwrap();
        let _path = EnvOverride::set(
            "FLOWRT_EXTERNAL_PATH",
            Some(root.join("packages").to_str().unwrap()),
        );
        let current_exe = root.join("workspace/flowrt/build/bin/release/app-supervisor");
        let external = LaunchExternalProcess {
            package: "camera_driver".into(),
            executable: "bin/camera-node".into(),
            args: vec![],
            working_dir: LaunchExternalWorkingDir::Package,
            health: LaunchExternalHealth::RuntimeSocket,
            required_backends: vec!["zenoh".into()],
            package_root: None,
        };

        let resolved = external_app_executable(&current_exe, &external).unwrap();

        assert_eq!(resolved.executable, executable.canonicalize().unwrap());
        assert_eq!(resolved.package_root, package_root.canonicalize().unwrap());
        assert_eq!(resolved.workspace_root, root.join("workspace"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn external_runtime_executable_rejects_escape_paths() {
        let root = temp_test_dir("external-escape-paths");
        let current_exe = root.join("workspace/flowrt/build/bin/release/app-supervisor");
        for executable in ["/bin/sh", "../driver", "bin/../driver", "./driver"] {
            let external = LaunchExternalProcess {
                package: "camera_driver".into(),
                executable: executable.into(),
                args: vec![],
                working_dir: LaunchExternalWorkingDir::Package,
                health: LaunchExternalHealth::RuntimeSocket,
                required_backends: vec!["zenoh".into()],
                package_root: Some(root.join("packages/camera_driver")),
            };

            let error = external_app_executable(&current_exe, &external)
                .expect_err("escape executable path should be rejected");

            assert!(
                error.contains("must be package-relative"),
                "unexpected error for {executable}: {error}"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn external_runtime_executable_rejects_symlink_escape() {
        let root = temp_test_dir("external-symlink-escape");
        let package_root = root.join("packages/camera_driver");
        let outside = root.join("outside/camera-node");
        std::fs::create_dir_all(package_root.join("bin")).unwrap();
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        std::fs::write(&outside, "").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, package_root.join("bin/camera-node")).unwrap();
        let current_exe = root.join("workspace/flowrt/build/bin/release/app-supervisor");
        let external = LaunchExternalProcess {
            package: "camera_driver".into(),
            executable: "bin/camera-node".into(),
            args: vec![],
            working_dir: LaunchExternalWorkingDir::Package,
            health: LaunchExternalHealth::RuntimeSocket,
            required_backends: vec!["zenoh".into()],
            package_root: Some(package_root),
        };

        #[cfg(unix)]
        {
            let error = external_app_executable(&current_exe, &external)
                .expect_err("symlink escaping package root should be rejected");
            assert!(error.contains("escapes package root"));
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn external_process_command_uses_manifest_args_and_env_contract() {
        let external = LaunchExternalProcess {
            package: "camera_driver".into(),
            executable: "bin/camera-node".into(),
            args: vec!["--device".into(), "/dev/video0".into()],
            working_dir: LaunchExternalWorkingDir::Package,
            health: LaunchExternalHealth::RuntimeSocket,
            required_backends: vec!["zenoh".into()],
            package_root: None,
        };
        let resolution = ExternalExecutableResolution {
            executable: PathBuf::from("/opt/flowrt/external/camera_driver/bin/camera-node"),
            package_root: PathBuf::from("/opt/flowrt/external/camera_driver"),
            workspace_root: PathBuf::from("/tmp/robot_ws"),
        };
        let zenoh = ZenohLaunchEnv {
            listen: "tcp/127.0.0.1:7447".into(),
            connect: String::new(),
        };
        let mut process = test_process("camera_proc", vec![]);
        process.backend = "zenoh".into();
        process.runtime_kind = "external".into();
        process.env.insert("APP_MODE".into(), "driver".into());
        process
            .env
            .insert("FLOWRT_PROCESS".into(), "user_override".into());

        let command = build_external_process_command(
            &resolution.executable,
            &process,
            Some(5),
            Some(&zenoh),
            &external,
            &resolution,
        );

        assert_eq!(command_args(&command), vec!["--device", "/dev/video0"]);
        assert_eq!(
            command.get_current_dir(),
            Some(Path::new("/opt/flowrt/external/camera_driver"))
        );
        assert_eq!(
            command_env(&command, "FLOWRT_PROCESS").as_deref(),
            Some("camera_proc")
        );
        assert_eq!(
            command_env(&command, "FLOWRT_EXTERNAL_PACKAGE").as_deref(),
            Some("camera_driver")
        );
        assert_eq!(
            command_env(&command, "FLOWRT_BACKEND").as_deref(),
            Some("zenoh")
        );
        assert_eq!(
            command_env(&command, "FLOWRT_RUN_STEPS").as_deref(),
            Some("5")
        );
        assert_eq!(
            command_env(&command, "FLOWRT_ZENOH_LISTEN").as_deref(),
            Some("tcp/127.0.0.1:7447")
        );
        assert_eq!(command_env(&command, "APP_MODE").as_deref(), Some("driver"));
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
        let plan = zenoh_launch_env_for_graph(&refs).unwrap();
        let env = &plan.env;

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
        let plan = zenoh_launch_env_for_graph(&refs).unwrap();
        assert!(plan.env.is_empty());
    }

    #[test]
    fn zenoh_port_lease_skips_locked_port() {
        let first = reserve_zenoh_port_lease("hub").unwrap();
        let second = reserve_zenoh_port_lease("spoke").unwrap();

        assert_ne!(first.port, second.port);
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
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [{
                    "name": "client.plan",
                    "client": "client.plan",
                    "client_instance": "client",
                    "client_port": "plan",
                    "server": "server.plan",
                    "server_instance": "server",
                    "server_port": "plan",
                    "request": "PlanRequest",
                    "response": "PlanResponse",
                    "backend": "inproc",
                    "timeout_ms": 5000,
                    "queue_depth": 32,
                    "overflow": "busy",
                    "lane": null,
                    "max_in_flight": 64
                }],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
                "processes": [{
                    "name": "sensor",
                    "backend": "zenoh",
                    "target": null,
                    "runtimes": ["rust"],
                    "runtime_kind": "rust",
                    "tasks": []
                }]
            }]
        }"#;
        let manifest = parse_launch_manifest(json).unwrap();
        assert!(manifest.profile_modes.is_empty());
        assert_eq!(manifest.graphs[0].mode, "strict");
        assert!(manifest.graphs[0].boundary_endpoints.is_empty());
        let process = &manifest.graphs[0].processes[0];
        assert_eq!(process.name, "sensor");
        assert!(process.target.is_none());
        assert_eq!(process.runtimes, vec!["rust"]);
        assert!(process.tasks.is_empty());
        assert!(process.depends_on.is_empty());
        assert_eq!(process.restart.policy, RestartPolicyKind::OnFailure);
        assert_eq!(process.restart.max_restarts, 3);
        assert_eq!(process.failure, "propagate");
        assert_eq!(process.readiness, ReadinessGate::ProcessStarted);
        assert!(process.env.is_empty());
        assert_eq!(process.startup_delay_ms, 0);
        assert_eq!(process.resource_placement, ResourcePlacement::default());
        let service = &manifest.graphs[0].services[0];
        assert_eq!(service.lane, None);
        assert_eq!(service.max_in_flight, 64);
    }

    #[test]
    fn manifest_deserialization_rejects_unknown_fields() {
        let json = r#"{
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
                "processes": [{
                    "name": "sensor",
                    "backend": "zenoh",
                    "runtime_kind": "rust",
                    "depends_onn": ["driver"]
                }]
            }]
        }"#;

        let error = parse_launch_manifest(json).expect_err("unknown manifest fields must fail");

        assert!(error.contains("unknown field"), "unexpected error: {error}");
    }

    #[test]
    fn manifest_deserialization_accepts_launch_artifact_metadata() {
        let json = r#"{
            "package": "demo",
            "ir_version": "0.1",
            "artifact": {
                "mode": "island",
                "temporary_island": true,
                "test_only": true
            },
            "profiles": ["dev"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
                "processes": []
            }]
        }"#;

        let manifest = parse_launch_manifest(json).unwrap();

        assert_eq!(manifest.artifact.mode, "island");
        assert!(manifest.artifact.temporary_island);
        assert!(manifest.artifact.test_only);
    }

    #[test]
    fn manifest_deserialization_accepts_island_boundary_metadata() {
        let json = r#"{
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["dev"],
            "profile_modes": [{ "name": "dev", "mode": "island" }],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "mode": "island",
                "scheduler": {},
                "channels": [],
                "boundary_endpoints": [{
                    "name": "sample_in",
                    "canonical_id": "boundary_0123456789abcdef",
                    "direction": "input",
                    "endpoint": "consumer.sample",
                    "instance": "consumer",
                    "port": "sample",
                    "message_type": "Sample"
                }],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
                "processes": []
            }]
        }"#;

        let manifest = parse_launch_manifest(json).unwrap();

        assert_eq!(manifest.profile_modes[0].name, "dev");
        assert_eq!(manifest.profile_modes[0].mode, "island");
        assert_eq!(manifest.graphs[0].mode, "island");
        assert_eq!(manifest.graphs[0].boundary_endpoints[0].name, "sample_in");
        assert_eq!(
            manifest.graphs[0].boundary_endpoints[0].endpoint,
            "consumer.sample"
        );
    }

    #[test]
    fn manifest_deserialization_rejects_unsupported_ir_version() {
        let json = r#"{
            "package": "demo",
            "ir_version": "9.9",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
                "processes": []
            }]
        }"#;

        let error = parse_launch_manifest(json).expect_err("future manifest version must fail");

        assert!(
            error.contains("unsupported FlowRT launch manifest IR version `9.9`"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_deserialization_with_process_env() {
        let json = r#"{
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
                "processes": [{
                    "name": "control",
                    "backend": "inproc",
                    "runtime_kind": "rust",
                    "env": {
                        "APP_MODE": "control",
                        "LOG_LEVEL": "debug"
                    }
                }]
            }]
        }"#;
        let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
        let process = &manifest.graphs[0].processes[0];

        assert_eq!(process.env["APP_MODE"], "control");
        assert_eq!(process.env["LOG_LEVEL"], "debug");
    }

    #[test]
    fn manifest_deserialization_with_external_process_metadata() {
        let json = r#"{
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
                "processes": [{
                    "name": "camera_proc",
                    "backend": "zenoh",
                    "runtime_kind": "external",
                    "external": {
                        "package": "camera_driver",
                        "executable": "bin/camera-node",
                        "args": ["--device", "/dev/video0"],
                        "working_dir": "workspace",
                        "health": "process_started",
                        "required_backends": ["zenoh"]
                    }
                }]
            }]
        }"#;
        let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
        let process = &manifest.graphs[0].processes[0];
        let external = process.external.as_ref().unwrap();

        assert_eq!(external.package, "camera_driver");
        assert_eq!(external.executable, "bin/camera-node");
        assert_eq!(external.args, vec!["--device", "/dev/video0"]);
        assert_eq!(external.working_dir, LaunchExternalWorkingDir::Workspace);
        assert_eq!(external.health, LaunchExternalHealth::ProcessStarted);
        assert_eq!(effective_readiness(process), ReadinessGate::ProcessStarted);
    }

    #[test]
    fn external_runtime_socket_health_upgrades_default_readiness() {
        let json = r#"{
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
                "processes": [{
                    "name": "camera_proc",
                    "backend": "zenoh",
                    "runtime_kind": "external",
                    "external": {
                        "package": "camera_driver",
                        "executable": "bin/camera-node"
                    }
                }]
            }]
        }"#;
        let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
        let process = &manifest.graphs[0].processes[0];

        assert_eq!(process.readiness, ReadinessGate::ProcessStarted);
        assert_eq!(effective_readiness(process), ReadinessGate::RuntimeReady);
    }

    #[test]
    fn manifest_deserialization_with_custom_policy() {
        let json = r#"{
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
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
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
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
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
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
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
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
    fn manifest_deserialization_accepts_io_resource_descriptor_schema() {
        let json = r#"{
            "package": "demo",
            "ir_version": "0.1",
            "profiles": ["default"],
            "targets": ["default"],
            "graphs": [{
                "name": "main",
                "scheduler": {},
                "channels": [],
                "services": [],
                "ros2_bridges": [],
                "instances": [],
                "tasks": [],
                "processes": [{
                    "name": "camera",
                    "backend": "iox2",
                    "runtime_kind": "rust",
                    "io_boundaries": [{
                        "instance": "camera",
                        "component": "camera",
                        "side_effects": ["device", "read"],
                        "readiness": "resource_ready",
                        "health": "runtime_reported",
                        "shutdown": "cooperative",
                        "resources": [{
                            "name": "frames",
                            "kind": "shm",
                            "required": true,
                            "descriptor": {
                                "kind": "frame",
                                "port": "frame",
                                "format": "rgb8",
                                "encoding": "row_major",
                                "metadata": {
                                    "width": "640",
                                    "height": "480"
                                },
                                "record_payload": false
                            }
                        }]
                    }]
                }]
            }]
        }"#;
        let manifest: LaunchManifest = serde_json::from_str(json).unwrap();
        let resource = &manifest.graphs[0].processes[0].io_boundaries[0].resources[0];
        let descriptor = resource.descriptor.as_ref().unwrap();

        assert_eq!(resource.name, "frames");
        assert!(resource.required);
        assert_eq!(descriptor.kind, "frame");
        assert_eq!(descriptor.port, "frame");
        assert_eq!(descriptor.format, "rgb8");
        assert_eq!(descriptor.encoding.as_deref(), Some("row_major"));
        assert_eq!(descriptor.metadata["width"], "640");
        assert!(!descriptor.record_payload);
    }

    #[test]
    fn expected_services_for_process_uses_server_instances() {
        let graph = LaunchGraph {
            name: "main".to_string(),
            mode: "strict".to_string(),
            scheduler: serde_json::json!({}),
            resource_contract: LaunchResourceContract::default(),
            channels: vec![],
            boundary_endpoints: vec![],
            services: vec![
                LaunchService {
                    name: "client.plan".to_string(),
                    client: "client.plan".to_string(),
                    client_instance: "client".to_string(),
                    client_port: "plan".to_string(),
                    server: "server.plan".to_string(),
                    server_instance: "server".to_string(),
                    server_port: "plan".to_string(),
                    request: "PlanRequest".to_string(),
                    response: "PlanResponse".to_string(),
                    backend: "inproc".to_string(),
                    timeout_ms: 100,
                    queue_depth: 1,
                    overflow: "error".to_string(),
                    lane: None,
                    max_in_flight: 64,
                },
                LaunchService {
                    name: "client.inspect".to_string(),
                    client: "client.inspect".to_string(),
                    client_instance: "client".to_string(),
                    client_port: "inspect".to_string(),
                    server: "inspector.inspect".to_string(),
                    server_instance: "inspector".to_string(),
                    server_port: "inspect".to_string(),
                    request: "InspectRequest".to_string(),
                    response: "InspectResponse".to_string(),
                    backend: "inproc".to_string(),
                    timeout_ms: 100,
                    queue_depth: 1,
                    overflow: "error".to_string(),
                    lane: None,
                    max_in_flight: 64,
                },
            ],
            ros2_bridges: vec![],
            instances: vec![],
            tasks: vec![],
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
    fn readiness_wait_aborts_when_shutdown_requested() {
        let supervisor_state = IntrospectionState::new();
        let shutdown = ShutdownToken::new_for_test();
        shutdown.request();
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let real_child = cmd.spawn().unwrap();
        let socket = crate::runtime_socket_path_for_pid(real_child.id());

        let mut child = SupervisedChild {
            name: "test_shutdown".into(),
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            external: None,
            external_resolution: None,
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            env: BTreeMap::new(),
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

        let result = wait_for_readiness(
            &supervisor_state,
            &mut child,
            &short_readiness_config(),
            &shutdown,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("shutdown requested"));
        assert!(child.finished);
        assert_eq!(child.state, "shutdown");
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
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            external: None,
            external_resolution: None,
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            env: BTreeMap::new(),
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
        let shutdown = ShutdownToken::new_for_test();
        let result = wait_for_runtime_ready(&supervisor_state, &mut child, &config, &shutdown);

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
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            external: None,
            external_resolution: None,
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            env: BTreeMap::new(),
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
        let shutdown = ShutdownToken::new_for_test();
        let result = wait_for_service_ready(&supervisor_state, &mut child, &config, &shutdown);

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
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            external: None,
            external_resolution: None,
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            env: BTreeMap::new(),
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
        let shutdown = ShutdownToken::new_for_test();
        let result = wait_for_runtime_ready(&supervisor_state, &mut child, &config, &shutdown);

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
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            external: None,
            external_resolution: None,
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            env: BTreeMap::new(),
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
        let shutdown = ShutdownToken::new_for_test();
        let result = wait_for_readiness(&supervisor_state, &mut child, &config, &shutdown);
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
            backend: "inproc".into(),
            runtime_kind: "rust".into(),
            external: None,
            external_resolution: None,
            app_exe: PathBuf::from("/tmp/fake"),
            zenoh_env: None,
            dependencies: vec![],
            env: BTreeMap::new(),
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
