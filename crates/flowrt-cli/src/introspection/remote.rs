use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use flowrt_selfdesc::SelfDescription;
use zenoh::Wait;

use super::*;

// ── 远程参数控制面（zenoh） ──────────────────────────────────────────────

/// zenoh 远程 runtime 发现结果。
#[derive(Debug)]
pub(crate) struct RemoteRuntimeEntry {
    pub(crate) key_expr: String,
    pub(crate) pid: u32,
    pub(crate) package: String,
    pub(crate) process: String,
    pub(crate) runtime: String,
    pub(crate) self_description_hash: String,
}

impl std::fmt::Display for RemoteRuntimeEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "pid={} package={} process={} runtime={} selfdesc={} key={}",
            self.pid,
            self.package,
            self.process,
            self.runtime,
            self.self_description_hash,
            self.key_expr
        )
    }
}

/// 解析 `flowrt/params/{package}/{selfdesc_hash}/{pid}` 格式的 key expression。
pub(crate) fn parse_remote_params_key_expr(key: &str) -> Option<(&str, &str, &str)> {
    let rest = key.strip_prefix("flowrt/params/")?;
    let (package, rest) = rest.split_once('/')?;
    let (hash, pid) = rest.split_once('/')?;
    if package.is_empty() || hash.is_empty() || pid.is_empty() || pid.contains('/') {
        return None;
    }
    Some((package, hash, pid))
}

/// 解析 `flowrt/op/{package}/{selfdesc_hash}/{pid}` 格式的 key expression。
pub(crate) fn parse_remote_operation_key_expr(key: &str) -> Option<(&str, &str, &str)> {
    let rest = key.strip_prefix("flowrt/op/")?;
    let (package, rest) = rest.split_once('/')?;
    let (hash, pid) = rest.split_once('/')?;
    if package.is_empty() || hash.is_empty() || pid.is_empty() || pid.contains('/') {
        return None;
    }
    Some((package, hash, pid))
}

/// 打开用于远程参数控制面的 zenoh session。
fn open_zenoh_params_session() -> Result<zenoh::Session> {
    let zenoh_config = flowrt::zenoh::config_from_environment().map_err(|error| {
        anyhow::anyhow!("failed to configure zenoh session for params discovery: {error}")
    })?;
    zenoh::open(zenoh_config).wait().map_err(|error| {
        anyhow::anyhow!("failed to open zenoh session for params discovery: {error:?}")
    })
}

/// 打开用于远程 Operation 控制面的 zenoh session。
pub(crate) fn open_zenoh_operation_session() -> Result<zenoh::Session> {
    let zenoh_config = flowrt::zenoh::config_from_environment().map_err(|error| {
        anyhow::anyhow!("failed to configure zenoh session for operation discovery: {error}")
    })?;
    zenoh::open(zenoh_config).wait().map_err(|error| {
        anyhow::anyhow!("failed to open zenoh session for operation discovery: {error:?}")
    })
}

/// 通过 zenoh 扫描所有远程 params 端点，返回匹配 `self_description_hash` 的 runtime。
///
/// 复用调用方提供的 session，避免每次 discovery 重复创建 zenoh 连接。
pub(crate) fn discover_remote_params_runtimes(
    session: &zenoh::Session,
    self_description_hash: &str,
    timeout_ms: u64,
) -> Result<Vec<RemoteRuntimeEntry>> {
    let request = flowrt::IntrospectionRequest::ParamList;
    let payload = serde_json::to_vec(&request)
        .map_err(|error| anyhow::anyhow!("failed to encode params discovery request: {error}"))?;
    let timeout = Duration::from_millis(timeout_ms);

    let receiver = session
        .get("flowrt/params/**")
        .with(zenoh::handlers::FifoChannel::new(64))
        .payload(zenoh::bytes::ZBytes::from(payload))
        .timeout(timeout)
        .wait()
        .map_err(|error| {
            anyhow::anyhow!("failed to send zenoh params discovery query: {error:?}")
        })?;

    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();

    while let Ok(Some(reply)) = receiver.recv_timeout(timeout) {
        let Ok(sample) = reply.result() else {
            continue;
        };
        let key = sample.key_expr().to_string();
        let Some((package, hash, pid_str)) = parse_remote_params_key_expr(&key) else {
            continue;
        };
        if hash != self_description_hash {
            continue;
        }
        if !seen.insert(key.clone()) {
            continue;
        }
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        // 克隆借用的字段，避免 move key 后 use-after-move。
        let entry_hash = hash.to_string();
        let entry_package_hint = package.to_string();
        let raw = sample.payload().to_bytes().to_vec();
        let Ok(response) = serde_json::from_slice::<flowrt::IntrospectionResponse>(&raw) else {
            continue;
        };
        let handshake = match &response {
            flowrt::IntrospectionResponse::ParamList { handshake, .. } => handshake,
            flowrt::IntrospectionResponse::Error { handshake, .. } => handshake,
            _ => continue,
        };
        let entry_package = if entry_package_hint.is_empty() {
            handshake.package.clone()
        } else {
            entry_package_hint
        };
        entries.push(RemoteRuntimeEntry {
            key_expr: key,
            pid,
            package: entry_package,
            process: handshake.process.clone(),
            runtime: handshake.runtime.clone(),
            self_description_hash: entry_hash,
        });
    }

    Ok(entries)
}

/// 通过 zenoh 扫描所有远程 Operation 端点，返回匹配 `self_description_hash` 的 runtime。
pub(crate) fn discover_remote_operation_runtimes(
    session: &zenoh::Session,
    self_description_hash: &str,
    timeout_ms: u64,
) -> Result<Vec<RemoteRuntimeEntry>> {
    let request = flowrt::IntrospectionRequest::Status;
    let payload = serde_json::to_vec(&request).map_err(|error| {
        anyhow::anyhow!("failed to encode operation discovery request: {error}")
    })?;
    let timeout = Duration::from_millis(timeout_ms);

    let receiver = session
        .get("flowrt/op/**")
        .with(zenoh::handlers::FifoChannel::new(64))
        .payload(zenoh::bytes::ZBytes::from(payload))
        .timeout(timeout)
        .wait()
        .map_err(|error| {
            anyhow::anyhow!("failed to send zenoh operation discovery query: {error:?}")
        })?;

    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();

    while let Ok(Some(reply)) = receiver.recv_timeout(timeout) {
        let Ok(sample) = reply.result() else {
            continue;
        };
        let key = sample.key_expr().to_string();
        let Some((package, hash, pid_str)) = parse_remote_operation_key_expr(&key) else {
            continue;
        };
        if hash != self_description_hash {
            continue;
        }
        if !seen.insert(key.clone()) {
            continue;
        }
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        let entry_hash = hash.to_string();
        let entry_package_hint = package.to_string();
        let raw = sample.payload().to_bytes().to_vec();
        let Ok(response) = serde_json::from_slice::<flowrt::IntrospectionResponse>(&raw) else {
            continue;
        };
        let handshake = match &response {
            flowrt::IntrospectionResponse::Status { handshake, .. }
            | flowrt::IntrospectionResponse::Error { handshake, .. } => handshake,
            _ => continue,
        };
        let entry_package = if entry_package_hint.is_empty() {
            handshake.package.clone()
        } else {
            entry_package_hint
        };
        entries.push(RemoteRuntimeEntry {
            key_expr: key,
            pid,
            package: entry_package,
            process: handshake.process.clone(),
            runtime: handshake.runtime.clone(),
            self_description_hash: entry_hash,
        });
    }

    Ok(entries)
}

/// 从远程 runtime 列表中选择唯一匹配项；多个匹配时要求用户显式选择。
pub(crate) fn select_remote_runtime(
    entries: Vec<RemoteRuntimeEntry>,
    self_description_hash: &str,
) -> Result<RemoteRuntimeEntry> {
    match entries.len() {
        0 => anyhow::bail!(
            "no remote FlowRT runtime matches self-description hash `{self_description_hash}`; \
             check that the runtime is running and the zenoh network is reachable"
        ),
        1 => Ok(entries.into_iter().next().expect("non-empty")),
        _ => {
            let listing = entries
                .iter()
                .enumerate()
                .map(|(i, entry)| format!("  [{}] {}", i + 1, entry))
                .collect::<Vec<_>>()
                .join("\n");
            anyhow::bail!(
                "multiple remote FlowRT runtimes match self-description hash \
                 `{self_description_hash}`; pass `--runtime <key_expr>` to choose one:\n{listing}"
            )
        }
    }
}

/// 请求远程 runtime 参数列表。
pub(crate) fn remote_params_list(
    self_description_hash: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_params_session()?;
    let runtime = select_remote_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_param_list(&session, &runtime.key_expr, timeout_ms)
        .map_err(|error| {
            anyhow::anyhow!("failed to list remote params from `{runtime}`: {error}")
        })?;
    let params = match response {
        flowrt::IntrospectionResponse::ParamList { handshake, params } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            params
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("failed to list remote params from `{runtime}`: {message}");
        }
        _ => {
            anyhow::bail!("remote runtime `{runtime}` returned unexpected response");
        }
    };
    if params.is_empty() {
        return Ok("no FlowRT parameters".to_string());
    }
    Ok(params
        .iter()
        .map(format_param_status)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// 请求远程 runtime 单个参数状态。
pub(crate) fn remote_params_get(
    self_description_hash: &str,
    name: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_params_session()?;
    let runtime = select_remote_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_param_get(&session, &runtime.key_expr, name, timeout_ms)
        .map_err(|error| {
            anyhow::anyhow!("failed to get remote param `{name}` from `{runtime}`: {error}")
        })?;
    match response {
        flowrt::IntrospectionResponse::ParamValue { handshake, param } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            Ok(format_param_status(&param))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("failed to get remote param `{name}` from `{runtime}`: {message}");
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

/// 请求远程 runtime 设置参数 pending 值。
pub(crate) fn remote_params_set(
    self_description_hash: &str,
    name: &str,
    raw_value: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_params_session()?;
    let runtime = select_remote_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    remote_params_set_with_target(
        &session,
        self_description_hash,
        &runtime,
        name,
        raw_value,
        timeout_ms,
    )
}

pub(crate) fn remote_params_set_from_file(
    self_description_hash: &str,
    file: &Path,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<ParamSetBatchResult> {
    let entries = load_param_set_file(file)?;
    let session = open_zenoh_params_session()?;
    let runtime = select_remote_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    params_set_batch(entries, |name, raw_value| {
        remote_params_set_with_target(
            &session,
            self_description_hash,
            &runtime,
            name,
            raw_value,
            timeout_ms,
        )
    })
}

/// 请求远程 runtime Operation invocation 状态。
pub(crate) fn remote_operation_status(
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    let runtime = select_remote_operation_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_status(
        &session,
        &runtime.key_expr,
        operation_id,
        timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!("failed to get remote operation `{operation_id}` from `{runtime}`: {error}")
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationValue {
            handshake,
            operation,
        } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            Ok(format!(
                "operation_id={} {}",
                operation_id,
                format_operation_status(&operation, None)
            ))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to get remote operation `{operation_id}` from `{runtime}`: {message}"
            );
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

pub(crate) fn remote_operation_status_json(
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    remote_operation_status_json_with_session(
        &session,
        self_description_hash,
        operation_id,
        runtime_key_expr,
        timeout_ms,
    )
}

pub(crate) fn remote_operation_status_json_with_session(
    session: &zenoh::Session,
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let runtime = select_remote_operation_runtime_for_request(
        session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_status(
        session,
        &runtime.key_expr,
        operation_id,
        timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!("failed to get remote operation `{operation_id}` from `{runtime}`: {error}")
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationValue {
            handshake,
            operation,
        } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            operation_json_value_with_target(
                "operation_value",
                Some(operation_id),
                &operation,
                None,
                Some(&runtime),
            )
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to get remote operation `{operation_id}` from `{runtime}`: {message}"
            );
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

/// 请求远程 runtime 取消 Operation invocation。
pub(crate) fn remote_operation_cancel(
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    let runtime = select_remote_operation_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_cancel(
        &session,
        &runtime.key_expr,
        operation_id,
        timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "failed to cancel remote operation `{operation_id}` via `{runtime}`: {error}"
        )
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationValue {
            handshake,
            operation,
        } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            Ok(format!(
                "operation_id={} {}",
                operation_id,
                format_operation_status(&operation, None)
            ))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to cancel remote operation `{operation_id}` via `{runtime}`: {message}"
            );
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

pub(crate) fn remote_operation_cancel_json(
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    remote_operation_cancel_json_with_session(
        &session,
        self_description_hash,
        operation_id,
        runtime_key_expr,
        timeout_ms,
    )
}

pub(crate) fn remote_operation_cancel_json_with_session(
    session: &zenoh::Session,
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let runtime = select_remote_operation_runtime_for_request(
        session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_cancel(
        session,
        &runtime.key_expr,
        operation_id,
        timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "failed to cancel remote operation `{operation_id}` via `{runtime}`: {error}"
        )
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationValue {
            handshake,
            operation,
        } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            operation_json_value_with_target(
                "operation_value",
                Some(operation_id),
                &operation,
                None,
                Some(&runtime),
            )
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to cancel remote operation `{operation_id}` via `{runtime}`: {message}"
            );
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

/// 请求远程 runtime Operation invocation result。
pub(crate) fn remote_operation_result(
    self_description: &SelfDescription,
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    let runtime = select_remote_operation_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_result(
        &session,
        &runtime.key_expr,
        operation_id,
        timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "failed to get remote operation result `{operation_id}` from `{runtime}`: {error}"
        )
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationResult { handshake, result } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            eprintln!("target: {runtime}");
            format_operation_result(self_description, &result)
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to get remote operation result `{operation_id}` from `{runtime}`: {message}"
            );
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

pub(crate) fn remote_operation_result_json(
    self_description: &SelfDescription,
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    remote_operation_result_json_with_session(
        &session,
        self_description,
        self_description_hash,
        operation_id,
        runtime_key_expr,
        timeout_ms,
    )
}

pub(crate) fn remote_operation_result_json_with_session(
    session: &zenoh::Session,
    self_description: &SelfDescription,
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let runtime = select_remote_operation_runtime_for_request(
        session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let response = flowrt::request_remote_operation_result(
        session,
        &runtime.key_expr,
        operation_id,
        timeout_ms,
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "failed to get remote operation result `{operation_id}` from `{runtime}`: {error}"
        )
    })?;
    match response {
        flowrt::IntrospectionResponse::OperationResult { handshake, result } => {
            ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
            operation_result_json_response(self_description, &result, Some(&runtime))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!(
                "failed to get remote operation result `{operation_id}` from `{runtime}`: {message}"
            );
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

/// 跟随远程 runtime Operation invocation events。
pub(crate) fn remote_operation_follow(
    self_description: &SelfDescription,
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    let runtime = select_remote_operation_runtime_for_request(
        &session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let mut cursor = 0;
    let mut lines = Vec::new();
    loop {
        let response = flowrt::request_remote_operation_observe(
            &session,
            &runtime.key_expr,
            operation_id,
            cursor,
            Some(64),
            timeout_ms,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to follow remote operation `{operation_id}` from `{runtime}`: {error}"
            )
        })?;
        match response {
            flowrt::IntrospectionResponse::OperationEvents {
                handshake,
                events,
                next_sequence,
                terminal,
                ..
            } => {
                ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
                let event_count = events.len();
                for event in &events {
                    lines.push(format_operation_event(self_description, event)?);
                }
                cursor = next_sequence;
                if terminal && event_count < 64 {
                    break;
                }
                if event_count == 0 {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            flowrt::IntrospectionResponse::Error { message, .. } => {
                anyhow::bail!(
                    "failed to follow remote operation `{operation_id}` from `{runtime}`: {message}"
                );
            }
            _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
        }
    }
    eprintln!("target: {runtime}");
    Ok(lines.join("\n"))
}

pub(crate) fn remote_operation_follow_json(
    self_description: &SelfDescription,
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let session = open_zenoh_operation_session()?;
    remote_operation_follow_json_with_session(
        &session,
        self_description,
        self_description_hash,
        operation_id,
        runtime_key_expr,
        timeout_ms,
    )
}

pub(crate) fn remote_operation_follow_json_with_session(
    session: &zenoh::Session,
    self_description: &SelfDescription,
    self_description_hash: &str,
    operation_id: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<String> {
    let runtime = select_remote_operation_runtime_for_request(
        session,
        self_description_hash,
        runtime_key_expr,
        timeout_ms,
    )?;
    let mut cursor = 0;
    let mut all_events = Vec::new();
    let terminal = loop {
        let response = flowrt::request_remote_operation_observe(
            session,
            &runtime.key_expr,
            operation_id,
            cursor,
            Some(64),
            timeout_ms,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "failed to follow remote operation `{operation_id}` from `{runtime}`: {error}"
            )
        })?;
        match response {
            flowrt::IntrospectionResponse::OperationEvents {
                handshake,
                events,
                next_sequence,
                terminal: response_terminal,
                ..
            } => {
                ensure_remote_handshake(&handshake, self_description_hash, &runtime)?;
                let event_count = events.len();
                all_events.extend(events);
                cursor = next_sequence;
                if response_terminal && event_count < 64 {
                    break response_terminal;
                }
                if event_count == 0 {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            flowrt::IntrospectionResponse::Error { message, .. } => {
                anyhow::bail!(
                    "failed to follow remote operation `{operation_id}` from `{runtime}`: {message}"
                );
            }
            _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
        }
    };
    operation_events_json_response(
        self_description,
        operation_id,
        &all_events,
        cursor,
        terminal,
        Some(&runtime),
    )
}

fn remote_params_set_with_target(
    session: &zenoh::Session,
    self_description_hash: &str,
    runtime: &RemoteRuntimeEntry,
    name: &str,
    raw_value: &str,
    timeout_ms: u64,
) -> Result<String> {
    let value = serde_json::from_str::<serde_json::Value>(raw_value).with_context(|| {
        format!("FlowRT parameter values must be valid JSON; got `{raw_value}`")
    })?;
    let response =
        flowrt::request_remote_param_set(session, &runtime.key_expr, name, value, timeout_ms)
            .map_err(|error| {
                anyhow::anyhow!("failed to set remote param `{name}` via `{runtime}`: {error}")
            })?;
    match response {
        flowrt::IntrospectionResponse::ParamValue { handshake, param } => {
            ensure_remote_handshake(&handshake, self_description_hash, runtime)?;
            eprintln!("target: {runtime}");
            Ok(format_param_status(&param))
        }
        flowrt::IntrospectionResponse::Error { message, .. } => {
            anyhow::bail!("failed to set remote param `{name}` via `{runtime}`: {message}");
        }
        _ => anyhow::bail!("remote runtime `{runtime}` returned unexpected response"),
    }
}

fn select_remote_runtime_for_request(
    session: &zenoh::Session,
    self_description_hash: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<RemoteRuntimeEntry> {
    if let Some(key_expr) = runtime_key_expr {
        return remote_runtime_entry_from_key_expr(
            session,
            key_expr,
            self_description_hash,
            timeout_ms,
        );
    }
    let entries = discover_remote_params_runtimes(session, self_description_hash, timeout_ms)?;
    select_remote_runtime(entries, self_description_hash)
}

pub(crate) fn select_remote_operation_runtime_for_request(
    session: &zenoh::Session,
    self_description_hash: &str,
    runtime_key_expr: Option<&str>,
    timeout_ms: u64,
) -> Result<RemoteRuntimeEntry> {
    if let Some(key_expr) = runtime_key_expr {
        return remote_operation_runtime_entry_from_key_expr(
            session,
            key_expr,
            self_description_hash,
            timeout_ms,
        );
    }
    let entries = discover_remote_operation_runtimes(session, self_description_hash, timeout_ms)?;
    select_remote_runtime(entries, self_description_hash)
}

fn remote_runtime_entry_from_key_expr(
    session: &zenoh::Session,
    key_expr: &str,
    self_description_hash: &str,
    timeout_ms: u64,
) -> Result<RemoteRuntimeEntry> {
    let Some((package, hash, pid_str)) = parse_remote_params_key_expr(key_expr) else {
        anyhow::bail!(
            "invalid remote FlowRT runtime key expression `{key_expr}`; expected `flowrt/params/<package>/<selfdesc_hash>/<pid>`"
        );
    };
    if hash != self_description_hash {
        anyhow::bail!(
            "remote FlowRT runtime key expression `{key_expr}` uses self-description hash `{hash}`, expected `{self_description_hash}`"
        );
    }
    let pid = pid_str.parse::<u32>().with_context(|| {
        format!(
            "remote FlowRT runtime key expression `{key_expr}` contains invalid pid `{pid_str}`"
        )
    })?;
    let response = flowrt::request_remote_param_list(session, key_expr, timeout_ms)
        .map_err(|error| anyhow::anyhow!("failed to query remote runtime `{key_expr}`: {error}"))?;
    let handshake = match response {
        flowrt::IntrospectionResponse::ParamList { handshake, .. }
        | flowrt::IntrospectionResponse::Error { handshake, .. } => handshake,
        _ => anyhow::bail!("remote runtime `{key_expr}` returned unexpected response"),
    };
    if handshake.self_description_hash != self_description_hash {
        anyhow::bail!(
            "remote runtime `{key_expr}` self-description hash `{}` does not match expected `{self_description_hash}`",
            handshake.self_description_hash
        );
    }
    Ok(RemoteRuntimeEntry {
        key_expr: key_expr.to_string(),
        pid,
        package: package.to_string(),
        process: handshake.process,
        runtime: handshake.runtime,
        self_description_hash: hash.to_string(),
    })
}

fn remote_operation_runtime_entry_from_key_expr(
    session: &zenoh::Session,
    key_expr: &str,
    self_description_hash: &str,
    timeout_ms: u64,
) -> Result<RemoteRuntimeEntry> {
    let Some((package, hash, pid_str)) = parse_remote_operation_key_expr(key_expr) else {
        anyhow::bail!(
            "invalid remote FlowRT runtime key expression `{key_expr}`; expected `flowrt/op/<package>/<selfdesc_hash>/<pid>`"
        );
    };
    if hash != self_description_hash {
        anyhow::bail!(
            "remote FlowRT runtime key expression `{key_expr}` uses self-description hash `{hash}`, expected `{self_description_hash}`"
        );
    }
    let pid = pid_str.parse::<u32>().with_context(|| {
        format!(
            "remote FlowRT runtime key expression `{key_expr}` contains invalid pid `{pid_str}`"
        )
    })?;
    let response = flowrt::request_remote_operation_overview(session, key_expr, timeout_ms)
        .map_err(|error| anyhow::anyhow!("failed to query remote runtime `{key_expr}`: {error}"))?;
    let handshake = match response {
        flowrt::IntrospectionResponse::Status { handshake, .. }
        | flowrt::IntrospectionResponse::Error { handshake, .. } => handshake,
        _ => anyhow::bail!("remote runtime `{key_expr}` returned unexpected response"),
    };
    if handshake.self_description_hash != self_description_hash {
        anyhow::bail!(
            "remote runtime `{key_expr}` self-description hash `{}` does not match expected `{self_description_hash}`",
            handshake.self_description_hash
        );
    }
    Ok(RemoteRuntimeEntry {
        key_expr: key_expr.to_string(),
        pid,
        package: package.to_string(),
        process: handshake.process,
        runtime: handshake.runtime,
        self_description_hash: hash.to_string(),
    })
}

pub(crate) fn ensure_remote_handshake(
    handshake: &flowrt::IntrospectionHandshake,
    expected_hash: &str,
    runtime: &RemoteRuntimeEntry,
) -> Result<()> {
    if handshake.self_description_hash == expected_hash {
        Ok(())
    } else {
        anyhow::bail!(
            "remote runtime `{runtime}` self-description hash `{}` does not match expected `{expected_hash}`",
            handshake.self_description_hash
        )
    }
}
