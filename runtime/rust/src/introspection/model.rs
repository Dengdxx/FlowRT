use serde::{Deserialize, Serialize};

use flowrt_record::RecordEnvelope;

use crate::recorder::RecorderStatus;
use crate::supervisor::resource_placement::ResourcePlacementStatus;

use super::paths::unix_time_ms;

/// 当前 introspection 协议版本。
pub const INTROSPECTION_PROTOCOL_VERSION: &str = "0.1";

/// runtime introspection 命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum IntrospectionRequest {
    /// 返回进程 handshake 与当前 live status。
    Status,
    /// 返回编译期 self-description JSON。
    SelfDescription,
    /// 返回指定 channel 的 latest raw ABI snapshot。
    ChannelSnapshot { channel: String },
    /// 为指定 channel 建立连接作用域的数据面观测。
    ObserveChannel {
        channel: String,
        #[serde(default)]
        mode: Option<String>,
    },
    /// 返回当前进程内可观察的参数列表。
    ParamList,
    /// 返回指定参数的当前值与 pending 值。
    ParamGet { name: String },
    /// 设置指定参数的 pending 值，由 generated shell 在 tick 边界应用。
    ParamSet {
        name: String,
        value: serde_json::Value,
    },
    /// 向 island boundary input 注入 canonical Message ABI payload。
    BoundaryPublish {
        endpoint: String,
        payload: Vec<u8>,
        published_at_ms: Option<u64>,
    },
    /// 请求取消当前 runtime 中已知的 Operation invocation。
    OperationCancel { operation_id: String },
    /// 请求启动当前 runtime 中已知的 Operation endpoint。
    OperationStart {
        operation: String,
        payload: Vec<u8>,
        timeout_ms: Option<u64>,
        owner: Option<String>,
    },
    /// 启动当前 runtime 的按需 recorder。
    RecorderStart {
        output: Option<String>,
        #[serde(default)]
        filters: Vec<String>,
        queue_depth: Option<usize>,
    },
    /// 停止当前 runtime 的 recorder。
    RecorderStop,
    /// 取走当前 runtime recorder 已暂存事件。
    RecorderDrain,
}

/// runtime introspection 响应。
//
// `Status` 变体本就远大于其余变体（携带完整 `IntrospectionStatus`）；boxing 会波及 28 处
// 构造/匹配点而与本枚举语义无关，故对 large_enum_variant 显式 allow。
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case")]
pub enum IntrospectionResponse {
    Status {
        handshake: IntrospectionHandshake,
        status: IntrospectionStatus,
    },
    SelfDescription {
        handshake: IntrospectionHandshake,
        json: String,
    },
    ChannelSnapshot {
        handshake: IntrospectionHandshake,
        channel: IntrospectionChannelSnapshot,
    },
    ObserveReady {
        handshake: IntrospectionHandshake,
        channel: IntrospectionChannelStatus,
    },
    ParamList {
        handshake: IntrospectionHandshake,
        params: Vec<IntrospectionParamStatus>,
    },
    ParamValue {
        handshake: IntrospectionHandshake,
        param: IntrospectionParamStatus,
    },
    BoundaryPublish {
        handshake: IntrospectionHandshake,
        boundary: IntrospectionBoundaryPublishStatus,
    },
    OperationValue {
        handshake: IntrospectionHandshake,
        operation: IntrospectionOperationStatus,
    },
    OperationStarted {
        handshake: IntrospectionHandshake,
        started: IntrospectionOperationStartStatus,
    },
    RecorderValue {
        handshake: IntrospectionHandshake,
        recorder: IntrospectionRecorderStatus,
    },
    RecorderEvents {
        handshake: IntrospectionHandshake,
        recorder: IntrospectionRecorderStatus,
        events: Vec<RecordEnvelope>,
    },
    Error {
        handshake: IntrospectionHandshake,
        message: String,
    },
}

/// CLI 连接 socket 后首先验证的进程身份。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionHandshake {
    pub protocol_version: String,
    pub pid: u32,
    pub started_at_unix_ms: u64,
    pub self_description_hash: String,
    pub package: String,
    pub process: String,
    pub runtime: String,
}

/// 运行态状态快照。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionStatus {
    pub tick_count: u64,
    #[serde(default)]
    pub clock: IntrospectionClockStatus,
    pub channels: Vec<IntrospectionChannelStatus>,
    /// v0.8+ active input latest/presence/stale 运行态状态。
    #[serde(default)]
    pub inputs: Vec<IntrospectionInputStatus>,
    /// v0.8+ route/backend/drop/overflow 运行态状态。
    #[serde(default)]
    pub routes: Vec<IntrospectionRouteStatus>,
    #[serde(default)]
    pub processes: Vec<IntrospectionProcessStatus>,
    /// v0.13+ abstract resource readiness/health 状态。
    #[serde(default)]
    pub resources: Vec<IntrospectionResourceStatus>,
    /// v0.8+ I/O boundary 运行态健康状态。
    #[serde(default)]
    pub io_boundaries: Vec<IntrospectionIoBoundaryStatus>,
    /// v0.13+ runtime 参数 live 状态，用于 status 级诊断和离线记录。
    #[serde(default)]
    pub params: Vec<IntrospectionParamStatus>,
    /// v0.4+ service 运行态健康状态。
    #[serde(default)]
    pub services: Vec<IntrospectionServiceStatus>,
    /// v0.6+ operation 运行态健康状态。
    #[serde(default)]
    pub operations: Vec<IntrospectionOperationStatus>,
    /// v0.5+ task 级调度健康快照。
    #[serde(default)]
    pub tasks: Vec<IntrospectionTaskHealth>,
    /// v0.5+ lane 级调度健康快照。
    #[serde(default)]
    pub lanes: Vec<IntrospectionLaneHealth>,
    /// v0.6+ recorder 运行态状态。
    #[serde(default)]
    pub recorder: IntrospectionRecorderStatus,
    /// v0.21+ per-instance 生命周期状态快照（按 instance 名 canonical 排序）。
    #[serde(default)]
    pub instances: Vec<IntrospectionInstanceStatus>,
    /// v0.23.3+ standby redundancy failover 事件（按记录顺序）。
    #[serde(default)]
    pub failovers: Vec<IntrospectionFailoverEvent>,
    /// v0.23.3+ graph critical health 使用的 instance 集合；空状态默认由 runtime 展开为所有实例。
    #[serde(default)]
    pub critical_instances: Vec<String>,
    /// v0.21.3+ 图级 health 聚合：每实例 lifecycle 的 worst-of 滚动
    /// （`faulted` > `degraded` > `healthy`）。始终存在。
    #[serde(default = "default_graph_health")]
    pub graph_health: String,
    /// v0.23.3+ 图级 critical subset health 聚合；未声明 critical subset 时等同 `graph_health`。
    #[serde(default = "default_graph_health")]
    pub graph_critical_health: String,
    /// v0.13+ 由 status/self-description 实体派生的结构化诊断快照。
    #[serde(default)]
    pub diagnostics: Vec<IntrospectionDiagnostic>,
}

/// graph health 默认值：无实例不健康即 `healthy`。
fn default_graph_health() -> String {
    "healthy".to_string()
}

/// 单个 instance 的生命周期观测项。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionInstanceStatus {
    pub instance: String,
    /// `LifecycleState::as_str()` 的 canonical 小写值。
    pub lifecycle_state: String,
    #[serde(default)]
    pub restart_count: u64,
    #[serde(default)]
    pub last_fault_reason: Option<String>,
    #[serde(default)]
    pub last_fault_tick: Option<u64>,
    #[serde(default)]
    pub last_transition_tick: Option<u64>,
}

/// 单次 standby redundancy failover 事件。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionFailoverEvent {
    pub event: String,
    pub group: String,
    pub old_active: String,
    pub new_active: String,
    pub tick_id: u64,
    pub reason: String,
}

/// 结构化诊断中的单个数值或状态指标。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionDiagnosticMetric {
    pub name: String,
    pub value: serde_json::Value,
}

/// runtime live status 的统一诊断项。
///
/// `entity_id` 使用 self-description / Contract IR 中的 canonical 名称；`reason` 和
/// `suggestion` 是结构化字段，不承载无法解析的日志 blob。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionDiagnostic {
    pub category: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub state: String,
    pub severity: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub suggestion: Option<String>,
    #[serde(default)]
    pub updated_unix_ms: Option<u64>,
    #[serde(default)]
    pub observed_ms: Option<u64>,
    #[serde(default)]
    pub metrics: Vec<IntrospectionDiagnosticMetric>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionClockStatus {
    #[serde(default = "default_clock_source")]
    pub source: String,
    #[serde(default)]
    pub tick_time_ms: Option<u64>,
    #[serde(default = "default_clock_unit")]
    pub unit: String,
    #[serde(default = "default_clock_field")]
    pub field: String,
}

impl Default for IntrospectionClockStatus {
    fn default() -> Self {
        Self {
            source: default_clock_source(),
            tick_time_ms: None,
            unit: default_clock_unit(),
            field: default_clock_field(),
        }
    }
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

/// 单个 channel 的运行态摘要。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionChannelStatus {
    pub name: String,
    pub message_type: String,
    pub published_count: u64,
    pub last_payload_len: Option<usize>,
    #[serde(default)]
    pub active_observers: u64,
    #[serde(default)]
    pub dropped_samples: u64,
}

/// 单个 active input 的 latest 读取状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionInputStatus {
    /// task 名称。
    #[serde(default)]
    pub task: String,
    /// task input port 名称。
    #[serde(default)]
    pub input: String,
    /// 对应 runtime channel 名称。
    #[serde(default)]
    pub channel: String,
    /// message 类型名称。
    #[serde(default)]
    pub message_type: String,
    /// 当前 latest view 是否有样本。
    #[serde(default)]
    pub present: bool,
    /// 当前 latest view 是否已 stale。
    #[serde(default)]
    pub stale: bool,
    /// 最近读取到的 channel revision。
    #[serde(default)]
    pub last_revision: Option<u64>,
    /// 最近一次读取时间（runtime monotonic 毫秒或 scheduler 毫秒）。
    #[serde(default)]
    pub last_read_ms: Option<u64>,
    /// 最近一次状态更新时间（Unix 毫秒）。
    #[serde(default)]
    pub updated_unix_ms: Option<u64>,
    /// 该 input 对应 route 的累计 drop 计数快照。
    #[serde(default)]
    pub dropped_samples: u64,
    /// 该 input 对应 route 的累计 backpressure 计数快照。
    #[serde(default)]
    pub backpressure_count: u64,
    /// 该 input 对应 route 的累计 overflow 计数快照。
    #[serde(default)]
    pub overflow_count: u64,
}

/// 单条 dataflow route 的 backend 与传输健康状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionRouteStatus {
    /// runtime route 名称，通常等于 channel 名。
    #[serde(default)]
    pub name: String,
    /// source endpoint，格式 `<instance>.<port>`。
    #[serde(default)]
    pub from: String,
    /// target endpoint，格式 `<instance>.<port>`。
    #[serde(default)]
    pub to: String,
    /// message 类型名称。
    #[serde(default)]
    pub message_type: String,
    /// 当前选择的 backend。
    #[serde(default)]
    pub backend: String,
    /// backend 选择原因，来自 resolver / self-description。
    #[serde(default)]
    pub selected_reason: String,
    /// 累计成功发布或进入 route 的样本数。
    #[serde(default)]
    pub published_count: u64,
    /// 累计 drop 计数。
    #[serde(default)]
    pub dropped_samples: u64,
    /// 累计 backpressure 计数。
    #[serde(default)]
    pub backpressure_count: u64,
    /// 累计 overflow 计数。
    #[serde(default)]
    pub overflow_count: u64,
    /// 最近一次发布或进入 route 的时间。
    #[serde(default)]
    pub last_publish_ms: Option<u64>,
    /// 最近一次 route/backend 错误。
    #[serde(default)]
    pub last_error: Option<String>,
    /// route 当前 backend health 状态。
    #[serde(default = "default_backend_health_state")]
    pub backend_health_state: String,
    /// route 当前 backend health 错误。
    #[serde(default)]
    pub backend_health_error: Option<String>,
    /// 当前 backend reconnect attempt。
    #[serde(default)]
    pub backend_reconnect_attempt: u32,
    /// 下一次 backend retry 的 Unix 毫秒时间戳。
    #[serde(default)]
    pub backend_next_retry_unix_ms: Option<u64>,
    /// 当前 backend health 是否仍可恢复。
    #[serde(default)]
    pub backend_recoverable: bool,
}

impl Default for IntrospectionRouteStatus {
    fn default() -> Self {
        Self {
            name: String::new(),
            from: String::new(),
            to: String::new(),
            message_type: String::new(),
            backend: String::new(),
            selected_reason: String::new(),
            published_count: 0,
            dropped_samples: 0,
            backpressure_count: 0,
            overflow_count: 0,
            last_publish_ms: None,
            last_error: None,
            backend_health_state: default_backend_health_state(),
            backend_health_error: None,
            backend_reconnect_attempt: 0,
            backend_next_retry_unix_ms: None,
            backend_recoverable: false,
        }
    }
}

fn default_backend_health_state() -> String {
    "ready".to_string()
}

/// 抽象 resource 的运行态 readiness/health 状态。
///
/// `state` 使用 `ready|pending|degraded|failed|unknown` 字符串，避免把后续
/// supervisor readiness gate 语义提前固化到 ABI enum。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionResourceStatus {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub capability: String,
    #[serde(default)]
    pub access: Option<String>,
    #[serde(default = "default_resource_state")]
    pub state: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub readiness: Option<String>,
    #[serde(default)]
    pub health: Option<String>,
    #[serde(default)]
    pub on_failure: Option<String>,
    #[serde(default)]
    pub contract_status: Option<String>,
    #[serde(default)]
    pub satisfied: Option<bool>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub provider_scope: Option<String>,
    #[serde(default)]
    pub provider_readiness_source: Option<String>,
    #[serde(default)]
    pub provider_health_source: Option<String>,
    #[serde(default)]
    pub diagnostic: Option<String>,
    #[serde(default)]
    pub suggestion: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub owner_process: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub updated_unix_ms: Option<u64>,
}

fn default_resource_state() -> String {
    "unknown".to_string()
}

/// 单个 channel 的 latest raw ABI snapshot。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionChannelSnapshot {
    pub published_count: u64,
    pub payload: Option<Vec<u8>>,
    pub published_at_ms: Option<u64>,
}

/// I/O boundary 声明资源的运行态状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionIoBoundaryResourceStatus {
    /// resource 名称，来自 Contract IR。
    pub name: String,
    /// resource 类型，使用 Contract IR 中的 canonical 名称。
    pub kind: String,
    /// 当前 resource 是否可用。
    #[serde(default)]
    pub ready: bool,
    /// 最近一次上报的普通状态说明。
    #[serde(default)]
    pub message: Option<String>,
    /// 最近一次上报的错误说明。
    #[serde(default)]
    pub last_error: Option<String>,
    /// 最近一次状态更新时间（Unix 毫秒）。
    #[serde(default)]
    pub updated_unix_ms: Option<u64>,
}

/// 单个 I/O boundary component 的运行态健康状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionIoBoundaryStatus {
    /// instance 名称。
    pub name: String,
    /// component 类型名称。
    pub component: String,
    /// runtime readiness 视角是否可用。
    #[serde(default)]
    pub ready: bool,
    /// 用户上报的健康状态。
    #[serde(default = "default_true")]
    pub healthy: bool,
    /// 最近一次 boundary 级错误说明。
    #[serde(default)]
    pub last_error: Option<String>,
    /// resource 级状态。
    #[serde(default)]
    pub resources: Vec<IntrospectionIoBoundaryResourceStatus>,
    /// 最近一次状态更新时间（Unix 毫秒）。
    #[serde(default)]
    pub updated_unix_ms: Option<u64>,
}

fn default_true() -> bool {
    true
}

/// supervisor 维护的子进程健康状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionProcessStatus {
    pub name: String,
    pub state: String,
    pub pid: Option<u32>,
    #[serde(default)]
    pub restart_count: u32,
    pub tick_count: Option<u64>,
    pub last_seen_unix_ms: Option<u64>,
    pub tick_stale: bool,
    pub exit_code: Option<i32>,
    /// 当前正在等待的 readiness gate 名称（如 `runtime_ready`、`service_ready`）。
    /// 进程已通过 readiness 检查或不需要等待时为 `None`。
    #[serde(default)]
    pub readiness_wait: Option<String>,
    /// 资源提示 desired/applied 状态。
    #[serde(default)]
    pub resource_placement: Option<ResourcePlacementStatus>,
}

/// 单个 service endpoint 的运行态健康状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionServiceStatus {
    /// service edge 名称。
    pub name: String,
    /// service 是否就绪。
    #[serde(default)]
    pub ready: bool,
    /// 当前 in-flight 请求数。
    #[serde(default)]
    pub in_flight: u64,
    /// 当前排队请求数。
    #[serde(default)]
    pub queued: u64,
    /// 累计请求总数。
    #[serde(default)]
    pub total_requests: u64,
    /// 累计超时次数。
    #[serde(default)]
    pub timeout_count: u64,
    /// 累计 busy 拒绝次数。
    #[serde(default)]
    pub busy_count: u64,
    /// 累计 unavailable 次数。
    #[serde(default)]
    pub unavailable_count: u64,
    /// 累计 late response / drop 次数。
    #[serde(default)]
    pub late_drop_count: u64,
}

/// 单个 Operation endpoint 的运行态健康状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionOperationStatus {
    /// operation 名称，格式 `<client_instance>.<client_port>`。
    #[serde(default)]
    pub name: String,
    /// operation endpoint 是否已注册且可接受控制请求。
    #[serde(default)]
    pub ready: bool,
    /// 当前运行中的 invocation 数。
    #[serde(default)]
    pub running: u64,
    /// 当前排队的 invocation 数。
    #[serde(default)]
    pub queued: u64,
    /// 当前非终态 invocation ID。
    #[serde(default)]
    pub current_operation_ids: Vec<String>,
    /// 累计已启动 invocation 数。
    #[serde(default)]
    pub total_started: u64,
    /// 累计成功完成次数。
    #[serde(default)]
    pub succeeded_count: u64,
    /// 累计失败次数。
    #[serde(default)]
    pub failed_count: u64,
    /// 累计取消次数。
    #[serde(default)]
    pub canceled_count: u64,
    /// 累计超时次数。
    #[serde(default)]
    pub timeout_count: u64,
    /// 累计抢占次数。
    #[serde(default)]
    pub preempted_count: u64,
    /// 当前 invocation 状态。
    #[serde(default)]
    pub current_state: Option<String>,
    /// 当前 control owner。
    #[serde(default)]
    pub current_owner: Option<String>,
    /// 当前 invocation deadline（runtime monotonic 毫秒）。
    #[serde(default)]
    pub current_deadline_ms: Option<u64>,
    /// 最近一条 operation 事件名。
    #[serde(default)]
    pub last_event: Option<String>,
    /// 最近一条 operation error。
    #[serde(default)]
    pub last_error: Option<String>,
    /// 最近一次状态转换时间戳（Unix 毫秒）。
    pub last_transition_ms: Option<u64>,
}

/// Operation start 请求被接受后的 live 状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionOperationStartStatus {
    pub operation_id: String,
    pub operation: IntrospectionOperationStatus,
}

/// recorder 运行态状态。
pub type IntrospectionRecorderStatus = RecorderStatus;

/// recorder 启动参数。socket 控制面会用当前 handshake 填充身份字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrospectionRecorderStart {
    pub output: Option<String>,
    pub filters: Vec<String>,
    pub queue_depth: Option<usize>,
    pub package: String,
    pub process: String,
    pub runtime_pid: u32,
    pub selfdesc_hash: String,
}

/// 单个 task 的调度健康快照。
///
/// 由 generated shell 在 scheduler step 边界填充，反映 task 级调度质量。
/// 所有字段使用 `serde(default)` 保证前向兼容。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionTaskHealth {
    /// task 名称。
    #[serde(default)]
    pub name: String,
    /// 所属 lane 名称。
    #[serde(default)]
    pub lane: String,
    /// 当前 task 是否已 admission 且尚未 completion。
    #[serde(default)]
    pub inflight: bool,
    /// runtime 计划该次 task 应被调度的毫秒时间。
    #[serde(default)]
    pub scheduled_time_ms: Option<u64>,
    /// scheduler 实际观察并 admission 该次 task 的毫秒时间。
    #[serde(default)]
    pub observed_time_ms: Option<u64>,
    /// runtime 观察到的非负迟到毫秒数。
    #[serde(default)]
    pub lateness_ms: Option<u64>,
    /// periodic task 本次 admission 已错过的周期数。
    #[serde(default)]
    pub missed_periods: Option<u64>,
    /// 上一轮执行是否越过本轮周期或调度窗口。
    #[serde(default)]
    pub overrun: Option<bool>,
    /// 累计 deadline miss 次数。
    #[serde(default)]
    pub deadline_missed: u64,
    /// 累计 stale input 次数。
    #[serde(default)]
    pub stale_input: u64,
    /// 累计 backpressure 事件次数。
    #[serde(default)]
    pub backpressure: u64,
    /// 累计 overflow 事件次数。
    #[serde(default)]
    pub overflow: u64,
    /// 累计 fairness 违规次数（如 lane 内优先级饥饿）。
    #[serde(default)]
    pub fairness_violations: u64,
    /// 累计运行次数。
    #[serde(default)]
    pub run_count: u64,
    /// 累计成功次数。
    #[serde(default)]
    pub success_count: u64,
    /// 连续失败次数。
    #[serde(default)]
    pub consecutive_failures: u64,
    /// 最近一次运行时间戳（Unix 毫秒）。
    pub last_run_ms: Option<u64>,
    /// 最近一次成功时间戳（Unix 毫秒）。
    pub last_success_ms: Option<u64>,
}

/// 单个 lane 的调度健康快照。
///
/// 反映 lane 级队列深度和公平性状态。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionLaneHealth {
    /// lane 名称。
    #[serde(default)]
    pub name: String,
    /// 当前队列中的 ready task 数。
    #[serde(default)]
    pub queue_depth: u64,
    /// 累计被调度执行的 task 总数。
    #[serde(default)]
    pub dispatched_count: u64,
    /// 累计 fairness 违规次数（如 lane 间轮转饥饿）。
    #[serde(default)]
    pub fairness_violations: u64,
}
/// generated shell 注册到 runtime 控制面的参数 schema。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionParamSchema {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub update: String,
    pub current: serde_json::Value,
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
    pub choices: Vec<serde_json::Value>,
}

/// runtime 参数状态快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionParamStatus {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub update: String,
    pub current: serde_json::Value,
    pub pending: Option<serde_json::Value>,
    #[serde(default = "default_param_apply_state")]
    pub apply_state: String,
    #[serde(default)]
    pub last_reject_reason: Option<String>,
    #[serde(default)]
    pub updated_unix_ms: Option<u64>,
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
    pub choices: Vec<serde_json::Value>,
}

fn default_param_apply_state() -> String {
    "applied".to_string()
}

/// boundary input 注入结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntrospectionBoundaryPublishStatus {
    /// boundary endpoint 名称。
    pub endpoint: String,
    /// Contract IR 中的 message 类型名称。
    pub message_type: String,
    /// 注入后的 boundary input revision。
    pub revision: u64,
    /// 注入时使用的 runtime 毫秒时间戳。
    pub published_at_ms: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntrospectionIdentity {
    pub self_description_hash: String,
    pub package: String,
    pub process: String,
    pub runtime: String,
}

impl IntrospectionIdentity {
    /// 构造当前进程的 handshake。
    pub fn handshake(&self) -> IntrospectionHandshake {
        IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: std::process::id(),
            started_at_unix_ms: unix_time_ms(),
            self_description_hash: self.self_description_hash.clone(),
            package: self.package.clone(),
            process: self.process.clone(),
            runtime: self.runtime.clone(),
        }
    }
}
