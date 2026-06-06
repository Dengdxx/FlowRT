//! FlowRT C ABI 基础类型。
//!
//! 本模块只提供跨语言边界可共享的 `repr(C)` POD 形状和值编码。它不是 Python
//! binding，也不暴露 backend SDK 句柄或 Rust/C++ runtime 对象所有权。

use std::ffi::c_char;

use crate::{BackendHealthSnapshot, BackendHealthState, BackendKind, ReconnectPolicy, Status};

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
