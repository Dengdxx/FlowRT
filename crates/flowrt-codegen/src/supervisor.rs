use flowrt_ir::ContractIr;

use crate::ros2_bridge::ros2_bridge_stem;
use crate::{contract_has_ros2_bridge, managed_header, rust_string_literal, sanitize_package_name};

pub(crate) fn emit_rust_supervisor_main() -> String {
    let mut output = managed_header();
    output.push_str(
        "\nfn main() {\n    let mut args = std::env::args().skip(1);\n    let mut run_ticks = None;\n    while let Some(arg) = args.next() {\n        match arg.as_str() {\n            \"--flowrt-run-ticks\" | \"--flowrt-run-steps\" => {\n                let Some(raw_ticks) = args.next() else {\n                    eprintln!(\"missing value for {arg}\");\n                    std::process::exit(2);\n                };\n                match raw_ticks.parse::<usize>() {\n                    Ok(ticks) if ticks > 0 => run_ticks = Some(ticks),\n                    _ => {\n                        eprintln!(\"invalid value for {arg}: {raw_ticks}\");\n                        std::process::exit(2);\n                    }\n                }\n            }\n            _ => {\n                eprintln!(\"unknown FlowRT supervisor argument: {arg}\");\n                std::process::exit(2);\n            }\n        }\n    }\n\n    match flowrt_app::supervisor::launch(run_ticks) {\n        Ok(()) => std::process::exit(0),\n        Err(error) => {\n            eprintln!(\"FlowRT supervisor failed: {error}\");\n            std::process::exit(1);\n        }\n    }\n}\n",
    );
    output
}

pub(crate) fn emit_rust_supervisor(contract: &ContractIr) -> String {
    let mut output = managed_header();
    output.push_str(&format!(
        "\nuse std::collections::{{BTreeSet, HashMap}};\nuse std::net::TcpListener;\nuse std::path::{{Path, PathBuf}};\nuse std::process::{{Child, Command}};\nuse std::time::Duration;\n\nconst PACKAGE_NAME: &str = {};\nconst RUST_APP_STEM: &str = {};\nconst CPP_APP_STEM: &str = {};\nconst ROS2_BRIDGE_STEM: &str = {};\nconst HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(100);\nconst TICK_STALE_AFTER_MS: u64 = 1_000;\n",
        rust_string_literal(&contract.package.name),
        rust_string_literal(&rust_app_stem(contract)),
        rust_string_literal(&cpp_app_stem(contract)),
        rust_string_literal(&ros2_bridge_app_stem(contract))
    ));
    output.push_str(
        r#"
const LAUNCH_MANIFEST: &str = include_str!("../../launch/launch.json");

#[derive(Debug, serde::Deserialize)]
struct LaunchManifest {
    graphs: Vec<LaunchGraph>,
}

#[derive(Debug, serde::Deserialize)]
struct LaunchGraph {
    processes: Vec<LaunchProcess>,
}

#[derive(Debug, serde::Deserialize)]
struct LaunchProcess {
    name: String,
    backend: String,
    runtime_kind: String,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default = "default_restart_policy")]
    restart: LaunchRestartPolicy,
    #[serde(default = "default_failure_propagation")]
    failure: String,
}

#[derive(Debug, Clone)]
struct ZenohLaunchEnv {
    listen: String,
    connect: String,
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum RestartPolicyKind {
    Never,
    OnFailure,
    Always,
}

type LaunchRestartPolicy = RestartPolicy;

#[derive(Debug, Clone, Copy, serde::Deserialize)]
struct RestartPolicy {
    policy: RestartPolicyKind,
    max_restarts: u32,
    initial_delay_ms: u64,
    max_delay_ms: u64,
}

const DEFAULT_RESTART_POLICY: RestartPolicy = RestartPolicy {
    policy: RestartPolicyKind::OnFailure,
    max_restarts: 3,
    initial_delay_ms: 100,
    max_delay_ms: 1_000,
};

impl RestartPolicy {
    fn can_restart(self, success: bool, restart_count: u32) -> bool {
        match self.policy {
            RestartPolicyKind::Never => false,
            RestartPolicyKind::OnFailure => !success && restart_count < self.max_restarts,
            RestartPolicyKind::Always => restart_count < self.max_restarts,
        }
    }

    fn delay_ms_for(self, restart_count: u32) -> u64 {
        let shift = restart_count.min(63);
        let multiplier = 1_u64.checked_shl(shift).unwrap_or(u64::MAX);
        self.initial_delay_ms
            .saturating_mul(multiplier)
            .min(self.max_delay_ms)
    }
}

fn default_restart_policy() -> LaunchRestartPolicy {
    DEFAULT_RESTART_POLICY
}

fn default_failure_propagation() -> String {
    "propagate".to_string()
}

struct SupervisedChild {
    name: String,
    app_exe: PathBuf,
    zenoh_env: Option<ZenohLaunchEnv>,
    dependencies: Vec<String>,
    restart_policy: RestartPolicy,
    failure: String,
    child: Child,
    socket: PathBuf,
    finished: bool,
    restart_count: u32,
    next_restart_unix_ms: Option<u64>,
    last_seen_unix_ms: Option<u64>,
    last_tick_count: Option<u64>,
    last_tick_changed_unix_ms: u64,
    state: String,
    exit_code: Option<i32>,
}

pub fn launch(run_ticks: Option<usize>) -> Result<(), String> {
    let manifest: LaunchManifest = serde_json::from_str(LAUNCH_MANIFEST)
        .map_err(|error| format!("failed to parse FlowRT launch manifest: {error}"))?;
    if manifest.graphs.is_empty() {
        return Err("FlowRT launch manifest does not contain a graph".to_string());
    }

    let current_exe = std::env::current_exe()
        .map_err(|error| format!("failed to resolve current executable: {error}"))?;

    let supervisor_state = flowrt::IntrospectionState::new();
    let _supervisor_status_server = flowrt::spawn_status_server(
        flowrt::IntrospectionIdentity {
            self_description_hash: crate::selfdesc::self_description_hash().to_string(),
            package: PACKAGE_NAME.to_string(),
            process: "flowrt_supervisor".to_string(),
            runtime: "supervisor".to_string(),
        },
        supervisor_state.clone(),
    )
    .ok();

    let mut children = Vec::new();
    for graph in &manifest.graphs {
        let zenoh_env = if should_auto_configure_zenoh() {
            zenoh_launch_env_for_graph(graph)?
        } else {
            HashMap::new()
        };
        let mut pending = graph.processes.iter().collect::<Vec<_>>();
        let mut spawned_names = BTreeSet::new();
        while !pending.is_empty() {
            let Some(index) = pending
                .iter()
                .position(|process| process_dependencies_satisfied(process, &spawned_names))
            else {
                return Err("FlowRT process dependencies contain a cycle or unknown process".to_string());
            };
            let process = pending.remove(index);
            let app_exe = app_executable_for_runtime(&current_exe, &process.runtime_kind)?;
            let process_zenoh_env = zenoh_env.get(&process.name).cloned();
            let child = spawn_flowrt_process(&app_exe, &process.name, run_ticks, process_zenoh_env.as_ref())?;
            let socket = flowrt::runtime_socket_path_for_pid(child.id());
            let child = SupervisedChild {
                name: process.name.clone(),
                app_exe,
                zenoh_env: process_zenoh_env,
                dependencies: process.depends_on.clone(),
                restart_policy: process.restart,
                failure: process.failure.clone(),
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
            children.push(child);
        }
    }
    if children.is_empty() {
        return Err("FlowRT launch manifest does not contain process groups".to_string());
    }

    supervise_children(&supervisor_state, &mut children, run_ticks)?;

    let mut failures = Vec::new();
    for child in children {
        if child.state == "failed" {
            failures.push(format!(
                "{} exited with code {}",
                child.name,
                child.exit_code
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

fn supervise_children(
    supervisor_state: &flowrt::IntrospectionState,
    children: &mut [SupervisedChild],
    run_ticks: Option<usize>,
) -> Result<(), String> {
    while children.iter().any(|child| !child.finished) {
        let mut failed_to_propagate = Vec::new();
        for child in children.iter_mut() {
            if child.finished {
                continue;
            }
            if let Some(next_restart_unix_ms) = child.next_restart_unix_ms {
                if unix_time_ms() >= next_restart_unix_ms {
                    restart_child(supervisor_state, child, run_ticks)?;
                } else {
                    record_child_health(supervisor_state, child, false);
                }
                continue;
            }
            if let Some(status) = child
                .child
                .try_wait()
                .map_err(|error| format!("failed to poll FlowRT process `{}`: {error}", child.name))?
            {
                child.exit_code = status.code();
                if child.restart_policy.can_restart(status.success(), child.restart_count) {
                    child.state = "restarting".to_string();
                    child.next_restart_unix_ms = Some(
                        unix_time_ms().saturating_add(
                            child.restart_policy.delay_ms_for(child.restart_count),
                        ),
                    );
                } else if status.success() {
                    child.finished = true;
                    child.state = "exited".to_string();
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
        std::thread::sleep(HEALTH_POLL_INTERVAL);
    }
    Ok(())
}

fn process_dependencies_satisfied(
    process: &LaunchProcess,
    spawned_names: &BTreeSet<String>,
) -> bool {
    process
        .depends_on
        .iter()
        .all(|dependency| spawned_names.contains(dependency))
}

fn spawn_flowrt_process(
    app_exe: &Path,
    process_name: &str,
    run_ticks: Option<usize>,
    zenoh_env: Option<&ZenohLaunchEnv>,
) -> Result<Child, String> {
    let mut command = Command::new(app_exe);
    if process_name != "ros2_bridge" {
        command.arg("--process").arg(process_name);
    } else {
        command.env("RMW_IMPLEMENTATION", "rmw_zenoh_cpp");
    }
    if let Some(run_ticks) = run_ticks {
        command.arg("--flowrt-run-steps").arg(run_ticks.to_string());
    }
    if let Some(env) = zenoh_env {
        command.env("FLOWRT_ZENOH_MODE", "peer");
        if !env.listen.is_empty() {
            command.env("FLOWRT_ZENOH_LISTEN", &env.listen);
        }
        if !env.connect.is_empty() {
            command.env("FLOWRT_ZENOH_CONNECT", &env.connect);
        }
        command.env("FLOWRT_ZENOH_NO_MULTICAST", "1");
    }
    command
        .spawn()
        .map_err(|error| format!("failed to start FlowRT process `{process_name}`: {error}"))
}

fn restart_child(
    supervisor_state: &flowrt::IntrospectionState,
    child: &mut SupervisedChild,
    run_ticks: Option<usize>,
) -> Result<(), String> {
    let restarted = spawn_flowrt_process(
        &child.app_exe,
        &child.name,
        run_ticks,
        child.zenoh_env.as_ref(),
    )?;
    child.child = restarted;
    child.socket = flowrt::runtime_socket_path_for_pid(child.child.id());
    child.restart_count = child.restart_count.saturating_add(1);
    child.next_restart_unix_ms = None;
    child.last_seen_unix_ms = None;
    child.last_tick_count = None;
    child.last_tick_changed_unix_ms = unix_time_ms();
    child.exit_code = None;
    child.state = "starting".to_string();
    record_child_health(supervisor_state, child, false);
    Ok(())
}

fn propagate_process_failure(
    supervisor_state: &flowrt::IntrospectionState,
    children: &mut [SupervisedChild],
    failed_process: &str,
) {
    let mut pending = vec![failed_process.to_string()];
    while let Some(failed) = pending.pop() {
        for child in children.iter_mut() {
            if child.finished || !child.dependencies.iter().any(|dependency| dependency == &failed) {
                continue;
            }
            let _ = child.child.kill();
            let _ = child.child.wait();
            child.finished = true;
            child.state = "failed".to_string();
            child.exit_code = None;
            record_child_health(supervisor_state, child, false);
            if child.failure == "propagate" {
                pending.push(child.name.clone());
            }
        }
    }
}

fn refresh_child_health(supervisor_state: &flowrt::IntrospectionState, child: &mut SupervisedChild) {
    let now = unix_time_ms();
    match flowrt::request_status(&child.socket) {
        Ok(flowrt::IntrospectionResponse::Status { status, .. }) => {
            child.last_seen_unix_ms = Some(now);
            if child.last_tick_count != Some(status.tick_count) {
                child.last_tick_count = Some(status.tick_count);
                child.last_tick_changed_unix_ms = now;
            }
            let tick_stale = now.saturating_sub(child.last_tick_changed_unix_ms) > TICK_STALE_AFTER_MS;
            child.state = if tick_stale {
                "stale".to_string()
            } else {
                "running".to_string()
            };
            record_child_health(supervisor_state, child, tick_stale);
        }
        _ => {
            let tick_stale = now.saturating_sub(child.last_tick_changed_unix_ms) > TICK_STALE_AFTER_MS;
            if tick_stale {
                child.state = "stale".to_string();
            }
            record_child_health(supervisor_state, child, tick_stale);
        }
    }
}

fn record_child_health(
    supervisor_state: &flowrt::IntrospectionState,
    child: &SupervisedChild,
    tick_stale: bool,
) {
    supervisor_state.record_process_health(flowrt::IntrospectionProcessStatus {
        name: child.name.clone(),
        state: child.state.clone(),
        pid: Some(child.child.id()),
        restart_count: child.restart_count,
        tick_count: child.last_tick_count,
        last_seen_unix_ms: child.last_seen_unix_ms,
        tick_stale,
        exit_code: child.exit_code,
    });
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or_default()
}

fn should_auto_configure_zenoh() -> bool {
    std::env::var_os("FLOWRT_ZENOH_MODE").is_none()
        && std::env::var_os("FLOWRT_ZENOH_LISTEN").is_none()
        && std::env::var_os("FLOWRT_ZENOH_CONNECT").is_none()
}

fn zenoh_launch_env_for_graph(graph: &LaunchGraph) -> Result<HashMap<String, ZenohLaunchEnv>, String> {
    let zenoh_processes = graph
        .processes
        .iter()
        .filter(|process| process.backend == "zenoh")
        .collect::<Vec<_>>();
    if zenoh_processes.is_empty() {
        return Ok(HashMap::new());
    }

    let hub = zenoh_processes[0];
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("failed to reserve local zenoh port for `{}`: {error}", hub.name))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("failed to read local zenoh port for `{}`: {error}", hub.name))?
        .port();
    let hub_locator = format!("tcp/127.0.0.1:{port}");
    drop(listener);

    let mut env = HashMap::new();
    for process in zenoh_processes {
        let listen = if process.name == hub.name {
            hub_locator.clone()
        } else {
            String::new()
        };
        let connect = if process.name == hub.name {
            String::new()
        } else {
            hub_locator.clone()
        };
        env.insert(
            process.name.clone(),
            ZenohLaunchEnv { listen, connect },
        );
    }
    Ok(env)
}

fn app_executable_for_runtime(current_exe: &Path, runtime_kind: &str) -> Result<PathBuf, String> {
    match runtime_kind {
        "rust" => rust_app_executable(current_exe),
        "cpp" => cpp_app_executable(current_exe),
        "ros2_bridge" => ros2_bridge_executable(current_exe),
        "mixed" => Err("FlowRT mixed process groups are not launchable yet".to_string()),
        other => Err(format!("unknown FlowRT process runtime_kind `{other}`")),
    }
}

fn rust_app_executable(current_exe: &Path) -> Result<PathBuf, String> {
    let mut path = current_exe.to_path_buf();
    path.set_file_name(binary_name(RUST_APP_STEM));
    Ok(path)
}

fn cpp_app_executable(current_exe: &Path) -> Result<PathBuf, String> {
    let build_dir = current_exe
        .parent()
        .and_then(|profile_dir| profile_dir.parent())
        .and_then(|target_dir| target_dir.parent())
        .ok_or_else(|| format!("failed to resolve FlowRT build directory from `{}`", current_exe.display()))?;
    let mut path = build_dir.join("cmake");
    path.push(binary_name(CPP_APP_STEM));
    Ok(path)
}

fn ros2_bridge_executable(current_exe: &Path) -> Result<PathBuf, String> {
    let build_dir = current_exe
        .parent()
        .and_then(|profile_dir| profile_dir.parent())
        .and_then(|target_dir| target_dir.parent())
        .ok_or_else(|| format!("failed to resolve FlowRT build directory from `{}`", current_exe.display()))?;
    let mut path = build_dir.join("cmake");
    path.push(binary_name(ROS2_BRIDGE_STEM));
    Ok(path)
}

fn binary_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}
"#,
    );
    output
}

fn rust_app_stem(contract: &ContractIr) -> String {
    format!(
        "{}-flowrt-app",
        sanitize_package_name(&contract.package.name).replace('_', "-")
    )
}

fn cpp_app_stem(contract: &ContractIr) -> String {
    format!(
        "{}_cpp_app",
        sanitize_package_name(&contract.package.name).replace('-', "_")
    )
}

fn ros2_bridge_app_stem(contract: &ContractIr) -> String {
    if contract_has_ros2_bridge(contract) {
        ros2_bridge_stem(contract)
    } else {
        String::new()
    }
}
