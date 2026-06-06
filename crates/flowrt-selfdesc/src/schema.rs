//! FlowRT self-description 共享 schema 类型。
//!
//! CLI、codegen 和 runtime 复用这些 serde 类型读写 `selfdesc.json` 或嵌入式
//! `.flowrt.selfdesc` section。未知字段由 serde 默认忽略，保证前向兼容。

use serde::{Deserialize, Serialize};

/// self-description schema 版本。
pub const SELF_DESCRIPTION_SCHEMA_VERSION: &str = "0.1";

/// `.flowrt.selfdesc` ELF/PE/Mach-O section 名称。
pub const SELF_DESCRIPTION_SECTION: &str = ".flowrt.selfdesc";

/// self-description 顶层结构。
///
/// 字段集合是 codegen 输出与 CLI 读取的超集；codegen 必填字段在 CLI 侧用
/// `#[serde(default)]` 降级，保证旧版 JSON 不报错。未来 service/operation 观测
/// 扩展字段同样用 `serde(default)` 预留。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescription {
    pub self_description_version: String,
    #[serde(default)]
    pub ir_version: String,
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub source_hash: String,
    #[serde(default)]
    pub package: SelfDescriptionPackage,
    #[serde(default)]
    pub profiles: Vec<SelfDescriptionProfile>,
    #[serde(default)]
    pub targets: Vec<SelfDescriptionTarget>,
    #[serde(default)]
    pub deployments: Vec<SelfDescriptionDeployment>,
    #[serde(default)]
    pub graphs: Vec<SelfDescriptionGraph>,
    #[serde(default)]
    pub message_abi: Vec<SelfDescriptionMessageAbi>,
    #[serde(default)]
    pub message_frames: Vec<SelfDescriptionMessageFrame>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelfDescriptionPackage {
    #[serde(default)]
    pub name: String,
    pub version: Option<String>,
    #[serde(default)]
    pub rsdl_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionProfile {
    pub name: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionTarget {
    pub name: String,
    pub platform: Option<String>,
    #[serde(default)]
    pub runtimes: Vec<String>,
    #[serde(default)]
    pub backends: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionDeployment {
    pub graph: String,
    pub profile: String,
    pub target: String,
    pub backend: String,
    #[serde(default)]
    pub satisfied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionGraph {
    pub name: String,
    #[serde(default)]
    pub scheduler: SelfDescriptionScheduler,
    #[serde(default)]
    pub instances: Vec<SelfDescriptionInstance>,
    #[serde(default)]
    pub tasks: Vec<SelfDescriptionTask>,
    #[serde(default)]
    pub channels: Vec<SelfDescriptionChannel>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelfDescriptionScheduler {
    #[serde(default)]
    pub worker_threads: u32,
    #[serde(default)]
    pub lanes: Vec<SelfDescriptionSchedulerLane>,
    #[serde(default)]
    pub tasks: Vec<SelfDescriptionSchedulerTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionSchedulerLane {
    pub name: String,
    pub kind: String,
    pub instance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionSchedulerTask {
    pub name: String,
    pub instance: String,
    pub lane: String,
    pub trigger: String,
    #[serde(default)]
    pub readiness: String,
    pub period_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
    pub priority: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionInstance {
    pub name: String,
    pub component: String,
    #[serde(default = "default_process")]
    pub process: String,
    pub target: Option<String>,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub params: Vec<SelfDescriptionParam>,
}

fn default_process() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionParam {
    pub name: String,
    #[serde(rename = "type", default)]
    pub ty: String,
    #[serde(default)]
    pub update: String,
    #[serde(default)]
    pub current: serde_json::Value,
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
    #[serde(default)]
    pub choices: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionTask {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub instance: String,
    #[serde(default)]
    pub trigger: String,
    #[serde(default)]
    pub readiness: String,
    pub period_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
    #[serde(default)]
    pub lane: String,
    pub priority: Option<u32>,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionChannel {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub message_type: String,
    #[serde(default)]
    pub backend: String,
    /// v0.4+ service endpoint 名称。
    #[serde(default)]
    pub service: Option<String>,
    /// v0.4+ zenoh key expression。
    #[serde(default)]
    pub key_expr: Option<String>,
    #[serde(default)]
    pub channel: String,
    pub depth: Option<u32>,
    #[serde(default)]
    pub overflow: String,
    #[serde(default)]
    pub stale_policy: String,
    pub max_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionMessageAbi {
    pub type_name: String,
    pub size_bytes: usize,
    /// 字节对齐；旧版 JSON 可能缺少此字段。
    #[serde(default)]
    pub align_bytes: usize,
    #[serde(default)]
    pub fields: Vec<SelfDescriptionFieldAbi>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionFieldAbi {
    pub name: String,
    #[serde(rename = "type", default)]
    pub ty: String,
    pub offset_bytes: usize,
    pub size_bytes: usize,
    #[serde(default)]
    pub align_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionMessageFrame {
    pub type_name: String,
    #[serde(default = "default_encoding")]
    pub encoding: String,
    #[serde(default)]
    pub header_size_bytes: usize,
    pub max_size_bytes: Option<usize>,
    #[serde(default)]
    pub variable: bool,
    #[serde(default)]
    pub fields: Vec<SelfDescriptionFrameField>,
}

fn default_encoding() -> String {
    "canonical_frame_v1".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionFrameField {
    pub name: String,
    #[serde(rename = "type", default)]
    pub ty: String,
    pub header_offset_bytes: usize,
    pub header_size_bytes: usize,
    pub tail_max_bytes: Option<usize>,
}
