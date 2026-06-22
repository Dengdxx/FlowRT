//! Zenoh control-plane adapter：跨机器远程参数查询与设置。
//!
//! 该模块只在启用 `zenoh` feature 时编译。它使用 zenoh query/queryable 实现参数的远程
//! list/get/set，复用与本机 Unix socket 路径相同的 schema 校验、structured error 和
//! pending/apply 语义。key expression 包含 package、selfdesc hash 和 PID，避免同机
//! 多进程冲突。
//!
//! 参数是 runtime control-plane 语义，不并入 graph 业务 Service。

use std::sync::Arc;

use zenoh::{Session, Wait, query::Query, query::Queryable};

use crate::introspection::{
    IntrospectionHandshake, IntrospectionRequest, IntrospectionResponse, IntrospectionState,
};

/// 参数控制面操作失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamsRemoteError {
    operation: &'static str,
    message: String,
}

impl ParamsRemoteError {
    fn new(operation: &'static str, message: impl std::fmt::Display) -> Self {
        Self {
            operation,
            message: message.to_string(),
        }
    }

    fn transport(operation: &'static str, error: impl std::fmt::Debug) -> Self {
        Self::new(operation, format!("{error:?}"))
    }

    /// 返回失败的操作名称。
    pub fn operation(&self) -> &'static str {
        self.operation
    }

    /// 返回不含具体 zenoh 类型的错误消息。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ParamsRemoteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.operation, self.message)
    }
}

impl std::error::Error for ParamsRemoteError {}

/// 远程参数 queryable 服务端。
///
/// 注册到 zenoh session 后，响应远程 `params list/get/set` 请求，复用 `IntrospectionState`
/// 的 schema 校验和 pending/apply 语义。drop 时自动注销 queryable。
pub struct ZenohParamsServer {
    key_expr: String,
    _session: Option<Session>,
    _queryable: Queryable<()>,
}

impl ZenohParamsServer {
    /// 在指定 zenoh session 上注册参数 queryable。
    ///
    /// `key_expr` 应使用 `params_key_expr()` 生成，包含 package、selfdesc hash 和 PID。
    pub fn open(
        session: &Session,
        key_expr: &str,
        handshake: IntrospectionHandshake,
        state: IntrospectionState,
    ) -> Result<Self, ParamsRemoteError> {
        let key_expr_owned = key_expr.to_string();
        let shared_state = Arc::new(state);
        let shared_handshake = Arc::new(handshake);

        let queryable = session
            .declare_queryable(key_expr_owned.clone())
            .callback(move |query: Query| {
                handle_params_query(query, &shared_handshake, &shared_state);
            })
            .wait()
            .map_err(|error| {
                ParamsRemoteError::transport("declare zenoh params queryable", error)
            })?;

        Ok(Self {
            key_expr: key_expr_owned,
            _session: None,
            _queryable: queryable,
        })
    }

    /// 使用 `FLOWRT_ZENOH_*` 环境变量打开独立 zenoh session 并注册参数 queryable。
    ///
    /// 生成的 runtime shell 使用该入口暴露远程参数控制面，不需要直接依赖 zenoh crate。
    pub fn open_from_environment(
        key_expr: &str,
        handshake: IntrospectionHandshake,
        state: IntrospectionState,
    ) -> Result<Self, ParamsRemoteError> {
        let session =
            zenoh::open(crate::zenoh::config_from_environment().map_err(|error| {
                ParamsRemoteError::new("configure zenoh params session", error)
            })?)
            .wait()
            .map_err(|error| ParamsRemoteError::transport("open zenoh params session", error))?;
        let key_expr_owned = key_expr.to_string();
        let shared_state = Arc::new(state);
        let shared_handshake = Arc::new(handshake);

        let queryable = session
            .declare_queryable(key_expr_owned.clone())
            .callback(move |query: Query| {
                handle_params_query(query, &shared_handshake, &shared_state);
            })
            .wait()
            .map_err(|error| {
                ParamsRemoteError::transport("declare zenoh params queryable", error)
            })?;

        Ok(Self {
            key_expr: key_expr_owned,
            _session: Some(session),
            _queryable: queryable,
        })
    }

    /// 返回 queryable 的 key expression。
    pub fn key_expr(&self) -> &str {
        &self.key_expr
    }
}

/// 远程 Operation queryable 服务端。
///
/// 注册到 zenoh session 后，响应远程 Operation status/cancel 请求，复用本机
/// `IntrospectionState` 的 invocation 查找、handler registry 和 cached state fallback。
pub struct ZenohOperationServer {
    key_expr: String,
    _session: Option<Session>,
    _queryable: Queryable<()>,
}

impl ZenohOperationServer {
    /// 在指定 zenoh session 上注册 Operation queryable。
    ///
    /// `key_expr` 应使用 `operation_key_expr()` 生成，包含 package、selfdesc hash 和 PID。
    pub fn open(
        session: &Session,
        key_expr: &str,
        handshake: IntrospectionHandshake,
        state: IntrospectionState,
    ) -> Result<Self, ParamsRemoteError> {
        let key_expr_owned = key_expr.to_string();
        let shared_state = Arc::new(state);
        let shared_handshake = Arc::new(handshake);

        let queryable = session
            .declare_queryable(key_expr_owned.clone())
            .callback(move |query: Query| {
                handle_operation_query(query, &shared_handshake, &shared_state);
            })
            .wait()
            .map_err(|error| {
                ParamsRemoteError::transport("declare zenoh operation queryable", error)
            })?;

        Ok(Self {
            key_expr: key_expr_owned,
            _session: None,
            _queryable: queryable,
        })
    }

    /// 使用 `FLOWRT_ZENOH_*` 环境变量打开独立 zenoh session 并注册 Operation queryable。
    pub fn open_from_environment(
        key_expr: &str,
        handshake: IntrospectionHandshake,
        state: IntrospectionState,
    ) -> Result<Self, ParamsRemoteError> {
        let session =
            zenoh::open(crate::zenoh::config_from_environment().map_err(|error| {
                ParamsRemoteError::new("configure zenoh operation session", error)
            })?)
            .wait()
            .map_err(|error| ParamsRemoteError::transport("open zenoh operation session", error))?;
        let key_expr_owned = key_expr.to_string();
        let shared_state = Arc::new(state);
        let shared_handshake = Arc::new(handshake);

        let queryable = session
            .declare_queryable(key_expr_owned.clone())
            .callback(move |query: Query| {
                handle_operation_query(query, &shared_handshake, &shared_state);
            })
            .wait()
            .map_err(|error| {
                ParamsRemoteError::transport("declare zenoh operation queryable", error)
            })?;

        Ok(Self {
            key_expr: key_expr_owned,
            _session: Some(session),
            _queryable: queryable,
        })
    }

    /// 返回 queryable 的 key expression。
    pub fn key_expr(&self) -> &str {
        &self.key_expr
    }
}

fn handle_params_query(
    query: Query,
    handshake: &IntrospectionHandshake,
    state: &IntrospectionState,
) {
    let reply_ke = query.key_expr().clone();

    let Some(payload) = query.payload() else {
        let response = IntrospectionResponse::Error {
            handshake: handshake.clone(),
            message: "empty zenoh params request payload".to_string(),
        };
        reply_json(&reply_ke, &response, &query);
        return;
    };

    let raw = payload.to_bytes().to_vec();
    let request: IntrospectionRequest = match serde_json::from_slice(&raw) {
        Ok(r) => r,
        Err(error) => {
            let response = IntrospectionResponse::Error {
                handshake: handshake.clone(),
                message: format!("invalid zenoh params request JSON: {error}"),
            };
            reply_json(&reply_ke, &response, &query);
            return;
        }
    };

    let response = match request {
        IntrospectionRequest::ParamList => IntrospectionResponse::ParamList {
            handshake: handshake.clone(),
            params: state.params(),
        },
        IntrospectionRequest::ParamGet { name } => match state.param(&name) {
            Some(param) => IntrospectionResponse::ParamValue {
                handshake: handshake.clone(),
                param,
            },
            None => IntrospectionResponse::Error {
                handshake: handshake.clone(),
                message: format!("unknown FlowRT parameter `{name}`"),
            },
        },
        IntrospectionRequest::ParamSet { name, value } => {
            match state.set_param_pending(&name, value) {
                Ok(param) => IntrospectionResponse::ParamValue {
                    handshake: handshake.clone(),
                    param,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            }
        }
        _ => IntrospectionResponse::Error {
            handshake: handshake.clone(),
            message: "unsupported zenoh params command; expected list/get/set".to_string(),
        },
    };

    reply_json(&reply_ke, &response, &query);
}

fn handle_operation_query(
    query: Query,
    handshake: &IntrospectionHandshake,
    state: &IntrospectionState,
) {
    let reply_ke = query.key_expr().clone();

    let Some(payload) = query.payload() else {
        let response = IntrospectionResponse::Error {
            handshake: handshake.clone(),
            message: "empty zenoh operation request payload".to_string(),
        };
        reply_json(&reply_ke, &response, &query);
        return;
    };

    let raw = payload.to_bytes().to_vec();
    let request: IntrospectionRequest = match serde_json::from_slice(&raw) {
        Ok(request) => request,
        Err(error) => {
            let response = IntrospectionResponse::Error {
                handshake: handshake.clone(),
                message: format!("invalid zenoh operation request JSON: {error}"),
            };
            reply_json(&reply_ke, &response, &query);
            return;
        }
    };

    let response = match request {
        IntrospectionRequest::Status => IntrospectionResponse::Status {
            handshake: handshake.clone(),
            status: state.status(),
        },
        IntrospectionRequest::OperationStatus { operation_id } => {
            match state.status_operation(&operation_id) {
                Ok(operation) => IntrospectionResponse::OperationValue {
                    handshake: handshake.clone(),
                    operation,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            }
        }
        IntrospectionRequest::OperationCancel { operation_id } => {
            match state.cancel_operation(&operation_id) {
                Ok(operation) => IntrospectionResponse::OperationValue {
                    handshake: handshake.clone(),
                    operation,
                },
                Err(message) => IntrospectionResponse::Error {
                    handshake: handshake.clone(),
                    message,
                },
            }
        }
        _ => IntrospectionResponse::Error {
            handshake: handshake.clone(),
            message: "unsupported zenoh operation command; expected status/cancel".to_string(),
        },
    };

    reply_json(&reply_ke, &response, &query);
}

fn reply_json(
    key_expr: &zenoh::key_expr::KeyExpr<'_>,
    response: &IntrospectionResponse,
    query: &Query,
) {
    match serde_json::to_vec(response) {
        Ok(bytes) => {
            let _ = query
                .reply(key_expr.clone(), zenoh::bytes::ZBytes::from(bytes))
                .wait();
        }
        Err(error) => {
            let fallback = IntrospectionResponse::Error {
                handshake: crate::introspection::IntrospectionHandshake {
                    protocol_version: crate::introspection::INTROSPECTION_PROTOCOL_VERSION
                        .to_string(),
                    pid: std::process::id(),
                    started_at_unix_ms: 0,
                    self_description_hash: String::new(),
                    package: String::new(),
                    process: String::new(),
                    runtime: String::new(),
                },
                message: format!("serialize params response: {error}"),
            };
            if let Ok(fallback_bytes) = serde_json::to_vec(&fallback) {
                let _ = query
                    .reply(key_expr.clone(), zenoh::bytes::ZBytes::from(fallback_bytes))
                    .wait();
            }
        }
    }
}

/// 生成确定性参数 key expression，包含 package、selfdesc hash 和 PID。
///
/// 格式：`flowrt/params/{package}/{selfdesc_hash}/{pid}`
/// 避免同机多进程或不同应用的参数 queryable 冲突。
pub fn params_key_expr(package: &str, selfdesc_hash: &str, pid: u32) -> String {
    format!("flowrt/params/{package}/{selfdesc_hash}/{pid}")
}

/// 生成确定性 Operation key expression，包含 package、selfdesc hash 和 PID。
///
/// 格式：`flowrt/op/{package}/{selfdesc_hash}/{pid}`
/// 避免同机多进程或不同应用的 Operation queryable 冲突。
pub fn operation_key_expr(package: &str, selfdesc_hash: &str, pid: u32) -> String {
    format!("flowrt/op/{package}/{selfdesc_hash}/{pid}")
}

/// 向远程 runtime 请求参数列表。
pub fn request_remote_param_list(
    session: &Session,
    key_expr: &str,
    timeout_ms: u64,
) -> Result<IntrospectionResponse, ParamsRemoteError> {
    let request = IntrospectionRequest::ParamList;
    send_params_query(session, key_expr, &request, timeout_ms)
}

/// 向远程 runtime 请求单个参数状态。
pub fn request_remote_param_get(
    session: &Session,
    key_expr: &str,
    name: &str,
    timeout_ms: u64,
) -> Result<IntrospectionResponse, ParamsRemoteError> {
    let request = IntrospectionRequest::ParamGet {
        name: name.to_string(),
    };
    send_params_query(session, key_expr, &request, timeout_ms)
}

/// 向远程 runtime 写入参数 pending 值。
pub fn request_remote_param_set(
    session: &Session,
    key_expr: &str,
    name: &str,
    value: serde_json::Value,
    timeout_ms: u64,
) -> Result<IntrospectionResponse, ParamsRemoteError> {
    let request = IntrospectionRequest::ParamSet {
        name: name.to_string(),
        value,
    };
    send_params_query(session, key_expr, &request, timeout_ms)
}

/// 向远程 runtime 请求 Operation 总览状态。
pub fn request_remote_operation_overview(
    session: &Session,
    key_expr: &str,
    timeout_ms: u64,
) -> Result<IntrospectionResponse, ParamsRemoteError> {
    let request = IntrospectionRequest::Status;
    send_operation_query(session, key_expr, &request, timeout_ms)
}

/// 向远程 runtime 请求单个 Operation invocation 状态。
pub fn request_remote_operation_status(
    session: &Session,
    key_expr: &str,
    operation_id: &str,
    timeout_ms: u64,
) -> Result<IntrospectionResponse, ParamsRemoteError> {
    let request = IntrospectionRequest::OperationStatus {
        operation_id: operation_id.to_string(),
    };
    send_operation_query(session, key_expr, &request, timeout_ms)
}

/// 向远程 runtime 请求取消单个 Operation invocation。
pub fn request_remote_operation_cancel(
    session: &Session,
    key_expr: &str,
    operation_id: &str,
    timeout_ms: u64,
) -> Result<IntrospectionResponse, ParamsRemoteError> {
    let request = IntrospectionRequest::OperationCancel {
        operation_id: operation_id.to_string(),
    };
    send_operation_query(session, key_expr, &request, timeout_ms)
}

fn send_params_query(
    session: &Session,
    key_expr: &str,
    request: &IntrospectionRequest,
    timeout_ms: u64,
) -> Result<IntrospectionResponse, ParamsRemoteError> {
    let payload = serde_json::to_vec(request)
        .map_err(|error| ParamsRemoteError::new("encode zenoh params request", error))?;

    let timeout = std::time::Duration::from_millis(timeout_ms);

    let receiver = session
        .get(key_expr)
        .with(zenoh::handlers::FifoChannel::new(1))
        .payload(zenoh::bytes::ZBytes::from(payload))
        .timeout(timeout)
        .wait()
        .map_err(|error| ParamsRemoteError::transport("send zenoh params query", error))?;

    let reply = match receiver.recv_timeout(timeout) {
        Ok(Some(reply)) => reply,
        Ok(None) => {
            return Err(ParamsRemoteError::new(
                "zenoh params query",
                "no reply received from remote runtime",
            ));
        }
        Err(_) => {
            return Err(ParamsRemoteError::new(
                "zenoh params query",
                "timeout waiting for remote runtime reply",
            ));
        }
    };

    let sample = reply
        .result()
        .map_err(|error| ParamsRemoteError::transport("zenoh params reply", error))?;

    let raw = sample.payload().to_bytes().to_vec();
    serde_json::from_slice(&raw)
        .map_err(|error| ParamsRemoteError::new("decode zenoh params response", error))
}

fn send_operation_query(
    session: &Session,
    key_expr: &str,
    request: &IntrospectionRequest,
    timeout_ms: u64,
) -> Result<IntrospectionResponse, ParamsRemoteError> {
    let payload = serde_json::to_vec(request)
        .map_err(|error| ParamsRemoteError::new("encode zenoh operation request", error))?;

    let timeout = std::time::Duration::from_millis(timeout_ms);

    let receiver = session
        .get(key_expr)
        .with(zenoh::handlers::FifoChannel::new(1))
        .payload(zenoh::bytes::ZBytes::from(payload))
        .timeout(timeout)
        .wait()
        .map_err(|error| ParamsRemoteError::transport("send zenoh operation query", error))?;

    let reply = match receiver.recv_timeout(timeout) {
        Ok(Some(reply)) => reply,
        Ok(None) => {
            return Err(ParamsRemoteError::new(
                "zenoh operation query",
                "no reply received from remote runtime",
            ));
        }
        Err(_) => {
            return Err(ParamsRemoteError::new(
                "zenoh operation query",
                "timeout waiting for remote runtime reply",
            ));
        }
    };

    let sample = reply
        .result()
        .map_err(|error| ParamsRemoteError::transport("zenoh operation reply", error))?;

    let raw = sample.payload().to_bytes().to_vec();
    serde_json::from_slice(&raw)
        .map_err(|error| ParamsRemoteError::new("decode zenoh operation response", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::introspection::IntrospectionParamSchema;

    fn test_key_expr(suffix: &str) -> String {
        format!(
            "flowrt/tests/params/{}/{}/{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            suffix
        )
    }

    fn make_test_handshake() -> IntrospectionHandshake {
        IntrospectionHandshake {
            protocol_version: crate::introspection::INTROSPECTION_PROTOCOL_VERSION.to_string(),
            pid: std::process::id(),
            started_at_unix_ms: 1000,
            self_description_hash: "test_hash".to_string(),
            package: "test_robot".to_string(),
            process: "main".to_string(),
            runtime: "rust".to_string(),
        }
    }

    fn make_test_state() -> IntrospectionState {
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
        state
    }

    fn test_session() -> (std::sync::MutexGuard<'static, ()>, zenoh::Session) {
        let guard = crate::zenoh_test_guard();
        let session = zenoh::open(crate::zenoh::config_from_environment().unwrap())
            .wait()
            .unwrap();
        (guard, session)
    }

    #[test]
    fn params_key_expr_contains_package_hash_and_pid() {
        let key = params_key_expr("robot_demo", "abc123", 42);
        assert_eq!(key, "flowrt/params/robot_demo/abc123/42");
    }

    #[test]
    fn remote_param_list_returns_registered_params() {
        let (_zenoh_guard, session) = test_session();
        let key_expr = test_key_expr("list");
        let handshake = make_test_handshake();
        let state = make_test_state();

        let _server =
            ZenohParamsServer::open(&session, &key_expr, handshake.clone(), state).unwrap();

        // 等待 queryable 注册传播
        std::thread::sleep(std::time::Duration::from_millis(100));

        let response =
            request_remote_param_list(&session, &key_expr, 5000).expect("remote param list");
        let IntrospectionResponse::ParamList {
            handshake: resp_hs,
            params,
        } = response
        else {
            panic!("expected ParamList response");
        };
        assert_eq!(resp_hs, handshake);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "controller.kp");
        assert_eq!(params[1].name, "controller.mode");
    }

    #[test]
    fn remote_param_get_returns_param_value() {
        let (_zenoh_guard, session) = test_session();
        let key_expr = test_key_expr("get");
        let handshake = make_test_handshake();
        let state = make_test_state();

        let _server = ZenohParamsServer::open(&session, &key_expr, handshake, state).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));

        let response = request_remote_param_get(&session, &key_expr, "controller.kp", 5000)
            .expect("remote param get");
        let IntrospectionResponse::ParamValue { param, .. } = response else {
            panic!("expected ParamValue response");
        };
        assert_eq!(param.name, "controller.kp");
        assert_eq!(param.current, serde_json::json!(1.0));
        assert!(param.pending.is_none());
    }

    #[test]
    fn remote_param_set_rejects_invalid_value() {
        let (_zenoh_guard, session) = test_session();
        let key_expr = test_key_expr("set-reject");
        let handshake = make_test_handshake();
        let state = make_test_state();

        let _server = ZenohParamsServer::open(&session, &key_expr, handshake, state).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));

        let response = request_remote_param_set(
            &session,
            &key_expr,
            "controller.kp",
            serde_json::json!(99.0),
            5000,
        )
        .expect("remote param set out of range");
        let IntrospectionResponse::Error { message, .. } = response else {
            panic!("expected Error response for out-of-range value");
        };
        assert_eq!(message, "FlowRT parameter `controller.kp` is above maximum");
    }

    #[test]
    fn remote_param_set_rejects_startup_only_param() {
        let (_zenoh_guard, session) = test_session();
        let key_expr = test_key_expr("set-startup");
        let handshake = make_test_handshake();
        let state = make_test_state();

        let _server = ZenohParamsServer::open(&session, &key_expr, handshake, state).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));

        let response = request_remote_param_set(
            &session,
            &key_expr,
            "controller.mode",
            serde_json::json!("safe"),
            5000,
        )
        .expect("remote param set startup-only");
        let IntrospectionResponse::Error { message, .. } = response else {
            panic!("expected Error response for startup-only param");
        };
        assert_eq!(
            message,
            "FlowRT parameter `controller.mode` is startup-only"
        );
    }

    #[test]
    fn remote_param_get_returns_error_for_unknown_param() {
        let (_zenoh_guard, session) = test_session();
        let key_expr = test_key_expr("get-unknown");
        let handshake = make_test_handshake();
        let state = make_test_state();

        let _server = ZenohParamsServer::open(&session, &key_expr, handshake, state).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));

        let response = request_remote_param_get(&session, &key_expr, "missing.param", 5000)
            .expect("remote param get unknown");
        let IntrospectionResponse::Error { message, .. } = response else {
            panic!("expected Error response for unknown param");
        };
        assert_eq!(message, "unknown FlowRT parameter `missing.param`");
    }

    #[test]
    fn remote_param_set_applies_pending_value() {
        let (_zenoh_guard, session) = test_session();
        let key_expr = test_key_expr("set-apply");
        let handshake = make_test_handshake();
        let state = make_test_state();

        let _server =
            ZenohParamsServer::open(&session, &key_expr, handshake, state.clone()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));

        let response = request_remote_param_set(
            &session,
            &key_expr,
            "controller.kp",
            serde_json::json!(5.0),
            5000,
        )
        .expect("remote param set");
        let IntrospectionResponse::ParamValue { param, .. } = response else {
            panic!("expected ParamValue response");
        };
        assert_eq!(param.pending, Some(serde_json::json!(5.0)));
        assert_eq!(
            state.pending_param("controller.kp"),
            Some(serde_json::json!(5.0))
        );
    }

    #[test]
    fn unsupported_command_returns_error() {
        let (_zenoh_guard, session) = test_session();
        let key_expr = test_key_expr("unsupported");
        let handshake = make_test_handshake();
        let state = make_test_state();

        let _server = ZenohParamsServer::open(&session, &key_expr, handshake, state).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 发送 Status 命令，应返回 unsupported 错误
        let payload = serde_json::to_vec(&IntrospectionRequest::Status).unwrap();
        let timeout = std::time::Duration::from_millis(5000);
        let receiver = session
            .get(&key_expr)
            .with(zenoh::handlers::FifoChannel::new(1))
            .payload(zenoh::bytes::ZBytes::from(payload))
            .timeout(timeout)
            .wait()
            .unwrap();
        let reply = receiver.recv_timeout(timeout).unwrap().unwrap();
        let sample = reply.result().unwrap();
        let raw = sample.payload().to_bytes().to_vec();
        let response: IntrospectionResponse = serde_json::from_slice(&raw).unwrap();
        let IntrospectionResponse::Error { message, .. } = response else {
            panic!("expected Error response for unsupported command");
        };
        assert!(message.contains("unsupported zenoh params command"));
    }

    #[test]
    fn empty_payload_returns_error() {
        let (_zenoh_guard, session) = test_session();
        let key_expr = test_key_expr("empty");
        let handshake = make_test_handshake();
        let state = make_test_state();

        let _server = ZenohParamsServer::open(&session, &key_expr, handshake, state).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));

        // 发送空 payload
        let timeout = std::time::Duration::from_millis(5000);
        let receiver = session
            .get(&key_expr)
            .with(zenoh::handlers::FifoChannel::new(1))
            .timeout(timeout)
            .wait()
            .unwrap();
        let reply = receiver.recv_timeout(timeout).unwrap().unwrap();
        let sample = reply.result().unwrap();
        let raw = sample.payload().to_bytes().to_vec();
        let response: IntrospectionResponse = serde_json::from_slice(&raw).unwrap();
        let IntrospectionResponse::Error { message, .. } = response else {
            panic!("expected Error response for empty payload");
        };
        assert!(message.contains("empty zenoh params request payload"));
    }

    #[test]
    fn remote_operation_status_and_cancel_use_introspection_state() {
        let (_zenoh_guard, session) = test_session();
        let key_expr = operation_key_expr("test_robot", "test_hash", std::process::id());
        let handshake = make_test_handshake();
        let state = IntrospectionState::new();
        state.record_operation_health(crate::introspection::IntrospectionOperationStatus {
            name: "controller.plan".to_string(),
            ready: true,
            running: 1,
            queued: 0,
            current_operation_ids: vec!["111:7:3".to_string()],
            total_started: 1,
            succeeded_count: 0,
            failed_count: 0,
            canceled_count: 0,
            timeout_count: 0,
            preempted_count: 0,
            current_state: Some("running".to_string()),
            current_owner: Some("controller.plan".to_string()),
            current_deadline_ms: Some(1500),
            last_event: Some("flowrt.operation.state_changed".to_string()),
            last_error: None,
            last_transition_ms: Some(12345),
        });

        let _server =
            ZenohOperationServer::open(&session, &key_expr, handshake.clone(), state).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));

        let status = request_remote_operation_status(&session, &key_expr, "111:7:3", 5000).unwrap();
        let IntrospectionResponse::OperationValue {
            handshake: status_handshake,
            operation,
        } = status
        else {
            panic!("expected OperationValue status response");
        };
        assert_eq!(status_handshake, handshake);
        assert_eq!(operation.name, "controller.plan");
        assert_eq!(operation.current_state.as_deref(), Some("running"));

        let canceled =
            request_remote_operation_cancel(&session, &key_expr, "111:7:3", 5000).unwrap();
        let IntrospectionResponse::OperationValue { operation, .. } = canceled else {
            panic!("expected OperationValue cancel response");
        };
        assert_eq!(operation.current_state.as_deref(), Some("cancel_requested"));
    }
}
