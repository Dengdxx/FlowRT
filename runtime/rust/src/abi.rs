//! FlowRT C ABI 基础类型。
//!
//! 本模块只提供跨语言边界可共享的 `repr(C)` POD 形状和值编码。它不是 Python
//! binding，也不暴露 backend SDK 句柄或 Rust/C++ runtime 对象所有权。

use std::ffi::{c_char, c_void};

use crate::service::{ServiceError, ServiceFrameHeader};
use crate::{
    BackendHealthSnapshot, BackendHealthState, BackendKind, ClockSource, FrameDescriptor,
    FrameLeaseStatus, OperationId, OperationState, ReconnectPolicy, Status,
};

pub const FLOWRT_ABI_VERSION_MAJOR: u32 = 0;
pub const FLOWRT_ABI_VERSION_MINOR: u32 = 2;

pub const FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MAJOR: u32 = 0;
pub const FLOWRT_C_COMPONENT_CALLBACK_ABI_VERSION_MINOR: u32 = 3;
pub const FLOWRT_ABI_FEATURE_C_COMPONENT_CALLBACKS_V0: u64 = 1;
pub const FLOWRT_ABI_FEATURE_C_COMPONENT_TASK_TIMING_V1: u64 = 1 << 1;

pub type FlowrtStatus = u32;
pub const FLOWRT_STATUS_OK: FlowrtStatus = 0;
pub const FLOWRT_STATUS_RETRY: FlowrtStatus = 1;
pub const FLOWRT_STATUS_ERROR: FlowrtStatus = 2;

pub type FlowrtBackendKind = u32;
pub const FLOWRT_BACKEND_INPROC: FlowrtBackendKind = 0;
pub const FLOWRT_BACKEND_IOX2: FlowrtBackendKind = 1;
pub const FLOWRT_BACKEND_ZENOH: FlowrtBackendKind = 2;

pub type FlowrtBackendHealthState = u32;
pub const FLOWRT_BACKEND_HEALTH_READY: FlowrtBackendHealthState = 0;
pub const FLOWRT_BACKEND_HEALTH_DEGRADED: FlowrtBackendHealthState = 1;
pub const FLOWRT_BACKEND_HEALTH_RECONNECTING: FlowrtBackendHealthState = 2;
pub const FLOWRT_BACKEND_HEALTH_FAILED: FlowrtBackendHealthState = 3;
pub const FLOWRT_BACKEND_HEALTH_UNSUPPORTED: FlowrtBackendHealthState = 4;

/// C ABI string borrowed view.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtStringView {
    pub data: *const c_char,
    pub len: usize,
}

impl FlowrtStringView {
    pub const fn null() -> Self {
        Self {
            data: std::ptr::null(),
            len: 0,
        }
    }

    pub fn from_utf8(value: &str) -> Self {
        Self {
            data: value.as_ptr().cast::<c_char>(),
            len: value.len(),
        }
    }
}

/// C ABI bytes borrowed view.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtBytesView {
    pub data: *const u8,
    pub len: usize,
}

impl FlowrtBytesView {
    pub const fn null() -> Self {
        Self {
            data: std::ptr::null(),
            len: 0,
        }
    }

    pub fn from_slice(value: &[u8]) -> Self {
        Self {
            data: value.as_ptr(),
            len: value.len(),
        }
    }
}

/// C ABI unsigned 128-bit POD，低 64 位在前，高 64 位在后。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlowrtU128 {
    pub lo: u64,
    pub hi: u64,
}

/// C ABI signed 128-bit POD，使用 two's complement 位模式，低 64 位在前。
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlowrtI128 {
    pub lo: u64,
    pub hi: u64,
}

/// C ABI reconnect policy.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtReconnectPolicy {
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_attempts: u32,
    pub has_max_attempts: u8,
    pub reserved: [u8; 3],
}

/// C ABI backend health snapshot.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtBackendHealthSnapshot {
    pub state: FlowrtBackendHealthState,
    pub attempt: u32,
    pub next_retry_unix_ms: u64,
    pub last_error: FlowrtStringView,
    pub has_next_retry_unix_ms: u8,
    pub recoverable: u8,
    pub reserved: [u8; 6],
}

pub type FlowrtFrameLeaseStatus = u32;
pub const FLOWRT_FRAME_LEASE_ATTACHED: FlowrtFrameLeaseStatus = 0;
pub const FLOWRT_FRAME_LEASE_ACQUIRED: FlowrtFrameLeaseStatus = 1;
pub const FLOWRT_FRAME_LEASE_RELEASED: FlowrtFrameLeaseStatus = 2;
pub const FLOWRT_FRAME_LEASE_EXPIRED: FlowrtFrameLeaseStatus = 3;
pub const FLOWRT_FRAME_LEASE_GENERATION_MISMATCH: FlowrtFrameLeaseStatus = 4;
pub const FLOWRT_FRAME_LEASE_ERROR: FlowrtFrameLeaseStatus = 5;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtResourceDescriptor {
    pub resource_id: FlowrtStringView,
    pub slot: FlowrtStringView,
    pub generation: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtFrameDescriptor {
    pub resource: FlowrtResourceDescriptor,
    pub size_bytes: u64,
    pub format: FlowrtStringView,
    pub encoding: FlowrtStringView,
    pub metadata_json: FlowrtStringView,
}

// ── 后续语言边界 view ──────────────────────────────────────────────────────

pub type FlowrtFrameEncoding = u32;
pub const FLOWRT_FRAME_ENCODING_FIXED_PLAIN: FlowrtFrameEncoding = 0;
pub const FLOWRT_FRAME_ENCODING_CANONICAL_FRAME_V1: FlowrtFrameEncoding = 1;

/// C ABI 借用 message frame view。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtFrameView {
    pub channel_name: FlowrtStringView,
    pub message_type: FlowrtStringView,
    pub schema_hash: u64,
    pub encoding: FlowrtFrameEncoding,
    pub flags: u32,
    pub frame: FlowrtBytesView,
    pub header: FlowrtBytesView,
    pub tail: FlowrtBytesView,
    pub source_time_ms: u64,
    pub published_at_ms: u64,
    pub revision: u64,
    pub has_source_time_ms: u8,
    pub has_published_at_ms: u8,
    pub has_revision: u8,
    pub reserved: [u8; 5],
}

pub type FlowrtParamsUpdateStatus = u32;
pub const FLOWRT_PARAMS_UPDATE_ACCEPTED: FlowrtParamsUpdateStatus = 0;
pub const FLOWRT_PARAMS_UPDATE_APPLIED: FlowrtParamsUpdateStatus = 1;
pub const FLOWRT_PARAMS_UPDATE_REJECTED: FlowrtParamsUpdateStatus = 2;
pub const FLOWRT_PARAMS_UPDATE_PARTIAL: FlowrtParamsUpdateStatus = 3;
pub const FLOWRT_PARAMS_UPDATE_UNSUPPORTED: FlowrtParamsUpdateStatus = 4;
pub const FLOWRT_PARAMS_UPDATE_ERROR: FlowrtParamsUpdateStatus = 5;

/// C ABI 单个参数借用 view。JSON 字段都是 UTF-8 借用切片。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtParamView {
    pub instance_name: FlowrtStringView,
    pub param_name: FlowrtStringView,
    pub type_name: FlowrtStringView,
    pub update_policy: FlowrtStringView,
    pub current_json: FlowrtStringView,
    pub pending_json: FlowrtStringView,
    pub min_json: FlowrtStringView,
    pub max_json: FlowrtStringView,
    pub choices_json: FlowrtStringView,
    pub schema_hash: u64,
    pub revision: u64,
    pub mutable_at_runtime: u8,
    pub has_pending: u8,
    pub has_min: u8,
    pub has_max: u8,
    pub reserved: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtParamsView {
    pub data: *const FlowrtParamView,
    pub len: usize,
    pub revision: u64,
    pub applied_unix_ms: u64,
    pub has_applied_unix_ms: u8,
    pub reserved: [u8; 7],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtParamsUpdateResult {
    pub status: FlowrtParamsUpdateStatus,
    pub applied_count: u32,
    pub rejected_count: u32,
    pub reserved0: u32,
    pub revision: u64,
    pub error_index: u64,
    pub has_error_index: u8,
    pub reserved: [u8; 7],
    pub message: FlowrtStringView,
}

/// C component callback 中暴露的 readonly 参数快照。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtCParamSnapshotV0 {
    pub abi_version: u32,
    pub param_count: u32,
    pub params: *const FlowrtParamView,
    pub reserved: [u8; 16],
}

pub type FlowrtOperationState = u32;
pub const FLOWRT_OPERATION_STATE_IDLE: FlowrtOperationState = 0;
pub const FLOWRT_OPERATION_STATE_STARTING: FlowrtOperationState = 1;
pub const FLOWRT_OPERATION_STATE_RUNNING: FlowrtOperationState = 2;
pub const FLOWRT_OPERATION_STATE_CANCEL_REQUESTED: FlowrtOperationState = 3;
pub const FLOWRT_OPERATION_STATE_SUCCEEDED: FlowrtOperationState = 4;
pub const FLOWRT_OPERATION_STATE_FAILED: FlowrtOperationState = 5;
pub const FLOWRT_OPERATION_STATE_CANCELLED: FlowrtOperationState = 6;
pub const FLOWRT_OPERATION_STATE_TIMED_OUT: FlowrtOperationState = 7;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlowrtOperationId {
    pub operation_key: u64,
    pub client_id: u64,
    pub sequence: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtOperationIdArrayView {
    pub data: *const FlowrtOperationId,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtOperationStatusView {
    pub operation_name: FlowrtStringView,
    pub current_operation_ids: FlowrtOperationIdArrayView,
    pub running: u64,
    pub queued: u64,
    pub total_started: u64,
    pub succeeded_count: u64,
    pub failed_count: u64,
    pub canceled_count: u64,
    pub timeout_count: u64,
    pub preempted_count: u64,
    pub last_transition_ms: u64,
    pub ready: u8,
    pub has_last_transition_ms: u8,
    pub reserved: [u8; 6],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtOperationProgressView {
    pub operation_name: FlowrtStringView,
    pub id: FlowrtOperationId,
    pub sequence: u64,
    pub progress: FlowrtFrameView,
    pub published_at_ms: u64,
    pub has_published_at_ms: u8,
    pub reserved: [u8; 7],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtOperationResultSummaryView {
    pub operation_name: FlowrtStringView,
    pub id: FlowrtOperationId,
    pub state: FlowrtOperationState,
    pub has_result: u8,
    pub has_error_message: u8,
    pub has_completed_unix_ms: u8,
    pub reserved0: u8,
    pub completed_unix_ms: u64,
    pub result: FlowrtFrameView,
    pub error_message: FlowrtStringView,
}

pub type FlowrtDiagnosticSeverity = u32;
pub const FLOWRT_DIAGNOSTIC_INFO: FlowrtDiagnosticSeverity = 0;
pub const FLOWRT_DIAGNOSTIC_WARN: FlowrtDiagnosticSeverity = 1;
pub const FLOWRT_DIAGNOSTIC_ERROR: FlowrtDiagnosticSeverity = 2;

pub type FlowrtResourceHealthState = u32;
pub const FLOWRT_RESOURCE_HEALTH_UNKNOWN: FlowrtResourceHealthState = 0;
pub const FLOWRT_RESOURCE_HEALTH_READY: FlowrtResourceHealthState = 1;
pub const FLOWRT_RESOURCE_HEALTH_DEGRADED: FlowrtResourceHealthState = 2;
pub const FLOWRT_RESOURCE_HEALTH_FAILED: FlowrtResourceHealthState = 3;
pub const FLOWRT_RESOURCE_HEALTH_UNAVAILABLE: FlowrtResourceHealthState = 4;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtDiagnosticView {
    pub source: FlowrtStringView,
    pub code: FlowrtStringView,
    pub message: FlowrtStringView,
    pub severity: FlowrtDiagnosticSeverity,
    pub reserved0: u32,
    pub timestamp_unix_ms: u64,
    pub has_timestamp_unix_ms: u8,
    pub reserved: [u8; 7],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtResourceHealthSnapshot {
    pub name: FlowrtStringView,
    pub capability: FlowrtStringView,
    pub state: FlowrtResourceHealthState,
    pub ready: u8,
    pub required: u8,
    pub has_updated_unix_ms: u8,
    pub has_generation: u8,
    pub updated_unix_ms: u64,
    pub generation: u64,
    pub message: FlowrtStringView,
    pub last_error: FlowrtStringView,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtDiagnosticArrayView {
    pub data: *const FlowrtDiagnosticView,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtResourceHealthArrayView {
    pub data: *const FlowrtResourceHealthSnapshot,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtDiagnosticsSnapshot {
    pub package_name: FlowrtStringView,
    pub process_name: FlowrtStringView,
    pub diagnostics: FlowrtDiagnosticArrayView,
    pub resources: FlowrtResourceHealthArrayView,
    pub generated_unix_ms: u64,
    pub healthy: u8,
    pub has_generated_unix_ms: u8,
    pub reserved: [u8; 6],
}

// ── C component callback ABI ───────────────────────────────────────────────

pub type FlowrtCOutputStatus = u32;
pub const FLOWRT_C_OUTPUT_UNWRITTEN: FlowrtCOutputStatus = 0;
pub const FLOWRT_C_OUTPUT_WRITTEN: FlowrtCOutputStatus = 1;
pub const FLOWRT_C_OUTPUT_TRUNCATED: FlowrtCOutputStatus = 2;
pub const FLOWRT_C_OUTPUT_ERROR: FlowrtCOutputStatus = 3;

pub type FlowrtCClockSource = u32;
pub const FLOWRT_C_CLOCK_SOURCE_RUNTIME: FlowrtCClockSource = 0;
pub const FLOWRT_C_CLOCK_SOURCE_REPLAY: FlowrtCClockSource = 1;

/// C component task timing POD。字符串字段为借用 view。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtCTaskTiming {
    pub step: u64,
    pub task_name: FlowrtStringView,
    pub trigger: FlowrtStringView,
    pub clock_source: FlowrtCClockSource,
    pub reserved0: u32,
    pub scheduled_time_ms: u64,
    pub observed_time_ms: u64,
    pub scheduled_delta_ms: u64,
    pub observed_delta_ms: u64,
    pub period_ms: u64,
    pub deadline_ms: u64,
    pub lateness_ms: u64,
    pub missed_periods: u64,
    pub has_period_ms: u8,
    pub has_deadline_ms: u8,
    pub deadline_missed: u8,
    pub overrun: u8,
    pub reserved: [u8; 4],
}

/// C component callback 只读上下文。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtCComponentContext {
    pub component_name: FlowrtStringView,
    pub instance_name: FlowrtStringView,
    pub task_name: FlowrtStringView,
    pub lane_name: FlowrtStringView,
    pub step: u64,
    pub tick_time_ms: u64,
    pub deadline_ms: u64,
    pub has_deadline_ms: u8,
    pub has_timing: u8,
    pub reserved: [u8; 6],
    pub timing: FlowrtCTaskTiming,
    pub params: FlowrtCParamSnapshotV0,
}

/// C component fixed-size input borrowed view.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtCInputView {
    pub name: FlowrtStringView,
    pub type_name: FlowrtStringView,
    pub schema_hash: u64,
    pub size_bytes: u64,
    pub payload: FlowrtBytesView,
    pub source_time_ms: u64,
    pub revision: u64,
    pub present: u8,
    pub stale: u8,
    pub reserved: [u8; 6],
}

/// C component output borrowed slot.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtCOutputSlot {
    pub name: FlowrtStringView,
    pub type_name: FlowrtStringView,
    pub schema_hash: u64,
    pub size_bytes: u64,
    pub data: *mut u8,
    pub capacity: usize,
    pub written_len: usize,
    pub status: FlowrtCOutputStatus,
    pub reserved: [u8; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtCInputArrayView {
    pub data: *const FlowrtCInputView,
    pub len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtCOutputArrayView {
    pub data: *mut FlowrtCOutputSlot,
    pub len: usize,
}

pub type FlowrtCLifecycleCallback =
    Option<unsafe extern "C" fn(*mut c_void, *const FlowrtCComponentContext) -> FlowrtStatus>;

pub type FlowrtCTaskCallback = Option<
    unsafe extern "C" fn(
        *mut c_void,
        *const FlowrtCComponentContext,
        *const FlowrtCInputArrayView,
        *mut FlowrtCOutputArrayView,
    ) -> FlowrtStatus,
>;

/// C component callback table。
///
/// 函数指针使用 nullable C pointer 表达，便于 adapter 在调用前做显式版本、大小和
/// 必填 callback 校验；通过校验后再按 runtime 策略提供缺省 no-op 或拒绝启动。
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlowrtCComponentCallbackTable {
    pub size: u32,
    pub version_major: u32,
    pub version_minor: u32,
    pub reserved0: u32,
    pub feature_flags: u64,
    pub user_data: *mut c_void,
    pub on_init: FlowrtCLifecycleCallback,
    pub on_start: FlowrtCLifecycleCallback,
    pub on_stop: FlowrtCLifecycleCallback,
    pub on_shutdown: FlowrtCLifecycleCallback,
    pub run_periodic: FlowrtCTaskCallback,
    pub run_on_message: FlowrtCTaskCallback,
    pub run_startup: FlowrtCTaskCallback,
    pub run_shutdown: FlowrtCTaskCallback,
    pub reserved: [u64; 8],
}

pub const fn status_to_abi(status: Status) -> FlowrtStatus {
    match status {
        Status::Ok => FLOWRT_STATUS_OK,
        Status::Retry => FLOWRT_STATUS_RETRY,
        Status::Error => FLOWRT_STATUS_ERROR,
    }
}

pub const fn backend_kind_to_abi(kind: BackendKind) -> FlowrtBackendKind {
    match kind {
        BackendKind::Inproc => FLOWRT_BACKEND_INPROC,
        BackendKind::Iox2 => FLOWRT_BACKEND_IOX2,
        BackendKind::Zenoh => FLOWRT_BACKEND_ZENOH,
    }
}

pub const fn backend_health_state_to_abi(state: BackendHealthState) -> FlowrtBackendHealthState {
    match state {
        BackendHealthState::Ready => FLOWRT_BACKEND_HEALTH_READY,
        BackendHealthState::Degraded => FLOWRT_BACKEND_HEALTH_DEGRADED,
        BackendHealthState::Reconnecting => FLOWRT_BACKEND_HEALTH_RECONNECTING,
        BackendHealthState::Failed => FLOWRT_BACKEND_HEALTH_FAILED,
        BackendHealthState::Unsupported => FLOWRT_BACKEND_HEALTH_UNSUPPORTED,
    }
}

pub const fn clock_source_to_c_abi(source: ClockSource) -> FlowrtCClockSource {
    match source {
        ClockSource::Runtime => FLOWRT_C_CLOCK_SOURCE_RUNTIME,
        ClockSource::Replay => FLOWRT_C_CLOCK_SOURCE_REPLAY,
    }
}

pub const fn reconnect_policy_to_abi(policy: ReconnectPolicy) -> FlowrtReconnectPolicy {
    match policy.max_attempts() {
        Some(max_attempts) => FlowrtReconnectPolicy {
            initial_delay_ms: policy.initial_delay_ms(),
            max_delay_ms: policy.max_delay_ms(),
            max_attempts,
            has_max_attempts: 1,
            reserved: [0; 3],
        },
        None => FlowrtReconnectPolicy {
            initial_delay_ms: policy.initial_delay_ms(),
            max_delay_ms: policy.max_delay_ms(),
            max_attempts: 0,
            has_max_attempts: 0,
            reserved: [0; 3],
        },
    }
}

pub fn backend_health_snapshot_to_abi(
    snapshot: &BackendHealthSnapshot,
) -> FlowrtBackendHealthSnapshot {
    let (last_error, has_next_retry_unix_ms, next_retry_unix_ms) = (
        snapshot
            .last_error
            .as_deref()
            .map(FlowrtStringView::from_utf8)
            .unwrap_or_else(FlowrtStringView::null),
        u8::from(snapshot.next_retry_unix_ms.is_some()),
        snapshot.next_retry_unix_ms.unwrap_or(0),
    );

    FlowrtBackendHealthSnapshot {
        state: backend_health_state_to_abi(snapshot.state),
        attempt: snapshot.attempt,
        next_retry_unix_ms,
        last_error,
        has_next_retry_unix_ms,
        recoverable: u8::from(snapshot.recoverable),
        reserved: [0; 6],
    }
}

pub const fn frame_lease_status_to_abi(status: FrameLeaseStatus) -> FlowrtFrameLeaseStatus {
    match status {
        FrameLeaseStatus::Attached => FLOWRT_FRAME_LEASE_ATTACHED,
        FrameLeaseStatus::Acquired => FLOWRT_FRAME_LEASE_ACQUIRED,
        FrameLeaseStatus::Released => FLOWRT_FRAME_LEASE_RELEASED,
        FrameLeaseStatus::Expired => FLOWRT_FRAME_LEASE_EXPIRED,
        FrameLeaseStatus::GenerationMismatch => FLOWRT_FRAME_LEASE_GENERATION_MISMATCH,
        FrameLeaseStatus::Error => FLOWRT_FRAME_LEASE_ERROR,
    }
}

pub fn frame_descriptor_to_abi<'a>(
    descriptor: &'a FrameDescriptor,
    metadata_json: &'a str,
) -> FlowrtFrameDescriptor {
    FlowrtFrameDescriptor {
        resource: FlowrtResourceDescriptor {
            resource_id: FlowrtStringView::from_utf8(descriptor.resource().resource_id()),
            slot: FlowrtStringView::from_utf8(descriptor.resource().slot()),
            generation: descriptor.resource().generation(),
        },
        size_bytes: descriptor.size_bytes(),
        format: FlowrtStringView::from_utf8(descriptor.format()),
        encoding: FlowrtStringView::from_utf8(descriptor.encoding()),
        metadata_json: FlowrtStringView::from_utf8(metadata_json),
    }
}

pub const fn operation_id_to_abi(id: OperationId) -> FlowrtOperationId {
    FlowrtOperationId {
        operation_key: id.operation_key,
        client_id: id.client_id,
        sequence: id.sequence,
    }
}

pub const fn operation_state_to_abi(state: OperationState) -> FlowrtOperationState {
    match state {
        OperationState::Idle => FLOWRT_OPERATION_STATE_IDLE,
        OperationState::Starting => FLOWRT_OPERATION_STATE_STARTING,
        OperationState::Running => FLOWRT_OPERATION_STATE_RUNNING,
        OperationState::CancelRequested => FLOWRT_OPERATION_STATE_CANCEL_REQUESTED,
        OperationState::Succeeded => FLOWRT_OPERATION_STATE_SUCCEEDED,
        OperationState::Failed => FLOWRT_OPERATION_STATE_FAILED,
        OperationState::Cancelled => FLOWRT_OPERATION_STATE_CANCELLED,
        OperationState::TimedOut => FLOWRT_OPERATION_STATE_TIMED_OUT,
    }
}

// ── Service ABI ────────────────────────────────────────────────────────────

/// Service error ABI 编码类型。
pub type FlowrtServiceError = u16;
pub const FLOWRT_SERVICE_OK: FlowrtServiceError = 0;
pub const FLOWRT_SERVICE_TIMEOUT: FlowrtServiceError = 1;
pub const FLOWRT_SERVICE_UNAVAILABLE: FlowrtServiceError = 2;
pub const FLOWRT_SERVICE_BUSY: FlowrtServiceError = 3;
pub const FLOWRT_SERVICE_REJECTED: FlowrtServiceError = 4;
pub const FLOWRT_SERVICE_CANCELLED: FlowrtServiceError = 5;
pub const FLOWRT_SERVICE_DEADLINE_EXCEEDED: FlowrtServiceError = 6;
pub const FLOWRT_SERVICE_PROTOCOL: FlowrtServiceError = 7;
pub const FLOWRT_SERVICE_BACKEND: FlowrtServiceError = 8;
pub const FLOWRT_SERVICE_WOULD_DEADLOCK: FlowrtServiceError = 9;
pub const FLOWRT_SERVICE_HANDLER_ERROR: FlowrtServiceError = 10;

/// service frame 魔数常量。
pub const FLOWRT_SERVICE_FRAME_MAGIC: u32 = 0x5352_5646;
/// service frame 协议版本常量。
pub const FLOWRT_SERVICE_FRAME_VERSION: u16 = 1;
/// service frame 固定 header 字节数。
pub const FLOWRT_SERVICE_FRAME_HEADER_SIZE: usize = 80;

/// C ABI service frame header，固定 80 字节。
///
/// 字段布局与 Rust `ServiceFrameHeader` 和 C++ `ServiceFrameHeader` 完全对齐。
/// 变长字段（payload、error message）通过尾部 VarSpan 描述符寻址，不包含在该结构体中。
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowrtServiceFrameHeader {
    pub magic: u32,
    pub version: u16,
    pub error_code: FlowrtServiceError,
    pub service_id: u64,
    pub session_id: u64,
    pub sequence: u64,
    pub correlation_id: u64,
    pub timeout_ms: u64,
    pub absolute_deadline_ms: u64,
    pub schema_hash: u64,
    pub payload_offset: u32,
    pub payload_len: u32,
    pub error_msg_offset: u32,
    pub error_msg_len: u32,
}

pub const fn service_error_to_abi(error: ServiceError) -> FlowrtServiceError {
    error as u16
}

pub fn service_frame_header_to_abi(header: &ServiceFrameHeader) -> FlowrtServiceFrameHeader {
    FlowrtServiceFrameHeader {
        magic: header.magic,
        version: header.version,
        error_code: header.error_code,
        service_id: header.service_id,
        session_id: header.session_id,
        sequence: header.sequence,
        correlation_id: header.correlation_id,
        timeout_ms: header.timeout_ms,
        absolute_deadline_ms: header.absolute_deadline_ms,
        schema_hash: header.schema_hash,
        payload_offset: header.payload_span.offset(),
        payload_len: header.payload_span.len(),
        error_msg_offset: header.error_msg_span.offset(),
        error_msg_len: header.error_msg_span.len(),
    }
}
