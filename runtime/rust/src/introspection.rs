//! FlowRT runtime 自描述与 live status 的最小 Unix socket 协议。
//!
//! socket 路径只用于发现候选进程；真实身份必须来自连接后的 handshake。协议保持同步、
//! JSON-line 和标准库实现，便于生成 shell 在不引入大型 runtime 依赖的情况下接入。

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, MutexGuard,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use flowrt_record::{RecordEntityKind, RecordEnvelope};

use crate::recorder::{
    RecorderRuntimeMetadata, RecorderStartConfig, RecorderStatus, RecorderTap, RecorderTapOutcome,
};
use crate::supervisor::resource_placement::ResourcePlacementStatus;
use crate::{FrameCodec, FrameDescriptor, FrameLeaseStatus};

/// 当前 introspection 协议版本。
pub const INTROSPECTION_PROTOCOL_VERSION: &str = "0.1";
const MAX_INTROSPECTION_CLIENT_THREADS: usize = 64;
const MAX_INTROSPECTION_OBSERVERS: usize = 32;
const INTROSPECTION_INITIAL_REQUEST_TIMEOUT: Duration = Duration::from_millis(100);
const INTROSPECTION_RESPONSE_WRITE_TIMEOUT: Duration = Duration::from_millis(100);

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
    pub channels: Vec<IntrospectionChannelStatus>,
    /// v0.8+ active input latest/presence/stale 运行态状态。
    #[serde(default)]
    pub inputs: Vec<IntrospectionInputStatus>,
    /// v0.8+ route/backend/drop/overflow 运行态状态。
    #[serde(default)]
    pub routes: Vec<IntrospectionRouteStatus>,
    #[serde(default)]
    pub processes: Vec<IntrospectionProcessStatus>,
    /// v0.8+ I/O boundary 运行态健康状态。
    #[serde(default)]
    pub io_boundaries: Vec<IntrospectionIoBoundaryStatus>,
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    /// 最近一次状态转换时间戳（Unix 毫秒）。
    pub last_transition_ms: Option<u64>,
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

/// 数据面 probe 记录结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntrospectionProbeRecord {
    pub recorded: bool,
    pub dropped: bool,
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
    pub min: Option<serde_json::Value>,
    pub max: Option<serde_json::Value>,
    pub choices: Vec<serde_json::Value>,
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

type BoundaryInputHandler =
    Arc<dyn Fn(&[u8], Option<u64>) -> std::result::Result<u64, String> + Send + Sync + 'static>;

#[derive(Clone)]
struct BoundaryInputState {
    message_type: String,
    handler: BoundaryInputHandler,
}

impl std::fmt::Debug for BoundaryInputState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BoundaryInputState")
            .field("message_type", &self.message_type)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct ChannelState {
    message_type: String,
    probe: IntrospectionChannelProbe,
}

#[derive(Debug)]
struct ChannelProbeInner {
    observer_count: AtomicU64,
    dropped_samples: AtomicU64,
    published_count: AtomicU64,
    latest: Mutex<ChannelProbeLatest>,
}

#[derive(Debug, Default)]
struct ChannelProbeLatest {
    payload: Option<Vec<u8>>,
    published_at_ms: Option<u64>,
    max_payload_len: Option<usize>,
}

/// 单个 channel 的按需数据面 probe。
#[derive(Debug, Clone)]
pub struct IntrospectionChannelProbe {
    inner: Arc<ChannelProbeInner>,
}

impl Default for IntrospectionChannelProbe {
    fn default() -> Self {
        Self::new(None)
    }
}

impl IntrospectionChannelProbe {
    fn new(max_payload_len: Option<usize>) -> Self {
        let payload = max_payload_len.map(Vec::with_capacity);
        Self {
            inner: Arc::new(ChannelProbeInner {
                observer_count: AtomicU64::new(0),
                dropped_samples: AtomicU64::new(0),
                published_count: AtomicU64::new(0),
                latest: Mutex::new(ChannelProbeLatest {
                    payload,
                    published_at_ms: None,
                    max_payload_len,
                }),
            }),
        }
    }

    /// 判断当前 channel 是否有 active echo observer。
    pub fn enabled(&self) -> bool {
        self.inner.observer_count.load(Ordering::Acquire) != 0
    }

    /// active observer 数量。
    pub fn active_count(&self) -> u64 {
        self.inner.observer_count.load(Ordering::Acquire)
    }

    /// 被 probe 丢弃的观测样本数量。
    pub fn dropped_samples(&self) -> u64 {
        self.inner.dropped_samples.load(Ordering::Acquire)
    }

    /// 记录一次 channel 发布事件；只更新控制面计数，不拷贝 payload。
    pub fn record_publish_event(&self) {
        let _ = self.inner.published_count.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |current| Some(current.saturating_add(1)),
        );
    }

    /// 建立一个 observer guard；guard drop 后自动关闭 probe。
    pub fn observe(&self) -> IntrospectionObserverGuard {
        self.inner.observer_count.fetch_add(1, Ordering::AcqRel);
        IntrospectionObserverGuard {
            inner: Arc::clone(&self.inner),
        }
    }

    /// 非阻塞记录观测 payload。无观察者时只做原子读取；锁繁忙或超出上界时丢弃观测样本。
    pub fn try_record_bytes(
        &self,
        payload: &[u8],
        published_at_ms: Option<u64>,
    ) -> IntrospectionProbeRecord {
        if !self.enabled() {
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: false,
            };
        }
        let Ok(mut latest) = self.inner.latest.try_lock() else {
            self.inner.dropped_samples.fetch_add(1, Ordering::Relaxed);
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: true,
            };
        };
        if latest
            .max_payload_len
            .is_some_and(|max_payload_len| payload.len() > max_payload_len)
        {
            self.inner.dropped_samples.fetch_add(1, Ordering::Relaxed);
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: true,
            };
        }
        let max_payload_len = latest.max_payload_len;
        let buffer = latest.payload.get_or_insert_with(Vec::new);
        if let Some(max_payload_len) = max_payload_len
            && buffer.capacity() < max_payload_len
        {
            self.inner.dropped_samples.fetch_add(1, Ordering::Relaxed);
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: true,
            };
        }
        buffer.clear();
        buffer.extend_from_slice(payload);
        latest.published_at_ms = published_at_ms;
        IntrospectionProbeRecord {
            recorded: true,
            dropped: false,
        }
    }

    fn force_record_bytes(&self, payload: Vec<u8>, published_at_ms: Option<u64>) {
        self.record_publish_event();
        let mut latest = self
            .inner
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        latest.payload = Some(payload);
        latest.published_at_ms = published_at_ms;
    }

    fn snapshot(&self) -> IntrospectionChannelSnapshot {
        let latest = self
            .inner
            .latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        IntrospectionChannelSnapshot {
            published_count: self.inner.published_count.load(Ordering::Acquire),
            payload: latest.payload.clone(),
            published_at_ms: latest.published_at_ms,
        }
    }
}

/// 连接作用域 observer guard。
#[derive(Debug)]
pub struct IntrospectionObserverGuard {
    inner: Arc<ChannelProbeInner>,
}

impl Drop for IntrospectionObserverGuard {
    fn drop(&mut self) {
        let mut current = self.inner.observer_count.load(Ordering::Acquire);
        while current != 0 {
            match self.inner.observer_count.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(next) => current = next,
            }
        }
    }
}

#[derive(Debug, Clone)]
struct ParamState {
    ty: String,
    update: String,
    current: serde_json::Value,
    pending: Option<serde_json::Value>,
    min: Option<serde_json::Value>,
    max: Option<serde_json::Value>,
    choices: Vec<serde_json::Value>,
}

/// runtime shell 可共享更新的 introspection live 状态。
#[derive(Debug, Clone, Default)]
pub struct IntrospectionState {
    inner: Arc<Mutex<IntrospectionStateInner>>,
    recorder: RecorderTap,
}

#[derive(Debug, Default)]
struct IntrospectionStateInner {
    tick_count: u64,
    self_description_json: Option<String>,
    channels: BTreeMap<String, ChannelState>,
    inputs: BTreeMap<String, IntrospectionInputStatus>,
    routes: BTreeMap<String, IntrospectionRouteStatus>,
    params: BTreeMap<String, ParamState>,
    boundary_inputs: BTreeMap<String, BoundaryInputState>,
    processes: BTreeMap<String, IntrospectionProcessStatus>,
    io_boundaries: BTreeMap<String, IntrospectionIoBoundaryStatus>,
    services: BTreeMap<String, IntrospectionServiceStatus>,
    operations: BTreeMap<String, IntrospectionOperationStatus>,
    tasks: BTreeMap<String, IntrospectionTaskHealth>,
    lanes: BTreeMap<String, IntrospectionLaneHealth>,
}

impl IntrospectionState {
    /// 构造空 live 状态。
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_inner(&self) -> MutexGuard<'_, IntrospectionStateInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 预注册一个 channel，使其在尚未发布样本时也出现在 status 中。
    pub fn register_channel(&self, name: impl Into<String>, message_type: impl Into<String>) {
        self.register_channel_with_probe_capacity(name, message_type, None);
    }

    /// 预注册一个带有有界 probe snapshot 容量的 channel。
    pub fn register_channel_with_probe_capacity(
        &self,
        name: impl Into<String>,
        message_type: impl Into<String>,
        max_payload_len: Option<usize>,
    ) {
        let name = name.into();
        let mut inner = self.lock_inner();
        inner.channels.entry(name).or_insert_with(|| ChannelState {
            message_type: message_type.into(),
            probe: IntrospectionChannelProbe::new(max_payload_len),
        });
    }

    /// 注册一个 runtime 参数，使 CLI 能查询并提交 pending 更新。
    pub fn register_param(&self, schema: IntrospectionParamSchema) {
        let mut inner = self.lock_inner();
        inner
            .params
            .entry(schema.name)
            .or_insert_with(|| ParamState {
                ty: schema.ty,
                update: schema.update,
                current: schema.current,
                pending: None,
                min: schema.min,
                max: schema.max,
                choices: schema.choices,
            });
    }

    /// 注册一个 island boundary input 的底层注入 handler。
    ///
    /// handler 接收 canonical Message ABI payload，由 generated shell 解码为真实消息类型后
    /// 写入 `BoundaryInput<T>`。普通 channel 不经过这里，避免生产 dataflow 获得隐式写入口。
    pub fn register_boundary_input_handler<F>(
        &self,
        endpoint: impl Into<String>,
        message_type: impl Into<String>,
        handler: F,
    ) where
        F: Fn(&[u8], Option<u64>) -> std::result::Result<u64, String> + Send + Sync + 'static,
    {
        let mut inner = self.lock_inner();
        inner.boundary_inputs.insert(
            endpoint.into(),
            BoundaryInputState {
                message_type: message_type.into(),
                handler: Arc::new(handler),
            },
        );
    }

    /// 注册一个 typed `BoundaryInput<T>`，供 `flowrt pub` 等控制面注入 canonical payload。
    pub fn register_boundary_input<T>(
        &self,
        endpoint: impl Into<String>,
        message_type: impl Into<String>,
        input: crate::BoundaryInput<T>,
    ) where
        T: FrameCodec + Send + Sync + 'static,
    {
        let endpoint = endpoint.into();
        let endpoint_for_error = endpoint.clone();
        self.register_boundary_input_handler(endpoint, message_type, move |payload, timestamp| {
            let value = T::decode_frame(payload).map_err(|error| {
                format!("decode FlowRT boundary input `{endpoint_for_error}`: {error}")
            })?;
            Ok(match timestamp {
                Some(timestamp) => input.inject_at(value, timestamp),
                None => input.inject(value),
            })
        });
    }

    /// 向已注册的 island boundary input 注入 canonical Message ABI payload。
    pub fn publish_boundary_input(
        &self,
        endpoint: &str,
        payload: Vec<u8>,
        published_at_ms: Option<u64>,
    ) -> std::result::Result<IntrospectionBoundaryPublishStatus, String> {
        let boundary = {
            let inner = self.lock_inner();
            inner.boundary_inputs.get(endpoint).cloned()
        }
        .ok_or_else(|| format!("unknown FlowRT boundary input `{endpoint}`"))?;
        let revision = (boundary.handler)(&payload, published_at_ms)?;
        Ok(IntrospectionBoundaryPublishStatus {
            endpoint: endpoint.to_string(),
            message_type: boundary.message_type,
            revision,
            published_at_ms,
        })
    }

    /// 注册编译期 self-description JSON，供在线 CLI 自动发现和格式化。
    pub fn set_self_description_json(&self, json: impl Into<String>) {
        let mut inner = self.lock_inner();
        inner.self_description_json = Some(json.into());
    }

    /// 返回当前 runtime 暴露的 self-description JSON。
    pub fn self_description_json(&self) -> Option<String> {
        let inner = self.lock_inner();
        inner.self_description_json.clone()
    }

    /// 增加 scheduler tick 计数。
    pub fn record_tick(&self) {
        let mut inner = self.lock_inner();
        inner.tick_count = inner.tick_count.saturating_add(1);
        let tick_count = inner.tick_count;
        drop(inner);
        self.recorder.record_runtime_event_json(
            RecordEntityKind::Clock,
            "scheduler_tick",
            "flowrt.clock.tick",
            serde_json::json!({ "tick_count": tick_count }),
        );
    }

    /// 启动 runtime recorder。
    pub fn start_recorder(&self, start: IntrospectionRecorderStart) -> IntrospectionRecorderStatus {
        self.recorder.start(RecorderStartConfig {
            output: start.output,
            filters: start.filters,
            queue_depth: start.queue_depth.unwrap_or(1024),
            metadata: RecorderRuntimeMetadata {
                package: start.package,
                process: start.process,
                runtime_pid: start.runtime_pid,
                selfdesc_hash: start.selfdesc_hash,
            },
        })
    }

    /// 停止 runtime recorder。
    pub fn stop_recorder(&self) -> IntrospectionRecorderStatus {
        self.recorder.stop()
    }

    /// 取走 recorder 暂存事件。
    pub fn drain_recorder_events(&self) -> Vec<RecordEnvelope> {
        self.recorder.drain_events()
    }

    /// 判断指定 channel 是否会被 recorder 采集。
    pub fn recorder_enabled_for_channel(&self, name: &str) -> bool {
        self.recorder.enabled_for_channel(name)
    }

    /// 判断指定 descriptor resource 是否会被 recorder 采集。
    pub fn recorder_enabled_for_descriptor(&self, resource_id: &str) -> bool {
        self.recorder.enabled_for_descriptor(resource_id)
    }

    /// 按需记录 channel sample。关闭时不复制 payload。
    pub fn try_record_channel_sample_bytes(
        &self,
        name: &str,
        message_type: impl AsRef<str>,
        payload: &[u8],
        published_at_ms: Option<u64>,
    ) -> RecorderTapOutcome {
        self.recorder.record_channel_sample_bytes(
            name,
            message_type.as_ref(),
            payload,
            published_at_ms,
        )
    }

    /// 记录 frame descriptor / side-channel lease 事件，不复制真实 payload。
    pub fn record_frame_descriptor_event(
        &self,
        name: &str,
        descriptor: &FrameDescriptor,
        status: FrameLeaseStatus,
        payload_recording: bool,
    ) -> RecorderTapOutcome {
        self.recorder
            .record_frame_descriptor_event(name, descriptor, status, payload_recording)
    }

    /// 按需记录 canonical frame channel sample。关闭时不复制 payload。
    pub fn try_record_channel_sample_frame_bytes(
        &self,
        name: &str,
        message_type: impl AsRef<str>,
        payload: &[u8],
        published_at_ms: Option<u64>,
    ) -> RecorderTapOutcome {
        self.recorder.record_channel_sample_frame_bytes(
            name,
            message_type.as_ref(),
            payload,
            published_at_ms,
        )
    }

    /// 记录 channel 发布的 latest raw ABI payload。
    pub fn record_channel_publish<T: Copy>(
        &self,
        name: impl Into<String>,
        message_type: impl Into<String>,
        value: &T,
        published_at_ms: Option<u64>,
    ) {
        self.record_channel_publish_bytes(name, message_type, bytes_of(value), published_at_ms);
    }

    /// 记录 channel 发布的 raw ABI bytes。
    pub fn record_channel_publish_bytes(
        &self,
        name: impl Into<String>,
        message_type: impl Into<String>,
        payload: Vec<u8>,
        published_at_ms: Option<u64>,
    ) {
        let name = name.into();
        let message_type = message_type.into();
        self.try_record_channel_sample_bytes(&name, &message_type, &payload, published_at_ms);
        let mut inner = self.lock_inner();
        let channel = inner
            .channels
            .entry(name.clone())
            .or_insert_with(|| ChannelState {
                message_type: message_type.clone(),
                probe: IntrospectionChannelProbe::new(None),
            });
        channel.message_type = message_type.clone();
        channel.probe.force_record_bytes(payload, published_at_ms);
    }

    /// 获取指定 channel 的 probe handle。
    pub fn channel_probe(&self, name: &str) -> Option<IntrospectionChannelProbe> {
        let inner = self.lock_inner();
        inner
            .channels
            .get(name)
            .map(|channel| channel.probe.clone())
    }

    /// 为指定 channel 建立连接作用域 observer guard。
    pub fn observe_channel(&self, name: &str) -> Option<IntrospectionObserverGuard> {
        self.channel_probe(name).map(|probe| probe.observe())
    }

    /// 返回指定 channel 当前 active observer 数量。
    pub fn active_probe_count(&self, name: &str) -> Option<u64> {
        self.channel_probe(name).map(|probe| probe.active_count())
    }

    /// 按需记录 channel 发布的 raw ABI bytes。
    pub fn try_probe_channel_publish_bytes(
        &self,
        name: &str,
        message_type: impl Into<String>,
        payload: &[u8],
        published_at_ms: Option<u64>,
    ) -> IntrospectionProbeRecord {
        let message_type = message_type.into();
        let probe = {
            let mut inner = self.lock_inner();
            let channel = inner
                .channels
                .entry(name.to_string())
                .or_insert_with(|| ChannelState {
                    message_type: message_type.clone(),
                    probe: IntrospectionChannelProbe::new(None),
                });
            channel.message_type = message_type.clone();
            channel.probe.clone()
        };
        let probe_record = probe.try_record_bytes(payload, published_at_ms);
        let recorder_record =
            self.try_record_channel_sample_bytes(name, message_type, payload, published_at_ms);
        IntrospectionProbeRecord {
            recorded: probe_record.recorded || recorder_record.recorded,
            dropped: probe_record.dropped || recorder_record.dropped,
        }
    }

    /// 按需记录 channel 发布的 Message ABI 对象表示。
    pub fn try_probe_channel_publish<T: Copy>(
        &self,
        name: &str,
        message_type: impl Into<String>,
        value: &T,
        published_at_ms: Option<u64>,
    ) -> IntrospectionProbeRecord {
        let recorder_enabled = self.recorder_enabled_for_channel(name);
        if self
            .channel_probe(name)
            .is_none_or(|probe| !probe.enabled())
            && !recorder_enabled
        {
            return IntrospectionProbeRecord {
                recorded: false,
                dropped: false,
            };
        }
        self.try_probe_channel_publish_bytes(
            name,
            message_type,
            bytes_of(value).as_slice(),
            published_at_ms,
        )
    }

    /// 返回当前 status 快照。
    pub fn status(&self) -> IntrospectionStatus {
        let inner = self.lock_inner();
        IntrospectionStatus {
            tick_count: inner.tick_count,
            channels: inner
                .channels
                .iter()
                .map(|(name, channel)| IntrospectionChannelStatus {
                    name: name.clone(),
                    message_type: channel.message_type.clone(),
                    published_count: channel.probe.snapshot().published_count,
                    last_payload_len: channel.probe.snapshot().payload.as_ref().map(Vec::len),
                    active_observers: channel.probe.active_count(),
                    dropped_samples: channel.probe.dropped_samples(),
                })
                .collect(),
            inputs: inner.inputs.values().cloned().collect(),
            routes: inner.routes.values().cloned().collect(),
            processes: inner.processes.values().cloned().collect(),
            io_boundaries: inner.io_boundaries.values().cloned().collect(),
            services: inner.services.values().cloned().collect(),
            operations: inner.operations.values().cloned().collect(),
            tasks: inner.tasks.values().cloned().collect(),
            lanes: inner.lanes.values().cloned().collect(),
            recorder: self.recorder.status(),
        }
    }

    /// 记录 supervisor 视角下的子进程健康状态。
    pub fn record_process_health(&self, status: IntrospectionProcessStatus) {
        let mut inner = self.lock_inner();
        inner.processes.insert(status.name.clone(), status);
    }

    /// 预注册一条 route，使 backend 选择原因和传输计数能进入 live status。
    pub fn register_route(&self, status: IntrospectionRouteStatus) {
        let mut inner = self.lock_inner();
        inner.routes.insert(status.name.clone(), status);
    }

    /// 记录 active input latest/presence/stale 状态。
    pub fn record_input_status(&self, status: IntrospectionInputStatus) {
        let key = input_status_key(&status);
        let mut inner = self.lock_inner();
        inner.inputs.insert(key, status);
    }

    /// 记录一次 generated shell input 读取结果。
    ///
    /// route/input 已预注册时只原地更新字段，避免高频 tick 路径反复分配诊断字符串。
    #[allow(clippy::too_many_arguments)]
    pub fn record_input_read(
        &self,
        key: &str,
        task: &str,
        input: &str,
        channel: &str,
        message_type: &str,
        present: bool,
        stale: bool,
        last_revision: Option<u64>,
        last_read_ms: Option<u64>,
    ) {
        let mut inner = self.lock_inner();
        let now = unix_time_ms();
        let (dropped_samples, backpressure_count, overflow_count) = inner
            .routes
            .get(channel)
            .map(|route| {
                (
                    route.dropped_samples,
                    route.backpressure_count,
                    route.overflow_count,
                )
            })
            .unwrap_or_default();
        let Some(status) = inner.inputs.get_mut(key) else {
            inner.inputs.insert(
                key.to_string(),
                IntrospectionInputStatus {
                    task: task.to_string(),
                    input: input.to_string(),
                    channel: channel.to_string(),
                    message_type: message_type.to_string(),
                    present,
                    stale,
                    last_revision,
                    last_read_ms,
                    updated_unix_ms: Some(now),
                    dropped_samples,
                    backpressure_count,
                    overflow_count,
                },
            );
            return;
        };
        status.present = present;
        status.stale = stale;
        status.last_revision = last_revision;
        status.last_read_ms = last_read_ms;
        status.updated_unix_ms = Some(now);
        status.dropped_samples = dropped_samples;
        status.backpressure_count = backpressure_count;
        status.overflow_count = overflow_count;
    }

    /// 记录 route 成功发布或进入传输路径。
    pub fn record_route_publish(&self, name: impl AsRef<str>, published_at_ms: Option<u64>) {
        let mut inner = self.lock_inner();
        let route = route_entry(&mut inner, name.as_ref());
        route.published_count = route.published_count.saturating_add(1);
        route.last_publish_ms = published_at_ms;
    }

    /// 记录 route drop。
    pub fn record_route_drop(&self, name: impl AsRef<str>) {
        let mut inner = self.lock_inner();
        let route = route_entry(&mut inner, name.as_ref());
        route.dropped_samples = route.dropped_samples.saturating_add(1);
    }

    /// 记录 route backpressure。
    pub fn record_route_backpressure(&self, name: impl AsRef<str>) {
        let mut inner = self.lock_inner();
        let route = route_entry(&mut inner, name.as_ref());
        route.backpressure_count = route.backpressure_count.saturating_add(1);
    }

    /// 记录 route overflow。
    pub fn record_route_overflow(&self, name: impl AsRef<str>) {
        let mut inner = self.lock_inner();
        let route = route_entry(&mut inner, name.as_ref());
        route.overflow_count = route.overflow_count.saturating_add(1);
    }

    /// 记录 route/backend 最近错误。
    pub fn record_route_error(&self, name: impl AsRef<str>, error: impl Into<String>) {
        let mut inner = self.lock_inner();
        let route = route_entry(&mut inner, name.as_ref());
        route.last_error = Some(error.into());
    }

    /// 预注册一个 I/O boundary，使其在尚未上报 health 前也出现在 status 中。
    pub fn register_io_boundary(
        &self,
        name: impl Into<String>,
        component: impl Into<String>,
        resources: Vec<IntrospectionIoBoundaryResourceStatus>,
    ) {
        let name = name.into();
        let component = component.into();
        let mut inner = self.lock_inner();
        inner
            .io_boundaries
            .entry(name.clone())
            .or_insert_with(|| IntrospectionIoBoundaryStatus {
                name,
                component,
                ready: false,
                healthy: true,
                last_error: None,
                resources,
                updated_unix_ms: None,
            });
    }

    /// 标记 I/O boundary readiness。
    pub fn mark_io_boundary_ready(&self, name: &str, ready: bool) {
        let mut inner = self.lock_inner();
        let boundary = io_boundary_entry(&mut inner, name);
        boundary.ready = ready;
        boundary.updated_unix_ms = Some(unix_time_ms());
    }

    /// 记录 I/O boundary 恢复健康。
    pub fn record_io_boundary_healthy(&self, name: &str) {
        let mut inner = self.lock_inner();
        let boundary = io_boundary_entry(&mut inner, name);
        boundary.healthy = true;
        boundary.last_error = None;
        boundary.updated_unix_ms = Some(unix_time_ms());
    }

    /// 记录 I/O boundary 错误。
    pub fn record_io_boundary_error(&self, name: &str, error: impl Into<String>) {
        let mut inner = self.lock_inner();
        let boundary = io_boundary_entry(&mut inner, name);
        boundary.healthy = false;
        boundary.last_error = Some(error.into());
        boundary.updated_unix_ms = Some(unix_time_ms());
    }

    /// 记录 I/O boundary resource readiness。
    pub fn record_io_boundary_resource_ready(
        &self,
        boundary_name: &str,
        resource_name: &str,
        ready: bool,
        message: Option<String>,
    ) {
        let mut inner = self.lock_inner();
        let boundary = io_boundary_entry(&mut inner, boundary_name);
        let now = unix_time_ms();
        let resource = io_boundary_resource_entry(boundary, resource_name);
        resource.ready = ready;
        resource.message = message;
        if ready {
            resource.last_error = None;
        }
        resource.updated_unix_ms = Some(now);
        boundary.updated_unix_ms = Some(now);
    }

    /// 记录 I/O boundary resource 错误。
    pub fn record_io_boundary_resource_error(
        &self,
        boundary_name: &str,
        resource_name: &str,
        error: impl Into<String>,
    ) {
        let mut inner = self.lock_inner();
        let boundary = io_boundary_entry(&mut inner, boundary_name);
        let now = unix_time_ms();
        let resource = io_boundary_resource_entry(boundary, resource_name);
        resource.ready = false;
        resource.last_error = Some(error.into());
        resource.updated_unix_ms = Some(now);
        boundary.healthy = false;
        boundary.updated_unix_ms = Some(now);
    }

    /// 预注册一个 service endpoint，使其在尚未收到请求时也出现在 status 中。
    pub fn register_service(&self, name: impl Into<String>) {
        let name = name.into();
        let mut inner = self.lock_inner();
        inner
            .services
            .entry(name.clone())
            .or_insert_with(|| IntrospectionServiceStatus {
                name,
                ready: false,
                in_flight: 0,
                queued: 0,
                total_requests: 0,
                timeout_count: 0,
                busy_count: 0,
                unavailable_count: 0,
                late_drop_count: 0,
            });
    }

    /// 标记预注册 service 已完成 lifecycle startup，可被 readiness gate 视为可用。
    pub fn mark_service_ready(&self, name: impl AsRef<str>) {
        let mut inner = self.lock_inner();
        if let Some(service) = inner.services.get_mut(name.as_ref()) {
            service.ready = true;
        }
    }

    /// 记录 service 运行态健康状态快照。
    pub fn record_service_health(&self, status: IntrospectionServiceStatus) {
        let name = status.name.clone();
        let payload = serde_json::json!({
            "ready": status.ready,
            "in_flight": status.in_flight,
            "queued": status.queued,
            "total_requests": status.total_requests,
        });
        let mut inner = self.lock_inner();
        inner.services.insert(name.clone(), status);
        drop(inner);
        self.recorder
            .record_service_event_json(&name, "service_health", payload);
    }

    /// 预注册一个 operation endpoint，使其在尚未收到 goal 时也出现在 status 中。
    pub fn register_operation(&self, name: impl Into<String>) {
        let name = name.into();
        let mut inner = self.lock_inner();
        inner
            .operations
            .entry(name.clone())
            .or_insert_with(|| IntrospectionOperationStatus {
                name,
                ready: true,
                ..Default::default()
            });
    }

    /// 记录 operation 运行态健康状态快照。
    pub fn record_operation_health(&self, status: IntrospectionOperationStatus) {
        let status_for_record = status.clone();
        let mut inner = self.lock_inner();
        inner.operations.insert(status.name.clone(), status);
        drop(inner);
        self.recorder.record_operation_event_json(
            &status_for_record.name,
            "flowrt.operation.status",
            serde_json::json!({
                "ready": status_for_record.ready,
                "running": status_for_record.running,
                "queued": status_for_record.queued,
                "current_operation_ids": status_for_record.current_operation_ids,
                "total_started": status_for_record.total_started,
                "succeeded": status_for_record.succeeded_count,
                "failed": status_for_record.failed_count,
                "canceled": status_for_record.canceled_count,
                "timeout": status_for_record.timeout_count,
                "preempted": status_for_record.preempted_count,
                "last_transition_ms": status_for_record.last_transition_ms,
            }),
        );
    }

    /// 请求取消指定 operation invocation。
    pub fn cancel_operation(
        &self,
        operation_id: &str,
    ) -> std::result::Result<IntrospectionOperationStatus, String> {
        let mut inner = self.lock_inner();
        for operation in inner.operations.values_mut() {
            let Some(position) = operation
                .current_operation_ids
                .iter()
                .position(|id| id == operation_id)
            else {
                continue;
            };
            if operation.running == 0 {
                return Err(format!(
                    "FlowRT operation `{operation_id}` is already finished"
                ));
            }
            operation.current_operation_ids.remove(position);
            operation.running = operation.running.saturating_sub(1);
            operation.canceled_count = operation.canceled_count.saturating_add(1);
            operation.last_transition_ms = Some(unix_time_ms());
            return Ok(operation.clone());
        }
        Err(format!("unknown FlowRT operation `{operation_id}`"))
    }

    /// 记录 task 调度健康快照。
    pub fn record_task_health(&self, health: IntrospectionTaskHealth) {
        let mut inner = self.lock_inner();
        inner.tasks.insert(health.name.clone(), health.clone());
        drop(inner);
        self.recorder.record_scheduler_event_json(
            RecordEntityKind::Task,
            &health.name,
            "flowrt.scheduler.task_health",
            serde_json::json!({
                "lane": health.lane,
                "deadline_missed": health.deadline_missed,
                "stale_input": health.stale_input,
                "backpressure": health.backpressure,
                "overflow": health.overflow,
                "fairness_violations": health.fairness_violations,
                "run_count": health.run_count,
                "success_count": health.success_count,
                "consecutive_failures": health.consecutive_failures,
                "last_run_ms": health.last_run_ms,
                "last_success_ms": health.last_success_ms,
            }),
        );
    }

    /// 记录 lane 调度健康快照。
    pub fn record_lane_health(&self, health: IntrospectionLaneHealth) {
        let mut inner = self.lock_inner();
        inner.lanes.insert(health.name.clone(), health.clone());
        drop(inner);
        self.recorder.record_scheduler_event_json(
            RecordEntityKind::Lane,
            &health.name,
            "flowrt.scheduler.lane_health",
            serde_json::json!({
                "queue_depth": health.queue_depth,
                "dispatched_count": health.dispatched_count,
                "fairness_violations": health.fairness_violations,
            }),
        );
    }

    /// 返回指定 task 的调度健康快照。
    pub fn task_health(&self, name: &str) -> Option<IntrospectionTaskHealth> {
        let inner = self.lock_inner();
        inner.tasks.get(name).cloned()
    }

    /// 返回指定 lane 的调度健康快照。
    pub fn lane_health(&self, name: &str) -> Option<IntrospectionLaneHealth> {
        let inner = self.lock_inner();
        inner.lanes.get(name).cloned()
    }

    /// 返回指定 channel 的 raw ABI snapshot。
    pub fn channel_snapshot(&self, name: &str) -> Option<IntrospectionChannelSnapshot> {
        let inner = self.lock_inner();
        inner
            .channels
            .get(name)
            .map(|channel| channel.probe.snapshot())
    }

    fn channel_status(&self, name: &str) -> Option<IntrospectionChannelStatus> {
        let inner = self.lock_inner();
        inner.channels.get(name).map(|channel| {
            let snapshot = channel.probe.snapshot();
            IntrospectionChannelStatus {
                name: name.to_string(),
                message_type: channel.message_type.clone(),
                published_count: snapshot.published_count,
                last_payload_len: snapshot.payload.as_ref().map(Vec::len),
                active_observers: channel.probe.active_count(),
                dropped_samples: channel.probe.dropped_samples(),
            }
        })
    }

    /// 返回参数状态列表。
    pub fn params(&self) -> Vec<IntrospectionParamStatus> {
        let inner = self.lock_inner();
        inner
            .params
            .iter()
            .map(|(name, param)| param_status(name, param))
            .collect()
    }

    /// 返回单个参数状态。
    pub fn param(&self, name: &str) -> Option<IntrospectionParamStatus> {
        let inner = self.lock_inner();
        inner
            .params
            .get(name)
            .map(|param| param_status(name, param))
    }

    /// 设置参数 pending 值。
    pub fn set_param_pending(
        &self,
        name: &str,
        value: serde_json::Value,
    ) -> std::result::Result<IntrospectionParamStatus, String> {
        let mut inner = self.lock_inner();
        let Some(param) = inner.params.get_mut(name) else {
            return Err(format!("unknown FlowRT parameter `{name}`"));
        };
        if param.update != "on_tick" {
            return Err(format!("FlowRT parameter `{name}` is startup-only"));
        }
        validate_param_json_value(name, param, &value)?;
        param.pending = Some(value);
        let status = param_status(name, param);
        drop(inner);
        self.recorder.record_param_event_json(
            name,
            "flowrt.param.set_pending",
            serde_json::json!({ "pending": status.pending }),
        );
        Ok(status)
    }

    /// 读取并清空参数 pending 值，供 generated shell 在 tick 边界应用。
    pub fn take_pending_param(&self, name: &str) -> Option<serde_json::Value> {
        let mut inner = self.lock_inner();
        inner
            .params
            .get_mut(name)
            .and_then(|param| param.pending.take())
    }

    /// 查询参数 pending 值，主要用于测试和 generated shell 快速检查。
    pub fn pending_param(&self, name: &str) -> Option<serde_json::Value> {
        let inner = self.lock_inner();
        inner
            .params
            .get(name)
            .and_then(|param| param.pending.clone())
    }

    /// 记录参数已经由 generated shell 应用为当前值。
    pub fn record_param_applied(&self, name: &str, value: serde_json::Value) {
        let mut inner = self.lock_inner();
        if let Some(param) = inner.params.get_mut(name) {
            param.current = value.clone();
            param.pending = None;
            drop(inner);
            self.recorder.record_param_event_json(
                name,
                "flowrt.param.applied",
                serde_json::json!({ "current": value }),
            );
        }
    }
}

/// 生成 handshake 的输入元数据。
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

/// 返回当前用户 runtime socket 目录。
///
/// 优先使用 `$XDG_RUNTIME_DIR/flowrt`；没有时 fallback 到 `/tmp/flowrt.<uid>`，避免不同用户
/// 的同名 PID socket 互相污染。
pub fn runtime_socket_dir() -> PathBuf {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("flowrt");
    }
    PathBuf::from(format!("/tmp/flowrt.{}", current_uid()))
}

/// 返回当前进程默认 runtime socket 路径。
pub fn runtime_socket_path_for_pid(pid: u32) -> PathBuf {
    runtime_socket_dir().join(format!("{pid}.sock"))
}

/// 扫描当前用户 runtime socket 目录中的 FlowRT socket 候选。
pub fn discover_runtime_sockets() -> std::io::Result<Vec<PathBuf>> {
    let dir = runtime_socket_dir();
    let mut sockets = Vec::new();
    match fs::read_dir(&dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let path = entry.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "sock")
                {
                    sockets.push(path);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    sockets.sort();
    Ok(sockets)
}

/// 启动一个最小 introspection status 服务。
pub fn spawn_status_server(
    identity: IntrospectionIdentity,
    state: IntrospectionState,
) -> std::io::Result<IntrospectionServer> {
    let handshake = identity.handshake();
    let path = runtime_socket_path_for_pid(handshake.pid);
    spawn_status_server_at(path, handshake, state)
}

/// 在指定路径启动一个最小 introspection status 服务，主要用于测试。
pub fn spawn_status_server_at(
    path: PathBuf,
    handshake: IntrospectionHandshake,
    state: IntrospectionState,
) -> std::io::Result<IntrospectionServer> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    reclaim_stale_socket_path(&path)?;
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    let server_path = path.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let active_clients = Arc::new(AtomicUsize::new(0));
    let active_observers = Arc::new(AtomicUsize::new(0));
    let handle = thread::Builder::new()
        .name("flowrt-introspection-server".to_string())
        .spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _addr)) => {
                        let Some(permit) = try_acquire_introspection_client_permit(
                            &active_clients,
                            MAX_INTROSPECTION_CLIENT_THREADS,
                        ) else {
                            let _ = write_error_response(
                                stream,
                                &handshake,
                                "FlowRT introspection connection limit reached",
                            );
                            continue;
                        };
                        let handshake = handshake.clone();
                        let state = state.clone();
                        let active_observers = Arc::clone(&active_observers);
                        let _ = thread::Builder::new()
                            .name("flowrt-introspection-client".to_string())
                            .spawn(move || {
                                let _ = handle_connection(
                                    stream,
                                    &handshake,
                                    &state,
                                    permit,
                                    active_observers,
                                );
                            });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        })?;
    Ok(IntrospectionServer {
        path: server_path,
        handle: Some(handle),
        stop,
    })
}

struct IntrospectionClientPermit {
    active_clients: Arc<AtomicUsize>,
}

impl Drop for IntrospectionClientPermit {
    fn drop(&mut self) {
        self.active_clients.fetch_sub(1, Ordering::AcqRel);
    }
}

fn try_acquire_introspection_client_permit(
    active_clients: &Arc<AtomicUsize>,
    max_clients: usize,
) -> Option<IntrospectionClientPermit> {
    let previous = active_clients.fetch_add(1, Ordering::AcqRel);
    if previous < max_clients {
        Some(IntrospectionClientPermit {
            active_clients: Arc::clone(active_clients),
        })
    } else {
        active_clients.fetch_sub(1, Ordering::AcqRel);
        None
    }
}

fn reclaim_stale_socket_path(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    match UnixStream::connect(path) {
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            format!("FlowRT runtime socket `{}` is already live", path.display()),
        )),
        Err(_) => fs::remove_file(path),
    }
}

/// 已启动的 introspection 服务。
#[derive(Debug)]
pub struct IntrospectionServer {
    path: PathBuf,
    handle: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl IntrospectionServer {
    /// 返回服务 socket 路径。
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for IntrospectionServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = fs::remove_file(&self.path);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// 向 introspection socket 请求 status。
pub fn request_status(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::Status)
}

/// 向 introspection socket 请求 status，并限制 socket 读写等待时间。
pub fn request_status_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::Status, timeout)
}

/// 向 introspection socket 请求 channel snapshot。
pub fn request_channel_snapshot(
    path: &Path,
    channel: impl Into<String>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::ChannelSnapshot {
            channel: channel.into(),
        },
    )
}

/// 向 introspection socket 请求 channel snapshot，并限制 socket 读写等待时间。
pub fn request_channel_snapshot_with_timeout(
    path: &Path,
    channel: impl Into<String>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::ChannelSnapshot {
            channel: channel.into(),
        },
        timeout,
    )
}

/// 向 introspection socket 请求 self-description JSON。
pub fn request_self_description(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::SelfDescription)
}

/// 向 introspection socket 请求 self-description JSON，并限制 socket 读写等待时间。
pub fn request_self_description_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::SelfDescription, timeout)
}

/// 向 introspection socket 打开 observe channel 连接。
pub fn observe_channel_stream(
    path: &Path,
    channel: impl Into<String>,
) -> std::io::Result<(UnixStream, IntrospectionResponse)> {
    let mut stream = UnixStream::connect(path)?;
    let request = serde_json::to_string(&IntrospectionRequest::ObserveChannel {
        channel: channel.into(),
        mode: Some("latest".to_string()),
    })
    .map_err(std::io::Error::other)?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    if line.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "FlowRT observe channel response was empty",
        ));
    }
    let response = serde_json::from_slice(&line).map_err(std::io::Error::other)?;
    Ok((stream, response))
}

/// 向 introspection socket 打开 observe channel 连接，并限制握手读写等待时间。
pub fn observe_channel_stream_with_timeout(
    path: &Path,
    channel: impl Into<String>,
    timeout: Duration,
) -> std::io::Result<(UnixStream, IntrospectionResponse)> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let request = serde_json::to_string(&IntrospectionRequest::ObserveChannel {
        channel: channel.into(),
        mode: Some("latest".to_string()),
    })
    .map_err(std::io::Error::other)?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                line.push(byte[0]);
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    if line.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "FlowRT observe channel response was empty",
        ));
    }
    let response = serde_json::from_slice(&line).map_err(std::io::Error::other)?;
    stream.set_read_timeout(None)?;
    stream.set_write_timeout(None)?;
    Ok((stream, response))
}

/// 向 introspection socket 请求参数列表。
pub fn request_param_list(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::ParamList)
}

/// 向 introspection socket 请求参数列表，并限制 socket 读写等待时间。
pub fn request_param_list_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::ParamList, timeout)
}

/// 向 introspection socket 请求单个参数状态。
pub fn request_param_get(
    path: &Path,
    name: impl Into<String>,
) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::ParamGet { name: name.into() })
}

/// 向 introspection socket 请求单个参数状态，并限制 socket 读写等待时间。
pub fn request_param_get_with_timeout(
    path: &Path,
    name: impl Into<String>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::ParamGet { name: name.into() },
        timeout,
    )
}

/// 向 introspection socket 写入参数 pending 值。
pub fn request_param_set(
    path: &Path,
    name: impl Into<String>,
    value: serde_json::Value,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::ParamSet {
            name: name.into(),
            value,
        },
    )
}

/// 向 introspection socket 写入参数 pending 值，并限制 socket 读写等待时间。
pub fn request_param_set_with_timeout(
    path: &Path,
    name: impl Into<String>,
    value: serde_json::Value,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::ParamSet {
            name: name.into(),
            value,
        },
        timeout,
    )
}

/// 向 introspection socket 请求注入 island boundary input。
pub fn request_boundary_publish(
    path: &Path,
    endpoint: impl Into<String>,
    payload: Vec<u8>,
    published_at_ms: Option<u64>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::BoundaryPublish {
            endpoint: endpoint.into(),
            payload,
            published_at_ms,
        },
    )
}

/// 向 introspection socket 请求注入 island boundary input，并限制 socket 等待时间。
pub fn request_boundary_publish_with_timeout(
    path: &Path,
    endpoint: impl Into<String>,
    payload: Vec<u8>,
    published_at_ms: Option<u64>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::BoundaryPublish {
            endpoint: endpoint.into(),
            payload,
            published_at_ms,
        },
        timeout,
    )
}

/// 向 introspection socket 请求取消 operation invocation。
pub fn request_operation_cancel(
    path: &Path,
    operation_id: impl Into<String>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::OperationCancel {
            operation_id: operation_id.into(),
        },
    )
}

/// 向 introspection socket 请求取消 operation invocation，并限制 socket 读写等待时间。
pub fn request_operation_cancel_with_timeout(
    path: &Path,
    operation_id: impl Into<String>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::OperationCancel {
            operation_id: operation_id.into(),
        },
        timeout,
    )
}

/// 向 introspection socket 请求启动 recorder。
pub fn request_recorder_start(
    path: &Path,
    output: Option<String>,
    filters: Vec<String>,
    queue_depth: Option<usize>,
) -> std::io::Result<IntrospectionResponse> {
    request(
        path,
        &IntrospectionRequest::RecorderStart {
            output,
            filters,
            queue_depth,
        },
    )
}

/// 向 introspection socket 请求启动 recorder，并限制 socket 读写等待时间。
pub fn request_recorder_start_with_timeout(
    path: &Path,
    output: Option<String>,
    filters: Vec<String>,
    queue_depth: Option<usize>,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(
        path,
        &IntrospectionRequest::RecorderStart {
            output,
            filters,
            queue_depth,
        },
        timeout,
    )
}

/// 向 introspection socket 请求停止 recorder。
pub fn request_recorder_stop(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::RecorderStop)
}

/// 向 introspection socket 请求停止 recorder，并限制 socket 读写等待时间。
pub fn request_recorder_stop_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::RecorderStop, timeout)
}

/// 向 introspection socket 请求取走 recorder 暂存事件。
pub fn request_recorder_drain(path: &Path) -> std::io::Result<IntrospectionResponse> {
    request(path, &IntrospectionRequest::RecorderDrain)
}

/// 向 introspection socket 请求取走 recorder 暂存事件，并限制 socket 读写等待时间。
pub fn request_recorder_drain_with_timeout(
    path: &Path,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    request_with_timeout(path, &IntrospectionRequest::RecorderDrain, timeout)
}

fn request(path: &Path, request: &IntrospectionRequest) -> std::io::Result<IntrospectionResponse> {
    let mut stream = UnixStream::connect(path)?;
    send_request_and_read_response(&mut stream, request)
}

fn request_with_timeout(
    path: &Path,
    request: &IntrospectionRequest,
    timeout: Duration,
) -> std::io::Result<IntrospectionResponse> {
    let mut stream = UnixStream::connect(path)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    send_request_and_read_response(&mut stream, request)
}

fn send_request_and_read_response(
    stream: &mut UnixStream,
    request: &IntrospectionRequest,
) -> std::io::Result<IntrospectionResponse> {
    let request = serde_json::to_string(request).map_err(std::io::Error::other)?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

fn write_error_response(
    mut stream: UnixStream,
    handshake: &IntrospectionHandshake,
    message: impl Into<String>,
) -> std::io::Result<()> {
    let response = IntrospectionResponse::Error {
        handshake: handshake.clone(),
        message: message.into(),
    };
    stream.write_all(
        serde_json::to_string(&response)
            .map_err(std::io::Error::other)?
            .as_bytes(),
    )?;
    stream.write_all(b"\n")
}

fn handle_connection(
    stream: UnixStream,
    handshake: &IntrospectionHandshake,
    state: &IntrospectionState,
    initial_permit: IntrospectionClientPermit,
    active_observers: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    let reader_stream = stream.try_clone()?;
    reader_stream.set_read_timeout(Some(INTROSPECTION_INITIAL_REQUEST_TIMEOUT))?;
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let request =
        serde_json::from_str::<IntrospectionRequest>(&line).map_err(std::io::Error::other)?;
    let _client_permit = initial_permit;
    stream.set_write_timeout(Some(INTROSPECTION_RESPONSE_WRITE_TIMEOUT))?;
    match request {
        IntrospectionRequest::Status => {
            let response = IntrospectionResponse::Status {
                handshake: handshake.clone(),
                status: state.status(),
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::SelfDescription => {
            let response = match state.self_description_json() {
                Some(json) => IntrospectionResponse::SelfDescription {
                    handshake: handshake.clone(),
                    json,
                },
                None => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "FlowRT self-description is not registered".to_string(),
                },
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::ChannelSnapshot { channel } => {
            let Some(channel) = state.channel_snapshot(&channel) else {
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "unknown FlowRT channel".to_string(),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let response = IntrospectionResponse::ChannelSnapshot {
                handshake: handshake.clone(),
                channel,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::ObserveChannel { channel, .. } => {
            let Some(observer_permit) = try_acquire_introspection_client_permit(
                &active_observers,
                MAX_INTROSPECTION_OBSERVERS,
            ) else {
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "FlowRT introspection observe connection limit reached".to_string(),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let Some(guard) = state.observe_channel(&channel) else {
                drop(observer_permit);
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "unknown FlowRT channel".to_string(),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let Some(channel_status) = state.channel_status(&channel) else {
                drop(observer_permit);
                drop(guard);
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: "unknown FlowRT channel".to_string(),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let response = IntrospectionResponse::ObserveReady {
                handshake: handshake.clone(),
                channel: channel_status,
            };
            let mut writer = stream.try_clone()?;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            stream.set_read_timeout(None)?;
            let mut reader = BufReader::new(stream);
            let _observer_permit = observer_permit;
            loop {
                let mut keepalive = String::new();
                match reader.read_line(&mut keepalive) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            drop(guard);
        }
        IntrospectionRequest::ParamList => {
            let response = IntrospectionResponse::ParamList {
                handshake: handshake.clone(),
                params: state.params(),
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::ParamGet { name } => {
            let Some(param) = state.param(&name) else {
                let response = IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message: format!("unknown FlowRT parameter `{name}`"),
                };
                let mut writer = stream;
                writer.write_all(
                    serde_json::to_string(&response)
                        .map_err(std::io::Error::other)?
                        .as_bytes(),
                )?;
                writer.write_all(b"\n")?;
                return Ok(());
            };
            let response = IntrospectionResponse::ParamValue {
                handshake: handshake.clone(),
                param,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::ParamSet { name, value } => {
            let response = match state.set_param_pending(&name, value) {
                Ok(param) => IntrospectionResponse::ParamValue {
                    handshake: handshake.clone(),
                    param,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::BoundaryPublish {
            endpoint,
            payload,
            published_at_ms,
        } => {
            let response = match state.publish_boundary_input(&endpoint, payload, published_at_ms) {
                Ok(boundary) => IntrospectionResponse::BoundaryPublish {
                    handshake: handshake.clone(),
                    boundary,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::OperationCancel { operation_id } => {
            let response = match state.cancel_operation(&operation_id) {
                Ok(operation) => IntrospectionResponse::OperationValue {
                    handshake: handshake.clone(),
                    operation,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::RecorderStart {
            output,
            filters,
            queue_depth,
        } => {
            let recorder = state.start_recorder(IntrospectionRecorderStart {
                output,
                filters,
                queue_depth,
                package: handshake.package.clone(),
                process: handshake.process.clone(),
                runtime_pid: handshake.pid,
                selfdesc_hash: handshake.self_description_hash.clone(),
            });
            let response = IntrospectionResponse::RecorderValue {
                handshake: handshake.clone(),
                recorder,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::RecorderStop => {
            let recorder = state.stop_recorder();
            let response = IntrospectionResponse::RecorderValue {
                handshake: handshake.clone(),
                recorder,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
        IntrospectionRequest::RecorderDrain => {
            let events = state.drain_recorder_events();
            let recorder = state.status().recorder;
            let response = IntrospectionResponse::RecorderEvents {
                handshake: handshake.clone(),
                recorder,
                events,
            };
            let mut writer = stream;
            writer.write_all(
                serde_json::to_string(&response)
                    .map_err(std::io::Error::other)?
                    .as_bytes(),
            )?;
            writer.write_all(b"\n")?;
        }
    }
    Ok(())
}

fn param_status(name: &str, param: &ParamState) -> IntrospectionParamStatus {
    IntrospectionParamStatus {
        name: name.to_string(),
        ty: param.ty.clone(),
        update: param.update.clone(),
        current: param.current.clone(),
        pending: param.pending.clone(),
        min: param.min.clone(),
        max: param.max.clone(),
        choices: param.choices.clone(),
    }
}

fn validate_param_json_value(
    name: &str,
    param: &ParamState,
    value: &serde_json::Value,
) -> std::result::Result<(), String> {
    if !json_value_matches_param_type(&param.ty, value) {
        return Err(format!(
            "FlowRT parameter `{name}` expects `{}` value",
            param.ty
        ));
    }
    if let Some(min) = &param.min
        && compare_param_json_values(&param.ty, value, min).is_some_and(|ordering| ordering.is_lt())
    {
        return Err(format!("FlowRT parameter `{name}` is below minimum"));
    }
    if let Some(max) = &param.max
        && compare_param_json_values(&param.ty, value, max).is_some_and(|ordering| ordering.is_gt())
    {
        return Err(format!("FlowRT parameter `{name}` is above maximum"));
    }
    if !param.choices.is_empty() && !param.choices.iter().any(|choice| choice == value) {
        return Err(format!(
            "FlowRT parameter `{name}` is not in declared enum choices"
        ));
    }
    Ok(())
}

fn json_value_matches_param_type(ty: &str, value: &serde_json::Value) -> bool {
    match ty {
        "bool" => value.is_boolean(),
        "string" => value.is_string(),
        "f32" | "f64" => value.is_number(),
        "u8" | "u16" | "u32" | "u64" => value.as_u64().is_some(),
        "i8" | "i16" | "i32" | "i64" => value.as_i64().is_some(),
        "array" => value.is_array(),
        "table" => value.is_object(),
        _ => false,
    }
}

fn compare_param_json_values(
    ty: &str,
    left: &serde_json::Value,
    right: &serde_json::Value,
) -> Option<std::cmp::Ordering> {
    match ty {
        "u8" | "u16" | "u32" | "u64" => {
            return Some(left.as_u64()?.cmp(&right.as_u64()?));
        }
        "i8" | "i16" | "i32" | "i64" => {
            return Some(left.as_i64()?.cmp(&right.as_i64()?));
        }
        _ => {}
    }
    match (left, right) {
        (serde_json::Value::Number(left), serde_json::Value::Number(right)) => {
            left.as_f64()?.partial_cmp(&right.as_f64()?)
        }
        (serde_json::Value::String(left), serde_json::Value::String(right)) => {
            Some(left.cmp(right))
        }
        _ => None,
    }
}

fn bytes_of<T: Copy>(value: &T) -> Vec<u8> {
    let mut bytes = vec![0u8; std::mem::size_of::<T>()];
    unsafe {
        // T: Copy 且只读取对象表示；这些 bytes 仅用于诊断快照，不反序列化成新所有权值。
        std::ptr::copy_nonoverlapping(
            (value as *const T).cast::<u8>(),
            bytes.as_mut_ptr(),
            bytes.len(),
        );
    }
    bytes
}

fn io_boundary_entry<'a>(
    inner: &'a mut IntrospectionStateInner,
    name: &str,
) -> &'a mut IntrospectionIoBoundaryStatus {
    inner
        .io_boundaries
        .entry(name.to_string())
        .or_insert_with(|| IntrospectionIoBoundaryStatus {
            name: name.to_string(),
            component: String::new(),
            ready: false,
            healthy: true,
            last_error: None,
            resources: Vec::new(),
            updated_unix_ms: None,
        })
}

fn route_entry<'a>(
    inner: &'a mut IntrospectionStateInner,
    name: &str,
) -> &'a mut IntrospectionRouteStatus {
    inner
        .routes
        .entry(name.to_string())
        .or_insert_with(|| IntrospectionRouteStatus {
            name: name.to_string(),
            ..Default::default()
        })
}

fn input_status_key(status: &IntrospectionInputStatus) -> String {
    if status.task.is_empty() {
        status.input.clone()
    } else if status.input.is_empty() {
        status.task.clone()
    } else {
        format!("{}.{}", status.task, status.input)
    }
}

fn io_boundary_resource_entry<'a>(
    boundary: &'a mut IntrospectionIoBoundaryStatus,
    resource_name: &str,
) -> &'a mut IntrospectionIoBoundaryResourceStatus {
    if let Some(index) = boundary
        .resources
        .iter()
        .position(|resource| resource.name == resource_name)
    {
        return &mut boundary.resources[index];
    }
    boundary
        .resources
        .push(IntrospectionIoBoundaryResourceStatus {
            name: resource_name.to_string(),
            kind: String::new(),
            ready: false,
            message: None,
            last_error: None,
            updated_unix_ms: None,
        });
    boundary
        .resources
        .last_mut()
        .expect("resource was just pushed")
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(unix)]
fn current_uid() -> u32 {
    unsafe { libc_getuid() }
}

#[cfg(unix)]
unsafe extern "C" {
    fn getuid() -> u32;
}

#[cfg(unix)]
unsafe fn libc_getuid() -> u32 {
    unsafe { getuid() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_uses_pid_name_under_runtime_dir() {
        let dir = runtime_socket_dir();
        let path = runtime_socket_path_for_pid(1234);

        assert_eq!(path, dir.join("1234.sock"));
    }

    #[test]
    fn status_server_returns_handshake_and_snapshot() {
        let root =
            std::env::temp_dir().join(format!("flowrt-introspection-test-{}", std::process::id()));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        for _ in 0..7 {
            state.record_tick();
        }
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![1u8; 48], Some(7));
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![2u8; 48], Some(8));
        state.record_channel_publish_bytes("source.imu_to_sink.imu", "Imu", vec![3u8; 48], Some(9));

        let server = spawn_status_server_at(socket.clone(), handshake.clone(), state.clone())
            .expect("server should start");
        let IntrospectionResponse::Status {
            handshake: response_handshake,
            status,
        } = request_status(server.path()).expect("status request should succeed")
        else {
            panic!("status request returned wrong response")
        };

        assert_eq!(response_handshake, handshake);
        assert_eq!(status.tick_count, 7);
        assert_eq!(
            status.channels,
            vec![IntrospectionChannelStatus {
                name: "source.imu_to_sink.imu".to_string(),
                message_type: "Imu".to_string(),
                published_count: 3,
                last_payload_len: Some(48),
                active_observers: 0,
                dropped_samples: 0,
            }]
        );

        state.record_tick();
        let IntrospectionResponse::Status { status, .. } =
            request_status(server.path()).expect("second status request should succeed")
        else {
            panic!("status request returned wrong response")
        };
        assert_eq!(status.tick_count, 8);

        let IntrospectionResponse::ChannelSnapshot { channel, .. } =
            request_channel_snapshot(server.path(), "source.imu_to_sink.imu")
                .expect("snapshot request should succeed")
        else {
            panic!("snapshot request returned wrong response")
        };
        assert_eq!(channel.published_count, 3);
        assert_eq!(channel.payload, Some(vec![3u8; 48]));
        assert_eq!(channel.published_at_ms, Some(9));
        let channel_json = serde_json::to_value(&channel).unwrap();
        assert!(channel_json.get("name").is_none());
        assert!(channel_json.get("message_type").is_none());

        let IntrospectionResponse::Error { message, .. } =
            request_channel_snapshot(server.path(), "missing.channel")
                .expect("missing channel should return structured error response")
        else {
            panic!("missing channel request returned wrong response")
        };
        assert_eq!(message, "unknown FlowRT channel");

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_server_returns_registered_self_description_json() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-selfdesc-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.set_self_description_json(r#"{"package":{"name":"robot_demo"}}"#);
        let server = spawn_status_server_at(socket.clone(), handshake.clone(), state)
            .expect("server should start");

        let IntrospectionResponse::SelfDescription {
            handshake: response_handshake,
            json,
        } = request_self_description(server.path())
            .expect("self-description request should succeed")
        else {
            panic!("self-description request returned wrong response")
        };

        assert_eq!(response_handshake, handshake);
        assert_eq!(json, r#"{"package":{"name":"robot_demo"}}"#);

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_includes_supervisor_process_health() {
        let state = IntrospectionState::new();

        state.record_process_health(IntrospectionProcessStatus {
            name: "sensors".to_string(),
            state: "running".to_string(),
            pid: Some(42),
            restart_count: 1,
            tick_count: Some(7),
            last_seen_unix_ms: Some(1000),
            tick_stale: false,
            exit_code: None,
            readiness_wait: None,
            resource_placement: None,
        });

        assert_eq!(
            state.status().processes,
            vec![IntrospectionProcessStatus {
                name: "sensors".to_string(),
                state: "running".to_string(),
                pid: Some(42),
                restart_count: 1,
                tick_count: Some(7),
                last_seen_unix_ms: Some(1000),
                tick_stale: false,
                exit_code: None,
                readiness_wait: None,
                resource_placement: None,
            }]
        );
    }

    #[test]
    fn status_server_reports_missing_self_description() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-missing-selfdesc-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let server = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
            .expect("server should start");

        let IntrospectionResponse::Error { message, .. } = request_self_description(server.path())
            .expect("missing self-description should return structured error")
        else {
            panic!("missing self-description request returned wrong response")
        };

        assert_eq!(message, "FlowRT self-description is not registered");

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn state_recovers_after_mutex_poison() {
        let state = IntrospectionState::new();
        let poison_state = state.clone();
        let poison_thread = thread::spawn(move || {
            let _guard = poison_state.inner.lock().unwrap();
            panic!("poison introspection state for test");
        });
        assert!(poison_thread.join().is_err());

        state.register_channel("source.count_to_sink.count", "Count");
        state.record_tick();
        state.record_channel_publish_bytes("source.count_to_sink.count", "Count", vec![7], Some(1));
        state.register_param(IntrospectionParamSchema {
            name: "controller.kp".to_string(),
            ty: "f32".to_string(),
            update: "on_tick".to_string(),
            current: serde_json::json!(1.0),
            min: None,
            max: None,
            choices: Vec::new(),
        });

        let status = state.status();
        assert_eq!(status.tick_count, 1);
        assert_eq!(status.channels.len(), 1);
        assert_eq!(
            state
                .channel_snapshot("source.count_to_sink.count")
                .unwrap()
                .payload,
            Some(vec![7])
        );
        assert!(
            state
                .set_param_pending("controller.kp", serde_json::json!(2.0))
                .is_ok()
        );
        assert_eq!(
            state.pending_param("controller.kp"),
            Some(serde_json::json!(2.0))
        );
        assert_eq!(
            state.take_pending_param("controller.kp"),
            Some(serde_json::json!(2.0))
        );
        state.record_param_applied("controller.kp", serde_json::json!(2.0));
        assert_eq!(
            state.param("controller.kp").unwrap().current,
            serde_json::json!(2.0)
        );
    }

    #[test]
    fn probe_recording_is_disabled_until_observer_guard_is_active() {
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");

        assert!(
            !state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[1, 2, 3, 4],
                    Some(10)
                )
                .recorded
        );
        let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
        assert_eq!(snapshot.published_count, 0);
        assert_eq!(snapshot.payload, None);

        let guard = state
            .observe_channel("source.imu_to_sink.imu")
            .expect("registered channel should be observable");
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(1));
        assert!(
            state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[5, 6, 7, 8],
                    Some(11)
                )
                .recorded
        );
        let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
        assert_eq!(snapshot.published_count, 0);
        assert_eq!(snapshot.payload, Some(vec![5, 6, 7, 8]));
        assert_eq!(snapshot.published_at_ms, Some(11));

        drop(guard);
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));
        assert!(
            !state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[9, 10, 11, 12],
                    Some(12)
                )
                .recorded
        );
        let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
        assert_eq!(snapshot.published_count, 0);
        assert_eq!(snapshot.payload, Some(vec![5, 6, 7, 8]));
    }

    #[test]
    fn publish_event_updates_status_count_without_payload_or_observer() {
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        let probe = state
            .channel_probe("source.imu_to_sink.imu")
            .expect("registered channel should expose probe");

        probe.record_publish_event();
        probe.record_publish_event();

        let snapshot = state.channel_snapshot("source.imu_to_sink.imu").unwrap();
        assert_eq!(snapshot.published_count, 2);
        assert_eq!(snapshot.payload, None);
        assert_eq!(snapshot.published_at_ms, None);
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));
    }

    #[test]
    fn bounded_probe_drops_oversized_payload_and_reports_drop_count() {
        let state = IntrospectionState::new();
        state.register_channel_with_probe_capacity("source.image_to_sink.image", "Image", Some(4));
        let guard = state
            .observe_channel("source.image_to_sink.image")
            .expect("registered channel should be observable");

        let record = state.try_probe_channel_publish_bytes(
            "source.image_to_sink.image",
            "Image",
            &[1, 2, 3, 4, 5],
            Some(10),
        );
        let snapshot = state
            .channel_snapshot("source.image_to_sink.image")
            .expect("registered channel should have snapshot state");
        let status = state
            .channel_status("source.image_to_sink.image")
            .expect("registered channel should have status");

        assert_eq!(
            record,
            IntrospectionProbeRecord {
                recorded: false,
                dropped: true,
            }
        );
        assert_eq!(snapshot.published_count, 0);
        assert_eq!(snapshot.payload.as_deref(), Some([].as_slice()));
        assert_eq!(status.active_observers, 1);
        assert_eq!(status.dropped_samples, 1);

        drop(guard);
        assert_eq!(
            state.active_probe_count("source.image_to_sink.image"),
            Some(0)
        );
    }

    #[test]
    fn observe_channel_socket_enables_probe_until_connection_closes() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-observe-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("server should start");

        let mut stream = UnixStream::connect(server.path()).unwrap();
        stream
            .write_all(
                br#"{"command":"observe_channel","channel":"source.imu_to_sink.imu","mode":"latest"}
"#,
            )
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.contains(r#""response":"observe_ready""#));

        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(1));
        assert!(
            state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[1, 2, 3],
                    Some(7)
                )
                .recorded
        );
        assert_eq!(
            state
                .channel_snapshot("source.imu_to_sink.imu")
                .unwrap()
                .payload,
            Some(vec![1, 2, 3])
        );

        drop(reader);
        drop(stream);
        for _ in 0..100 {
            if state.active_probe_count("source.imu_to_sink.imu") == Some(0) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn observe_unknown_channel_returns_error_without_enabling_probe() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-observe-missing-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("server should start");

        let (_stream, response) = observe_channel_stream(server.path(), "missing.channel")
            .expect("missing channel should return structured error");

        assert!(matches!(
            response,
            IntrospectionResponse::Error { message, .. } if message == "unknown FlowRT channel"
        ));
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));
        assert_eq!(state.active_probe_count("missing.channel"), None);

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn observe_channel_stream_helper_keeps_probe_enabled_until_stream_drops() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-observe-helper-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 43,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("server should start");

        let (stream, response) =
            observe_channel_stream(server.path(), "source.imu_to_sink.imu").unwrap();
        assert!(matches!(
            response,
            IntrospectionResponse::ObserveReady { .. }
        ));
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(1));
        assert!(
            state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu",
                    "Imu",
                    &[9, 8, 7],
                    Some(8)
                )
                .recorded
        );

        drop(stream);
        for _ in 0..100 {
            if state.active_probe_count("source.imu_to_sink.imu") == Some(0) {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(state.active_probe_count("source.imu_to_sink.imu"), Some(0));

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn observe_connections_do_not_exhaust_status_control_plane() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-observe-cap-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 44,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("server should start");

        let mut streams = Vec::new();
        for _ in 0..MAX_INTROSPECTION_OBSERVERS {
            let (stream, response) =
                observe_channel_stream(server.path(), "source.imu_to_sink.imu").unwrap();
            assert!(matches!(
                response,
                IntrospectionResponse::ObserveReady { .. }
            ));
            streams.push(stream);
        }
        assert_eq!(
            state.active_probe_count("source.imu_to_sink.imu"),
            Some(MAX_INTROSPECTION_OBSERVERS as u64)
        );

        let response = request_status_with_timeout(server.path(), Duration::from_millis(100))
            .expect("status request should remain available while observe streams are open");
        assert!(matches!(response, IntrospectionResponse::Status { .. }));

        let (_extra_stream, response) =
            observe_channel_stream(server.path(), "source.imu_to_sink.imu")
                .expect("excess observe should receive structured error");
        assert!(matches!(
            response,
            IntrospectionResponse::Error { message, .. }
                if message == "FlowRT introspection observe connection limit reached"
        ));

        drop(streams);
        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn idle_clients_are_closed_by_initial_request_timeout() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-idle-cap-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 45,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let server = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
            .expect("server should start");

        let mut idle_streams = Vec::new();
        for _ in 0..MAX_INTROSPECTION_CLIENT_THREADS {
            idle_streams.push(UnixStream::connect(server.path()).unwrap());
        }
        thread::sleep(INTROSPECTION_INITIAL_REQUEST_TIMEOUT + Duration::from_millis(100));

        let response = request_status_with_timeout(server.path(), Duration::from_millis(100))
            .expect("idle clients should time out and release connection slots");
        assert!(matches!(response, IntrospectionResponse::Status { .. }));

        drop(idle_streams);
        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_server_refuses_to_replace_live_socket() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-live-socket-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let first =
            spawn_status_server_at(socket.clone(), handshake.clone(), IntrospectionState::new())
                .expect("first server should start");

        let error = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
            .expect_err("live socket must not be replaced by a second server");

        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
        assert!(request_status(first.path()).is_ok());

        drop(first);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn introspection_client_permit_limits_and_releases_active_connections() {
        let active = Arc::new(AtomicUsize::new(0));
        let first = try_acquire_introspection_client_permit(&active, 1)
            .expect("first client should acquire permit");

        assert_eq!(active.load(Ordering::Acquire), 1);
        assert!(try_acquire_introspection_client_permit(&active, 1).is_none());
        assert_eq!(active.load(Ordering::Acquire), 1);

        drop(first);
        assert_eq!(active.load(Ordering::Acquire), 0);
        assert!(try_acquire_introspection_client_permit(&active, 1).is_some());
    }

    #[test]
    fn status_server_removes_stale_socket_file_before_binding() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-stale-socket-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        fs::create_dir_all(&root).unwrap();
        fs::write(&socket, b"stale").unwrap();
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };

        let server = spawn_status_server_at(socket.clone(), handshake, IntrospectionState::new())
            .expect("stale socket path should be reclaimed");

        assert!(request_status(server.path()).is_ok());
        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn status_server_handles_runtime_parameter_requests() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-params-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.register_param(IntrospectionParamSchema {
            name: "controller.kp".to_string(),
            ty: "f32".to_string(),
            update: "on_tick".to_string(),
            current: serde_json::json!(1.0),
            min: Some(serde_json::json!(0.0)),
            max: Some(serde_json::json!(10.0)),
            choices: Vec::new(),
        });
        state.register_param(IntrospectionParamSchema {
            name: "controller.mode".to_string(),
            ty: "string".to_string(),
            update: "startup".to_string(),
            current: serde_json::json!("normal"),
            min: None,
            max: None,
            choices: vec![serde_json::json!("normal"), serde_json::json!("safe")],
        });

        let server = spawn_status_server_at(socket.clone(), handshake.clone(), state.clone())
            .expect("server should start");

        let IntrospectionResponse::ParamList { params, .. } =
            request_param_list(server.path()).expect("param list request should succeed")
        else {
            panic!("param list returned wrong response")
        };
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "controller.kp");
        assert_eq!(params[0].current, serde_json::json!(1.0));
        assert!(params[0].pending.is_none());

        let IntrospectionResponse::ParamValue { param, .. } =
            request_param_get(server.path(), "controller.kp")
                .expect("param get request should succeed")
        else {
            panic!("param get returned wrong response")
        };
        assert_eq!(param.current, serde_json::json!(1.0));

        let IntrospectionResponse::ParamValue { param, .. } =
            request_param_set(server.path(), "controller.kp", serde_json::json!(2.5))
                .expect("param set request should succeed")
        else {
            panic!("param set returned wrong response")
        };
        assert_eq!(param.current, serde_json::json!(1.0));
        assert_eq!(param.pending, Some(serde_json::json!(2.5)));
        assert_eq!(
            state.pending_param("controller.kp"),
            Some(serde_json::json!(2.5))
        );
        state.record_param_applied("controller.kp", serde_json::json!(2.5));
        assert_eq!(state.pending_param("controller.kp"), None);

        let IntrospectionResponse::Error { message, .. } =
            request_param_set(server.path(), "controller.mode", serde_json::json!("safe"))
                .expect("startup param set should return structured error")
        else {
            panic!("startup param set returned wrong response")
        };
        assert_eq!(
            message,
            "FlowRT parameter `controller.mode` is startup-only"
        );

        let IntrospectionResponse::Error { message, .. } =
            request_param_set(server.path(), "controller.kp", serde_json::json!(12.0))
                .expect("out-of-range param set should return structured error")
        else {
            panic!("out-of-range param set returned wrong response")
        };
        assert_eq!(message, "FlowRT parameter `controller.kp` is above maximum");

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn param_runtime_validation_compares_large_integer_bounds_exactly() {
        let param = ParamState {
            ty: "u64".to_string(),
            update: "on_tick".to_string(),
            current: serde_json::json!(9007199254740992_u64),
            pending: None,
            min: None,
            max: Some(serde_json::json!(9007199254740992_u64)),
            choices: vec![],
        };

        let error = validate_param_json_value(
            "controller.limit",
            &param,
            &serde_json::json!(9007199254740993_u64),
        )
        .expect_err("value above a large integer max must be rejected exactly");

        assert_eq!(
            error,
            "FlowRT parameter `controller.limit` is above maximum"
        );
    }

    #[test]
    fn status_server_reports_and_cancels_operation_status() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-operation-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        state.register_operation("controller.plan");
        state.record_operation_health(IntrospectionOperationStatus {
            name: "controller.plan".to_string(),
            ready: true,
            running: 1,
            queued: 2,
            current_operation_ids: vec!["111:7:3".to_string()],
            total_started: 9,
            succeeded_count: 5,
            failed_count: 1,
            canceled_count: 0,
            timeout_count: 1,
            preempted_count: 0,
            last_transition_ms: Some(12345),
        });

        let server = spawn_status_server_at(socket.clone(), handshake.clone(), state.clone())
            .expect("server should start");

        let IntrospectionResponse::Status { status, .. } =
            request_status(server.path()).expect("operation status request should succeed")
        else {
            panic!("status returned wrong response")
        };
        assert_eq!(status.operations.len(), 1);
        assert_eq!(status.operations[0].name, "controller.plan");
        assert_eq!(status.operations[0].running, 1);
        assert_eq!(
            status.operations[0].current_operation_ids,
            vec!["111:7:3".to_string()]
        );

        let IntrospectionResponse::OperationValue { operation, .. } =
            request_operation_cancel(server.path(), "111:7:3")
                .expect("operation cancel request should succeed")
        else {
            panic!("operation cancel returned wrong response")
        };
        assert_eq!(operation.name, "controller.plan");
        assert_eq!(operation.running, 0);
        assert_eq!(operation.canceled_count, 1);
        assert!(operation.current_operation_ids.is_empty());

        let IntrospectionResponse::Error { message, .. } =
            request_operation_cancel(server.path(), "111:7:3")
                .expect("finished operation should return structured error")
        else {
            panic!("second operation cancel returned wrong response")
        };
        assert_eq!(message, "unknown FlowRT operation `111:7:3`");

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn task_health_recording_and_status_snapshot() {
        let state = IntrospectionState::new();

        state.record_task_health(IntrospectionTaskHealth {
            name: "imu_task".to_string(),
            lane: "sensor_lane".to_string(),
            deadline_missed: 3,
            stale_input: 1,
            backpressure: 0,
            overflow: 0,
            fairness_violations: 0,
            run_count: 100,
            success_count: 97,
            consecutive_failures: 0,
            last_run_ms: Some(1000),
            last_success_ms: Some(1000),
        });

        state.record_task_health(IntrospectionTaskHealth {
            name: "control_task".to_string(),
            lane: "control_lane".to_string(),
            deadline_missed: 0,
            stale_input: 0,
            backpressure: 5,
            overflow: 2,
            fairness_violations: 1,
            run_count: 50,
            success_count: 48,
            consecutive_failures: 1,
            last_run_ms: Some(2000),
            last_success_ms: Some(1900),
        });

        let status = state.status();
        assert_eq!(status.tasks.len(), 2);

        let imu = state.task_health("imu_task").unwrap();
        assert_eq!(imu.name, "imu_task");
        assert_eq!(imu.deadline_missed, 3);
        assert_eq!(imu.run_count, 100);
        assert_eq!(imu.success_count, 97);
        assert_eq!(imu.consecutive_failures, 0);

        let control = state.task_health("control_task").unwrap();
        assert_eq!(control.backpressure, 5);
        assert_eq!(control.overflow, 2);
        assert_eq!(control.consecutive_failures, 1);
        assert_eq!(control.last_success_ms, Some(1900));

        assert!(state.task_health("missing_task").is_none());
    }

    #[test]
    fn io_boundary_health_recording_and_status_snapshot() {
        let state = IntrospectionState::new();
        state.register_io_boundary(
            "camera",
            "CameraDriver",
            vec![IntrospectionIoBoundaryResourceStatus {
                name: "camera_shm".to_string(),
                kind: "shm".to_string(),
                ..Default::default()
            }],
        );

        state.mark_io_boundary_ready("camera", true);
        state.record_io_boundary_resource_ready("camera", "camera_shm", true, None);
        state.record_io_boundary_error("camera", "frame timeout");

        let status = state.status();
        assert_eq!(status.io_boundaries.len(), 1);
        let boundary = &status.io_boundaries[0];
        assert_eq!(boundary.name, "camera");
        assert_eq!(boundary.component, "CameraDriver");
        assert!(boundary.ready);
        assert!(!boundary.healthy);
        assert_eq!(boundary.last_error.as_deref(), Some("frame timeout"));
        assert_eq!(boundary.resources.len(), 1);
        assert_eq!(boundary.resources[0].name, "camera_shm");
        assert_eq!(boundary.resources[0].kind, "shm");
        assert!(boundary.resources[0].ready);
    }

    #[test]
    fn boundary_publish_request_invokes_registered_handler() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-boundary-pub-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "island_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
        let captured_for_handler = Arc::clone(&captured);
        state.register_boundary_input_handler("sample_in", "Sample", move |payload, _| {
            *captured_for_handler.lock().unwrap() = payload.to_vec();
            Ok(7)
        });

        let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("server should start");

        let IntrospectionResponse::BoundaryPublish { boundary, .. } =
            request_boundary_publish(server.path(), "sample_in", vec![1, 2, 3, 4], Some(123))
                .expect("boundary publish request should succeed")
        else {
            panic!("boundary publish returned wrong response")
        };
        assert_eq!(boundary.endpoint, "sample_in");
        assert_eq!(boundary.message_type, "Sample");
        assert_eq!(boundary.revision, 7);
        assert_eq!(boundary.published_at_ms, Some(123));
        assert_eq!(*captured.lock().unwrap(), vec![1, 2, 3, 4]);

        let IntrospectionResponse::Error { message, .. } =
            request_boundary_publish(server.path(), "missing", vec![9], None)
                .expect("unknown boundary publish should return structured error")
        else {
            panic!("unknown boundary publish returned wrong response")
        };
        assert_eq!(message, "unknown FlowRT boundary input `missing`");

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn lane_health_recording_and_status_snapshot() {
        let state = IntrospectionState::new();

        state.record_lane_health(IntrospectionLaneHealth {
            name: "sensor_lane".to_string(),
            queue_depth: 2,
            dispatched_count: 500,
            fairness_violations: 0,
        });

        state.record_lane_health(IntrospectionLaneHealth {
            name: "control_lane".to_string(),
            queue_depth: 0,
            dispatched_count: 250,
            fairness_violations: 3,
        });

        let status = state.status();
        assert_eq!(status.lanes.len(), 2);

        let sensor = state.lane_health("sensor_lane").unwrap();
        assert_eq!(sensor.queue_depth, 2);
        assert_eq!(sensor.dispatched_count, 500);

        let control = state.lane_health("control_lane").unwrap();
        assert_eq!(control.queue_depth, 0);
        assert_eq!(control.dispatched_count, 250);

        assert!(state.lane_health("missing_lane").is_none());
    }

    #[test]
    fn health_fields_serialize_with_defaults_for_backward_compat() {
        // 旧版 JSON 不含 operations/tasks/lanes 字段时应解析为默认空列表。
        let status: IntrospectionStatus =
            serde_json::from_str(r#"{"tick_count":1,"channels":[],"processes":[],"services":[]}"#)
                .unwrap();
        assert!(status.inputs.is_empty());
        assert!(status.routes.is_empty());
        assert!(status.operations.is_empty());
        assert!(status.tasks.is_empty());
        assert!(status.lanes.is_empty());
    }

    #[test]
    fn recorder_disabled_does_not_capture_channel_payload() {
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");

        let outcome = state.try_record_channel_sample_bytes(
            "source.imu_to_sink.imu",
            "Imu",
            &[1, 2, 3, 4],
            Some(10),
        );

        assert!(!outcome.recorded);
        assert!(!outcome.dropped);
        let status = state.status();
        assert!(!status.recorder.enabled);
        assert_eq!(status.recorder.bytes_written, 0);
        assert_eq!(state.drain_recorder_events().len(), 0);
    }

    #[test]
    fn recorder_start_captures_channel_sample_and_reports_status() {
        let state = IntrospectionState::new();
        state.start_recorder(IntrospectionRecorderStart {
            output: Some("memory://test.mcap".to_string()),
            filters: vec!["channel:source.imu_to_sink.imu".to_string()],
            queue_depth: Some(4),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime_pid: 42,
            selfdesc_hash: "abc123".to_string(),
        });

        let outcome = state.try_record_channel_sample_bytes(
            "source.imu_to_sink.imu",
            "Imu",
            &[1, 2, 3, 4],
            Some(10),
        );

        assert!(outcome.recorded);
        assert!(!outcome.dropped);
        let events = state.drain_recorder_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].entity.name, "source.imu_to_sink.imu");
        assert_eq!(events[0].payload, vec![1, 2, 3, 4]);

        let status = state.status();
        assert!(status.recorder.enabled);
        assert_eq!(
            status.recorder.output.as_deref(),
            Some("memory://test.mcap")
        );
        assert_eq!(status.recorder.dropped_count, 0);
        assert_eq!(
            status.recorder.active_filters,
            vec!["channel:source.imu_to_sink.imu"]
        );
    }

    #[test]
    fn recorder_marks_channel_frame_payload_encoding() {
        let state = IntrospectionState::new();
        state.start_recorder(IntrospectionRecorderStart {
            output: None,
            filters: vec!["channel".to_string()],
            queue_depth: Some(4),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime_pid: 42,
            selfdesc_hash: "abc123".to_string(),
        });

        let outcome = state.try_record_channel_sample_frame_bytes(
            "source.packet_to_sink.packet",
            "Packet",
            &[1, 2, 3, 4],
            Some(10),
        );

        assert!(outcome.recorded);
        let events = state.drain_recorder_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].payload_encoding,
            flowrt_record::PayloadEncoding::CanonicalFrame
        );
    }

    #[test]
    fn recorder_makes_probe_publish_report_recorded_without_echo_observer() {
        let state = IntrospectionState::new();
        state.register_channel("source.imu_to_sink.imu", "Imu");
        state.start_recorder(IntrospectionRecorderStart {
            output: None,
            filters: vec!["channel".to_string()],
            queue_depth: Some(4),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime_pid: 42,
            selfdesc_hash: "abc123".to_string(),
        });

        let outcome = state.try_probe_channel_publish_bytes(
            "source.imu_to_sink.imu",
            "Imu",
            &[9, 8],
            Some(10),
        );

        assert!(outcome.recorded);
        assert!(!outcome.dropped);
        assert_eq!(
            state
                .channel_snapshot("source.imu_to_sink.imu")
                .unwrap()
                .published_count,
            0
        );
        let events = state.drain_recorder_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, vec![9, 8]);
    }

    #[test]
    fn recorder_bounded_queue_reports_dropped_count() {
        let state = IntrospectionState::new();
        state.start_recorder(IntrospectionRecorderStart {
            output: None,
            filters: vec!["all".to_string()],
            queue_depth: Some(1),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime_pid: 42,
            selfdesc_hash: "abc123".to_string(),
        });

        let first = state.try_record_channel_sample_bytes("a.out_to_b.in", "Msg", &[1], Some(1));
        let second = state.try_record_channel_sample_bytes("a.out_to_b.in", "Msg", &[2], Some(2));

        assert!(first.recorded);
        assert!(!first.dropped);
        assert!(!second.recorded);
        assert!(second.dropped);
        let status = state.status();
        assert_eq!(status.recorder.dropped_count, 1);
        assert_eq!(status.recorder.queued_events, 1);
        assert_eq!(state.drain_recorder_events().len(), 1);
    }

    #[test]
    fn status_server_controls_recorder_and_drains_events() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-recorder-test-{}",
            std::process::id()
        ));
        let socket = root.join("worker.sock");
        let handshake = IntrospectionHandshake {
            protocol_version: INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: 42,
            started_at_unix_ms: 1000,
            self_description_hash: "abc123".to_string(),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        };
        let state = IntrospectionState::new();
        let server = spawn_status_server_at(socket.clone(), handshake, state.clone())
            .expect("status server should start");

        let started = request_recorder_start(
            &socket,
            Some("memory://socket.mcap".to_string()),
            vec!["channel:source.imu_to_sink.imu".to_string()],
            Some(4),
        )
        .expect("recorder start request should succeed");
        let IntrospectionResponse::RecorderValue { recorder, .. } = started else {
            panic!("recorder start returned wrong response")
        };
        assert!(recorder.enabled);
        assert_eq!(recorder.output.as_deref(), Some("memory://socket.mcap"));

        state.try_record_channel_sample_bytes("source.imu_to_sink.imu", "Imu", &[9, 8], Some(11));
        let drained =
            request_recorder_drain(&socket).expect("recorder drain request should succeed");
        let IntrospectionResponse::RecorderEvents {
            events, recorder, ..
        } = drained
        else {
            panic!("recorder drain returned wrong response")
        };
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].entity.name, "source.imu_to_sink.imu");
        assert_eq!(recorder.queued_events, 0);

        let stopped = request_recorder_stop(&socket).expect("recorder stop request should succeed");
        let IntrospectionResponse::RecorderValue { recorder, .. } = stopped else {
            panic!("recorder stop returned wrong response")
        };
        assert!(!recorder.enabled);

        drop(server);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn registered_service_is_not_ready_until_marked() {
        let state = IntrospectionState::new();
        state.register_service("planner.plan");

        let status = state.status();
        assert_eq!(status.services.len(), 1);
        assert_eq!(status.services[0].name, "planner.plan");
        assert!(!status.services[0].ready);

        state.mark_service_ready("planner.plan");

        let status = state.status();
        assert!(status.services[0].ready);
    }

    #[test]
    fn request_status_with_timeout_returns_when_peer_stalls() {
        let root = std::env::temp_dir().join(format!(
            "flowrt-introspection-stall-test-{}",
            std::process::id()
        ));
        let socket = root.join("stall.sock");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("test temp dir should be created");
        let listener = UnixListener::bind(&socket).expect("test listener should bind");
        let handle = thread::spawn(move || {
            let (_stream, _addr) = listener.accept().expect("test listener should accept");
            thread::sleep(Duration::from_millis(100));
        });

        let error = request_status_with_timeout(&socket, Duration::from_millis(10))
            .expect_err("stalled peer should time out");

        assert!(
            matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
            "unexpected error kind: {error:?}"
        );
        handle.join().expect("stall thread should exit");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recorder_captures_param_operation_and_scheduler_events() {
        let state = IntrospectionState::new();
        state.start_recorder(IntrospectionRecorderStart {
            output: None,
            filters: vec!["all".to_string()],
            queue_depth: Some(16),
            package: "robot_demo".to_string(),
            process: "main".to_string(),
            runtime_pid: 42,
            selfdesc_hash: "abc123".to_string(),
        });
        state.register_param(IntrospectionParamSchema {
            name: "controller.kp".to_string(),
            ty: "f64".to_string(),
            update: "on_tick".to_string(),
            current: serde_json::json!(1.0),
            min: None,
            max: None,
            choices: vec![],
        });
        state
            .set_param_pending("controller.kp", serde_json::json!(2.0))
            .expect("param set should be accepted");
        state.record_param_applied("controller.kp", serde_json::json!(2.0));
        state.record_service_health(IntrospectionServiceStatus {
            name: "planner.plan_to_executor.execute".to_string(),
            ready: true,
            in_flight: 1,
            queued: 0,
            total_requests: 1,
            timeout_count: 0,
            busy_count: 0,
            unavailable_count: 0,
            late_drop_count: 0,
        });
        state.record_operation_health(IntrospectionOperationStatus {
            name: "controller.plan".to_string(),
            ready: true,
            running: 1,
            queued: 0,
            current_operation_ids: vec!["1:2:3".to_string()],
            total_started: 1,
            succeeded_count: 0,
            failed_count: 0,
            canceled_count: 0,
            timeout_count: 0,
            preempted_count: 0,
            last_transition_ms: Some(12),
        });
        state.record_task_health(IntrospectionTaskHealth {
            name: "control_loop".to_string(),
            lane: "control".to_string(),
            run_count: 1,
            success_count: 1,
            ..Default::default()
        });
        state.record_lane_health(IntrospectionLaneHealth {
            name: "control".to_string(),
            queue_depth: 0,
            dispatched_count: 1,
            fairness_violations: 0,
        });

        let events = state.drain_recorder_events();
        assert!(events.iter().any(|event| {
            event.event_kind == flowrt_record::RecordEventKind::ParamEvent
                && event.entity.name == "controller.kp"
        }));
        assert!(events.iter().any(|event| {
            event.event_kind == flowrt_record::RecordEventKind::ServiceEvent
                && event.entity.name == "planner.plan_to_executor.execute"
        }));
        assert!(events.iter().any(|event| {
            event.event_kind == flowrt_record::RecordEventKind::OperationEvent
                && event.entity.name == "controller.plan"
        }));
        assert!(
            events
                .iter()
                .any(|event| event.event_kind == flowrt_record::RecordEventKind::SchedulerEvent)
        );
    }

    #[test]
    fn input_read_records_presence_and_route_counters() {
        let state = IntrospectionState::new();
        state.register_route(IntrospectionRouteStatus {
            name: "source.packet_to_sink.packet".to_string(),
            from: "source.packet".to_string(),
            to: "sink.packet".to_string(),
            message_type: "Packet".to_string(),
            backend: "zenoh".to_string(),
            selected_reason: "explicit".to_string(),
            dropped_samples: 1,
            backpressure_count: 2,
            overflow_count: 3,
            ..Default::default()
        });

        state.record_input_read(
            "sink.main.packet",
            "sink.main",
            "packet",
            "source.packet_to_sink.packet",
            "Packet",
            true,
            false,
            Some(7),
            Some(42),
        );

        let status = state.status();
        assert_eq!(status.inputs.len(), 1);
        let input = &status.inputs[0];
        assert_eq!(input.task, "sink.main");
        assert_eq!(input.input, "packet");
        assert_eq!(input.channel, "source.packet_to_sink.packet");
        assert_eq!(input.message_type, "Packet");
        assert!(input.present);
        assert!(!input.stale);
        assert_eq!(input.last_revision, Some(7));
        assert_eq!(input.last_read_ms, Some(42));
        assert_eq!(input.dropped_samples, 1);
        assert_eq!(input.backpressure_count, 2);
        assert_eq!(input.overflow_count, 3);
    }

    #[test]
    fn health_fields_serialize_roundtrip() {
        let status = IntrospectionStatus {
            tick_count: 42,
            channels: vec![],
            processes: vec![],
            inputs: vec![IntrospectionInputStatus {
                task: "sink.main".to_string(),
                input: "packet".to_string(),
                channel: "source.packet_to_sink.packet".to_string(),
                message_type: "Packet".to_string(),
                present: true,
                stale: false,
                last_revision: Some(7),
                last_read_ms: Some(996),
                updated_unix_ms: Some(997),
                dropped_samples: 1,
                backpressure_count: 2,
                overflow_count: 3,
            }],
            routes: vec![IntrospectionRouteStatus {
                name: "source.packet_to_sink.packet".to_string(),
                from: "source.packet".to_string(),
                to: "sink.packet".to_string(),
                message_type: "Packet".to_string(),
                backend: "zenoh".to_string(),
                selected_reason: "variable_frame_auto_fallback".to_string(),
                published_count: 11,
                dropped_samples: 1,
                backpressure_count: 2,
                overflow_count: 3,
                last_publish_ms: Some(995),
                last_error: Some("queue overflow".to_string()),
            }],
            io_boundaries: vec![IntrospectionIoBoundaryStatus {
                name: "camera".to_string(),
                component: "CameraDriver".to_string(),
                ready: true,
                healthy: true,
                last_error: None,
                resources: vec![IntrospectionIoBoundaryResourceStatus {
                    name: "camera_shm".to_string(),
                    kind: "shm".to_string(),
                    ready: true,
                    message: None,
                    last_error: None,
                    updated_unix_ms: Some(997),
                }],
                updated_unix_ms: Some(998),
            }],
            services: vec![],
            operations: vec![IntrospectionOperationStatus {
                name: "controller.plan".to_string(),
                ready: true,
                running: 1,
                queued: 0,
                current_operation_ids: vec!["1:2:3".to_string()],
                total_started: 1,
                succeeded_count: 0,
                failed_count: 0,
                canceled_count: 0,
                timeout_count: 0,
                preempted_count: 0,
                last_transition_ms: Some(998),
            }],
            tasks: vec![IntrospectionTaskHealth {
                name: "t1".to_string(),
                lane: "l1".to_string(),
                deadline_missed: 5,
                stale_input: 2,
                backpressure: 1,
                overflow: 0,
                fairness_violations: 0,
                run_count: 100,
                success_count: 95,
                consecutive_failures: 0,
                last_run_ms: Some(1000),
                last_success_ms: Some(999),
            }],
            lanes: vec![IntrospectionLaneHealth {
                name: "l1".to_string(),
                queue_depth: 3,
                dispatched_count: 200,
                fairness_violations: 1,
            }],
            recorder: Default::default(),
        };

        let json = serde_json::to_string(&status).unwrap();
        let parsed: IntrospectionStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.inputs.len(), 1);
        assert_eq!(parsed.inputs[0].task, "sink.main");
        assert!(parsed.inputs[0].present);
        assert_eq!(parsed.routes.len(), 1);
        assert_eq!(parsed.routes[0].backend, "zenoh");
        assert_eq!(
            parsed.routes[0].selected_reason,
            "variable_frame_auto_fallback"
        );
        assert_eq!(parsed.operations.len(), 1);
        assert_eq!(parsed.operations[0].name, "controller.plan");
        assert_eq!(parsed.io_boundaries.len(), 1);
        assert_eq!(parsed.io_boundaries[0].name, "camera");
        assert_eq!(parsed.tasks.len(), 1);
        assert_eq!(parsed.tasks[0].name, "t1");
        assert_eq!(parsed.tasks[0].deadline_missed, 5);
        assert_eq!(parsed.lanes.len(), 1);
        assert_eq!(parsed.lanes[0].queue_depth, 3);
    }
}
