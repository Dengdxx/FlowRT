//! FlowRT Rust runtime 的基础 API。
//!
//! 本 crate 只承载 runtime shell 和用户组件需要直接接触的薄接口：状态、上下文、输入视图、
//! 输出句柄、channel 语义和 backend 抽象。用户算法不应依赖具体通信库 API。

pub mod abi;
pub mod backend;
pub mod channel;
pub mod descriptor;
pub mod executor;
pub mod frame;
pub mod inproc;
pub mod introspection;
#[cfg(feature = "iox2")]
pub mod iox2;
pub mod lifecycle;
pub mod operation;
#[cfg(feature = "zenoh")]
pub mod params_remote;
pub mod recorder;
pub mod replay;
pub mod service;
pub mod shutdown;
pub mod supervisor;
pub mod synchronizer;
pub mod time_driver;
pub mod wire;
#[cfg(feature = "zenoh")]
pub mod zenoh;

#[cfg(all(test, feature = "zenoh"))]
pub(crate) fn zenoh_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static ZENOH_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ZENOH_TEST_MUTEX
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

pub use backend::{
    Backend, BackendHealthSnapshot, BackendHealthState, BackendHealthTracker, BackendKind,
    InprocBackend, InprocScheduler, Iox2Backend, ReconnectPolicy, Scheduler, ZenohBackend,
    inproc_backend, iox2_backend, zenoh_backend,
};
pub use channel::{
    BackendCapabilities, BoundaryInput, BoundaryInputRead, BoundaryOutput, BoundaryOutputSinkGuard,
    ChannelError, ChannelWriteOutcome, FifoChannel, FifoRead, LatestChannel, OverflowPolicy,
    StaleConfig, StalePolicy,
};
pub use descriptor::{
    FrameDescriptor, FrameDescriptorError, FrameDescriptorFields, FrameLease, FrameLeaseError,
    FrameLeaseStatus, FrameMetadata, ResourceDescriptor,
};
pub use executor::{
    DeterministicExecutor, FutureExecutor, FutureHandle, LaneId, LaneKind, PeriodicSpec,
    ReadyBatch, ScheduleEvent, ScheduleWaiter, TaskAdmission, TaskId, TaskRunOutcome,
    TaskRunOutput, TaskRunResult, TaskSpec, WorkerCompletionQueue, WorkerPool, WorkerSubmitError,
};
pub use frame::{FrameCodec, FrameDecoder, VAR_SPAN_WIRE_SIZE, VarSpan, append_tail_block};
#[cfg(feature = "iox2")]
pub use iceoryx2::prelude::ZeroCopySend;
pub use introspection::{
    INTROSPECTION_PROTOCOL_VERSION, IntrospectionBoundaryPublishStatus, IntrospectionChannelProbe,
    IntrospectionChannelStatus, IntrospectionClockStatus, IntrospectionDiagnostic,
    IntrospectionDiagnosticMetric, IntrospectionHandshake, IntrospectionIdentity,
    IntrospectionInputStatus, IntrospectionIoBoundaryResourceStatus, IntrospectionIoBoundaryStatus,
    IntrospectionLaneHealth, IntrospectionObserverGuard, IntrospectionOperationStatus,
    IntrospectionParamSchema, IntrospectionParamStatus, IntrospectionProbeRecord,
    IntrospectionProcessStatus, IntrospectionRecorderStart, IntrospectionRecorderStatus,
    IntrospectionRequest, IntrospectionResourceStatus, IntrospectionResponse,
    IntrospectionRouteStatus, IntrospectionServer, IntrospectionServiceStatus, IntrospectionState,
    IntrospectionStatus, IntrospectionTaskHealth, discover_runtime_sockets, observe_channel_stream,
    observe_channel_stream_with_timeout, request_boundary_publish,
    request_boundary_publish_with_timeout, request_channel_snapshot,
    request_channel_snapshot_with_timeout, request_operation_cancel,
    request_operation_cancel_with_timeout, request_param_get, request_param_get_with_timeout,
    request_param_list, request_param_list_with_timeout, request_param_set,
    request_param_set_with_timeout, request_recorder_drain, request_recorder_drain_with_timeout,
    request_recorder_start, request_recorder_start_with_timeout, request_recorder_stop,
    request_recorder_stop_with_timeout, request_self_description,
    request_self_description_with_timeout, request_status, request_status_with_timeout,
    runtime_socket_dir, runtime_socket_path_for_pid, spawn_status_server, spawn_status_server_at,
};
pub use lifecycle::LifecycleState;
pub use operation::{
    OperationCancelToken, OperationClientError, OperationConcurrencyPolicy, OperationControl,
    OperationControlError, OperationError, OperationHandlerResult, OperationHealthCounters,
    OperationHealthSnapshot, OperationId, OperationLifecycle, OperationOwner, OperationPolicy,
    OperationPreemptPolicy, OperationProgress, OperationProgressPublisher, OperationRuntimeEvent,
    OperationRuntimeEventKind, OperationStartAck, OperationStartRequest, OperationState,
    OperationStatusSnapshot, monotonic_time_ms,
};
#[cfg(feature = "zenoh")]
pub use params_remote::{
    ParamsRemoteError, ZenohParamsServer, params_key_expr, request_remote_param_get,
    request_remote_param_list, request_remote_param_set,
};
pub use recorder::{
    RecorderRuntimeMetadata, RecorderStartConfig, RecorderStatus, RecorderTap, RecorderTapOutcome,
};
pub use replay::{boundary_replay_events, replay_driver_from_mcap};
pub use service::{
    Deadline, InprocServiceClient, InprocServiceConfig, InprocServiceServer, LaneGuard, RequestId,
    SERVICE_FRAME_HEADER_SIZE, SERVICE_FRAME_MAGIC, SERVICE_FRAME_VERSION, ServiceCallHandle,
    ServiceError, ServiceFrameHeader, ServiceOverflowPolicy, ServiceRegistry, ServiceResult,
    ServiceStatsSnapshot, decode_service_frame, encode_service_frame, enter_lane, fnv1a64,
};
pub use shutdown::{ShutdownToken, install_signal_shutdown_token};
pub use time_driver::{ReplayDriver, ReplayEvent, Step, StepController, SteppedDriver, TimeDriver};
pub use wire::{WireCodec, WireCodecError};

/// 生成组件接口返回的执行状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// 本次步骤完成，调度器可以继续执行后续 tick。
    #[default]
    Ok,
    /// 本次步骤未完成，调用方可按调度策略稍后重试。
    Retry,
    /// 本次步骤失败，调度器应停止当前运行序列并向上报告。
    Error,
}

impl Status {
    /// 返回成功状态。
    pub const fn ok() -> Self {
        Self::Ok
    }
}

/// 外部 global tick 协调器授予单进程 runtime shell 的一步执行许可。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalTick {
    /// 全局 tick 序号，由 coordinator 分配。
    pub tick_id: u64,
    /// 本 tick 对应的逻辑毫秒时间。
    pub logical_time_ms: u64,
}

/// runtime shell 完成外部 tick 后回报给 coordinator 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalTickReport {
    /// 完成的 tick 序号。
    pub tick_id: u64,
    /// 本 tick 的 FlowRT 执行状态。
    pub status: Status,
}

impl ExternalTickReport {
    /// 构造指定状态的 tick 回报。
    pub const fn new(tick_id: u64, status: Status) -> Self {
        Self { tick_id, status }
    }

    /// 构造成功 tick 回报。
    pub const fn ok(tick_id: u64) -> Self {
        Self::new(tick_id, Status::Ok)
    }
}

/// I/O boundary 组件可用的运行态上报上下文。
///
/// 该类型只表达 FlowRT 的资源、readiness 和 health 语义，不暴露串口、SHM、网络或 backend
/// SDK 句柄。真实 I/O 仍由用户代码管理。
#[derive(Debug, Clone)]
pub struct BoundaryContext {
    instance: String,
    component: String,
    state: IntrospectionState,
}

impl BoundaryContext {
    /// 创建一个绑定到指定 instance 的 boundary 上下文。
    pub fn new(
        instance: impl Into<String>,
        component: impl Into<String>,
        state: IntrospectionState,
    ) -> Self {
        Self {
            instance: instance.into(),
            component: component.into(),
            state,
        }
    }

    /// 返回当前 boundary instance 名称。
    pub fn instance(&self) -> &str {
        &self.instance
    }

    /// 返回当前 boundary component 类型名称。
    pub fn component(&self) -> &str {
        &self.component
    }

    /// 标记 boundary 已满足 readiness gate。
    pub fn mark_ready(&self) {
        self.state.mark_io_boundary_ready(&self.instance, true);
    }

    /// 标记 boundary 暂不可用。
    pub fn mark_not_ready(&self) {
        self.state.mark_io_boundary_ready(&self.instance, false);
    }

    /// 标记 boundary 当前健康。
    pub fn report_healthy(&self) {
        self.state.record_io_boundary_healthy(&self.instance);
    }

    /// 上报 boundary 级错误。
    pub fn report_error(&self, error: impl Into<String>) {
        self.state
            .record_io_boundary_error(&self.instance, error.into());
    }

    /// 标记声明资源已就绪。
    pub fn mark_resource_ready(&self, resource: impl AsRef<str>) {
        self.state
            .record_io_boundary_resource_ready(&self.instance, resource.as_ref(), true, None);
    }

    /// 标记声明资源暂未就绪并给出状态说明。
    pub fn mark_resource_not_ready(&self, resource: impl AsRef<str>, message: impl Into<String>) {
        self.state.record_io_boundary_resource_ready(
            &self.instance,
            resource.as_ref(),
            false,
            Some(message.into()),
        );
    }

    /// 上报声明资源错误。
    pub fn report_resource_error(&self, resource: impl AsRef<str>, error: impl Into<String>) {
        self.state.record_io_boundary_resource_error(
            &self.instance,
            resource.as_ref(),
            error.into(),
        );
    }

    /// 记录 frame descriptor / side-channel lease 事件，不复制真实 payload。
    pub fn record_frame_descriptor_event(
        &self,
        name: impl AsRef<str>,
        descriptor: &FrameDescriptor,
        status: FrameLeaseStatus,
        payload_recording: bool,
    ) -> RecorderTapOutcome {
        self.state.record_frame_descriptor_event(
            name.as_ref(),
            descriptor,
            status,
            payload_recording,
        )
    }

    /// 记录标准 fixed ABI frame descriptor 字段，不复制真实 payload。
    pub fn record_frame_descriptor_fields_event(
        &self,
        name: impl AsRef<str>,
        descriptor: FrameDescriptorFields,
        status: FrameLeaseStatus,
        payload_recording: bool,
    ) -> Result<RecorderTapOutcome, FrameDescriptorError> {
        let descriptor = descriptor.to_descriptor()?;
        Ok(self.record_frame_descriptor_event(name, &descriptor, status, payload_recording))
    }

    /// 便捷记录 side-channel acquire 成功事件。
    pub fn record_frame_descriptor_acquired(
        &self,
        name: impl AsRef<str>,
        descriptor: &FrameDescriptor,
        payload_recording: bool,
    ) -> RecorderTapOutcome {
        self.record_frame_descriptor_event(
            name,
            descriptor,
            FrameLeaseStatus::Acquired,
            payload_recording,
        )
    }

    /// 便捷记录 side-channel release 事件。
    pub fn record_frame_descriptor_released(
        &self,
        name: impl AsRef<str>,
        descriptor: &FrameDescriptor,
        payload_recording: bool,
    ) -> RecorderTapOutcome {
        self.record_frame_descriptor_event(
            name,
            descriptor,
            FrameLeaseStatus::Released,
            payload_recording,
        )
    }
}

/// task timing 使用的毫秒时间来源。
///
/// `Runtime` 表示由 generated scheduler 维护的运行态单调毫秒模型；`Replay` 表示
/// `flowrt replay` 或 temporary island overlay 使用 fixture `at_ms` 驱动同一毫秒模型。
/// 该枚举只标记来源，不在 runtime primitive 中采样 wall-clock 时间。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockSource {
    /// 运行态 scheduler 的逻辑毫秒时间。
    Runtime,
    /// replay fixture 驱动的模拟毫秒时间。
    Replay,
}

/// 单次 task 回调可观察的调度时间上下文。
///
/// 该结构由 generated scheduler 或测试注入，runtime primitive 本身不读取 system time。
/// `scheduled_*` 表达 scheduler 计划时间，`observed_*` 表达本次回调实际观察到的 runtime
/// 毫秒时间；在 replay 模式下这些值来自 fixture `at_ms` 驱动的同一时间模型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTiming {
    /// 当前 scheduler step 序号。
    pub step: u64,
    /// 当前执行的 task 名称。
    pub task_name: String,
    /// task trigger 名称，例如 `periodic`、`on_message`、`startup` 或 `shutdown`。
    pub trigger: String,
    /// 本次 timing 的时间来源。
    pub clock_source: ClockSource,
    /// scheduler 计划唤醒或执行该 task 的毫秒时间。
    pub scheduled_time_ms: u64,
    /// runtime 在回调边界观察到的毫秒时间。
    pub observed_time_ms: u64,
    /// 相对上一计划时间的毫秒间隔。
    pub scheduled_delta_ms: u64,
    /// 相对上一观察时间的毫秒间隔。
    pub observed_delta_ms: u64,
    /// periodic task 的声明周期；非 periodic task 为 `None`。
    pub period_ms: Option<u64>,
    /// task 的声明 deadline；未声明时为 `None`。
    pub deadline_ms: Option<u64>,
    /// `observed_time_ms` 超过 `scheduled_time_ms` 的毫秒数；未迟到时为 0。
    pub lateness_ms: u64,
    /// scheduler 已跳过的周期数。
    pub missed_periods: u64,
    /// 本次回调是否超过声明 deadline。
    pub deadline_missed: bool,
    /// 本次执行是否超过声明周期或调度窗口。
    pub overrun: bool,
}

/// runtime 传递给生命周期钩子和调度步骤的上下文。
///
/// 普通组件看到空上下文；I/O boundary 组件会收到带 `BoundaryContext` 的上下文，用于
/// 上报资源、readiness 和 health。task 回调可额外收到 `TaskTiming`，生命周期上下文默认
/// 不携带 timing。上下文不暴露底层 backend SDK。
#[derive(Debug, Default)]
pub struct Context {
    boundary: Option<BoundaryContext>,
    timing: Option<TaskTiming>,
}

impl Context {
    /// 构造普通组件上下文。
    pub fn new() -> Self {
        Self::default()
    }

    /// 构造带 task timing 的上下文。
    pub fn with_timing(timing: TaskTiming) -> Self {
        let mut context = Self::default();
        context.set_timing(timing);
        context
    }

    /// 构造 I/O boundary 上下文。
    pub fn for_boundary(boundary: BoundaryContext) -> Self {
        Self {
            boundary: Some(boundary),
            timing: None,
        }
    }

    /// 返回当前 I/O boundary 上下文；普通组件返回 `None`。
    pub fn boundary(&self) -> Option<&BoundaryContext> {
        self.boundary.as_ref()
    }

    /// 返回当前 I/O boundary 可变上下文；普通组件返回 `None`。
    pub fn boundary_mut(&mut self) -> Option<&mut BoundaryContext> {
        self.boundary.as_mut()
    }

    /// 替换当前 task timing；生命周期上下文可保持 `None`。
    pub fn set_timing(&mut self, timing: TaskTiming) {
        self.timing = Some(timing);
    }

    /// 返回当前 task timing；普通生命周期上下文返回 `None`。
    pub fn timing(&self) -> Option<&TaskTiming> {
        self.timing.as_ref()
    }

    /// 返回当前 task 的逻辑观察时间（毫秒）；生命周期上下文无 timing 时返回 `None`。
    ///
    /// 这是用户算法获取时间的规范入口：调度时间一律来自 runtime 时钟——realtime 下是 runtime
    /// monotonic 时间，simulated_replay 下是注入事件驱动的逻辑时间。用户不得改用 `Instant::now`
    /// 或 `SystemTime`，否则回放将失去确定性。
    pub fn now_ms(&self) -> Option<u64> {
        self.timing.as_ref().map(|timing| timing.observed_time_ms)
    }

    /// 返回当前 task 的逻辑观察时间（秒），便于连续量积分。
    pub fn now_secs(&self) -> Option<f64> {
        self.now_ms().map(|ms| ms as f64 / 1000.0)
    }

    /// 返回相对上一次本 task 运行的观察时间间隔（毫秒）；首个样本为 0。
    pub fn dt_ms(&self) -> Option<u64> {
        self.timing.as_ref().map(|timing| timing.observed_delta_ms)
    }

    /// 返回相对上一次本 task 运行的观察时间间隔（秒），是积分步长的规范来源。
    pub fn dt_secs(&self) -> Option<f64> {
        self.dt_ms().map(|ms| ms as f64 / 1000.0)
    }

    /// 判断当前上下文是否属于 I/O boundary 组件。
    pub fn is_io_boundary(&self) -> bool {
        self.boundary.is_some()
    }
}

/// latest snapshot 输入视图。
///
/// `Latest<'_, T>` 不拥有消息对象，只在一次用户回调期间借用 runtime shell 中的最新样本。
/// `present()` 表示当前是否有可读样本，`stale()` 表示样本是否超过 RSDL 声明的 freshness
/// 约束。用户代码不得保存内部引用到回调之外。
#[derive(Debug, Clone, Copy)]
pub struct Latest<'a, T> {
    value: Option<&'a T>,
    stale: bool,
}

impl<'a, T> Latest<'a, T> {
    /// 从可选借用值和 stale 标记构造输入视图。
    pub fn new(value: Option<&'a T>, stale: bool) -> Self {
        Self { value, stale }
    }

    /// 判断当前输入是否有样本。
    pub fn present(&self) -> bool {
        self.value.is_some()
    }

    /// 判断当前样本是否已过期。
    pub fn stale(&self) -> bool {
        self.stale
    }

    /// 借用当前样本。
    pub fn as_ref(&self) -> Option<&'a T> {
        self.value
    }
}

/// 组件输出端口的单样本写入句柄。
///
/// 用户回调通过 `write()` 设置本次输出。generated shell 在回调返回后取走该值，只有
/// task 返回 `Status::Ok` 时才由 scheduler 线程提交到对应 channel；如果用户没有写入，
/// 该端口本次 tick 不产生输出。
#[derive(Debug, Default)]
pub struct Output<T> {
    value: Option<T>,
}

/// generated shell 在 scheduler 线程提交输出时使用的 typed commit record。
///
/// 该类型只保存 FlowRT 层的 task、port、payload 和调度时间上下文。真正写入哪个 backend
/// route 由 generated shell 根据 `port` 或外层闭包决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputCommitRecord<T> {
    /// 产生该输出的 task id。
    pub task: TaskId,
    /// 产生该输出的 task 名称。
    pub task_name: String,
    /// 输出端口名称。
    pub port: String,
    /// 待发布 payload。
    pub payload: T,
    /// scheduler 决定发布输出的 runtime 毫秒时间。
    pub published_at_ms: u64,
    /// 本次 task tick 对应的 runtime 毫秒时间。
    pub tick_time_ms: u64,
}

impl<T> OutputCommitRecord<T> {
    /// 用 route-specific closure 消费 payload 并提交。
    pub fn commit_with<R>(self, commit: impl FnOnce(T, u64, u64) -> R) -> R {
        commit(self.payload, self.published_at_ms, self.tick_time_ms)
    }
}

impl<T> Output<T> {
    /// 构造空输出句柄。
    pub fn new() -> Self {
        Self { value: None }
    }

    /// 写入本次回调的输出样本；重复调用时后一次写入覆盖前一次值。
    pub fn write(&mut self, value: T) {
        self.value = Some(value);
    }

    /// 取走当前输出样本并清空句柄。
    pub fn take(&mut self) -> Option<T> {
        self.value.take()
    }

    /// 取走当前输出样本并附加 scheduler commit 所需上下文。
    pub fn take_commit_record(
        &mut self,
        task: TaskId,
        task_name: impl Into<String>,
        port: impl Into<String>,
        published_at_ms: u64,
        tick_time_ms: u64,
    ) -> Option<OutputCommitRecord<T>> {
        self.take().map(|payload| OutputCommitRecord {
            task,
            task_name: task_name.into(),
            port: port.into(),
            payload,
            published_at_ms,
            tick_time_ms,
        })
    }

    /// 借用当前输出样本。
    pub fn as_ref(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// 可变借用当前输出样本。
    pub fn as_mut(&mut self) -> Option<&mut T> {
        self.value.as_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_tracks_presence_and_staleness() {
        let value = 42u32;
        let latest = Latest::new(Some(&value), true);
        assert!(latest.present());
        assert!(latest.stale());
        assert_eq!(latest.as_ref(), Some(&42));
    }

    #[test]
    fn output_can_store_and_take_values() {
        let mut output = Output::new();
        output.write(7u32);
        assert_eq!(output.as_ref(), Some(&7));
        assert_eq!(output.take(), Some(7));
        assert!(output.as_ref().is_none());
    }

    #[test]
    fn output_can_form_commit_record_without_changing_write_api() {
        let mut output = Output::new();
        output.write(7u32);

        let record = output
            .take_commit_record(TaskId(42), "camera.step", "pose", 100, 90)
            .expect("written output should form a commit record");

        assert_eq!(record.task, TaskId(42));
        assert_eq!(record.task_name, "camera.step");
        assert_eq!(record.port, "pose");
        assert_eq!(record.payload, 7);
        assert_eq!(record.published_at_ms, 100);
        assert_eq!(record.tick_time_ms, 90);
        assert!(output.take().is_none());
    }

    #[test]
    fn context_exposes_optional_boundary_context() {
        let plain = Context::new();
        assert!(!plain.is_io_boundary());
        assert!(plain.boundary().is_none());
        assert!(plain.timing().is_none());

        let state = IntrospectionState::new();
        let boundary = BoundaryContext::new("camera", "CameraDriver", state.clone());
        let mut context = Context::for_boundary(boundary);
        assert!(context.is_io_boundary());
        assert_eq!(context.boundary().unwrap().instance(), "camera");
        assert_eq!(context.boundary().unwrap().component(), "CameraDriver");
        assert!(context.timing().is_none());

        context.set_timing(TaskTiming {
            step: 9,
            task_name: "camera.capture".to_string(),
            trigger: "periodic".to_string(),
            clock_source: ClockSource::Replay,
            scheduled_time_ms: 1_000,
            observed_time_ms: 1_008,
            scheduled_delta_ms: 20,
            observed_delta_ms: 28,
            period_ms: Some(20),
            deadline_ms: Some(5),
            lateness_ms: 8,
            missed_periods: 0,
            deadline_missed: true,
            overrun: true,
        });
        assert_eq!(
            context.timing().map(|timing| timing.task_name.as_str()),
            Some("camera.capture")
        );
        assert!(context.is_io_boundary());
    }

    #[test]
    fn context_exposes_optional_task_timing() {
        let timing = TaskTiming {
            step: 3,
            task_name: "planner.update".to_string(),
            trigger: "on_message".to_string(),
            clock_source: ClockSource::Runtime,
            scheduled_time_ms: 120,
            observed_time_ms: 125,
            scheduled_delta_ms: 40,
            observed_delta_ms: 45,
            period_ms: None,
            deadline_ms: Some(10),
            lateness_ms: 5,
            missed_periods: 2,
            deadline_missed: false,
            overrun: false,
        };

        let mut context = Context::with_timing(timing.clone());

        assert!(!context.is_io_boundary());
        assert!(context.boundary().is_none());
        assert_eq!(context.timing(), Some(&timing));

        let updated = TaskTiming {
            clock_source: ClockSource::Replay,
            scheduled_time_ms: 200,
            observed_time_ms: 200,
            deadline_missed: true,
            ..timing
        };
        context.set_timing(updated.clone());

        assert_eq!(context.timing(), Some(&updated));
    }

    #[test]
    fn external_tick_step_reads_snapshot_before_commit() {
        let mut channel = LatestChannel::new();
        channel.publish_at(0u32, 0);
        let grant = ExternalTick {
            tick_id: 1,
            logical_time_ms: 10,
        };
        let report = ExternalTickReport::ok(grant.tick_id);
        let tick_1_input = channel.view_at(grant.logical_time_ms);

        let mut output = Output::new();
        output.write(1u32);
        let commit = output
            .take_commit_record(
                TaskId(1),
                "producer.main",
                "value",
                grant.logical_time_ms,
                grant.logical_time_ms,
            )
            .expect("task output should form commit record");

        assert_eq!(tick_1_input.as_ref(), Some(&0));
        assert_eq!(channel.view_at(grant.logical_time_ms).as_ref(), Some(&0));
        commit.commit_with(|payload, published_at_ms, _tick_time_ms| {
            channel.publish_at(payload, published_at_ms);
        });
        assert_eq!(channel.view_at(20).as_ref(), Some(&1));
        assert_eq!(report.tick_id, grant.tick_id);
        assert_eq!(report.status, Status::Ok);
    }

    #[test]
    fn tick_done_records_route_health_and_lifecycle_delta() {
        let grant = ExternalTick {
            tick_id: 9,
            logical_time_ms: 90,
        };
        let state = IntrospectionState::new();
        state.record_tick_at(grant.logical_time_ms, "global_tick");
        state.record_task_health(IntrospectionTaskHealth {
            name: "controller.main".to_string(),
            lane: "controller_serial".to_string(),
            run_count: 1,
            success_count: 1,
            scheduled_time_ms: Some(grant.logical_time_ms),
            observed_time_ms: Some(grant.logical_time_ms),
            last_run_ms: Some(grant.logical_time_ms),
            last_success_ms: Some(grant.logical_time_ms),
            ..Default::default()
        });

        let report = ExternalTickReport::new(grant.tick_id, Status::Retry);
        let status = state.status();

        assert_eq!(status.tick_count, 1);
        assert_eq!(status.clock.source, "global_tick");
        assert_eq!(status.clock.tick_time_ms, Some(grant.logical_time_ms));
        assert_eq!(
            state
                .task_health("controller.main")
                .and_then(|health| health.last_success_ms),
            Some(grant.logical_time_ms)
        );
        assert_eq!(report.tick_id, grant.tick_id);
        assert_eq!(report.status, Status::Retry);
    }

    #[test]
    fn context_now_and_dt_read_runtime_clock() {
        let timing = TaskTiming {
            step: 1,
            task_name: "planner.update".to_string(),
            trigger: "periodic".to_string(),
            clock_source: ClockSource::Replay,
            scheduled_time_ms: 1_000,
            observed_time_ms: 1_005,
            scheduled_delta_ms: 5,
            observed_delta_ms: 6,
            period_ms: Some(5),
            deadline_ms: None,
            lateness_ms: 0,
            missed_periods: 0,
            deadline_missed: false,
            overrun: false,
        };
        let context = Context::with_timing(timing);
        assert_eq!(context.now_ms(), Some(1_005));
        assert_eq!(context.dt_ms(), Some(6));
        assert_eq!(context.now_secs(), Some(1_005.0 / 1000.0));
        assert_eq!(context.dt_secs(), Some(6.0 / 1000.0));

        // 生命周期上下文无 timing：now/dt 一律返回 None，不伪装出有效时间。
        let lifecycle = Context::new();
        assert_eq!(lifecycle.now_ms(), None);
        assert_eq!(lifecycle.dt_ms(), None);
        assert_eq!(lifecycle.now_secs(), None);
        assert_eq!(lifecycle.dt_secs(), None);
    }
}
