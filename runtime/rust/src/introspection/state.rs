use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use flowrt_record::{RecordEntityKind, RecordEnvelope};

use crate::recorder::{
    RecorderRuntimeMetadata, RecorderStartConfig, RecorderTap, RecorderTapOutcome,
};
use crate::{FrameCodec, FrameDescriptor, FrameLeaseStatus};

use super::facts::{RuntimeObservabilityFacts, input_status_key};
use super::model::*;
use super::params::{ParamState, param_status, validate_param_json_value};
use super::paths::unix_time_ms;
use super::probe::{
    IntrospectionChannelProbe, IntrospectionObserverGuard, IntrospectionProbeRecord,
};

type BoundaryInputHandler =
    Arc<dyn Fn(&[u8], Option<u64>) -> std::result::Result<u64, String> + Send + Sync + 'static>;

/// 从 boundary input 的 canonical frame payload 读出 sensor sample-time（纳秒）的提取器。
/// 由生成 shell 对声明了 timestamp 源的 boundary 提供（typed decode 后读字段，无需 offset 数学）。
type SampleTimeFn = Arc<dyn Fn(&[u8]) -> Option<u64> + Send + Sync + 'static>;

#[derive(Clone)]
struct BoundaryInputState {
    message_type: String,
    handler: BoundaryInputHandler,
    sample_time_ns_fn: Option<SampleTimeFn>,
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
pub(super) struct ChannelState {
    pub(super) message_type: String,
    pub(super) probe: IntrospectionChannelProbe,
}

type OperationCancelHandler =
    Arc<dyn Fn(&str) -> std::result::Result<IntrospectionOperationStatus, String> + Send + Sync>;

#[derive(Clone, Default)]
pub struct IntrospectionState {
    pub(super) inner: Arc<Mutex<IntrospectionStateInner>>,
    recorder: RecorderTap,
}

impl std::fmt::Debug for IntrospectionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntrospectionState").finish_non_exhaustive()
    }
}

#[derive(Default)]
pub(super) struct IntrospectionStateInner {
    pub(super) tick_count: u64,
    pub(super) clock: IntrospectionClockStatus,
    self_description_json: Option<String>,
    pub(super) channels: BTreeMap<String, ChannelState>,
    pub(super) inputs: BTreeMap<String, IntrospectionInputStatus>,
    pub(super) routes: BTreeMap<String, IntrospectionRouteStatus>,
    pub(super) params: BTreeMap<String, ParamState>,
    boundary_inputs: BTreeMap<String, BoundaryInputState>,
    pub(super) processes: BTreeMap<String, IntrospectionProcessStatus>,
    pub(super) resources: BTreeMap<String, IntrospectionResourceStatus>,
    pub(super) io_boundaries: BTreeMap<String, IntrospectionIoBoundaryStatus>,
    pub(super) services: BTreeMap<String, IntrospectionServiceStatus>,
    pub(super) operations: BTreeMap<String, IntrospectionOperationStatus>,
    operation_cancel_handlers: BTreeMap<String, OperationCancelHandler>,
    pub(super) tasks: BTreeMap<String, IntrospectionTaskHealth>,
    pub(super) lanes: BTreeMap<String, IntrospectionLaneHealth>,
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
                apply_state: "applied".to_string(),
                last_reject_reason: None,
                updated_unix_ms: None,
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
                sample_time_ns_fn: None,
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

    /// 注册带 sensor sample-time 提取器的 typed boundary input。
    ///
    /// `sample_time_fn` 从 canonical frame payload 读出 sample-time（纳秒）——由生成 shell 对声明了
    /// timestamp 源的 boundary 提供（typed decode 后读字段，无需 frame offset 数学）。录制该 boundary
    /// 激励时填入 envelope.sample_time_ns，供 event-time 回放按 sensor 采集时刻步进。
    pub fn register_boundary_input_with_sample_time<T, S>(
        &self,
        endpoint: impl Into<String>,
        message_type: impl Into<String>,
        input: crate::BoundaryInput<T>,
        sample_time_fn: S,
    ) where
        T: FrameCodec + Send + Sync + 'static,
        S: Fn(&[u8]) -> Option<u64> + Send + Sync + 'static,
    {
        let endpoint = endpoint.into();
        let endpoint_for_error = endpoint.clone();
        let handler = move |payload: &[u8], timestamp: Option<u64>| {
            let value = T::decode_frame(payload).map_err(|error| {
                format!("decode FlowRT boundary input `{endpoint_for_error}`: {error}")
            })?;
            Ok(match timestamp {
                Some(timestamp) => input.inject_at(value, timestamp),
                None => input.inject(value),
            })
        };
        let mut inner = self.lock_inner();
        inner.boundary_inputs.insert(
            endpoint,
            BoundaryInputState {
                message_type: message_type.into(),
                handler: Arc::new(handler),
                sample_time_ns_fn: Some(Arc::new(sample_time_fn)),
            },
        );
    }

    /// 向已注册的 island boundary input 注入 canonical Message ABI payload。
    ///
    /// recorder 开启且覆盖该 endpoint 时，注入会作为 canonical frame channel sample 记录，
    /// 作为确定性回放的边界激励来源——回放只重放这些外部激励，由 runtime 重新推导下游 channel。
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
        if self.recorder_enabled_for_channel(endpoint) {
            let sample_time_ns = boundary
                .sample_time_ns_fn
                .as_ref()
                .and_then(|extract| extract(&payload));
            let _ = self.try_record_channel_sample_frame_bytes_with_sample_time(
                endpoint,
                &boundary.message_type,
                &payload,
                published_at_ms,
                sample_time_ns,
            );
        }
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
        self.record_tick_at(0, "realtime");
    }

    /// 按统一 runtime 毫秒时间模型记录 scheduler tick。
    pub fn record_tick_at(&self, tick_time_ms: u64, time_source: impl Into<String>) {
        let time_source = time_source.into();
        let mut inner = self.lock_inner();
        inner.tick_count = inner.tick_count.saturating_add(1);
        let tick_count = inner.tick_count;
        inner.clock = IntrospectionClockStatus {
            source: time_source.clone(),
            tick_time_ms: Some(tick_time_ms),
            unit: "ms".to_string(),
            field: "tick_time_ms".to_string(),
        };
        drop(inner);
        self.recorder.record_runtime_event_json_at(
            RecordEntityKind::Clock,
            "scheduler_tick",
            "flowrt.clock.tick",
            serde_json::json!({
                "tick_count": tick_count,
                "tick_time_ms": tick_time_ms,
                "time_source": time_source,
                "time_unit": "ms",
            }),
            Some(tick_time_ms.saturating_mul(1_000_000)),
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

    /// 按需记录带 sensor sample-time（纳秒）的 canonical frame channel sample。
    pub fn try_record_channel_sample_frame_bytes_with_sample_time(
        &self,
        name: &str,
        message_type: impl AsRef<str>,
        payload: &[u8],
        published_at_ms: Option<u64>,
        sample_time_ns: Option<u64>,
    ) -> RecorderTapOutcome {
        self.recorder
            .record_channel_sample_frame_bytes_with_sample_time(
                name,
                message_type.as_ref(),
                payload,
                published_at_ms,
                sample_time_ns,
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
        let recorder = self.recorder.status();
        let inner = self.lock_inner();
        RuntimeObservabilityFacts::from_state_inner(&inner, recorder).status_snapshot()
    }

    /// 将当前诊断项写入 recorder。`status()` 本身保持无副作用，避免内部轮询污染录制。
    pub fn record_current_diagnostics(&self) {
        let recorder = self.recorder.status();
        let facts = {
            let inner = self.lock_inner();
            RuntimeObservabilityFacts::from_state_inner(&inner, recorder)
        };
        self.record_diagnostics_events(&facts);
    }

    fn record_diagnostics_events(&self, facts: &RuntimeObservabilityFacts) {
        for event in facts.recorder_diagnostic_events() {
            self.recorder.record_diagnostics_fact(&event);
        }
    }

    /// 记录 supervisor 视角下的子进程健康状态。
    pub fn record_process_health(&self, status: IntrospectionProcessStatus) {
        let mut inner = self.lock_inner();
        inner.processes.insert(status.name.clone(), status);
    }

    /// 预注册一个抽象 resource，使未知运行态也可被 `flowrt status` 发现。
    pub fn register_resource(&self, status: IntrospectionResourceStatus) {
        let mut inner = self.lock_inner();
        inner.resources.entry(status.name.clone()).or_insert(status);
    }

    /// 记录抽象 resource 的最新运行态。
    pub fn record_resource_status(&self, status: IntrospectionResourceStatus) {
        let mut inner = self.lock_inner();
        inner.resources.insert(status.name.clone(), status);
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

    /// 注册 operation cancel control hook。
    pub fn register_operation_cancel_handler<F>(&self, name: impl Into<String>, handler: F)
    where
        F: Fn(&str) -> std::result::Result<IntrospectionOperationStatus, String>
            + Send
            + Sync
            + 'static,
    {
        let name = name.into();
        let mut inner = self.lock_inner();
        inner
            .operation_cancel_handlers
            .insert(name, Arc::new(handler));
    }

    /// 记录 operation 运行态健康状态快照。
    pub fn record_operation_health(&self, status: IntrospectionOperationStatus) {
        let status_for_record = status.clone();
        let mut inner = self.lock_inner();
        inner.operations.insert(status.name.clone(), status);
        drop(inner);
        self.recorder.record_operation_event_json_at(
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
                "state": status_for_record.current_state,
                "owner": status_for_record.current_owner,
                "deadline_ms": status_for_record.current_deadline_ms,
                "last_event": status_for_record.last_event,
                "last_error": status_for_record.last_error,
                "last_transition_ms": status_for_record.last_transition_ms,
            }),
            status_for_record
                .last_transition_ms
                .map(|value| value.saturating_mul(1_000_000)),
        );
    }

    /// 记录 operation 状态转换事件。
    pub fn record_operation_transition(
        &self,
        operation: &str,
        operation_id: &str,
        state: &str,
        owner: Option<&str>,
        deadline_ms: Option<u64>,
    ) {
        let now = unix_time_ms();
        {
            let mut inner = self.lock_inner();
            let entry = inner.operations.entry(operation.to_string()).or_default();
            entry.name = operation.to_string();
            entry.ready = true;
            entry.current_state = Some(state.to_string());
            entry.current_owner = owner.map(str::to_string);
            entry.current_deadline_ms = deadline_ms;
            entry.last_event = Some("flowrt.operation.state_changed".to_string());
            entry.last_error = None;
            entry.last_transition_ms = Some(now);
            if !matches!(
                state,
                "idle" | "succeeded" | "failed" | "cancelled" | "timed_out"
            ) {
                if !entry
                    .current_operation_ids
                    .iter()
                    .any(|id| id == operation_id)
                {
                    entry.current_operation_ids.push(operation_id.to_string());
                }
                entry.running = 1;
            } else {
                entry.current_operation_ids.retain(|id| id != operation_id);
                entry.running = 0;
            }
        }
        self.recorder.record_operation_event_json(
            operation,
            "flowrt.operation.state_changed",
            serde_json::json!({
                "operation_id": operation_id,
                "state": state,
                "owner": owner,
                "deadline_ms": deadline_ms,
                "transition_ms": now,
            }),
        );
    }

    /// 记录 operation progress 事件。
    pub fn record_operation_progress(&self, operation: &str, operation_id: &str, sequence: u64) {
        {
            let mut inner = self.lock_inner();
            let entry = inner.operations.entry(operation.to_string()).or_default();
            entry.name = operation.to_string();
            entry.last_event = Some("flowrt.operation.progress".to_string());
        }
        self.recorder.record_operation_event_json(
            operation,
            "flowrt.operation.progress",
            serde_json::json!({
                "operation_id": operation_id,
                "sequence": sequence,
            }),
        );
    }

    /// 记录 operation result/error 事件。
    pub fn record_operation_result(
        &self,
        operation: &str,
        operation_id: &str,
        result: &str,
        error: Option<&str>,
    ) {
        let event = if error.is_some() || result == "failed" {
            "flowrt.operation.error"
        } else {
            "flowrt.operation.result"
        };
        {
            let mut inner = self.lock_inner();
            let entry = inner.operations.entry(operation.to_string()).or_default();
            entry.name = operation.to_string();
            entry.last_event = Some(event.to_string());
            entry.last_error = error.map(str::to_string);
        }
        self.recorder.record_operation_event_json(
            operation,
            event,
            serde_json::json!({
                "operation_id": operation_id,
                "result": result,
                "error": error,
            }),
        );
    }

    /// 请求取消指定 operation invocation。
    pub fn cancel_operation(
        &self,
        operation_id: &str,
    ) -> std::result::Result<IntrospectionOperationStatus, String> {
        let handler = {
            let inner = self.lock_inner();
            inner.operations.values().find_map(|operation| {
                if operation
                    .current_operation_ids
                    .iter()
                    .any(|id| id == operation_id)
                {
                    inner
                        .operation_cancel_handlers
                        .get(&operation.name)
                        .cloned()
                } else {
                    None
                }
            })
        };
        if let Some(handler) = handler {
            return handler(operation_id);
        }

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
            let _ = position;
            operation.current_state = Some("cancel_requested".to_string());
            operation.last_event = Some("flowrt.operation.state_changed".to_string());
            operation.last_error = None;
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
                "inflight": health.inflight,
                "scheduled_time_ms": health.scheduled_time_ms,
                "observed_time_ms": health.observed_time_ms,
                "lateness_ms": health.lateness_ms,
                "missed_periods": health.missed_periods,
                "overrun": health.overrun,
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

    pub(super) fn channel_status(&self, name: &str) -> Option<IntrospectionChannelStatus> {
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
        param.apply_state = "pending".to_string();
        param.last_reject_reason = None;
        param.updated_unix_ms = Some(unix_time_ms());
        let status = param_status(name, param);
        drop(inner);
        self.recorder.record_param_event_json(
            name,
            "flowrt.param.set_pending",
            serde_json::json!({ "pending": status.pending }),
        );
        Ok(status)
    }

    /// 读取参数 pending 值但不清空，供 generated shell 在 tick 边界先校验再提交。
    pub fn peek_pending_param(&self, name: &str) -> Option<serde_json::Value> {
        self.pending_param(name)
    }

    /// 读取并清空参数 pending 值。
    ///
    /// 新 generated shell 使用 `peek_pending_param` 和 applied/rejected 状态转换；该方法保留给
    /// 旧生成物和测试辅助。
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
            if param.pending.as_ref() == Some(&value) {
                param.pending = None;
            }
            param.current = value.clone();
            param.apply_state = "applied".to_string();
            param.last_reject_reason = None;
            param.updated_unix_ms = Some(unix_time_ms());
            drop(inner);
            self.recorder.record_param_event_json(
                name,
                "flowrt.param.applied",
                serde_json::json!({ "current": value }),
            );
        }
    }

    /// 记录参数 pending 值被 runtime apply 边界拒绝，保留旧 current。
    pub fn record_param_rejected(
        &self,
        name: &str,
        value: serde_json::Value,
        reason: impl Into<String>,
    ) {
        let reason = reason.into();
        let mut inner = self.lock_inner();
        if let Some(param) = inner.params.get_mut(name) {
            if param.pending.as_ref() == Some(&value) {
                param.pending = None;
            }
            param.apply_state = "rejected".to_string();
            param.last_reject_reason = Some(reason.clone());
            param.updated_unix_ms = Some(unix_time_ms());
            drop(inner);
            self.recorder.record_param_event_json(
                name,
                "flowrt.param.rejected",
                serde_json::json!({ "rejected": value, "reason": reason }),
            );
        }
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
