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
    #[serde(default, skip_serializing_if = "ContractArtifactIr::is_default")]
    pub artifact: ContractArtifactIr,
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

/// runtime 时间基准来源。
///
/// 标记 generated scheduler 用哪种时钟推进调度：`Realtime` 由 runtime monotonic 时间驱动；
/// `SimulatedReplay` 由注入事件 / fixture 的逻辑毫秒驱动，使调度结果不受回放物理快慢影响；
/// `ExternalStepped` 由外部 stepper 推进逻辑时钟（保留长期模型边界，v0.16.0 暂不支持）。
/// clock source 是 normalization 派生事实：validator 必须按 `temporary_overlay` 重新推导并
/// 拒绝不一致或暂不支持的取值，不能信任落盘 IR 中可被手工改写的来源。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockSourceKind {
    /// runtime monotonic 实时时钟。
    #[default]
    Realtime,
    /// 注入事件 / fixture 驱动的模拟回放逻辑时钟。
    SimulatedReplay,
    /// 外部 stepper 推进的逻辑时钟（保留扩展边界，v0.16.0 暂不支持）。
    ExternalStepped,
}

impl ClockSourceKind {
    /// 判断是否为实时时钟来源；用于 canonical JSON 跳过默认值。
    pub fn is_realtime(&self) -> bool {
        matches!(self, ClockSourceKind::Realtime)
    }

    /// 返回 clock source 的 canonical 名称，作为 validator、codegen 和 self-description 共享的
    /// 唯一字符串事实源。
    pub fn label(&self) -> &'static str {
        match self {
            ClockSourceKind::Realtime => "realtime",
            ClockSourceKind::SimulatedReplay => "simulated_replay",
            ClockSourceKind::ExternalStepped => "external_stepped",
        }
    }
}

/// 当前 Contract IR 产物的使用边界。
///
/// 常规 normalized IR 默认为 strict、非测试产物；temporary island overlay 会把这里标记为
/// test-only island，让 self-description、launch manifest、bundle/deploy gate 共享同一事实源。
/// `clock_source` 把调度时钟来源提升为 IR 一等事实，取代 codegen 散落的 `temporary_overlay`
/// 推断；它由 normalization 派生、validator 重新校验。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractArtifactIr {
    #[serde(default)]
    pub mode: GraphMode,
    #[serde(default, skip_serializing_if = "is_false")]
    pub temporary_island: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub test_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporary_overlay: Option<TemporaryOverlayIr>,
    #[serde(default, skip_serializing_if = "ClockSourceKind::is_realtime")]
    pub clock_source: ClockSourceKind,
}

impl ContractArtifactIr {
    pub fn is_default(&self) -> bool {
        self.mode == GraphMode::Strict
            && !self.temporary_island
            && !self.test_only
            && self.temporary_overlay.is_none()
            && self.clock_source.is_realtime()
    }
}

/// temporary overlay 产物来源和边界映射元数据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporaryOverlayIr {
    pub kind: String,
    pub original_profile_mode: GraphMode,
    pub generated_by: TemporaryOverlayGenerationIr,
    #[serde(default)]
    pub boundary_mappings: Vec<TemporaryOverlayBoundaryMappingIr>,
}

/// temporary overlay 生成来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporaryOverlayGenerationIr {
    pub command: String,
    pub source: String,
}

impl Default for TemporaryOverlayGenerationIr {
    fn default() -> Self {
        Self {
            command: "flowrt prepare".to_string(),
            source: "cli".to_string(),
        }
    }
}

/// temporary overlay 的单条 boundary 映射来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporaryOverlayBoundaryMappingIr {
    pub direction: BoundaryDirection,
    pub name: String,
    pub endpoint: String,
    pub source: String,
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
    #[serde(default, skip_serializing_if = "is_false")]
    pub empty: bool,
    pub fields: Vec<FieldIr>,
    /// sample-time 源（sensor event-time），归一化自 `[type.<Name>.timestamp]`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<TimestampSourceIr>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// sample-time 源声明：消息中承载 sample 时间戳的字段及其时钟语义。
///
/// 这是 sensor event-time 的建模基线：回放、同步与调度从用户消息字段读取 sample-time，
/// 不引入隐藏 wall-clock 或外部时间源。`field` 必须指向本消息的一个 unsigned 整数标量字段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimestampSourceIr {
    /// 承载 sample-time 的消息字段名。
    pub field: String,
    /// 时间单位。
    pub unit: TimestampUnit,
    /// 时间基准。
    pub epoch: TimestampEpoch,
    /// 所属逻辑时钟域名。
    pub clock_domain: String,
}

/// sample-time 时间单位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampUnit {
    Ns,
    Us,
    Ms,
}

/// sample-time 时间基准。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampEpoch {
    Monotonic,
    Unix,
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
    pub concurrency: TaskConcurrency,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_concurrency: Option<TaskConcurrency>,
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
    pub id: EntityId,
    pub name: String,
    pub capability: CapabilityAtom,
    #[serde(default)]
    pub access: ResourceAccess,
    pub required: bool,
    #[serde(default)]
    pub readiness: ResourceReadinessGate,
    #[serde(default)]
    pub health: ResourceHealthPolicy,
    #[serde(default)]
    pub on_failure: ResourceFailurePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<ResourceDescriptorSchemaIr>,
}

/// 组件对资源的访问方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceAccess {
    Read,
    Write,
    #[default]
    ReadWrite,
    Exclusive,
}

/// resource readiness gate。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceReadinessGate {
    BeforeInit,
    #[default]
    BeforeStart,
    Lazy,
}

/// resource health 对系统健康的影响。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceHealthPolicy {
    #[default]
    Required,
    Optional,
    Ignored,
}

/// resource 故障传播策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceFailurePolicy {
    #[default]
    StopProcess,
    RestartProcess,
    Degrade,
    StopGraph,
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

/// 实例故障策略。0.21.1 放行 `FailFast`/`Isolate`/`Restart`；`Degrade` 仍为保留值
/// （validator 拒绝），留待 0.21.2 降级数据语义切片实现。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum InstanceFailurePolicy {
    /// 故障即按既有逆序清理停机（今天的行为）。
    #[default]
    FailFast,
    /// 进程内隔离故障 instance：停其全部 task，图继续跑，不恢复。
    Isolate,
    /// 进程内重启故障 instance：先隔离，再按退避重跑 `on_init`/`on_start`（同一对象）。
    Restart,
    /// 保留：降级续跑（0.21.2 起可达）。
    Degrade,
}

/// 进程内重启参数，字段镜像 supervisor `ProcessRestartPolicy`，便于进程内/进程间语义对齐。
///
/// 仅 `InstanceFailurePolicy::Restart` 携带。退避按 clock-ms 度量：首次 `initial_delay_ms`，
/// 每次连续失败翻倍封顶 `max_delay_ms`；连续失败达 `max_restarts` 进入终态 `Faulted`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceRestartParamsIr {
    pub max_restarts: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
}

/// 进程内重启默认参数，对齐 supervisor `DEFAULT_RESTART_POLICY`（OnFailure/3/100/1000）。
pub const DEFAULT_INSTANCE_RESTART: InstanceRestartParamsIr = InstanceRestartParamsIr {
    max_restarts: 3,
    initial_delay_ms: 100,
    max_delay_ms: 1000,
};

/// 实例故障处理合同：策略 + 仅 restart 时的退避参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct InstanceFaultPolicyIr {
    pub policy: InstanceFailurePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<InstanceRestartParamsIr>,
}

impl InstanceFaultPolicyIr {
    /// 默认合同（fail_fast、无重启参数）；用于 canonical JSON 跳过整段。
    pub fn is_default(&self) -> bool {
        self.policy == InstanceFailurePolicy::FailFast && self.restart.is_none()
    }
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
    #[serde(default, skip_serializing_if = "InstanceFaultPolicyIr::is_default")]
    pub fault: InstanceFaultPolicyIr,
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

/// 图级 health 反应策略：图内出现终态不可恢复故障时的图级动作。0.21.3 起放行。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GraphFaultReaction {
    /// 续跑：终态故障保持隔离，图继续运行（今天的行为）。
    #[default]
    Continue,
    /// 受控停机：首个终态故障触发图 graceful 停机（逆序清理 on_stop/on_shutdown）。
    Stop,
}

/// 图级 health 反应合同。聚合每实例 lifecycle 后据此驱动图级动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GraphHealthPolicyIr {
    pub on_faulted: GraphFaultReaction,
}

impl GraphHealthPolicyIr {
    /// 默认合同（continue）；用于 canonical JSON 跳过整段。
    pub fn is_default(&self) -> bool {
        self.on_faulted == GraphFaultReaction::Continue
    }
}

/// 归一化后的 dataflow graph。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphIr {
    pub id: EntityId,
    pub name: String,
    #[serde(default, skip_serializing_if = "GraphHealthPolicyIr::is_default")]
    pub health: GraphHealthPolicyIr,
    pub instances: Vec<InstanceIr>,
    pub processes: Vec<ProcessIr>,
    #[serde(default)]
    pub external_processes: Vec<ExternalProcessIr>,
    #[serde(default)]
    pub resource_providers: Vec<ResourceProviderIr>,
    #[serde(default)]
    pub resource_satisfactions: Vec<ResourceSatisfactionIr>,
    pub tasks: Vec<TaskIr>,
    pub binds: Vec<ChannelEdgeIr>,
    pub services: Vec<ServiceEdgeIr>,
    #[serde(default)]
    pub operations: Vec<OperationEdgeIr>,
    #[serde(default)]
    pub boundary_endpoints: Vec<BoundaryEndpointIr>,
    pub ros2_bridges: Vec<Ros2BridgeIr>,
    #[serde(default)]
    pub sync_groups: Vec<SyncGroupIr>,
}

/// graph 级抽象 resource provider。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceProviderIr {
    pub id: EntityId,
    pub name: String,
    pub capabilities: Vec<CapabilityAtom>,
    pub scope: ResourceProviderScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<EntityRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_package: Option<String>,
    pub health_source: String,
    pub readiness_source: String,
}

/// resource provider 的作用域。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceProviderScope {
    Target,
    Process,
    ExternalPackage,
}

/// graph 中某个 instance requirement 的派生满足状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceSatisfactionIr {
    pub id: EntityId,
    pub instance: EntityRef,
    pub component: EntityRef,
    pub requirement: EntityRef,
    pub resource: String,
    pub capability: CapabilityAtom,
    pub access: ResourceAccess,
    pub required: bool,
    pub readiness: ResourceReadinessGate,
    pub health: ResourceHealthPolicy,
    pub on_failure: ResourceFailurePolicy,
    pub satisfied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<EntityRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}

/// graph 级 typed boundary endpoint。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundaryEndpointIr {
    pub id: EntityId,
    pub name: String,
    pub direction: BoundaryDirection,
    pub port: PortRef,
    pub ty: crate::TypeExpr,
}

/// boundary endpoint 方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryDirection {
    Input,
    Output,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boundary_endpoint: Option<EntityRef>,
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
    #[serde(default)]
    pub concurrency: TaskConcurrency,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_concurrency: Option<TaskConcurrency>,
    pub readiness: TaskReadiness,
    pub period_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
    pub lane: Option<String>,
    pub priority: Option<u32>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_group: Option<EntityRef>,
}

/// 多传感器同步组：按 event-time（0.18.0 sample-time）把 N 路输入对齐成同步集，供
/// `on_synchronized` task 消费。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncGroupIr {
    pub id: EntityId,
    pub name: String,
    pub instance: EntityRef,
    pub inputs: Vec<String>,
    pub tolerance_ms: u64,
    #[serde(default)]
    pub late_policy: SyncLatePolicy,
}

/// 同步组迟到样本策略。v1 仅 `DropLate`（丢弃早于已发窗口的样本）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncLatePolicy {
    #[default]
    DropLate,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_affinity: Option<BackendThreadAffinity>,
    pub channel: ChannelKind,
    pub depth: Option<u32>,
    pub overflow: OverflowPolicy,
    pub stale: StalePolicy,
    pub max_age_ms: Option<u64>,
    pub policy_source: ChannelPolicySourceIr,
    pub capability_requirements: Vec<CapabilityAtom>,
    /// 反馈边：true 表示这是一条单位延迟回边（z⁻¹）。拓扑排序剔除它以断环；
    /// channel 构造期播种零初值，消费者读到的是上一 tick 的上游输出。
    #[serde(default)]
    pub feedback: bool,
    /// 反馈边初值：源消息字面量（`ParamValue::Table`）。`None` 表示零初值。
    /// fifo 反馈边按 depth 拍延迟，每拍均以该初值播种。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init: Option<ParamValue>,
}

/// route backend 字段来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelBackendSource {
    Explicit,
    ProfileDefault,
    AutoFallback,
}

/// route backend 的线程亲和事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendThreadAffinity {
    /// backend endpoint 可以由 worker 线程直接持有或调用。
    SendSafe,
    /// transport commit 必须留在 scheduler/local owner 线程。
    SchedulerLocalCommit,
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
    pub mode: GraphMode,
    pub backend: BackendName,
    pub scheduler: SchedulerDefaults,
    pub defaults: PolicyDefaults,
}

/// profile 选择的 graph 完整性模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphMode {
    #[default]
    Strict,
    Island,
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
    C,
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

/// component/task 执行单元的并发语义。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskConcurrency {
    #[default]
    Exclusive,
    Parallel,
}

/// task 触发类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Periodic,
    OnMessage,
    Startup,
    Shutdown,
    OnSynchronized,
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
            artifact: Default::default(),
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
    fn clock_source_realtime_omitted_and_simulated_replay_roundtrips() {
        // 默认 realtime artifact 整体不写入 canonical JSON。
        let realtime = minimal_contract();
        let json = realtime.to_canonical_json().unwrap();
        assert!(
            !json.contains("clock_source"),
            "default realtime clock source must be omitted: {json}"
        );

        // simulated_replay 必须落盘并 round-trip 回 IR。
        let mut replay = minimal_contract();
        replay.artifact.clock_source = ClockSourceKind::SimulatedReplay;
        let json = replay.to_canonical_json().unwrap();
        assert!(
            json.contains("\"clock_source\": \"simulated_replay\""),
            "simulated_replay clock source must serialize: {json}"
        );
        let parsed = ContractIr::from_json_str(&json).unwrap();
        assert_eq!(
            parsed.artifact.clock_source,
            ClockSourceKind::SimulatedReplay
        );
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
    fn instance_fault_policy_default_omitted_and_restart_roundtrips() {
        let default = InstanceFaultPolicyIr::default();
        assert!(default.is_default());
        let restart = InstanceFaultPolicyIr {
            policy: InstanceFailurePolicy::Restart,
            restart: Some(DEFAULT_INSTANCE_RESTART),
        };
        assert!(!restart.is_default());
        let json = serde_json::to_string(&restart).unwrap();
        let parsed: InstanceFaultPolicyIr = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, restart);
        // 默认整段在 InstanceIr skip_serializing_if 下不写入；isolate 无 restart 段。
        let isolate = InstanceFaultPolicyIr {
            policy: InstanceFailurePolicy::Isolate,
            restart: None,
        };
        assert!(!isolate.is_default());
        assert!(!serde_json::to_string(&isolate).unwrap().contains("restart"));
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
