use std::collections::BTreeMap;
use std::path::PathBuf;

/// 从文件系统加载并展开 imports 后的 RSDL 文档。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedDocument {
    pub document: RawDocument,
    pub sources: Vec<LoadedSource>,
    pub modules: Vec<RawModuleDocument>,
    pub compositions: Vec<RawCompositionDocument>,
}

impl LoadedDocument {
    /// 返回用于 source hash 的规范化 source bundle 文本。
    pub fn source_bundle_text(&self) -> String {
        let mut sources = self.sources.clone();
        sources.sort_by(|left, right| left.path.cmp(&right.path));

        let mut output = String::new();
        for source in sources {
            output.push_str("-- ");
            output.push_str(&source.path.to_string_lossy().replace('\\', "/"));
            output.push_str(" --\n");
            output.push_str(&source.content);
            if !source.content.ends_with('\n') {
                output.push('\n');
            }
        }
        output
    }
}

/// source bundle 中的一个 RSDL 源文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSource {
    pub path: PathBuf,
    pub content: String,
}

/// 一个 module 源文件贡献的符号集合。
#[derive(Debug, Clone, PartialEq)]
pub struct RawModuleDocument {
    pub module: RawModule,
    pub types: BTreeMap<String, RawType>,
    pub components: BTreeMap<String, RawComponent>,
    pub source: PathBuf,
}

/// 一个 composition 源文件贡献的系统装配集合。
#[derive(Debug, Clone, PartialEq)]
pub struct RawCompositionDocument {
    pub instances: BTreeMap<String, RawInstance>,
    pub graph: Option<RawGraph>,
    pub processes: Vec<RawProcess>,
    pub external_processes: Vec<RawExternalProcess>,
    pub resource_providers: Vec<RawResourceProvider>,
    pub binds: Vec<RawDataflowBind>,
    pub service_binds: Vec<RawServiceBind>,
    pub operation_binds: Vec<RawOperationBind>,
    pub ros2_bridges: Vec<RawRos2Bridge>,
    pub boundary_inputs: Vec<RawBoundaryEndpoint>,
    pub boundary_outputs: Vec<RawBoundaryEndpoint>,
    pub sync_groups: Vec<RawSyncGroup>,
    pub redundancy_groups: Vec<RawRedundancyGroup>,
    pub profiles: BTreeMap<String, RawProfile>,
    pub targets: BTreeMap<String, RawTarget>,
    pub source: PathBuf,
}

/// 语义归一化前的 RSDL v0.1 文档。
#[derive(Debug, Clone, PartialEq)]
pub struct RawDocument {
    pub package: RawPackage,
    pub workspace: Option<RawWorkspace>,
    pub types: BTreeMap<String, RawType>,
    pub components: BTreeMap<String, RawComponent>,
    pub instances: BTreeMap<String, RawInstance>,
    pub graph: Option<RawGraph>,
    pub processes: Vec<RawProcess>,
    pub external_processes: Vec<RawExternalProcess>,
    pub resource_providers: Vec<RawResourceProvider>,
    pub binds: Vec<RawDataflowBind>,
    pub service_binds: Vec<RawServiceBind>,
    pub operation_binds: Vec<RawOperationBind>,
    pub ros2_bridges: Vec<RawRos2Bridge>,
    pub boundary_inputs: Vec<RawBoundaryEndpoint>,
    pub boundary_outputs: Vec<RawBoundaryEndpoint>,
    pub sync_groups: Vec<RawSyncGroup>,
    pub redundancy_groups: Vec<RawRedundancyGroup>,
    pub profiles: BTreeMap<String, RawProfile>,
    pub targets: BTreeMap<String, RawTarget>,
}

/// `[package]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPackage {
    pub name: String,
    pub version: Option<String>,
    pub rsdl_version: String,
    pub imports: BTreeMap<String, Vec<String>>,
}

/// `[graph]` 表：graph 级装配策略。当前仅承载 `[graph.health]`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawGraph {
    pub health: Option<RawGraphHealth>,
}

/// `[graph.health]` 表：图级 health 反应策略。取值校验在 IR/validator 阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawGraphHealth {
    /// `on_faulted`：终态故障时图级动作原始字符串（`continue`/`stop`）。
    pub on_faulted: Option<String>,
    /// `critical`：参与 graph critical health 聚合的 instance 名集合；空表示所有 instance。
    pub critical: Vec<String>,
}

/// `[[redundancy.group]]` 表项：graph 级冗余实例组。取值和成员一致性在 IR/validator 阶段校验。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRedundancyGroup {
    pub name: String,
    pub mode: String,
    pub primary: String,
    pub standby: Vec<String>,
    pub trigger: String,
}

/// `[workspace]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawWorkspace {
    pub modules: Vec<String>,
    pub compositions: Vec<String>,
}

/// `[module]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawModule {
    pub name: String,
}

/// `[type.<Name>]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawType {
    pub empty: bool,
    pub fields: Vec<RawField>,
    /// `[type.<Name>.timestamp]` 声明的 sample-time 源（sensor event-time）。
    pub timestamp: Option<RawTimestampSource>,
}

/// `[type.<Name>.timestamp]` 表：声明该消息携带 sample 时间戳的字段与时钟语义。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTimestampSource {
    /// 承载 sample-time 的消息字段名。
    pub field: String,
    /// 时间单位：`ns` / `us` / `ms`，缺省 `ns`。
    pub unit: Option<String>,
    /// 时间基准：`monotonic` / `unix`，缺省 `monotonic`。
    pub epoch: Option<String>,
    /// 所属逻辑时钟域名，缺省 `sensor`。
    pub clock_domain: Option<String>,
}

/// 消息字段声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawField {
    pub name: String,
    pub ty: String,
}

/// `[component.<name>]` 表。
#[derive(Debug, Clone, PartialEq)]
pub struct RawComponent {
    pub language: String,
    pub kind: Option<String>,
    pub concurrency: Option<String>,
    pub build: RawComponentBuild,
    pub input: Vec<RawPort>,
    pub output: Vec<RawPort>,
    pub service_clients: Vec<RawServicePort>,
    pub service_servers: Vec<RawServicePort>,
    pub operation_clients: Vec<RawOperationPort>,
    pub operation_servers: Vec<RawOperationPort>,
    pub params: BTreeMap<String, RawValue>,
    pub io_side_effect: Vec<String>,
    pub io_readiness: Option<String>,
    pub io_health: Option<String>,
    pub io_shutdown: Option<String>,
    pub resources: Vec<RawResourceRequirement>,
}

/// `[component.<name>.build]` 表。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawComponentBuild {
    pub pkg_config: Vec<String>,
}

/// `[component.<name>.resource.<resource_name>]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResourceRequirement {
    pub name: String,
    pub capability: String,
    pub access: Option<String>,
    pub required: bool,
    pub readiness: Option<String>,
    pub health: Option<String>,
    pub on_failure: Option<String>,
    pub descriptor: Option<RawResourceDescriptor>,
}

/// `[[resource.provider]]` 表项，声明抽象 resource capability 的提供者。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResourceProvider {
    pub name: String,
    pub capabilities: Vec<String>,
    pub scope: String,
    pub target: Option<String>,
    pub process: Option<String>,
    pub external_package: Option<String>,
    pub health_source: String,
    pub readiness_source: String,
}

/// `[component.<name>.resource.<resource_name>.descriptor]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawResourceDescriptor {
    pub kind: String,
    pub port: Option<String>,
    pub format: String,
    pub encoding: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub record_payload: bool,
}

/// 组件端口声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPort {
    pub name: String,
    pub ty: String,
}

/// 组件 service client/server 端口声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawServicePort {
    pub name: String,
    pub request: String,
    pub response: String,
}

/// 组件 operation client/server 端口声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawOperationPort {
    pub name: String,
    pub goal: String,
    pub feedback: String,
    pub result: String,
}

/// `[instance.<name>]` 表。
#[derive(Debug, Clone, PartialEq)]
pub struct RawInstance {
    pub component: String,
    pub process: Option<String>,
    pub target: Option<String>,
    pub params: BTreeMap<String, RawValue>,
    pub tasks: Vec<RawTask>,
    /// `instance.<name>.failure_policy`：故障策略原始字符串扁平糖；取值校验在 IR/validator 阶段。
    pub failure_policy: Option<String>,
    /// `[instance.<name>.fault]`：故障处理子表；与扁平 `failure_policy` 互斥（双写在 IR 阶段拒绝）。
    pub fault: Option<RawInstanceFault>,
}

/// `[instance.<name>.fault]` 表。取值/范围/互斥校验在 IR/validator 阶段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInstanceFault {
    pub policy: Option<String>,
    pub max_restarts: Option<u32>,
    pub initial_delay_ms: Option<u64>,
    pub max_delay_ms: Option<u64>,
}

/// `[instance.<name>.task]` 或 `[[instance.<name>.task]]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTask {
    pub name: Option<String>,
    pub trigger: String,
    pub concurrency: Option<String>,
    pub readiness: Option<String>,
    pub period_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
    pub lane: Option<String>,
    pub priority: Option<u32>,
    pub input: Vec<String>,
    pub output: Vec<String>,
    /// `on_synchronized` task 引用的 `[[sync]]` 组名；其余 trigger 必须为空。
    pub sync: Option<String>,
}

/// `[[process]]` 表项，描述 graph 级进程编排策略。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProcess {
    pub name: String,
    pub depends_on: Vec<String>,
    pub restart: Option<String>,
    pub max_restarts: Option<u32>,
    pub initial_delay_ms: Option<u64>,
    pub max_delay_ms: Option<u64>,
    pub failure: Option<String>,
    pub readiness: Option<String>,
    pub startup_delay_ms: Option<u64>,
    pub env: BTreeMap<String, String>,
    pub cpu_affinity: Vec<u32>,
    pub nice: Option<i32>,
    pub rt_policy: Option<String>,
    pub rt_priority: Option<u32>,
}

/// `[[external_process]]` 表项，描述由外部 package/executable 提供的进程。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawExternalProcess {
    pub process: String,
    pub package: String,
    pub executable: String,
    pub args: Vec<String>,
    pub working_dir: Option<String>,
    pub health: Option<String>,
    pub required_backends: Vec<String>,
}

/// `[[bind.dataflow]]` 表项。
#[derive(Debug, Clone, PartialEq)]
pub struct RawDataflowBind {
    pub from: String,
    pub to: String,
    pub backend: Option<String>,
    pub channel: String,
    pub depth: Option<u32>,
    pub overflow: Option<String>,
    pub stale_policy: Option<String>,
    pub max_age_ms: Option<u64>,
    /// 反馈边标记：true 表示这是一条单位延迟回边，允许参与 graph 环路。
    pub feedback: bool,
    /// 反馈边初值：字段名→字面值。省略表示零初值；给出则按源消息类型播种。
    /// fifo 反馈边按 depth 拍延迟，每拍均以该初值播种。
    pub init: Option<BTreeMap<String, RawValue>>,
}

/// `[[sync]]` 表项：声明一个多传感器同步组。
///
/// 将一个 instance 的 ≥2 个输入端口按 sample-time（event-time）对齐成同步集，
/// 经 `on_synchronized` task 投递。语义校验（端口存在、唯一 incoming bind、
/// timestamp 源、tolerance>0）在 validator 完成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSyncGroup {
    pub name: String,
    pub instance: String,
    pub inputs: Vec<String>,
    pub tolerance_ms: Option<u64>,
}

/// `[[bind.service]]` 表项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawServiceBind {
    pub client: String,
    pub server: String,
    pub backend: Option<String>,
    pub timeout_ms: Option<u64>,
    pub queue_depth: Option<u32>,
    pub overflow: Option<String>,
    pub lane: Option<String>,
    pub max_in_flight: Option<u32>,
}

/// `[[bind.operation]]` 表项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawOperationBind {
    pub client: String,
    pub server: String,
    pub backend: Option<String>,
    pub timeout_ms: Option<u64>,
    pub concurrency: Option<String>,
    pub preempt: Option<String>,
    pub queue_depth: Option<u32>,
    pub max_in_flight: Option<u32>,
    pub feedback: Option<String>,
    pub result_retention_ms: Option<u64>,
}

/// `[[bridge.ros2]]` 表项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRos2Bridge {
    pub flowrt: String,
    pub ros2_topic: String,
    pub ros2_type: String,
    pub direction: String,
    pub field: Option<String>,
}

/// graph/profile 使用的拓扑完整性模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RawGraphMode {
    #[default]
    Strict,
    Island,
}

/// `[[boundary.input]]` 或 `[[boundary.output]]` 表项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawBoundaryEndpoint {
    pub name: String,
    pub port: String,
    pub ty: String,
}

/// `[profile.<name>]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProfile {
    pub mode: RawGraphMode,
    pub backend: Option<String>,
    pub worker_threads: Option<u32>,
    pub default_overflow: Option<String>,
    pub default_stale_policy: Option<String>,
    pub max_age_ms: Option<u64>,
    pub determinism: Option<RawProfileDeterminism>,
}

/// `[profile.<name>.determinism]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProfileDeterminism {
    pub mode: Option<String>,
    pub timeout_ms: Option<u64>,
    pub on_timeout: Option<String>,
}

/// `[target.<name>]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTarget {
    pub platform: Option<String>,
    pub runtime: Vec<String>,
    pub backends: Vec<String>,
}

/// component 和 instance 参数表接受的 TOML value 子集。
#[derive(Debug, Clone, PartialEq)]
pub enum RawValue {
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<RawValue>),
    Table(BTreeMap<String, RawValue>),
}
