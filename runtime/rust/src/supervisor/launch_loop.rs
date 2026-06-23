//! Supervisor launch 主循环、子进程状态和健康记录。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use crate::introspection::{
    IntrospectionIdentity, IntrospectionProcessStatus, IntrospectionResourceStatus,
    IntrospectionState,
};
use crate::shutdown::ShutdownToken;

use super::command::{
    ExternalExecutableResolution, app_executable_for_runtime, build_external_process_command,
    build_process_command_with_status_out, external_app_executable, spawn_launch_process,
};
use super::dependency::process_dependencies_satisfied;
use super::manifest::{
    LaunchExternalProcess, LaunchProcess, ReadinessGate, RestartPolicy, effective_readiness,
    parse_launch_manifest,
};
use super::readiness::{
    ReadinessConfig, expected_services_for_process, readiness_gate_label, wait_for_readiness,
    wait_for_startup_delay,
};
use super::resource_placement::{self, ResourcePlacement, ResourcePlacementStatus};
use super::resources::{
    ProcessResourceGate, ResourceGateAction, ResourceGatePhase, apply_resource_placement_to_pid,
    apply_startup_resource_gate, evaluate_child_resource_gates, process_resource_gate,
    record_resource_gate_statuses, resource_status_from_contract,
};
use super::time::unix_time_ms;
use super::zenoh::{
    ZenohLaunchEnv, ZenohLaunchPlan, should_auto_configure_zenoh, zenoh_launch_env_for_graph,
};

/// 子进程健康轮询间隔。
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// tick 无变化超过此阈值则标记 stale。
const TICK_STALE_AFTER_MS: u64 = 1_000;
/// supervisor 主动终止子进程时等待其自行退出的宽限时间。
const CHILD_TERMINATE_GRACE: Duration = Duration::from_millis(500);
/// bounded run 达成后等待子进程自行走完 shutdown/status 写出的宽限时间。
const RUN_LIMIT_NATURAL_EXIT_GRACE: Duration = Duration::from_millis(1_500);
/// 主动终止子进程时的轮询间隔。
const CHILD_TERMINATE_POLL_INTERVAL: Duration = Duration::from_millis(20);

// SupervisorConfig
// ---------------------------------------------------------------------------

/// 生成物传入的 supervisor 配置。
pub struct SupervisorConfig {
    /// 嵌入的 launch manifest JSON。
    pub manifest_json: &'static str,
    /// Rust 应用二进制名 stem（不含平台后缀）。
    pub rust_app_stem: &'static str,
    /// C++ 应用二进制名 stem。
    pub cpp_app_stem: &'static str,
    /// ROS2 bridge 二进制名 stem。
    pub ros2_bridge_stem: &'static str,
    /// 包名。
    pub package_name: &'static str,
    /// self-description hash（由生成物的 selfdesc 模块提供）。
    pub self_description_hash: fn() -> &'static str,
}

struct LaunchStatusSnapshot {
    output: PathBuf,
    child_dir: PathBuf,
}

fn launch_status_snapshot_from_env() -> Result<Option<LaunchStatusSnapshot>, String> {
    let Some(output) = std::env::var_os("FLOWRT_STATUS_OUT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        return Ok(None);
    };
    let child_dir = status_child_dir(&output);
    let _ = std::fs::remove_dir_all(&child_dir);
    std::fs::create_dir_all(&child_dir).map_err(|error| {
        format!(
            "failed to create launch status snapshot dir `{}`: {error}",
            child_dir.display()
        )
    })?;
    Ok(Some(LaunchStatusSnapshot { output, child_dir }))
}

fn status_child_dir(output: &Path) -> PathBuf {
    let mut child_dir = output.as_os_str().to_os_string();
    child_dir.push(".children");
    PathBuf::from(child_dir)
}

fn process_writes_status_snapshot(process: &LaunchProcess) -> bool {
    matches!(process.runtime_kind.as_str(), "rust" | "cpp" | "c")
}

pub(crate) fn aggregate_status_snapshots(
    status_dir: &Path,
    processes: &[LaunchProcess],
) -> Result<serde_json::Value, String> {
    let mut entries = Vec::with_capacity(processes.len());
    for process in processes {
        let path = status_dir.join(format!("{}.status.json", process.name));
        let text = std::fs::read_to_string(&path).map_err(|error| {
            format!(
                "failed to read status snapshot `{}`: {error}",
                path.display()
            )
        })?;
        let status: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
            format!(
                "failed to parse status snapshot `{}`: {error}",
                path.display()
            )
        })?;
        entries.push(serde_json::json!({
            "process": process.name,
            "runtime": process.runtime_kind,
            "status": status,
        }));
    }
    Ok(serde_json::json!({
        "mode": "launch",
        "processes": entries,
    }))
}

#[cfg(test)]
pub(crate) fn aggregate_status_snapshots_for_test(
    status_dir: &Path,
    process_names: &[&str],
) -> Result<serde_json::Value, String> {
    let processes = process_names
        .iter()
        .map(|name| LaunchProcess {
            name: (*name).to_string(),
            backend: "inproc".to_string(),
            target: None,
            runtimes: Vec::new(),
            runtime_kind: "rust".to_string(),
            external: None,
            depends_on: Vec::new(),
            env: BTreeMap::new(),
            restart: super::manifest::DEFAULT_RESTART_POLICY,
            failure: "propagate".to_string(),
            readiness: ReadinessGate::ProcessStarted,
            startup_delay_ms: 0,
            instances: Vec::new(),
            tasks: Vec::new(),
            resource_placement: ResourcePlacement::default(),
            io_boundaries: Vec::new(),
        })
        .collect::<Vec<_>>();
    aggregate_status_snapshots(status_dir, &processes)
}

// ---------------------------------------------------------------------------
// SupervisedChild（运行时内部状态）
// ---------------------------------------------------------------------------

pub(super) struct SupervisedChild {
    pub(super) name: String,
    pub(super) backend: String,
    pub(super) runtime_kind: String,
    pub(super) external: Option<LaunchExternalProcess>,
    pub(super) external_resolution: Option<ExternalExecutableResolution>,
    pub(super) app_exe: PathBuf,
    pub(super) zenoh_env: Option<ZenohLaunchEnv>,
    pub(super) dependencies: Vec<String>,
    pub(super) env: BTreeMap<String, String>,
    pub(super) restart_policy: RestartPolicy,
    pub(super) failure: String,
    pub(super) readiness: ReadinessGate,
    pub(super) expected_services: Vec<String>,
    pub(super) startup_delay_ms: u64,
    pub(super) resource_gate: ProcessResourceGate,
    pub(super) resource_degraded: bool,
    pub(super) resource_wait: Option<String>,
    pub(super) resource_placement: ResourcePlacement,
    pub(super) resource_placement_status: ResourcePlacementStatus,
    pub(super) child: Child,
    pub(super) socket: PathBuf,
    pub(super) finished: bool,
    pub(super) restart_count: u32,
    pub(super) next_restart_unix_ms: Option<u64>,
    pub(super) last_seen_unix_ms: Option<u64>,
    pub(super) last_tick_count: Option<u64>,
    pub(super) last_tick_changed_unix_ms: u64,
    pub(super) state: String,
    pub(super) exit_code: Option<i32>,
}

impl SupervisedChild {
    pub(super) fn terminate(&mut self, state: &'static str) {
        if self.finished {
            return;
        }
        self.request_termination();
        let deadline = Instant::now() + CHILD_TERMINATE_GRACE;
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.exit_code = status.code();
                    self.finished = true;
                    self.state = state.to_string();
                    return;
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(CHILD_TERMINATE_POLL_INTERVAL);
                }
                Ok(None) => {
                    self.force_kill();
                    let status = self.child.wait().ok();
                    self.exit_code = status.and_then(|status| status.code());
                    self.finished = true;
                    self.state = state.to_string();
                    return;
                }
                Err(_) => {
                    self.finished = true;
                    self.state = state.to_string();
                    self.exit_code = None;
                    return;
                }
            }
        }
    }

    fn request_termination(&mut self) {
        #[cfg(unix)]
        {
            let pid = self.child.id();
            if pid <= i32::MAX as u32 {
                let pgid = -(pid as i32);
                // 子进程启动时已经 setpgid(0, 0)。若遇到旧进程或 setpgid 失败，
                // 后面的 Child::kill 回退仍会终止直接子进程。
                let signaled = unsafe { libc::kill(pgid, libc::SIGTERM) == 0 };
                if signaled {
                    return;
                }
            }
        }
        let _ = self.child.kill();
    }

    fn force_kill(&mut self) {
        #[cfg(unix)]
        {
            let pid = self.child.id();
            if pid <= i32::MAX as u32 {
                let pgid = -(pid as i32);
                let _ = unsafe { libc::kill(pgid, libc::SIGKILL) };
            }
        }
        let _ = self.child.kill();
    }
}

impl Drop for SupervisedChild {
    fn drop(&mut self) {
        self.terminate("terminated");
    }
}

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// Supervisor 主入口。
///
/// 解析 manifest，按依赖顺序启动进程，执行监控循环。
pub fn launch(config: &SupervisorConfig, run_ticks: Option<usize>) -> Result<(), String> {
    let manifest = parse_launch_manifest(config.manifest_json)?;
    if manifest.graphs.is_empty() {
        return Err("FlowRT launch manifest does not contain a graph".to_string());
    }

    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;
    let shutdown = crate::install_signal_shutdown_token();
    let status_snapshot = launch_status_snapshot_from_env()?;

    let supervisor_state = IntrospectionState::new();
    let _supervisor_status_server = crate::spawn_status_server(
        IntrospectionIdentity {
            self_description_hash: (config.self_description_hash)().to_string(),
            package: config.package_name.to_string(),
            process: "flowrt_supervisor".to_string(),
            runtime: "supervisor".to_string(),
        },
        supervisor_state.clone(),
    )
    .ok();

    let mut zenoh_launch_plans = Vec::new();
    let mut children = Vec::new();
    let mut status_processes = Vec::new();
    for graph in &manifest.graphs {
        let zenoh_plan = if should_auto_configure_zenoh() {
            let refs: Vec<&LaunchProcess> = graph.processes.iter().collect();
            zenoh_launch_env_for_graph(&refs)?
        } else {
            ZenohLaunchPlan::empty()
        };
        let mut pending = graph.processes.iter().collect::<Vec<_>>();
        // ready_names 包含已通过 readiness gate 的进程名。
        // spawned_names 包含已启动（PID 存在）的进程名，用于依赖排序。
        let mut spawned_names = BTreeSet::new();
        let mut ready_names = BTreeSet::new();
        while !pending.is_empty() {
            let Some(index) = pending.iter().position(|process| {
                process_dependencies_satisfied(process, &spawned_names, &ready_names)
            }) else {
                return Err(
                    "FlowRT process dependencies contain a cycle or unknown process".to_string(),
                );
            };
            let process = pending.remove(index);
            let expected_services = expected_services_for_process(graph, process);
            let resource_gate_decision =
                apply_startup_resource_gate(&supervisor_state, graph, process)?;
            let external_resolution = if process.runtime_kind == "external" {
                let external = process.external.as_ref().ok_or_else(|| {
                    format!(
                        "external process `{}` is missing launch manifest external metadata",
                        process.name
                    )
                })?;
                Some(
                    external_app_executable(&current_exe, external).map_err(|error| {
                        format!(
                            "failed to resolve external process `{}`: {error}",
                            process.name
                        )
                    })?,
                )
            } else {
                None
            };
            let app_exe = if let Some(resolution) = &external_resolution {
                resolution.executable.clone()
            } else {
                app_executable_for_runtime(
                    &current_exe,
                    &process.runtime_kind,
                    config.rust_app_stem,
                    config.cpp_app_stem,
                    config.ros2_bridge_stem,
                )?
            };
            let process_zenoh_env = zenoh_plan.env.get(&process.name).cloned();
            let child = spawn_launch_process(
                &app_exe,
                process,
                run_ticks,
                process_zenoh_env.as_ref(),
                external_resolution.as_ref(),
                status_snapshot.as_ref().and_then(|snapshot| {
                    process_writes_status_snapshot(process).then_some(snapshot.child_dir.as_path())
                }),
            )?;
            let resource_applied =
                apply_resource_placement_to_pid(&process.resource_placement, Some(child.id()));
            let socket = crate::runtime_socket_path_for_pid(child.id());
            let mut child = SupervisedChild {
                name: process.name.clone(),
                backend: process.backend.clone(),
                runtime_kind: process.runtime_kind.clone(),
                external: process.external.clone(),
                external_resolution,
                app_exe,
                zenoh_env: process_zenoh_env,
                dependencies: process.depends_on.clone(),
                env: process.env.clone(),
                restart_policy: process.restart,
                failure: process.failure.clone(),
                readiness: effective_readiness(process),
                expected_services,
                startup_delay_ms: process.startup_delay_ms,
                resource_gate: process_resource_gate(graph, process),
                resource_degraded: resource_gate_decision.action == ResourceGateAction::Degrade,
                resource_wait: None,
                resource_placement: process.resource_placement.clone(),
                resource_placement_status: ResourcePlacementStatus {
                    desired: process.resource_placement.clone(),
                    applied: resource_applied,
                },
                child,
                socket,
                finished: false,
                restart_count: 0,
                next_restart_unix_ms: None,
                last_seen_unix_ms: None,
                last_tick_count: None,
                last_tick_changed_unix_ms: unix_time_ms(),
                state: "starting".to_string(),
                exit_code: None,
            };
            record_child_health(&supervisor_state, &child, false);
            spawned_names.insert(process.name.clone());
            if status_snapshot.is_some() && process_writes_status_snapshot(process) {
                status_processes.push(process.clone());
            }

            // 等待 readiness gate，然后再启动依赖此进程的后续进程。
            wait_for_readiness(
                &supervisor_state,
                &mut child,
                &ReadinessConfig::default(),
                &shutdown,
            )?;
            ready_names.insert(process.name.clone());

            // readiness 通过后执行错峰启动延迟。
            wait_for_startup_delay(&supervisor_state, &mut child, &shutdown)?;

            child.state = if child.resource_degraded {
                "degraded".to_string()
            } else {
                "running".to_string()
            };
            record_child_health(&supervisor_state, &child, false);
            children.push(child);
        }
        zenoh_launch_plans.push(zenoh_plan);
    }
    if children.is_empty() {
        return Err("FlowRT launch manifest does not contain process groups".to_string());
    }

    supervise_children(
        &supervisor_state,
        &mut children,
        run_ticks,
        &shutdown,
        status_snapshot
            .as_ref()
            .map(|snapshot| snapshot.child_dir.as_path()),
    )?;
    if let Some(snapshot) = &status_snapshot {
        let aggregate = aggregate_status_snapshots(&snapshot.child_dir, &status_processes)?;
        if let Some(parent) = snapshot
            .output
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "failed to create status snapshot parent `{}`: {error}",
                    parent.display()
                )
            })?;
        }
        let json = serde_json::to_string_pretty(&aggregate)
            .map_err(|error| format!("failed to encode launch status snapshot: {error}"))?;
        std::fs::write(&snapshot.output, format!("{json}\n")).map_err(|error| {
            format!(
                "failed to write launch status snapshot `{}`: {error}",
                snapshot.output.display()
            )
        })?;
    }

    let mut failures = Vec::new();
    for child in children {
        if child.state == "failed" {
            failures.push(format!(
                "{} exited with code {}",
                child.name,
                child
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_string())
            ));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

// ---------------------------------------------------------------------------
// 监控循环
// ---------------------------------------------------------------------------

pub(super) fn supervise_children(
    supervisor_state: &IntrospectionState,
    children: &mut [SupervisedChild],
    run_ticks: Option<usize>,
    shutdown: &ShutdownToken,
    status_dir: Option<&Path>,
) -> Result<(), String> {
    let mut run_limit_reached_at: Option<Instant> = None;
    while children.iter().any(|child| !child.finished) {
        if shutdown.is_requested() {
            terminate_active_children(supervisor_state, children, "shutdown");
            return Ok(());
        }
        let completing_run_limit = run_limit_reached_at.is_some();
        let mut failed_to_propagate = Vec::new();
        for child in children.iter_mut() {
            if child.finished || child.next_restart_unix_ms.is_some() {
                continue;
            }
            if let Some(status) = child.child.try_wait().map_err(|error| {
                format!("failed to poll FlowRT process `{}`: {error}", child.name)
            })? {
                child.exit_code = status.code();
                if child
                    .restart_policy
                    .can_restart(status.success(), child.restart_count)
                {
                    child.state = "restarting".to_string();
                    child.next_restart_unix_ms = Some(
                        unix_time_ms()
                            .saturating_add(child.restart_policy.delay_ms_for(child.restart_count)),
                    );
                } else if status.success() {
                    child.finished = true;
                    child.state = if completing_run_limit {
                        "completed".to_string()
                    } else {
                        "exited".to_string()
                    };
                } else {
                    child.finished = true;
                    child.state = "failed".to_string();
                    if child.failure == "propagate" {
                        failed_to_propagate.push(child.name.clone());
                    }
                }
                record_child_health(supervisor_state, child, false);
                continue;
            }
            refresh_child_health(supervisor_state, child);
        }
        for failed_process in failed_to_propagate {
            propagate_process_failure(supervisor_state, children, &failed_process);
        }
        if run_ticks_satisfied(children, run_ticks) {
            let reached_at = run_limit_reached_at.get_or_insert_with(Instant::now);
            if reached_at.elapsed() >= RUN_LIMIT_NATURAL_EXIT_GRACE {
                terminate_active_children(supervisor_state, children, "completed");
                return Ok(());
            }
        } else {
            run_limit_reached_at = None;
        }
        let (spawned_names, ready_names) = child_dependency_snapshot(children);
        for child in children.iter_mut() {
            if child.finished {
                continue;
            }
            if let Some(next_restart_unix_ms) = child.next_restart_unix_ms {
                if unix_time_ms() >= next_restart_unix_ms {
                    if child_dependencies_satisfied(child, &spawned_names, &ready_names) {
                        restart_child(supervisor_state, child, run_ticks, shutdown, status_dir)?;
                    } else {
                        child.state = "waiting_dependencies".to_string();
                        child.next_restart_unix_ms = Some(
                            unix_time_ms().saturating_add(HEALTH_POLL_INTERVAL.as_millis() as u64),
                        );
                        record_child_health(supervisor_state, child, false);
                    }
                } else {
                    record_child_health(supervisor_state, child, false);
                }
                continue;
            }
        }
        std::thread::sleep(HEALTH_POLL_INTERVAL);
    }
    Ok(())
}

fn run_ticks_satisfied(children: &[SupervisedChild], run_ticks: Option<usize>) -> bool {
    let Some(run_ticks) = run_ticks else {
        return false;
    };
    children
        .iter()
        .filter(|child| !child.finished)
        .all(|child| {
            child
                .last_tick_count
                .is_some_and(|tick_count| tick_count >= run_ticks as u64)
        })
}

pub(super) fn restart_child(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    run_ticks: Option<usize>,
    shutdown: &ShutdownToken,
    status_dir: Option<&Path>,
) -> Result<(), String> {
    let resource_gate_decision =
        evaluate_child_resource_gates(supervisor_state, child, ResourceGatePhase::Restart);
    record_resource_gate_statuses(supervisor_state, &resource_gate_decision.statuses);
    match resource_gate_decision.action {
        ResourceGateAction::Start | ResourceGateAction::Degrade => {
            child.resource_degraded = resource_gate_decision.action == ResourceGateAction::Degrade;
            child.resource_wait = None;
        }
        ResourceGateAction::WaitRestart => {
            child.state = "waiting_resources".to_string();
            child.resource_wait = resource_gate_decision.wait_label();
            child.next_restart_unix_ms =
                Some(unix_time_ms().saturating_add(HEALTH_POLL_INTERVAL.as_millis() as u64));
            record_child_health(supervisor_state, child, false);
            return Ok(());
        }
        ResourceGateAction::StopProcess => {
            child.finished = true;
            child.state = "stopped".to_string();
            child.resource_wait = resource_gate_decision.wait_label();
            record_child_health(supervisor_state, child, false);
            return Ok(());
        }
        ResourceGateAction::StopGraph => {
            child.finished = true;
            child.state = "failed".to_string();
            child.resource_wait = resource_gate_decision.wait_label();
            record_child_health(supervisor_state, child, false);
            return Err(resource_gate_decision
                .error_message(&child.name)
                .unwrap_or_else(|| {
                    format!(
                        "FlowRT resource gate stopped graph before restarting process `{}`",
                        child.name
                    )
                }));
        }
    }
    let restart_process = LaunchProcess {
        name: child.name.clone(),
        backend: child.backend.clone(),
        target: None,
        runtimes: Vec::new(),
        runtime_kind: child.runtime_kind.clone(),
        external: child.external.clone(),
        depends_on: child.dependencies.clone(),
        env: child.env.clone(),
        restart: child.restart_policy,
        failure: child.failure.clone(),
        readiness: child.readiness,
        startup_delay_ms: child.startup_delay_ms,
        instances: Vec::new(),
        tasks: Vec::new(),
        resource_placement: child.resource_placement.clone(),
        io_boundaries: Vec::new(),
    };
    let restarted = if child.runtime_kind == "external" {
        let external = child.external.as_ref().ok_or_else(|| {
            format!(
                "external process `{}` is missing restart metadata",
                child.name
            )
        })?;
        let resolution = child.external_resolution.as_ref().ok_or_else(|| {
            format!(
                "external process `{}` package `{}` executable `{}` lost resolved path",
                child.name, external.package, external.executable
            )
        })?;
        build_external_process_command(
            &child.app_exe,
            &restart_process,
            run_ticks,
            child.zenoh_env.as_ref(),
            external,
            resolution,
        )
        .spawn()
        .map_err(|error| {
            format!(
                "failed to restart FlowRT process `{}` executable `{}`: {error}",
                child.name,
                child.app_exe.display()
            )
        })?
    } else {
        let child_status_dir = if matches!(child.runtime_kind.as_str(), "rust" | "cpp" | "c") {
            status_dir
        } else {
            None
        };
        build_process_command_with_status_out(
            &child.app_exe,
            &restart_process,
            run_ticks,
            child.zenoh_env.as_ref(),
            child_status_dir,
        )
        .spawn()
        .map_err(|error| {
            format!(
                "failed to restart FlowRT process `{}` executable `{}`: {error}",
                child.name,
                child.app_exe.display()
            )
        })?
    };
    let resource_applied =
        apply_resource_placement_to_pid(&child.resource_placement, Some(restarted.id()));
    child.child = restarted;
    child.socket = crate::runtime_socket_path_for_pid(child.child.id());
    child.restart_count = child.restart_count.saturating_add(1);
    child.next_restart_unix_ms = None;
    child.last_seen_unix_ms = None;
    child.last_tick_count = None;
    child.last_tick_changed_unix_ms = unix_time_ms();
    child.exit_code = None;
    child.state = "starting".to_string();
    child.resource_placement_status = ResourcePlacementStatus {
        desired: child.resource_placement.clone(),
        applied: resource_applied,
    };
    record_child_health(supervisor_state, child, false);
    wait_for_readiness(
        supervisor_state,
        child,
        &ReadinessConfig::default(),
        shutdown,
    )?;
    wait_for_startup_delay(supervisor_state, child, shutdown)?;
    child.state = if child.resource_degraded {
        "degraded".to_string()
    } else {
        "running".to_string()
    };
    record_child_health(supervisor_state, child, false);
    Ok(())
}

fn child_dependency_snapshot(children: &[SupervisedChild]) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut spawned_names = BTreeSet::new();
    let mut ready_names = BTreeSet::new();
    for child in children {
        if child.finished || child.next_restart_unix_ms.is_some() {
            continue;
        }
        spawned_names.insert(child.name.clone());
        if matches!(child.state.as_str(), "running" | "degraded" | "stale") {
            ready_names.insert(child.name.clone());
        }
    }
    (spawned_names, ready_names)
}

pub(super) fn child_dependencies_satisfied(
    child: &SupervisedChild,
    spawned_names: &BTreeSet<String>,
    ready_names: &BTreeSet<String>,
) -> bool {
    child.dependencies.iter().all(|dependency| {
        ready_names.contains(dependency)
            || (child.readiness == ReadinessGate::ProcessStarted
                && spawned_names.contains(dependency))
    })
}

pub(super) fn terminate_active_children(
    supervisor_state: &IntrospectionState,
    children: &mut [SupervisedChild],
    state: &'static str,
) {
    for child in children.iter_mut().filter(|child| !child.finished) {
        child.terminate(state);
        record_child_health(supervisor_state, child, false);
    }
}

fn propagate_process_failure(
    supervisor_state: &IntrospectionState,
    children: &mut [SupervisedChild],
    failed_process: &str,
) {
    let mut pending = vec![failed_process.to_string()];
    while let Some(failed) = pending.pop() {
        for child in children.iter_mut() {
            if child.finished
                || !child
                    .dependencies
                    .iter()
                    .any(|dependency| dependency == &failed)
            {
                continue;
            }
            child.terminate("failed");
            child.exit_code = None;
            record_child_health(supervisor_state, child, false);
            if child.failure == "propagate" {
                pending.push(child.name.clone());
            }
        }
    }
}

pub(super) fn refresh_child_health(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
) {
    let now = unix_time_ms();
    match crate::introspection::request_status_with_timeout(&child.socket, HEALTH_POLL_INTERVAL) {
        Ok(crate::IntrospectionResponse::Status { status, .. }) => {
            child.last_seen_unix_ms = Some(now);
            if child.last_tick_count != Some(status.tick_count) {
                child.last_tick_count = Some(status.tick_count);
                child.last_tick_changed_unix_ms = now;
            }
            record_child_reported_resource_statuses(supervisor_state, child, &status);
            let tick_stale =
                now.saturating_sub(child.last_tick_changed_unix_ms) > TICK_STALE_AFTER_MS;
            child.state = if tick_stale {
                "stale".to_string()
            } else if child.resource_degraded {
                "degraded".to_string()
            } else {
                "running".to_string()
            };
            record_child_health(supervisor_state, child, tick_stale);
        }
        _ => {
            let tick_stale =
                now.saturating_sub(child.last_tick_changed_unix_ms) > TICK_STALE_AFTER_MS;
            if tick_stale {
                child.state = "stale".to_string();
            }
            record_child_health(supervisor_state, child, tick_stale);
        }
    }
}

pub(super) fn record_child_reported_resource_statuses(
    supervisor_state: &IntrospectionState,
    child: &mut SupervisedChild,
    status: &crate::IntrospectionStatus,
) {
    for resource in &status.resources {
        let mut reported = resource.clone();
        if reported.owner_process.is_none() {
            reported.owner_process = Some(child.name.clone());
        }
        if reported.source.is_none() {
            reported.source = Some("runtime".to_string());
        }
        if matches!(reported.state.as_str(), "degraded" | "failed") {
            child.resource_degraded = true;
            child.resource_wait = Some(format!(
                "resource={} capability={} state={} policy={}",
                reported.name,
                reported.capability,
                reported.state,
                reported.on_failure.as_deref().unwrap_or("runtime_reported")
            ));
        }
        supervisor_state.record_resource_status(reported);
    }

    for boundary in &status.io_boundaries {
        for resource in &boundary.resources {
            let full_name = format!("{}.{}", boundary.name, resource.name);
            let contract = child.resource_gate.resources.iter().find(|contract| {
                contract.instance == boundary.name && contract.resource == resource.name
            });
            let mut state = if resource.ready { "ready" } else { "pending" };
            if resource.last_error.is_some() {
                state = if contract.is_some_and(|contract| contract.required) {
                    "failed"
                } else {
                    "degraded"
                };
            }
            let mut reported = contract.map_or_else(
                || IntrospectionResourceStatus {
                    name: full_name.clone(),
                    capability: resource.kind.clone(),
                    state: state.to_string(),
                    required: false,
                    source: Some("io_boundary".to_string()),
                    owner_process: Some(child.name.clone()),
                    last_error: resource.last_error.clone(),
                    updated_unix_ms: resource.updated_unix_ms,
                    ..Default::default()
                },
                |contract| {
                    let mut status = resource_status_from_contract(contract, state);
                    status.owner_process = Some(child.name.clone());
                    status.source = Some("io_boundary".to_string());
                    status.satisfied = Some(state == "ready");
                    status.contract_status = Some(if state == "ready" {
                        "satisfied".to_string()
                    } else if contract.required {
                        "unsatisfied".to_string()
                    } else {
                        "optional_unsatisfied".to_string()
                    });
                    status.last_error = resource.last_error.clone();
                    status.updated_unix_ms = resource.updated_unix_ms.or(status.updated_unix_ms);
                    status
                },
            );
            if reported.state == "failed" || reported.state == "degraded" {
                child.resource_degraded = true;
                child.resource_wait = Some(format!(
                    "resource={} capability={} state={} policy={}",
                    reported.name,
                    reported.capability,
                    reported.state,
                    reported.on_failure.as_deref().unwrap_or("runtime_reported")
                ));
            }
            if reported.diagnostic.is_none() {
                reported.diagnostic = resource.message.clone();
            }
            supervisor_state.record_resource_status(reported);
        }
    }
}

pub(super) fn record_child_health(
    supervisor_state: &IntrospectionState,
    child: &SupervisedChild,
    tick_stale: bool,
) {
    let readiness_wait = child
        .resource_wait
        .as_deref()
        .or(match child.state.as_str() {
            "waiting_readiness" => Some(readiness_gate_label(child.readiness)),
            _ => None,
        });
    let resource_placement = if resource_placement::has_placement(&child.resource_placement) {
        Some(child.resource_placement_status.clone())
    } else {
        None
    };
    supervisor_state.record_process_health(IntrospectionProcessStatus {
        name: child.name.clone(),
        state: child.state.clone(),
        pid: Some(child.child.id()),
        restart_count: child.restart_count,
        tick_count: child.last_tick_count,
        last_seen_unix_ms: child.last_seen_unix_ms,
        tick_stale,
        exit_code: child.exit_code,
        readiness_wait: readiness_wait.map(|s| s.to_string()),
        resource_placement,
    });
}
