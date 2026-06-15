//! FlowRT runtime recorder tap。
//!
//! Recorder 默认关闭。关闭时热路径只做一次 atomic 检查；开启后事件进入有界队列，
//! 调用方可 drain 后交给 MCAP writer 或测试 fake。

use std::collections::VecDeque;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{SystemTime, UNIX_EPOCH};

use flowrt_record::{
    DESCRIPTOR_RECORD_SCHEMA_NAME, DescriptorRecordPayload, DescriptorRecordStatus,
    PayloadEncoding, RECORD_SCHEMA_VERSION, RecordEntity, RecordEntityKind, RecordEnvelope,
    RecordEventKind,
};
use serde::{Deserialize, Serialize};

use crate::introspection::facts::RecorderDiagnosticFact;
use crate::{FrameDescriptor, FrameLeaseStatus};

/// recorder 启动时绑定的 runtime 元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderRuntimeMetadata {
    pub package: String,
    pub process: String,
    pub runtime_pid: u32,
    pub selfdesc_hash: String,
}

/// recorder 启动配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderStartConfig {
    pub output: Option<String>,
    pub filters: Vec<String>,
    pub queue_depth: usize,
    pub metadata: RecorderRuntimeMetadata,
}

/// recorder 状态快照，进入 introspection status。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecorderStatus {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub dropped_count: u64,
    #[serde(default)]
    pub bytes_written: u64,
    #[serde(default)]
    pub active_filters: Vec<String>,
    #[serde(default)]
    pub queued_events: u64,
}

/// 一次 tap 尝试的结果。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecorderTapOutcome {
    pub recorded: bool,
    pub dropped: bool,
}

#[derive(Debug, Clone)]
pub struct RecorderTap {
    enabled: Arc<AtomicBool>,
    inner: Arc<Mutex<RecorderInner>>,
}

#[derive(Debug)]
struct RecorderInner {
    config: Option<RecorderStartConfig>,
    queue: VecDeque<RecordEnvelope>,
    dropped_count: u64,
    bytes_written: u64,
    sequence: u64,
}

impl Default for RecorderTap {
    fn default() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            inner: Arc::new(Mutex::new(RecorderInner {
                config: None,
                queue: VecDeque::new(),
                dropped_count: 0,
                bytes_written: 0,
                sequence: 0,
            })),
        }
    }
}

impl RecorderTap {
    pub fn new() -> Self {
        Self::default()
    }

    /// 启动 recorder，并清空上一轮暂存事件和计数。
    pub fn start(&self, mut config: RecorderStartConfig) -> RecorderStatus {
        config.queue_depth = config.queue_depth.max(1);
        config.filters = normalize_filters(config.filters);

        let mut inner = self.lock_inner();
        inner.queue.clear();
        inner.dropped_count = 0;
        inner.bytes_written = 0;
        inner.sequence = 0;
        inner.config = Some(config);
        self.enabled.store(true, Ordering::Release);
        status_from_inner(&inner, true)
    }

    /// 停止 recorder；已暂存事件保留到调用方 drain。
    pub fn stop(&self) -> RecorderStatus {
        self.enabled.store(false, Ordering::Release);
        let mut inner = self.lock_inner();
        inner.config = None;
        status_from_inner(&inner, false)
    }

    pub fn status(&self) -> RecorderStatus {
        let enabled = self.enabled.load(Ordering::Acquire);
        let inner = self.lock_inner();
        status_from_inner(&inner, enabled)
    }

    pub fn drain_events(&self) -> Vec<RecordEnvelope> {
        let mut inner = self.lock_inner();
        inner.queue.drain(..).collect()
    }

    /// cheap path：关闭时不进 mutex。
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// 判断指定 channel 是否会被当前 recorder 采集。
    pub fn enabled_for_channel(&self, name: &str) -> bool {
        if !self.enabled() {
            return false;
        }
        let inner = self.lock_inner();
        let Some(config) = inner.config.as_ref() else {
            return false;
        };
        filter_matches(&config.filters, "channel", name)
    }

    pub fn enabled_for_operation(&self, name: &str) -> bool {
        if !self.enabled() {
            return false;
        }
        let inner = self.lock_inner();
        let Some(config) = inner.config.as_ref() else {
            return false;
        };
        filter_matches(&config.filters, "operation", name)
    }

    pub fn enabled_for_descriptor(&self, resource_id: &str) -> bool {
        if !self.enabled() {
            return false;
        }
        let inner = self.lock_inner();
        let Some(config) = inner.config.as_ref() else {
            return false;
        };
        filter_matches(&config.filters, "descriptor", resource_id)
    }

    pub fn record_channel_sample_bytes(
        &self,
        name: &str,
        type_name: &str,
        payload: &[u8],
        published_at_ms: Option<u64>,
    ) -> RecorderTapOutcome {
        self.record_channel_sample_with_encoding(
            name,
            type_name,
            PayloadEncoding::RawAbi,
            payload,
            published_at_ms,
        )
    }

    pub fn record_channel_sample_frame_bytes(
        &self,
        name: &str,
        type_name: &str,
        payload: &[u8],
        published_at_ms: Option<u64>,
    ) -> RecorderTapOutcome {
        self.record_channel_sample_with_encoding(
            name,
            type_name,
            PayloadEncoding::CanonicalFrame,
            payload,
            published_at_ms,
        )
    }

    fn record_channel_sample_with_encoding(
        &self,
        name: &str,
        type_name: &str,
        payload_encoding: PayloadEncoding,
        payload: &[u8],
        published_at_ms: Option<u64>,
    ) -> RecorderTapOutcome {
        if !self.enabled() {
            return RecorderTapOutcome::default();
        }
        let entity = RecordEntity {
            kind: RecordEntityKind::Channel,
            name: name.to_string(),
            instance: instance_from_endpoint(name),
            task: None,
            type_name: Some(type_name.to_string()),
        };
        self.record_bytes(
            "channel",
            name,
            RecordEventKind::ChannelSample,
            entity,
            payload_encoding,
            type_name,
            payload,
            published_at_ms.map(|value| value.saturating_mul(1_000_000)),
        )
    }

    pub fn record_frame_descriptor_event(
        &self,
        name: &str,
        descriptor: &FrameDescriptor,
        status: FrameLeaseStatus,
        payload_recording: bool,
    ) -> RecorderTapOutcome {
        let resource_id = descriptor.resource().resource_id();
        if !self.enabled_for_descriptor(resource_id) {
            return RecorderTapOutcome::default();
        }
        let payload = DescriptorRecordPayload {
            resource_id: resource_id.to_string(),
            slot: descriptor.resource().slot().to_string(),
            generation: descriptor.resource().generation(),
            size_bytes: descriptor.size_bytes(),
            format: descriptor.format().to_string(),
            encoding: descriptor.encoding().to_string(),
            metadata: descriptor.metadata().clone(),
            status: descriptor_status(status),
            payload_recording,
        };
        let Ok(payload) = serde_json::to_vec(&payload) else {
            return RecorderTapOutcome {
                recorded: false,
                dropped: true,
            };
        };
        let entity = RecordEntity {
            kind: RecordEntityKind::Resource,
            name: resource_id.to_string(),
            instance: instance_from_endpoint(name),
            task: None,
            type_name: Some("FrameDescriptor".to_string()),
        };
        self.record_bytes(
            "descriptor",
            resource_id,
            RecordEventKind::DescriptorEvent,
            entity,
            PayloadEncoding::Json,
            DESCRIPTOR_RECORD_SCHEMA_NAME,
            &payload,
            None,
        )
    }

    pub fn record_frame_descriptor_status(
        &self,
        descriptor: &FrameDescriptor,
        status: FrameLeaseStatus,
        payload_recording: bool,
    ) -> RecorderTapOutcome {
        self.record_frame_descriptor_event(
            descriptor.resource().resource_id(),
            descriptor,
            status,
            payload_recording,
        )
    }

    pub fn record_param_event_json(
        &self,
        name: &str,
        payload_schema: &str,
        payload: serde_json::Value,
    ) -> RecorderTapOutcome {
        if !self.enabled() {
            return RecorderTapOutcome::default();
        }
        let Ok(payload) = serde_json::to_vec(&payload) else {
            return RecorderTapOutcome {
                recorded: false,
                dropped: true,
            };
        };
        let entity = RecordEntity {
            kind: RecordEntityKind::Param,
            name: name.to_string(),
            instance: instance_from_endpoint(name),
            task: None,
            type_name: None,
        };
        self.record_bytes(
            "param",
            name,
            RecordEventKind::ParamEvent,
            entity,
            PayloadEncoding::Json,
            payload_schema,
            &payload,
            None,
        )
    }

    pub fn record_operation_event_json(
        &self,
        name: &str,
        payload_schema: &str,
        payload: serde_json::Value,
    ) -> RecorderTapOutcome {
        self.record_operation_event_json_at(name, payload_schema, payload, None)
    }

    pub fn record_operation_event_json_at(
        &self,
        name: &str,
        payload_schema: &str,
        payload: serde_json::Value,
        monotonic_ns: Option<u64>,
    ) -> RecorderTapOutcome {
        if !self.enabled_for_operation(name) {
            return RecorderTapOutcome::default();
        }
        let Ok(payload) = serde_json::to_vec(&payload) else {
            return RecorderTapOutcome {
                recorded: false,
                dropped: true,
            };
        };
        let entity = RecordEntity {
            kind: RecordEntityKind::Operation,
            name: name.to_string(),
            instance: instance_from_endpoint(name),
            task: None,
            type_name: None,
        };
        self.record_bytes(
            "operation",
            name,
            RecordEventKind::OperationEvent,
            entity,
            PayloadEncoding::Json,
            payload_schema,
            &payload,
            monotonic_ns,
        )
    }

    pub fn record_service_event_json(
        &self,
        name: &str,
        payload_schema: &str,
        payload: serde_json::Value,
    ) -> RecorderTapOutcome {
        if !self.enabled() {
            return RecorderTapOutcome::default();
        }
        let Ok(payload) = serde_json::to_vec(&payload) else {
            return RecorderTapOutcome {
                recorded: false,
                dropped: true,
            };
        };
        let entity = RecordEntity {
            kind: RecordEntityKind::Service,
            name: name.to_string(),
            instance: instance_from_endpoint(name),
            task: None,
            type_name: None,
        };
        self.record_bytes(
            "service",
            name,
            RecordEventKind::ServiceEvent,
            entity,
            PayloadEncoding::Json,
            payload_schema,
            &payload,
            None,
        )
    }

    pub fn record_scheduler_event_json(
        &self,
        kind: RecordEntityKind,
        name: &str,
        payload_schema: &str,
        payload: serde_json::Value,
    ) -> RecorderTapOutcome {
        if !self.enabled() {
            return RecorderTapOutcome::default();
        }
        let Ok(payload) = serde_json::to_vec(&payload) else {
            return RecorderTapOutcome {
                recorded: false,
                dropped: true,
            };
        };
        let entity = RecordEntity {
            kind,
            name: name.to_string(),
            instance: None,
            task: (kind == RecordEntityKind::Task).then(|| name.to_string()),
            type_name: None,
        };
        self.record_bytes(
            "scheduler",
            name,
            RecordEventKind::SchedulerEvent,
            entity,
            PayloadEncoding::Json,
            payload_schema,
            &payload,
            None,
        )
    }

    pub fn record_diagnostics_event_json(
        &self,
        name: &str,
        payload_schema: &str,
        payload: serde_json::Value,
        monotonic_ns: Option<u64>,
    ) -> RecorderTapOutcome {
        if !self.enabled() {
            return RecorderTapOutcome::default();
        }
        let Ok(payload) = serde_json::to_vec(&payload) else {
            return RecorderTapOutcome {
                recorded: false,
                dropped: true,
            };
        };
        let entity = RecordEntity {
            kind: RecordEntityKind::Diagnostic,
            name: name.to_string(),
            instance: instance_from_endpoint(name),
            task: None,
            type_name: None,
        };
        self.record_bytes(
            "diagnostics",
            name,
            RecordEventKind::DiagnosticsEvent,
            entity,
            PayloadEncoding::Json,
            payload_schema,
            &payload,
            monotonic_ns,
        )
    }

    pub(crate) fn record_diagnostics_fact(
        &self,
        fact: &RecorderDiagnosticFact,
    ) -> RecorderTapOutcome {
        self.record_diagnostics_event_json(
            &fact.entity_id,
            fact.payload_schema,
            fact.payload.clone(),
            fact.monotonic_ns,
        )
    }

    pub fn record_runtime_event_json(
        &self,
        kind: RecordEntityKind,
        name: &str,
        payload_schema: &str,
        payload: serde_json::Value,
    ) -> RecorderTapOutcome {
        self.record_runtime_event_json_at(kind, name, payload_schema, payload, None)
    }

    pub fn record_runtime_event_json_at(
        &self,
        kind: RecordEntityKind,
        name: &str,
        payload_schema: &str,
        payload: serde_json::Value,
        monotonic_ns: Option<u64>,
    ) -> RecorderTapOutcome {
        if !self.enabled() {
            return RecorderTapOutcome::default();
        }
        let Ok(payload) = serde_json::to_vec(&payload) else {
            return RecorderTapOutcome {
                recorded: false,
                dropped: true,
            };
        };
        let event_kind = if kind == RecordEntityKind::Clock {
            RecordEventKind::ClockEvent
        } else {
            RecordEventKind::RuntimeEvent
        };
        let entity = RecordEntity {
            kind,
            name: name.to_string(),
            instance: None,
            task: None,
            type_name: None,
        };
        self.record_bytes(
            "runtime",
            name,
            event_kind,
            entity,
            PayloadEncoding::Json,
            payload_schema,
            &payload,
            monotonic_ns,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn record_bytes(
        &self,
        filter_kind: &str,
        filter_name: &str,
        event_kind: RecordEventKind,
        entity: RecordEntity,
        payload_encoding: PayloadEncoding,
        payload_schema: &str,
        payload: &[u8],
        monotonic_ns: Option<u64>,
    ) -> RecorderTapOutcome {
        let mut inner = self.lock_inner();
        let Some(config) = inner.config.clone() else {
            return RecorderTapOutcome::default();
        };
        if !filter_matches(&config.filters, filter_kind, filter_name) {
            return RecorderTapOutcome::default();
        }
        if inner.queue.len() >= config.queue_depth {
            inner.dropped_count = inner.dropped_count.saturating_add(1);
            return RecorderTapOutcome {
                recorded: false,
                dropped: true,
            };
        }

        let sequence = inner.sequence;
        inner.sequence = inner.sequence.saturating_add(1);
        let envelope = RecordEnvelope {
            schema_version: RECORD_SCHEMA_VERSION,
            event_kind,
            package: config.metadata.package,
            process: config.metadata.process,
            runtime_pid: config.metadata.runtime_pid,
            selfdesc_hash: config.metadata.selfdesc_hash,
            monotonic_ns: monotonic_ns.unwrap_or(0),
            wall_unix_ns: wall_unix_ns(),
            sequence,
            entity,
            payload_encoding,
            payload_schema: payload_schema.to_string(),
            payload: payload.to_vec(),
        };
        inner.bytes_written = inner
            .bytes_written
            .saturating_add(envelope.payload.len() as u64);
        inner.queue.push_back(envelope);
        RecorderTapOutcome {
            recorded: true,
            dropped: false,
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, RecorderInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn status_from_inner(inner: &RecorderInner, enabled: bool) -> RecorderStatus {
    let config = inner.config.as_ref();
    RecorderStatus {
        enabled,
        output: config.and_then(|config| config.output.clone()),
        dropped_count: inner.dropped_count,
        bytes_written: inner.bytes_written,
        active_filters: config
            .map(|config| config.filters.clone())
            .unwrap_or_default(),
        queued_events: inner.queue.len() as u64,
    }
}

fn normalize_filters(filters: Vec<String>) -> Vec<String> {
    let mut filters = filters
        .into_iter()
        .map(|filter| filter.trim().to_string())
        .filter(|filter| !filter.is_empty())
        .collect::<Vec<_>>();
    if filters.is_empty() {
        filters.push("all".to_string());
    }
    filters.sort();
    filters.dedup();
    filters
}

fn filter_matches(filters: &[String], kind: &str, name: &str) -> bool {
    filters
        .iter()
        .any(|filter| filter == "all" || filter == kind || filter == &format!("{kind}:{name}"))
}

fn instance_from_endpoint(name: &str) -> Option<String> {
    name.split_once('.')
        .map(|(instance, _)| instance.to_string())
}

fn descriptor_status(status: FrameLeaseStatus) -> DescriptorRecordStatus {
    match status {
        FrameLeaseStatus::Attached => DescriptorRecordStatus::Attached,
        FrameLeaseStatus::Acquired => DescriptorRecordStatus::Acquired,
        FrameLeaseStatus::Released => DescriptorRecordStatus::Released,
        FrameLeaseStatus::Expired => DescriptorRecordStatus::Expired,
        FrameLeaseStatus::GenerationMismatch => DescriptorRecordStatus::GenerationMismatch,
        FrameLeaseStatus::Error => DescriptorRecordStatus::Error,
    }
}

fn wall_unix_ns() -> u64 {
    let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return 0;
    };
    duration
        .as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(duration.subsec_nanos()))
}
