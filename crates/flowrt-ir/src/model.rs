use std::collections::BTreeMap;

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
        serde_json::from_str(source)
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

/// 消息类型声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeIr {
    pub id: EntityId,
    pub name: String,
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
    pub name: String,
    pub language: LanguageKind,
    pub kind: ComponentKind,
    pub inputs: Vec<PortIr>,
    pub outputs: Vec<PortIr>,
    pub params: Vec<ParamIr>,
    pub lifecycle: LifecycleSurface,
}

/// 组件端口声明。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortIr {
    pub name: String,
    pub ty: crate::TypeExpr,
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
    pub tasks: Vec<TaskIr>,
    pub binds: Vec<ChannelEdgeIr>,
    pub ros2_bridges: Vec<Ros2BridgeIr>,
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
}

/// instance 的执行单元。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskIr {
    pub id: EntityId,
    pub name: String,
    pub instance: EntityRef,
    pub trigger: TriggerKind,
    pub period_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
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
    pub defaults: PolicyDefaults,
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
    pub platform: Option<String>,
    pub runtime: Vec<LanguageKind>,
    pub backends: Vec<BackendName>,
    pub capabilities: Vec<CapabilityAtom>,
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
}

/// 组件接入类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Native,
    Adapter,
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
