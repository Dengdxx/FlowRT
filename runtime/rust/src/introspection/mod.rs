//! FlowRT runtime 自描述与 live status 的最小 Unix socket 协议。
//!
//! socket 路径只用于发现候选进程；真实身份必须来自连接后的 handshake。协议保持同步、
//! JSON-line 和标准库实现，便于生成 shell 在不引入大型 runtime 依赖的情况下接入。

mod client;
mod diagnostics;
pub(crate) mod facts;
mod model;
mod params;
mod paths;
mod probe;
mod server;
mod state;

pub use client::{
    observe_channel_stream, observe_channel_stream_with_timeout, request_boundary_publish,
    request_boundary_publish_with_timeout, request_channel_snapshot,
    request_channel_snapshot_with_timeout, request_operation_cancel,
    request_operation_cancel_with_timeout, request_operation_start,
    request_operation_start_with_timeout, request_param_get, request_param_get_with_timeout,
    request_param_list, request_param_list_with_timeout, request_param_set,
    request_param_set_with_timeout, request_recorder_drain, request_recorder_drain_with_timeout,
    request_recorder_start, request_recorder_start_with_timeout, request_recorder_stop,
    request_recorder_stop_with_timeout, request_self_description,
    request_self_description_with_timeout, request_status, request_status_with_timeout,
};
pub use model::{
    INTROSPECTION_PROTOCOL_VERSION, IntrospectionBoundaryPublishStatus,
    IntrospectionChannelSnapshot, IntrospectionChannelStatus, IntrospectionClockStatus,
    IntrospectionDiagnostic, IntrospectionDiagnosticMetric, IntrospectionFailoverEvent,
    IntrospectionHandshake, IntrospectionIdentity, IntrospectionInputStatus,
    IntrospectionIoBoundaryResourceStatus, IntrospectionIoBoundaryStatus, IntrospectionLaneHealth,
    IntrospectionOperationStartStatus, IntrospectionOperationStatus, IntrospectionParamSchema,
    IntrospectionParamStatus, IntrospectionProcessStatus, IntrospectionRecorderStart,
    IntrospectionRecorderStatus, IntrospectionRequest, IntrospectionResourceStatus,
    IntrospectionResponse, IntrospectionRouteStatus, IntrospectionServiceStatus,
    IntrospectionStatus, IntrospectionTaskHealth,
};
pub use paths::{discover_runtime_sockets, runtime_socket_dir, runtime_socket_path_for_pid};
pub use probe::{IntrospectionChannelProbe, IntrospectionObserverGuard, IntrospectionProbeRecord};
pub use server::{IntrospectionServer, spawn_status_server, spawn_status_server_at};
pub use state::IntrospectionState;

#[cfg(test)]
use params::{ParamState, validate_param_json_value};
#[cfg(test)]
use server::{
    INTROSPECTION_INITIAL_REQUEST_TIMEOUT, MAX_INTROSPECTION_CLIENT_THREADS,
    MAX_INTROSPECTION_OBSERVERS, try_acquire_introspection_client_permit,
};

#[cfg(test)]
mod tests;
