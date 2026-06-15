//! 进程 readiness gate 和 service ready 检查。

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use crate::introspection::IntrospectionState;
use crate::shutdown::ShutdownToken;

use super::launch_loop::{SupervisedChild, record_child_health};
use super::manifest::{LaunchGraph, LaunchProcess, ReadinessGate};

/// readiness 等待轮询间隔。
pub(super) const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// readiness 等待超时时间。
pub(super) const READINESS_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) struct ReadinessConfig {
    pub(super) timeout: Duration,
    pub(super) poll_interval: Duration,
}

impl Default for ReadinessConfig {
    fn default() -> Self {
        Self {
            timeout: READINESS_TIMEOUT,
            poll_interval: READINESS_POLL_INTERVAL,
        }
    }
}

/// 等待子进程通过 readiness gate。
///
/// - `ProcessStarted`：进程已启动即通过，无需额外等待。
/// - `RuntimeReady`：轮询 introspection socket 直到握手成功或超时。
/// - `ServiceReady`：轮询 introspection socket 直到握手成功且所有 service endpoint
///   就绪或超时。
///
/// 超时返回错误，同时终止子进程。
pub(super) fn wait_for_readiness(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    config: &ReadinessConfig,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    abort_child_if_shutdown_requested(supervisor_state, child, shutdown)?;
    match child.readiness {
        ReadinessGate::ProcessStarted => Ok(()),
        ReadinessGate::RuntimeReady => {
            child.state = "waiting_readiness".to_string();
            record_child_health(supervisor_state, child, false);
            wait_for_runtime_ready(supervisor_state, child, config, shutdown)
        }
        ReadinessGate::ServiceReady => {
            child.state = "waiting_readiness".to_string();
            record_child_health(supervisor_state, child, false);
            wait_for_service_ready(supervisor_state, child, config, shutdown)
        }
    }
}

fn abort_child_if_shutdown_requested(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    if !shutdown.is_requested() {
        return Ok(());
    }
    child.terminate("shutdown");
    record_child_health(supervisor_state, child, false);
    Err(format!(
        "FlowRT supervisor shutdown requested while waiting for process `{}`",
        child.name
    ))
}

/// 等待子进程 runtime introspection 握手成功。
pub(super) fn wait_for_runtime_ready(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    config: &ReadinessConfig,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + config.timeout;
    loop {
        abort_child_if_shutdown_requested(supervisor_state, child, shutdown)?;
        // 检查子进程是否已退出。
        if let Some(status) = child.child.try_wait().map_err(|error| {
            format!(
                "failed to poll FlowRT process `{}` during readiness wait: {error}",
                child.name
            )
        })? {
            child.exit_code = status.code();
            child.finished = true;
            child.state = "readiness_failed".to_string();
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` exited during readiness wait (code: {})",
                child.name,
                child
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ));
        }

        // 检查是否超时。
        if std::time::Instant::now() >= deadline {
            child.terminate("readiness_timeout");
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` readiness timed out waiting for runtime_ready",
                child.name
            ));
        }

        // 轮询 introspection socket；单次 socket 读写不能超过外层 readiness deadline。
        match crate::introspection::request_status_with_timeout(
            &child.socket,
            readiness_socket_timeout(deadline, config.poll_interval),
        ) {
            Ok(crate::IntrospectionResponse::Status { .. }) => return Ok(()),
            _ => sleep_or_abort_child(
                supervisor_state,
                child,
                config.poll_interval,
                shutdown,
                "readiness wait",
            )?,
        }
    }
}

/// 等待子进程 runtime introspection 握手成功且该进程预期承载的 service endpoint 就绪。
pub(super) fn wait_for_service_ready(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    config: &ReadinessConfig,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + config.timeout;
    loop {
        abort_child_if_shutdown_requested(supervisor_state, child, shutdown)?;
        // 检查子进程是否已退出。
        if let Some(status) = child.child.try_wait().map_err(|error| {
            format!(
                "failed to poll FlowRT process `{}` during readiness wait: {error}",
                child.name
            )
        })? {
            child.exit_code = status.code();
            child.finished = true;
            child.state = "readiness_failed".to_string();
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` exited during readiness wait (code: {})",
                child.name,
                child
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ));
        }

        // 检查是否超时。
        if std::time::Instant::now() >= deadline {
            child.terminate("readiness_timeout");
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` readiness timed out waiting for service_ready",
                child.name
            ));
        }

        // 轮询 introspection socket：需要握手成功且预期 service 全部就绪。
        match crate::introspection::request_status_with_timeout(
            &child.socket,
            readiness_socket_timeout(deadline, config.poll_interval),
        ) {
            Ok(crate::IntrospectionResponse::Status { status, .. }) => {
                if expected_services_ready(&child.expected_services, &status.services) {
                    return Ok(());
                }
                // 有 service 但未全部就绪，继续等待。
                sleep_or_abort_child(
                    supervisor_state,
                    child,
                    config.poll_interval,
                    shutdown,
                    "readiness wait",
                )?;
            }
            _ => sleep_or_abort_child(
                supervisor_state,
                child,
                config.poll_interval,
                shutdown,
                "readiness wait",
            )?,
        }
    }
}

fn readiness_socket_timeout(deadline: Instant, poll_interval: Duration) -> Duration {
    let remaining = deadline.saturating_duration_since(Instant::now());
    let timeout = remaining.min(poll_interval);
    if timeout.is_zero() {
        Duration::from_millis(1)
    } else {
        timeout
    }
}

fn sleep_or_abort_child(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    duration: Duration,
    shutdown: &ShutdownToken,
    wait_context: &str,
) -> Result<(), String> {
    if duration.is_zero() {
        return abort_child_if_shutdown_requested(supervisor_state, child, shutdown);
    }
    let deadline = Instant::now() + duration;
    loop {
        abort_child_if_shutdown_requested(supervisor_state, child, shutdown)?;
        if let Some(status) = child.child.try_wait().map_err(|error| {
            format!(
                "failed to poll FlowRT process `{}` while waiting: {error}",
                child.name
            )
        })? {
            child.exit_code = status.code();
            child.finished = true;
            child.state = "readiness_failed".to_string();
            record_child_health(supervisor_state, child, false);
            return Err(format!(
                "FlowRT process `{}` exited during {wait_context} (code: {})",
                child.name,
                child
                    .exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ));
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(());
        }
        std::thread::sleep((deadline - now).min(Duration::from_millis(10)));
    }
}

pub(super) fn wait_for_startup_delay(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    shutdown: &ShutdownToken,
) -> Result<(), String> {
    if child.startup_delay_ms == 0 {
        return Ok(());
    }
    child.state = "delaying".to_string();
    record_child_health(supervisor_state, child, false);
    sleep_or_abort_child(
        supervisor_state,
        child,
        Duration::from_millis(child.startup_delay_ms),
        shutdown,
        "startup delay",
    )
}

pub(super) fn expected_services_for_process(
    graph: &LaunchGraph,
    process: &LaunchProcess,
) -> Vec<String> {
    let instances = process
        .instances
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    graph
        .services
        .iter()
        .filter(|service| instances.contains(service.server_instance.as_str()))
        .map(|service| service.name.clone())
        .collect()
}

pub(super) fn expected_services_ready(
    expected_services: &[String],
    live_services: &[crate::IntrospectionServiceStatus],
) -> bool {
    if expected_services.is_empty() {
        return true;
    }
    expected_services.iter().all(|expected| {
        live_services
            .iter()
            .any(|service| service.name == *expected && service.ready)
    })
}

/// 返回 readiness gate 的可读标签。
pub(super) fn readiness_gate_label(gate: ReadinessGate) -> &'static str {
    match gate {
        ReadinessGate::ProcessStarted => "process_started",
        ReadinessGate::RuntimeReady => "runtime_ready",
        ReadinessGate::ServiceReady => "service_ready",
    }
}
