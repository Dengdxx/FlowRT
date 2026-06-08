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
    /// v0.4+ component 类型声明摘要。
    #[serde(default)]
    pub component_types: Vec<SelfDescriptionComponentType>,
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

/// 可复用组件类型声明摘要。
///
/// 记录 component 的语言、kind 和声明端口（inputs、outputs、service_clients、
/// service_servers、operation_clients、operation_servers、params），让 CLI 在不读
/// RSDL 的情况下展示组件视图。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionComponentType {
    /// 组件类型名称。
    #[serde(default)]
    pub name: String,
    /// 组件所属 runtime 语言。
    #[serde(default)]
    pub language: String,
    /// 组件 kind：native / adapter / external_process。
    #[serde(default)]
    pub kind: String,
    /// 输入端口声明。
    #[serde(default)]
    pub inputs: Vec<SelfDescriptionPortDecl>,
    /// 输出端口声明。
    #[serde(default)]
    pub outputs: Vec<SelfDescriptionPortDecl>,
    /// service client 端口声明。
    #[serde(default)]
    pub service_clients: Vec<SelfDescriptionServicePortDecl>,
    /// service server 端口声明。
    #[serde(default)]
    pub service_servers: Vec<SelfDescriptionServicePortDecl>,
    /// operation client 端口声明。
    #[serde(default)]
    pub operation_clients: Vec<SelfDescriptionOperationPortDecl>,
    /// operation server 端口声明。
    #[serde(default)]
    pub operation_servers: Vec<SelfDescriptionOperationPortDecl>,
    /// 参数 schema 声明。
    #[serde(default)]
    pub params: Vec<SelfDescriptionParamDecl>,
}

/// 端口声明（输入/输出）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionPortDecl {
    pub name: String,
    #[serde(rename = "type", default)]
    pub ty: String,
}

/// service 端口声明（client/server）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionServicePortDecl {
    pub name: String,
    #[serde(default)]
    pub request_type: String,
    #[serde(default)]
    pub response_type: String,
}

/// operation 端口声明（client/server）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionOperationPortDecl {
    pub name: String,
    #[serde(default)]
    pub goal_type: String,
    #[serde(default)]
    pub feedback_type: String,
    #[serde(default)]
    pub result_type: String,
}

/// 参数 schema 声明（来自 component 类型定义）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionParamDecl {
    pub name: String,
    #[serde(rename = "type", default)]
    pub ty: String,
    #[serde(default)]
    pub update: String,
    pub default: Option<serde_json::Value>,
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
    #[serde(default)]
    pub choices: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionGraph {
    pub name: String,
    #[serde(default)]
    pub scheduler: SelfDescriptionScheduler,
    /// v0.7+ external process package/executable 摘要。
    #[serde(default)]
    pub external_processes: Vec<SelfDescriptionExternalProcess>,
    #[serde(default)]
    pub instances: Vec<SelfDescriptionInstance>,
    #[serde(default)]
    pub tasks: Vec<SelfDescriptionTask>,
    #[serde(default)]
    pub channels: Vec<SelfDescriptionChannel>,
    /// v0.4+ service endpoint 拓扑。
    #[serde(default)]
    pub services: Vec<SelfDescriptionServiceEndpoint>,
    /// v0.6+ operation endpoint 拓扑。
    #[serde(default)]
    pub operations: Vec<SelfDescriptionOperationEndpoint>,
}

/// external process 静态包元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionExternalProcess {
    #[serde(default)]
    pub process: String,
    #[serde(default)]
    pub package: String,
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_dir: String,
    #[serde(default)]
    pub health: String,
    #[serde(default)]
    pub required_backends: Vec<String>,
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

/// service endpoint 静态拓扑信息。
///
/// 来自 Contract IR 中 `graph.services` 的 request/response 绑定。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionServiceEndpoint {
    /// service edge 名称，格式 `<client>.<port>_to_<server>.<port>`。
    #[serde(default)]
    pub name: String,
    /// Contract IR 中 service edge 的稳定实体 ID。
    #[serde(default)]
    pub canonical_id: String,
    /// client 端 instance 名称。
    #[serde(default)]
    pub client_instance: String,
    /// client 端 service port 名称。
    #[serde(default)]
    pub client_port: String,
    /// server 端 instance 名称。
    #[serde(default)]
    pub server_instance: String,
    /// server 端 service port 名称。
    #[serde(default)]
    pub server_port: String,
    /// request 消息类型。
    #[serde(default)]
    pub request_type: String,
    /// response 消息类型。
    #[serde(default)]
    pub response_type: String,
    /// 解析后的通信后端名称；当前 IR 尚未直接记录，由 deployment 推导。
    #[serde(default)]
    pub backend: String,
    /// 请求超时（毫秒）。
    pub timeout_ms: Option<u64>,
    /// 队列深度。
    pub queue_depth: Option<u32>,
    /// 溢出策略。
    #[serde(default)]
    pub overflow: String,
    /// 调度 lane。
    #[serde(default)]
    pub lane: String,
    /// 最大并发 in-flight 请求数。
    pub max_in_flight: Option<u32>,
}

/// operation endpoint 静态拓扑信息。
///
/// Operation 是用户主语义；`lowering` 只用于调试视图展开内部 service/channel。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfDescriptionOperationEndpoint {
    /// operation 名称，格式 `<client_instance>.<client_port>`。
    #[serde(default)]
    pub name: String,
    /// Contract IR 中 operation edge 的稳定实体 ID。
    #[serde(default)]
    pub canonical_id: String,
    /// client 端 instance 名称。
    #[serde(default)]
    pub client_instance: String,
    /// client 端 operation port 名称。
    #[serde(default)]
    pub client_port: String,
    /// server 端 instance 名称。
    #[serde(default)]
    pub server_instance: String,
    /// server 端 operation port 名称。
    #[serde(default)]
    pub server_port: String,
    /// goal 消息类型。
    #[serde(default)]
    pub goal_type: String,
    /// feedback 消息类型。
    #[serde(default)]
    pub feedback_type: String,
    /// result 消息类型。
    #[serde(default)]
    pub result_type: String,
    /// 解析后的通信后端名称。
    #[serde(default)]
    pub backend: String,
    /// 请求超时（毫秒）。
    pub timeout_ms: Option<u64>,
    /// 并发策略。
    #[serde(default)]
    pub concurrency: String,
    /// 抢占策略。
    #[serde(default)]
    pub preempt: String,
    /// 队列深度。
    pub queue_depth: Option<u32>,
    /// 最大并发 in-flight invocation 数。
    pub max_in_flight: Option<u32>,
    /// feedback channel 策略。
    #[serde(default)]
    pub feedback: String,
    /// result 保留时间（毫秒）。
    pub result_retention_ms: Option<u64>,
    /// 调试视图使用的内部 lowering 引用。
    #[serde(default)]
    pub lowering: SelfDescriptionOperationLowering,
}

/// Operation lowering 后的内部 endpoint 引用。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelfDescriptionOperationLowering {
    #[serde(default)]
    pub start_service: String,
    #[serde(default)]
    pub cancel_service: String,
    #[serde(default)]
    pub status_service: String,
    #[serde(default)]
    pub feedback_channel: String,
    #[serde(default)]
    pub result_channel: String,
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

// ---------------------------------------------------------------------------
// 调度健康模型 — language-neutral health Interface
//
// 以下类型定义 task 级和 lane 级调度健康指标的 schema 声明。
// 所有字段使用 `serde(default)` 保证前向兼容：旧版 JSON 不含健康字段时
// 解析为零值，不会让旧应用崩溃。
// ---------------------------------------------------------------------------

/// task 级健康指标 schema 声明。
///
/// 描述单个 task 的调度健康维度。静态 self-description 声明存在哪些健康
/// 维度；live introspection 填充运行态计数器。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelfDescriptionTaskHealth {
    /// 是否声明 deadline miss 观测。
    #[serde(default)]
    pub has_deadline: bool,
    /// 是否声明 stale input 观测。
    #[serde(default)]
    pub has_stale: bool,
    /// 是否声明 backpressure 观测。
    #[serde(default)]
    pub has_backpressure: bool,
    /// 是否声明 overflow 观测。
    #[serde(default)]
    pub has_overflow: bool,
    /// 是否声明 fairness 观测。
    #[serde(default)]
    pub has_fairness: bool,
    /// 声明的 deadline（毫秒）。None 表示无 deadline。
    pub deadline_ms: Option<u64>,
    /// 声明的调度周期（毫秒）。None 表示非周期 task。
    pub period_ms: Option<u64>,
}

/// lane 级健康指标 schema 声明。
///
/// 描述单个 lane 的队列和执行健康维度。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelfDescriptionLaneHealth {
    /// 是否声明 queue depth 观测。
    #[serde(default)]
    pub has_queue_depth: bool,
    /// 是否声明 dispatched count 观测。
    #[serde(default)]
    pub has_dispatched_count: bool,
    /// 是否声明 fairness 观测。
    #[serde(default)]
    pub has_fairness: bool,
}
