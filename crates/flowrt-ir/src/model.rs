use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

/// FlowRT Contract IR 顶层文档。
///
/// 这是 RSDL 归一化后的唯一语义入口。validator、codegen 和 runtime shell 都应消费该结构，
/// 而不是回头解释 RSDL 源文本。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractIr {
    pub ir_version: String,
    pub schema_version: String,
    pub source_hash: String,
    pub package_id: EntityId,
    pub package: PackageIr,
    pub modules: Vec<ModuleIr>,
    pub types: Vec<TypeIr>,
    pub components: Vec<ComponentIr>,
    pub graphs: Vec<GraphIr>,
    pub profiles: Vec<ProfileIr>,
    pub targets: Vec<TargetIr>,
    pub deployments: Vec<DeploymentIr>,
}

impl ContractIr {
    /// 将 IR 序列化为稳定的 pretty JSON。
    pub fn to_canonical_json(&self) -> serde_json::Result<String> {
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        Ok(json)
    }

    /// 解析已落盘的 Contract IR JSON 文档。
    pub fn from_json_str(source: &str) -> serde_json::Result<Self> {
        use serde::de::Error as _;

        let mut ignored_fields = Vec::new();
        let mut deserializer = serde_json::Deserializer::from_str(source);
        let contract = serde_ignored::deserialize(&mut deserializer, |path| {
            ignored_fields.push(path.to_string());
        })?;
        deserializer.end()?;
        if let Some(path) = ignored_fields.first() {
            return Err(serde_json::Error::custom(format!(
                "unknown Contract IR field `{path}`"
            )));
        }
        Ok(contract)
    }
}

/// 不透明且确定性的实体标识符。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityId(pub String);

/// 已解析的实体引用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityRef {
    pub id: EntityId,
    pub name: String,
}

/// 包级元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageIr {
    pub name: String,
    pub version: Option<String>,
    pub rsdl_version: String,
    pub imports: Vec<ImportIr>,
}

/// 归一化后的 import 声明。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportIr {
    pub kind: String,
    pub patterns: Vec<String>,
}

/// workspace/module 边界。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleIr {
    pub name: String,
    pub source: String,
}

/// 消息类型声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeIr {
    pub id: EntityId,
    pub module: Option<String>,
    pub name: String,
    pub qualified_name: String,
    pub generated_name: String,
    pub fields: Vec<FieldIr>,
}

/// 消息字段声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldIr {
    pub name: String,
    pub ty: crate::TypeExpr,
    pub default: Option<ParamValue>,
}

/// 可复用组件类型声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentIr {
    pub id: EntityId,
    pub module: Option<String>,
    pub name: String,
    pub qualified_name: String,
    pub generated_name: String,
    pub language: LanguageKind,
    pub kind: ComponentKind,
    #[serde(default)]
    pub build: ComponentBuildIr,
    pub inputs: Vec<PortIr>,
    pub outputs: Vec<PortIr>,
    pub service_clients: Vec<ServicePortIr>,
    pub service_servers: Vec<ServicePortIr>,
    #[serde(default)]
    pub operation_clients: Vec<OperationPortIr>,
    #[serde(default)]
    pub operation_servers: Vec<OperationPortIr>,
    pub params: Vec<ParamIr>,
    #[serde(default)]
    pub resources: Vec<ResourceRequirementIr>,
    #[serde(default)]
    pub io_boundary: Option<IoBoundaryIr>,
    pub lifecycle: LifecycleSurface,
}

/// 组件的语言侧构建依赖。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentBuildIr {
    #[serde(default)]
    pub pkg_config: Vec<String>,
}

/// 组件声明的资源需求。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRequirementIr {
    pub name: String,
    pub kind: ResourceKind,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<ResourceDescriptorSchemaIr>,
}

/// 资源对普通 channel 暴露的 descriptor schema。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDescriptorSchemaIr {
    pub kind: ResourceDescriptorKind,
    pub port: String,
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub record_payload: bool,
}

/// FlowRT 当前认识的 side-channel descriptor 类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceDescriptorKind {
    Frame,
}

/// FlowRT 认识的 I/O boundary 资源类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Serial,
    Shm,
    Udp,
    File,
    Device,
    Sdk,
}

/// 进程内 I/O boundary 的静态策略。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoBoundaryIr {
    pub side_effects: Vec<IoSideEffect>,
    pub readiness: IoBoundaryReadiness,
    pub health: IoBoundaryHealth,
    pub shutdown: IoBoundaryShutdown,
}

/// I/O boundary 声明的副作用类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IoSideEffect {
    Read,
    Write,
    Network,
    Filesystem,
    Device,
    Compute,
}

/// I/O boundary readiness gate。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IoBoundaryReadiness {
    ComponentStarted,
    ResourceReady,
}

/// I/O boundary health 来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IoBoundaryHealth {
    RuntimeReported,
    ProcessStatus,
}

/// I/O boundary 关闭策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IoBoundaryShutdown {
    Cooperative,
    BestEffort,
}

/// 组件端口声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortIr {
    pub name: String,
    pub ty: crate::TypeExpr,
}

/// 组件 service client/server 端口声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServicePortIr {
    pub name: String,
    pub request: crate::TypeExpr,
    pub response: crate::TypeExpr,
}

/// 组件 operation client/server 端口声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationPortIr {
    pub name: String,
    pub goal: crate::TypeExpr,
    pub feedback: crate::TypeExpr,
    pub result: crate::TypeExpr,
}

/// 组件参数声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamIr {
    pub name: String,
    pub ty: ParamType,
    pub default: ParamValue,
    pub update: ParamUpdatePolicy,
    pub min: Option<ParamValue>,
    pub max: Option<ParamValue>,
    pub choices: Vec<ParamValue>,
}

/// graph 中的组件实例。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstanceIr {
    pub id: EntityId,
    pub name: String,
    pub component: EntityRef,
    pub params: Vec<ParamValueIr>,
    pub process: Option<String>,
    pub target: Option<EntityRef>,
}

/// 实例级参数值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamValueIr {
    pub name: String,
    pub value: ParamValue,
}

/// 参数 schema 支持的标量类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    Bool,
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    String,
    Array,
    Table,
}

/// 参数运行时更新策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamUpdatePolicy {
    Startup,
    OnTick,
}

/// 归一化后的 dataflow graph。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphIr {
    pub id: EntityId,
    pub name: String,
    pub instances: Vec<InstanceIr>,
    pub processes: Vec<ProcessIr>,
    #[serde(default)]
    pub external_processes: Vec<ExternalProcessIr>,
    pub tasks: Vec<TaskIr>,
    pub binds: Vec<ChannelEdgeIr>,
    pub services: Vec<ServiceEdgeIr>,
    #[serde(default)]
    pub operations: Vec<OperationEdgeIr>,
    pub ros2_bridges: Vec<Ros2BridgeIr>,
}

/// graph 级运行进程编排策略。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcessIr {
    pub name: String,
    pub depends_on: Vec<String>,
    pub restart: ProcessRestartPolicy,
    pub failure_propagation: ProcessFailurePropagation,
    pub readiness: ProcessReadinessGate,
    pub startup_delay_ms: u64,
    pub env: BTreeMap<String, String>,
    pub cpu_affinity: Vec<u32>,
    pub nice: Option<i32>,
    pub rt_policy: Option<RtPolicy>,
    pub rt_priority: Option<u32>,
}

/// 由外部 package/executable 提供的 graph process。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalProcessIr {
    pub process: String,
    pub package: String,
    pub executable: String,
    pub args: Vec<String>,
    pub working_dir: ExternalWorkingDir,
    pub health: ExternalHealthKind,
    pub required_backends: Vec<BackendName>,
}

/// external process 的工作目录策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalWorkingDir {
    Package,
    Workspace,
}

/// external process 的健康检查方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalHealthKind {
    ProcessStarted,
    RuntimeSocket,
}

/// 进程 readiness gate 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessReadinessGate {
    ProcessStarted,
    RuntimeReady,
    ServiceReady,
}

/// Linux 实时调度策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RtPolicy {
    Fifo,
    RoundRobin,
}

/// 进程重启策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessRestartPolicy {
    pub policy: ProcessRestartPolicyKind,
    pub max_restarts: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
}

/// 进程异常退出后的重启类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRestartPolicyKind {
    Never,
    OnFailure,
    Always,
}

/// 当前进程失败时 supervisor 是否向依赖它的进程传播故障。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessFailurePropagation {
    Propagate,
    Isolate,
}

/// 两个 service 端口之间的 request/response bind。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceEdgeIr {
    pub id: EntityId,
    pub client: ServicePortRef,
    pub server: ServicePortRef,
    pub backend: BackendName,
    pub backend_source: ServiceBackendSource,
    pub policy: ServicePolicyIr,
    pub policy_source: ServicePolicySourceIr,
}

/// service 端口引用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePortRef {
    pub instance: EntityRef,
    pub port: String,
}

/// service backend 字段来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceBackendSource {
    /// 用户在 RSDL 中显式指定了 backend。
    Explicit,
    /// 由 auto resolver 根据拓扑自动选择。
    AutoResolved,
}

/// service overflow 行为：队列满时的处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceOverflowPolicy {
    /// 返回 busy 错误，不阻塞调用方。
    Busy,
    /// 返回 error 错误。
    Error,
}

/// 归一化后的 service policy。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServicePolicyIr {
    pub timeout_ms: u64,
    pub queue_depth: u32,
    pub overflow: ServiceOverflowPolicy,
    pub lane: Option<String>,
    pub max_in_flight: u32,
}

/// service policy 各字段是否来自显式 bind 声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePolicySourceIr {
    pub backend: PolicyValueSource,
    pub timeout_ms: PolicyValueSource,
    pub queue_depth: PolicyValueSource,
    pub overflow: PolicyValueSource,
    pub lane: PolicyValueSource,
    pub max_in_flight: PolicyValueSource,
}

/// 两个 operation 端口之间的 typed long-running command bind。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationEdgeIr {
    pub id: EntityId,
    pub client: OperationPortRef,
    pub server: OperationPortRef,
    pub backend: BackendName,
    pub backend_source: OperationBackendSource,
    pub policy: OperationPolicyIr,
    pub policy_source: OperationPolicySourceIr,
}

/// operation 端口引用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationPortRef {
    pub instance: EntityRef,
    pub port: String,
}

/// operation backend 字段来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationBackendSource {
    Explicit,
    AutoResolved,
}

/// operation 并发策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationConcurrencyPolicy {
    Reject,
    Queue,
}

/// operation 抢占策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPreemptPolicy {
    Reject,
    CancelRunning,
}

/// operation feedback 保留策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationFeedbackPolicy {
    Latest,
    Fifo,
}

/// 归一化后的 operation policy。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationPolicyIr {
    pub timeout_ms: u64,
    pub concurrency: OperationConcurrencyPolicy,
    pub preempt: OperationPreemptPolicy,
    pub queue_depth: u32,
    pub max_in_flight: u32,
    pub feedback: OperationFeedbackPolicy,
    pub result_retention_ms: u64,
}

/// operation policy 各字段是否来自显式 bind 声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationPolicySourceIr {
    pub backend: PolicyValueSource,
    pub timeout_ms: PolicyValueSource,
    pub concurrency: PolicyValueSource,
    pub preempt: PolicyValueSource,
    pub queue_depth: PolicyValueSource,
    pub max_in_flight: PolicyValueSource,
    pub feedback: PolicyValueSource,
    pub result_retention_ms: PolicyValueSource,
}

/// FlowRT 与 ROS2 的静态桥接声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ros2BridgeIr {
    pub id: EntityId,
    pub name: String,
    pub flowrt: PortRef,
    pub ros2_topic: String,
    pub ros2_type: String,
    pub direction: Ros2BridgeDirection,
    pub field: String,
    pub backend: BackendName,
}

/// ROS2 bridge 方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ros2BridgeDirection {
    FlowrtToRos2,
    Ros2ToFlowrt,
}

/// instance 的执行单元。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskIr {
    pub id: EntityId,
    pub name: String,
    pub instance: EntityRef,
    pub trigger: TriggerKind,
    pub readiness: TaskReadiness,
    pub period_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
    pub lane: Option<String>,
    pub priority: Option<u32>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

/// 两个端口之间的 typed channel edge。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelEdgeIr {
    pub id: EntityId,
    pub from: PortRef,
    pub to: PortRef,
    pub backend: BackendName,
    pub backend_policy_source: PolicyValueSource,
    pub backend_source: ChannelBackendSource,
    pub channel: ChannelKind,
    pub depth: Option<u32>,
    pub overflow: OverflowPolicy,
    pub stale: StalePolicy,
    pub max_age_ms: Option<u64>,
    pub policy_source: ChannelPolicySourceIr,
    pub capability_requirements: Vec<CapabilityAtom>,
}

/// route backend 字段来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelBackendSource {
    Explicit,
    ProfileDefault,
    AutoFallback,
}

/// channel policy 字段的来源，用于 profile 投影时只重算默认项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyValueSource {
    Explicit,
    ProfileDefault,
}

/// channel policy 各字段是否来自显式 bind 声明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelPolicySourceIr {
    pub overflow: PolicyValueSource,
    pub stale: PolicyValueSource,
    pub max_age_ms: PolicyValueSource,
}

/// 端口引用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortRef {
    pub instance: EntityRef,
    pub port: String,
}

/// 运行 profile。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileIr {
    pub id: EntityId,
    pub name: String,
    pub backend: BackendName,
    pub scheduler: SchedulerDefaults,
    pub defaults: PolicyDefaults,
}

/// profile 级 scheduler 默认值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchedulerDefaults {
    pub worker_threads: u32,
}

/// profile 级 channel policy 默认值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyDefaults {
    pub default_overflow: OverflowPolicy,
    pub default_stale_policy: StalePolicy,
    pub max_age_ms: Option<u64>,
}

/// 部署目标声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetIr {
    pub id: EntityId,
    pub name: String,
    pub platform: Option<TargetPlatform>,
    pub runtime: Vec<LanguageKind>,
    pub backends: Vec<BackendName>,
    pub capabilities: Vec<CapabilityAtom>,
}

/// FlowRT 当前支持的部署目标平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetPlatform {
    LinuxAmd64,
    LinuxArm64,
}

impl TargetPlatform {
    pub fn parse_alias(value: &str) -> Option<Self> {
        match value {
            "linux-amd64" | "linux-x86_64" => Some(Self::LinuxAmd64),
            "linux-arm64" | "linux-aarch64" => Some(Self::LinuxArm64),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "linux-amd64",
            Self::LinuxArm64 => "linux-arm64",
        }
    }
}

impl fmt::Display for TargetPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for TargetPlatform {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TargetPlatform {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse_alias(&value).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "unsupported target platform `{value}`; expected `linux-amd64` or `linux-arm64`"
            ))
        })
    }
}

/// graph、profile、target 和 backend 组合后的部署结果。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeploymentIr {
    pub id: EntityId,
    pub graph: EntityRef,
    pub profile: EntityRef,
    pub target: EntityRef,
    pub backend: BackendName,
    pub required_capabilities: Vec<CapabilityAtom>,
    pub satisfied: bool,
}

/// backend 名称。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BackendName(pub String);

/// backend capability atom。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CapabilityAtom(pub String);

/// 参数值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<ParamValue>),
    Table(BTreeMap<String, ParamValue>),
}

/// 组件实现语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageKind {
    Cpp,
    Rust,
    External,
}

/// 组件接入类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Native,
    IoBoundary,
    External,
}

/// task 触发类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Periodic,
    OnMessage,
    Startup,
    Shutdown,
}

/// on_message task 的 readiness 聚合语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskReadiness {
    AnyReady,
    AllReady,
}

/// channel 语义类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Latest,
    Fifo,
}

/// channel overflow policy。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicy {
    DropOldest,
    DropNewest,
    Error,
    Block,
}

/// stale data policy。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StalePolicy {
    Warn,
    Drop,
    HoldLast,
    Error,
}

/// 组件生命周期接口面。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleSurface {
    pub on_init: bool,
    pub on_start: bool,
    pub on_stop: bool,
    pub on_shutdown: bool,
}

impl LifecycleSurface {
    /// 返回 v0.1 预留的最小生命周期接口面。
    pub fn reserved_v0_1() -> Self {
        Self {
            on_init: true,
            on_start: true,
            on_stop: true,
            on_shutdown: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CONTRACT_IR_VERSION, CONTRACT_SCHEMA_VERSION, RSDL_VERSION};

    fn minimal_contract() -> ContractIr {
        ContractIr {
            ir_version: CONTRACT_IR_VERSION.to_string(),
            schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
            source_hash: "sha256:test".to_string(),
            package_id: EntityId("package:demo".to_string()),
            package: PackageIr {
                name: "demo".to_string(),
                version: None,
                rsdl_version: RSDL_VERSION.to_string(),
                imports: Vec::new(),
            },
            modules: Vec::new(),
            types: Vec::new(),
            components: Vec::new(),
            graphs: Vec::new(),
            profiles: Vec::new(),
            targets: Vec::new(),
            deployments: Vec::new(),
        }
    }

    #[test]
    fn from_json_str_rejects_unknown_top_level_field() {
        let mut value = serde_json::to_value(minimal_contract()).unwrap();
        value["unexpected"] = serde_json::json!(true);
        let source = serde_json::to_string(&value).unwrap();

        let error = ContractIr::from_json_str(&source).expect_err("unknown field must fail");

        assert!(
            error.to_string().contains("unknown Contract IR field"),
            "unexpected error: {error}"
        );
        assert!(
            error.to_string().contains("unexpected"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn from_json_str_rejects_unknown_nested_field() {
        let mut value = serde_json::to_value(minimal_contract()).unwrap();
        value["package"]["unexpected"] = serde_json::json!("bad");
        let source = serde_json::to_string(&value).unwrap();

        let error = ContractIr::from_json_str(&source).expect_err("nested unknown field must fail");

        assert!(
            error.to_string().contains("unknown Contract IR field"),
            "unexpected error: {error}"
        );
        assert!(
            error.to_string().contains("package"),
            "unexpected error: {error}"
        );
        assert!(
            error.to_string().contains("unexpected"),
            "unexpected error: {error}"
        );
    }
}
