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
    pub processes: Vec<RawProcess>,
    pub binds: Vec<RawDataflowBind>,
    pub service_binds: Vec<RawServiceBind>,
    pub operation_binds: Vec<RawOperationBind>,
    pub ros2_bridges: Vec<RawRos2Bridge>,
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
    pub processes: Vec<RawProcess>,
    pub binds: Vec<RawDataflowBind>,
    pub service_binds: Vec<RawServiceBind>,
    pub operation_binds: Vec<RawOperationBind>,
    pub ros2_bridges: Vec<RawRos2Bridge>,
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
    pub fields: Vec<RawField>,
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
    pub input: Vec<RawPort>,
    pub output: Vec<RawPort>,
    pub service_clients: Vec<RawServicePort>,
    pub service_servers: Vec<RawServicePort>,
    pub operation_clients: Vec<RawOperationPort>,
    pub operation_servers: Vec<RawOperationPort>,
    pub params: BTreeMap<String, RawValue>,
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
}

/// `[instance.<name>.task]` 或 `[[instance.<name>.task]]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTask {
    pub name: Option<String>,
    pub trigger: String,
    pub readiness: Option<String>,
    pub period_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
    pub lane: Option<String>,
    pub priority: Option<u32>,
    pub input: Vec<String>,
    pub output: Vec<String>,
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

/// `[[bind.dataflow]]` 表项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawDataflowBind {
    pub from: String,
    pub to: String,
    pub backend: Option<String>,
    pub channel: String,
    pub depth: Option<u32>,
    pub overflow: Option<String>,
    pub stale_policy: Option<String>,
    pub max_age_ms: Option<u64>,
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

/// `[profile.<name>]` 表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProfile {
    pub backend: Option<String>,
    pub worker_threads: Option<u32>,
    pub default_overflow: Option<String>,
    pub default_stale_policy: Option<String>,
    pub max_age_ms: Option<u64>,
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
