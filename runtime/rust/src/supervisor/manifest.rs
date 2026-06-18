//! Launch manifest DTO 和重启策略。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

use super::resource_placement::ResourcePlacement;

const SUPPORTED_LAUNCH_IR_VERSION: &str = "0.1";
const SUPPORTED_RESOURCE_CONTRACT_VERSION: &str = "0.1";

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
    #[serde(default)]
    pub determinism: LaunchDeterminism,
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
    #[serde(default)]
    pub temporary_overlay: Option<serde_json::Value>,
    #[serde(default)]
    pub clock: LaunchClock,
}

impl Default for LaunchArtifact {
    fn default() -> Self {
        Self {
            mode: default_graph_mode(),
            temporary_island: false,
            test_only: false,
            temporary_overlay: None,
            clock: LaunchClock::default(),
        }
    }
}

/// manifest 中 supervisor 观察到的调度时钟元数据。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchClock {
    #[serde(default = "default_clock_source")]
    pub source: String,
    #[serde(default = "default_clock_unit")]
    pub unit: String,
    #[serde(default = "default_clock_field")]
    pub field: String,
}

impl Default for LaunchClock {
    fn default() -> Self {
        Self {
            source: default_clock_source(),
            unit: default_clock_unit(),
            field: default_clock_field(),
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

/// manifest 中的 profile 级确定性执行合同。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchDeterminism {
    #[serde(default = "default_determinism_mode")]
    pub mode: String,
    #[serde(default)]
    pub tick_timeout_ms: u64,
    #[serde(default = "default_determinism_timeout_policy")]
    pub on_timeout: String,
    #[serde(default)]
    pub processes: Vec<String>,
}

impl Default for LaunchDeterminism {
    fn default() -> Self {
        Self {
            mode: default_determinism_mode(),
            tick_timeout_ms: 0,
            on_timeout: default_determinism_timeout_policy(),
            processes: Vec::new(),
        }
    }
}

/// manifest 中的 graph 节点。
#[derive(Debug, Clone, Deserialize)]
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

fn default_clock_source() -> String {
    "realtime".to_string()
}

fn default_clock_unit() -> String {
    "ms".to_string()
}

fn default_clock_field() -> String {
    "tick_time_ms".to_string()
}

fn default_determinism_mode() -> String {
    "process_local".to_string()
}

fn default_determinism_timeout_policy() -> String {
    "fault_graph".to_string()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchResourceContract {
    #[serde(default = "default_resource_contract_version")]
    pub resource_contract_version: String,
    #[serde(default)]
    pub requirements: Vec<LaunchResourceRequirement>,
    #[serde(default)]
    pub providers: Vec<LaunchResourceProvider>,
    #[serde(default)]
    pub satisfactions: Vec<LaunchResourceSatisfaction>,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchResourceRequirement {
    pub instance: String,
    pub component: String,
    pub name: String,
    pub capability: String,
    pub access: String,
    pub required: bool,
    pub readiness: String,
    pub health: String,
    pub on_failure: String,
    pub satisfaction: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchResourceProvider {
    pub name: String,
    pub scope: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub process: Option<String>,
    #[serde(default)]
    pub external_package: Option<String>,
    #[serde(default)]
    pub readiness_source: Option<String>,
    #[serde(default)]
    pub health_source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchResourceSatisfaction {
    pub instance: String,
    pub component: String,
    pub resource: String,
    pub capability: String,
    pub access: String,
    pub required: bool,
    pub readiness: String,
    pub health: String,
    pub on_failure: String,
    pub status: String,
    pub satisfied: bool,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub diagnostic: Option<String>,
}

/// manifest 中的 island boundary endpoint 静态摘要。
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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
#[derive(Debug, Clone, Deserialize)]
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

pub(super) fn effective_readiness(process: &LaunchProcess) -> ReadinessGate {
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

pub(super) fn default_failure_propagation() -> String {
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

pub(super) fn parse_launch_manifest(manifest_json: &str) -> Result<LaunchManifest, String> {
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
