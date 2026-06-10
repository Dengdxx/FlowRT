//! FlowRT C ABI 基础类型。
//!
//! 本模块只提供跨语言边界可共享的 `repr(C)` POD 形状和值编码。它不是 Python
//! binding，也不暴露 backend SDK 句柄或 Rust/C++ runtime 对象所有权。

use std::ffi::c_char;

use crate::service::{ServiceError, ServiceFrameHeader};
use crate::{
    BackendHealthSnapshot, BackendHealthState, BackendKind, FrameDescriptor, FrameLeaseStatus,
    ReconnectPolicy, Status,
};

pub const FLOWRT_ABI_VERSION_MAJOR: u32 = 0;
pub const FLOWRT_ABI_VERSION_MINOR: u32 = 1;

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
