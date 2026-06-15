use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use super::super::launch_loop::SupervisedChild;
use super::super::manifest::parse_launch_manifest;
use super::super::resource_placement::{ResourcePlacement, ResourcePlacementStatus};
use super::super::resources::ProcessResourceGate;
use super::super::time::unix_time_ms;
use super::super::*;

pub(super) static ZENOH_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub(super) static EXTERNAL_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub(super) static ROS2_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) struct EnvOverride {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvOverride {
    pub(super) fn set(key: &'static str, value: Option<&str>) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: 测试调用方必须持有对应环境变量 mutex。
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self { key, previous }
    }
}

impl Drop for EnvOverride {
    fn drop(&mut self) {
        // SAFETY: 测试调用方必须持有对应环境变量 mutex。
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

pub(super) fn command_args(command: &Command) -> Vec<String> {
    command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

pub(super) fn command_env(command: &Command, key: &str) -> Option<String> {
    command
        .get_envs()
        .find(|(env_key, _)| env_key.to_string_lossy() == key)
        .and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
}

pub(super) fn path_list(raw: &str) -> Vec<String> {
    std::env::split_paths(raw)
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

pub(super) fn test_process(name: &str, depends_on: Vec<String>) -> LaunchProcess {
    LaunchProcess {
        name: name.into(),
        backend: "inproc".into(),
        target: None,
        runtimes: vec![],
        runtime_kind: "rust".into(),
        external: None,
        depends_on,
        env: BTreeMap::new(),
        restart: DEFAULT_RESTART_POLICY,
        failure: "propagate".into(),
        readiness: ReadinessGate::ProcessStarted,
        startup_delay_ms: 0,
        instances: vec![],
        tasks: vec![],
        resource_placement: ResourcePlacement::default(),
        io_boundaries: vec![],
    }
}

pub(super) struct ResourceGateFixtureOptions<'a> {
    pub(super) readiness: &'a str,
    pub(super) required: bool,
    pub(super) health: &'a str,
    pub(super) on_failure: &'a str,
    pub(super) status: &'a str,
    pub(super) satisfied: bool,
    pub(super) provider: Option<&'a str>,
    pub(super) diagnostic: Option<&'a str>,
}

pub(super) fn resource_gate_fixture(
    options: ResourceGateFixtureOptions<'_>,
) -> (LaunchGraph, LaunchProcess) {
    let manifest = parse_launch_manifest(&resource_gate_manifest_json(&options)).unwrap();
    let graph = manifest.graphs.into_iter().next().unwrap();
    let process = graph.processes[0].clone();
    (graph, process)
}

pub(super) fn resource_gate_manifest_json(options: &ResourceGateFixtureOptions<'_>) -> String {
    let provider_value = options
        .provider
        .map(|name| format!(r#""{name}""#))
        .unwrap_or_else(|| "null".to_string());
    let diagnostic_value = options
        .diagnostic
        .map(|message| format!(r#""{message}""#))
        .unwrap_or_else(|| "null".to_string());
    let readiness = options.readiness;
    let required = options.required;
    let health = options.health;
    let on_failure = options.on_failure;
    let status = options.status;
    let satisfied = options.satisfied;
    format!(
        r#"{{
        "package": "demo",
        "ir_version": "0.1",
        "profiles": ["default"],
        "targets": ["edge"],
        "graphs": [{{
            "name": "main",
            "scheduler": {{}},
            "resource_contract": {{
                "resource_contract_version": "0.1",
                "providers": [{{
                    "name": "sensor_provider",
                    "scope": "process",
                    "capabilities": ["perception.samples"],
                    "target": null,
                    "process": "sensor_proc",
                    "external_package": null,
                    "readiness_source": "provider_ready",
                    "health_source": "provider_health"
                }}],
                "requirements": [{{
                    "instance": "sensor",
                    "component": "sensor",
                    "name": "samples",
                    "capability": "perception.samples",
                    "access": "read_write",
                    "required": {required},
                    "readiness": "{readiness}",
                    "health": "{health}",
                    "on_failure": "{on_failure}",
                    "satisfaction": "{status}",
                    "provider": {provider_value},
                    "diagnostic": {diagnostic_value}
                }}],
                "satisfactions": [{{
                    "instance": "sensor",
                    "component": "sensor",
                    "resource": "samples",
                    "capability": "perception.samples",
                    "access": "read_write",
                    "required": {required},
                    "readiness": "{readiness}",
                    "health": "{health}",
                    "on_failure": "{on_failure}",
                    "status": "{status}",
                    "satisfied": {satisfied},
                    "provider": {provider_value},
                    "diagnostic": {diagnostic_value}
                }}]
            }},
            "channels": [],
            "services": [],
            "ros2_bridges": [],
            "instances": [],
            "tasks": [],
            "processes": [{{
                "name": "sensor_proc",
                "backend": "inproc",
                "runtime_kind": "rust",
                "instances": ["sensor"],
                "tasks": []
            }}]
        }}]
    }}"#
    )
}

pub(super) fn temp_test_dir(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "flowrt-supervisor-{name}-{}-{}",
        std::process::id(),
        unix_time_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("test temp dir should be created");
    root
}

pub(super) fn supervised_child_for_test(name: &str, child: Child) -> SupervisedChild {
    let socket = crate::runtime_socket_path_for_pid(child.id());
    SupervisedChild {
        name: name.into(),
        backend: "inproc".into(),
        runtime_kind: "rust".into(),
        external: None,
        external_resolution: None,
        app_exe: PathBuf::from("/tmp/fake"),
        zenoh_env: None,
        dependencies: vec![],
        env: BTreeMap::new(),
        restart_policy: DEFAULT_RESTART_POLICY,
        failure: "propagate".into(),
        readiness: ReadinessGate::ProcessStarted,
        expected_services: vec![],
        startup_delay_ms: 0,
        resource_gate: ProcessResourceGate::default(),
        resource_degraded: false,
        resource_wait: None,
        resource_placement: ResourcePlacement::default(),
        resource_placement_status: ResourcePlacementStatus::default(),
        child,
        socket,
        finished: false,
        restart_count: 0,
        next_restart_unix_ms: None,
        last_seen_unix_ms: None,
        last_tick_count: None,
        last_tick_changed_unix_ms: unix_time_ms(),
        state: "running".into(),
        exit_code: None,
    }
}

#[cfg(unix)]
pub(super) fn process_exists(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(unix)]
pub(super) fn assert_process_exits(pid: u32) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if !process_exists(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("process {pid} should have exited");
}

#[cfg(unix)]
pub(super) fn wait_for_child_exit(child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if child
            .try_wait()
            .expect("test child status should be readable")
            .is_some()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("test child should have exited");
}
