use super::*;

#[test]
fn launch_manifest_groups_instances_by_process() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10
priority = 7
readiness = "all_ready"
lane = "sink_serial"

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let processes = launch["graphs"][0]["processes"].as_array().unwrap();

    assert_eq!(processes.len(), 2);
    assert_eq!(processes[0]["name"], "control");
    assert_eq!(processes[0]["backend"], "iox2");
    assert_eq!(processes[0]["target"], "linux");
    assert_eq!(processes[0]["runtimes"], serde_json::json!(["rust"]));
    assert_eq!(processes[0]["runtime_kind"], "rust");
    assert_eq!(processes[0]["instances"], serde_json::json!(["sink"]));
    assert_eq!(
        processes[0]["tasks"],
        serde_json::json!([
            {
                "name": "main",
                "instance": "sink",
                "trigger": "on_message",
                "period_ms": null,
                "deadline_ms": 10,
                "priority": 7,
                "readiness": "all_ready",
                "lane": "sink_serial",
                "inputs": ["value"],
                "outputs": []
            }
        ])
    );
    let graph_tasks = launch["graphs"][0]["tasks"].as_array().unwrap();
    let source_task = graph_tasks
        .iter()
        .find(|task| task["instance"] == "source")
        .unwrap();
    let sink_task = graph_tasks
        .iter()
        .find(|task| task["instance"] == "sink")
        .unwrap();
    assert_eq!(source_task["priority"], serde_json::json!(null));
    assert_eq!(source_task["inputs"], serde_json::json!([]));
    assert_eq!(source_task["outputs"], serde_json::json!(["value"]));
    assert_eq!(sink_task["priority"], 7);
    assert_eq!(sink_task["readiness"], "all_ready");
    assert_eq!(sink_task["lane"], "sink_serial");
    assert_eq!(sink_task["inputs"], serde_json::json!(["value"]));
    assert_eq!(sink_task["outputs"], serde_json::json!([]));
    assert_eq!(launch["graphs"][0]["scheduler"]["worker_threads"], 1);
    assert_eq!(
        launch["graphs"][0]["scheduler"]["lanes"],
        serde_json::json!([
            {"name": "sink_serial", "kind": "serial", "instance": "sink"},
            {"name": "source_serial", "kind": "serial", "instance": "source"}
        ])
    );
    assert_eq!(
        launch["graphs"][0]["scheduler"]["tasks"][0],
        serde_json::json!({
            "name": "main",
            "instance": "sink",
            "lane": "sink_serial",
            "trigger": "on_message",
            "readiness": "all_ready",
            "period_ms": null,
            "deadline_ms": 10,
            "priority": 7
        })
    );
    assert_eq!(processes[1]["name"], "sensors");
    assert_eq!(processes[1]["backend"], "iox2");
    assert_eq!(processes[1]["target"], "linux");
    assert_eq!(processes[1]["runtimes"], serde_json::json!(["rust"]));
    assert_eq!(processes[1]["runtime_kind"], "rust");
    assert_eq!(processes[1]["instances"], serde_json::json!(["source"]));
}

#[test]
fn launch_manifest_exposes_process_orchestration_policy() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensor_proc"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control_proc"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[[process]]
name = "sensor_proc"
restart = "on_failure"
max_restarts = 5
initial_delay_ms = 50
max_delay_ms = 500
failure = "propagate"

[[process]]
name = "control_proc"
depends_on = ["sensor_proc"]
restart = "never"
failure = "isolate"

[profile.default]
backend = "iox2"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let processes = launch["graphs"][0]["processes"].as_array().unwrap();
    let control = processes
        .iter()
        .find(|process| process["name"] == "control_proc")
        .unwrap();
    let sensors = processes
        .iter()
        .find(|process| process["name"] == "sensor_proc")
        .unwrap();

    assert_eq!(control["depends_on"], serde_json::json!(["sensor_proc"]));
    assert_eq!(control["restart"]["policy"], "never");
    assert_eq!(control["restart"]["max_restarts"], 0);
    assert_eq!(control["failure"], "isolate");
    assert_eq!(sensors["depends_on"], serde_json::json!([]));
    assert_eq!(sensors["restart"]["policy"], "on_failure");
    assert_eq!(sensors["restart"]["max_restarts"], 5);
    assert_eq!(sensors["restart"]["initial_delay_ms"], 50);
    assert_eq!(sensors["restart"]["max_delay_ms"], 500);
    assert_eq!(sensors["failure"], "propagate");
}

#[test]
fn launch_manifest_exposes_service_binds() {
    let ir = contract_from_source(
        r#"
[package]
name = "service_demo"
rsdl_version = "0.1"

[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[component.client]
language = "rust"
service_client = ["plan:PlanRequest->PlanResponse"]

[component.server]
language = "rust"
service_server = ["plan:PlanRequest->PlanResponse"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let services = launch["graphs"][0]["services"].as_array().unwrap();

    assert_eq!(services.len(), 1);
    assert_eq!(services[0]["client"], "client.plan");
    assert_eq!(services[0]["server"], "server.plan");
    assert_eq!(services[0]["request"], "PlanRequest");
    assert_eq!(services[0]["response"], "PlanResponse");
}

#[test]
fn launch_manifest_marks_mixed_process_runtime_kind() {
    let ir = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "main"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "main"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let process = &launch["graphs"][0]["processes"][0];

    assert_eq!(process["name"], "main");
    assert_eq!(process["runtimes"], serde_json::json!(["cpp", "rust"]));
    assert_eq!(process["runtime_kind"], "mixed");
}

#[test]
fn launch_manifest_exposes_iox2_channel_services() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let channels = launch["graphs"][0]["channels"].as_array().unwrap();
    let channel = &channels[0];

    assert_eq!(channels.len(), 1);
    assert_eq!(channel["from"], "source.value");
    assert_eq!(channel["to"], "sink.value");
    assert_eq!(channel["backend"], "iox2");
    assert_eq!(
        channel["service"],
        "FlowRT/robot_demo/default/bind_0/source_value_to_sink_value"
    );
    assert_eq!(channel["channel"], "latest");
    assert_eq!(channel["depth"], 1);
    assert_eq!(channel["overflow"], "drop_oldest");
    assert_eq!(channel["stale_policy"], "warn");
    assert!(channel["max_age_ms"].is_null());
}

#[test]
fn rust_shell_exposes_process_run_entrypoint() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let rust_main = artifact_content(&bundle, "rust/src/main.rs");
    let rust_lib = artifact_content(&bundle, "rust/src/lib.rs");

    assert!(rust_shell.contains("pub fn run_process(self, backend: &dyn flowrt::Backend, process: &str, run_ticks: Option<usize>) -> flowrt::Status"));
    assert!(rust_shell.contains("\"control\" => self.run_process_control(backend, run_ticks)"));
    assert!(rust_shell.contains("\"sensors\" => self.run_process_sensors(backend, run_ticks)"));
    assert!(
        rust_shell.contains(
            "pub fn run_process(process: &str, run_ticks: Option<usize>) -> flowrt::Status"
        )
    );
    assert!(rust_main.contains("--process"));
    // 生成应用内部兼容参数：`--flowrt-run-ticks` 作为 `--flowrt-run-steps` 的别名保留
    assert!(rust_main.contains("--flowrt-run-ticks"));
    assert!(rust_main.contains("--flowrt-run-steps"));
    assert!(rust_main.contains("flowrt_app::runtime_shell::run_process(process, run_ticks)"));
    assert!(rust_lib.contains("pub use runtime_shell::{run, run_process, App};"));
}

#[test]
fn cpp_shell_exposes_process_run_entrypoint() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "cpp"
input = ["value:u32"]

[instance.source]
component = "source"
process = "control"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let cpp_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    let cpp_main = artifact_content(&bundle, "cpp/src/main.cpp");

    assert!(cpp_header.contains(
        "flowrt::Status step_process_control(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events);"
    ));
    assert!(
            cpp_header
                .contains("flowrt::Status run_process_control(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks);")
        );
    assert!(cpp_shell.contains("flowrt::Status App::step_process_control"));
    assert!(cpp_shell.contains("flowrt::Status App::run_process_control"));
    assert!(cpp_shell.contains("if (process == \"control\")"));
    assert!(cpp_shell.contains("return run_process_control(backend, run_ticks);"));
    assert!(cpp_main.contains("--process"));
    // 生成应用内部兼容参数：`--flowrt-run-ticks` 作为 `--flowrt-run-steps` 的别名保留
    assert!(cpp_main.contains("--flowrt-run-ticks"));
    assert!(cpp_main.contains("--flowrt-run-steps"));
    assert!(cpp_main.contains("flowrt_app::run_process(process, run_ticks)"));
}

#[test]
fn emits_rust_supervisor_artifacts_for_process_launch() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let paths = bundle
        .artifacts
        .iter()
        .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let supervisor = artifact_content(&bundle, "rust/src/supervisor.rs");
    let supervisor_main = artifact_content(&bundle, "rust/src/supervisor_main.rs");
    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");

    assert!(paths.contains(&"rust/src/supervisor.rs".to_string()));
    assert!(paths.contains(&"rust/src/supervisor_main.rs".to_string()));
    assert!(
        supervisor
            .contains("const LAUNCH_MANIFEST: &str = include_str!(\"../../launch/launch.json\");")
    );
    assert!(supervisor.contains("flowrt::supervisor::SupervisorConfig"));
    assert!(supervisor.contains("rust_app_stem: \"robot-demo-flowrt-app\""));
    assert!(supervisor.contains("flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)"));
    // 生成应用内部兼容参数：`--flowrt-run-ticks` 作为 `--flowrt-run-steps` 的别名保留
    assert!(supervisor_main.contains("--flowrt-run-ticks"));
    assert!(supervisor_main.contains("--flowrt-run-steps"));
    assert!(supervisor_main.contains("flowrt_app::supervisor::launch(run_ticks)"));
    assert!(cargo_manifest.contains("[[bin]]\nname = \"robot-demo-flowrt-supervisor\""));
    assert!(cargo_manifest.contains("path = \"../rust/src/supervisor_main.rs\""));
}

#[test]
fn rust_supervisor_selects_app_executable_from_runtime_kind() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "cpp_source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "rust_sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let supervisor = artifact_content(&bundle, "rust/src/supervisor.rs");

    assert!(supervisor.contains("flowrt::supervisor::SupervisorConfig"));
    assert!(supervisor.contains("rust_app_stem: \"robot-demo-flowrt-app\""));
    assert!(supervisor.contains("cpp_app_stem: \"robot_demo_cpp_app\""));
    assert!(supervisor.contains("flowrt::supervisor::launch(&SUPERVISOR_CONFIG, run_ticks)"));
}
