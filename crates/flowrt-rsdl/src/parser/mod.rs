use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use toml::Value;

use crate::ast::*;
use crate::{Result, RsdlError};

mod imports;
mod schema;
mod tables;
mod values;
mod workspace;

use imports::{canonicalize_existing, expand_imports, logical_source_path, read_source};
use schema::validate_top_level_sections;
use tables::{
    parse_binds, parse_boundary_endpoints, parse_component, parse_external_processes,
    parse_instance, parse_module, parse_named_tables, parse_operation_binds, parse_package,
    parse_processes, parse_profile, parse_ros2_bridges, parse_service_binds, parse_target,
    parse_type, parse_workspace,
};
use workspace::expand_workspace;

#[derive(Debug)]
struct ParsedDocument {
    package: Option<RawPackage>,
    workspace: Option<RawWorkspace>,
    module: Option<RawModule>,
    types: BTreeMap<String, RawType>,
    components: BTreeMap<String, RawComponent>,
    instances: BTreeMap<String, RawInstance>,
    processes: Vec<RawProcess>,
    external_processes: Vec<RawExternalProcess>,
    binds: Vec<RawDataflowBind>,
    service_binds: Vec<RawServiceBind>,
    operation_binds: Vec<RawOperationBind>,
    ros2_bridges: Vec<RawRos2Bridge>,
    boundary_inputs: Vec<RawBoundaryEndpoint>,
    boundary_outputs: Vec<RawBoundaryEndpoint>,
    profiles: BTreeMap<String, RawProfile>,
    targets: BTreeMap<String, RawTarget>,
}

/// 从磁盘解析一个 `.rsdl` 文件。
pub fn parse_file(path: impl AsRef<Path>) -> Result<RawDocument> {
    Ok(load_file(path)?.document)
}

/// 从磁盘加载一个 `.rsdl` 文件，并展开 `[package.imports]`。
pub fn load_file(path: impl AsRef<Path>) -> Result<LoadedDocument> {
    let path = path.as_ref();
    let root_path = canonicalize_existing(path)?;
    let package_root = root_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut loaded_paths = std::collections::BTreeSet::new();
    let mut sources = Vec::new();
    let mut document = load_root_document(&root_path, &package_root, &mut sources)?;
    loaded_paths.insert(root_path.clone());
    let mut modules = Vec::new();
    let mut compositions = Vec::new();

    if document.workspace.is_some() {
        expand_workspace(
            &mut document,
            &root_path,
            &package_root,
            &mut loaded_paths,
            &mut sources,
            &mut modules,
            &mut compositions,
        )?;
        return Ok(LoadedDocument {
            document,
            sources,
            modules,
            compositions,
        });
    }

    expand_imports(
        &mut document,
        &root_path,
        &package_root,
        &mut loaded_paths,
        &mut sources,
    )?;

    Ok(LoadedDocument {
        document,
        sources,
        modules,
        compositions,
    })
}

fn load_root_document(
    path: &Path,
    package_root: &Path,
    sources: &mut Vec<LoadedSource>,
) -> Result<RawDocument> {
    let source = read_source(path)?;
    sources.push(LoadedSource {
        path: logical_source_path(path, package_root),
        content: source.clone(),
    });
    parsed_to_raw(parse_source(&source, true)?)
}

/// 解析 RSDL v0.1 源文本。
pub fn parse_str(source: &str) -> Result<RawDocument> {
    parsed_to_raw(parse_source(source, true)?)
}

fn parse_source(source: &str, require_package: bool) -> Result<ParsedDocument> {
    let value: Value = source.parse()?;
    let root = value.as_table().ok_or_else(|| RsdlError::InvalidValue {
        context: "document".to_string(),
        message: "expected a TOML table document".to_string(),
    })?;
    validate_top_level_sections(root)?;

    let package = match root.get("package").and_then(Value::as_table) {
        Some(package_table) => Some(parse_package(package_table)?),
        None if require_package => return Err(RsdlError::MissingPackage),
        None => None,
    };

    Ok(ParsedDocument {
        package,
        workspace: root
            .get("workspace")
            .and_then(Value::as_table)
            .map(parse_workspace)
            .transpose()?,
        module: root
            .get("module")
            .and_then(Value::as_table)
            .map(parse_module)
            .transpose()?,
        types: parse_named_tables(root, "type", parse_type)?,
        components: parse_named_tables(root, "component", parse_component)?,
        instances: parse_named_tables(root, "instance", parse_instance)?,
        processes: parse_processes(root)?,
        external_processes: parse_external_processes(root)?,
        binds: parse_binds(root)?,
        service_binds: parse_service_binds(root)?,
        operation_binds: parse_operation_binds(root)?,
        ros2_bridges: parse_ros2_bridges(root)?,
        boundary_inputs: parse_boundary_endpoints(root, "input")?,
        boundary_outputs: parse_boundary_endpoints(root, "output")?,
        profiles: parse_named_tables(root, "profile", parse_profile)?,
        targets: parse_named_tables(root, "target", parse_target)?,
    })
}

fn parsed_to_raw(parsed: ParsedDocument) -> Result<RawDocument> {
    Ok(RawDocument {
        package: parsed.package.ok_or(RsdlError::MissingPackage)?,
        workspace: parsed.workspace,
        types: parsed.types,
        components: parsed.components,
        instances: parsed.instances,
        processes: parsed.processes,
        external_processes: parsed.external_processes,
        binds: parsed.binds,
        service_binds: parsed.service_binds,
        operation_binds: parsed.operation_binds,
        ros2_bridges: parsed.ros2_bridges,
        boundary_inputs: parsed.boundary_inputs,
        boundary_outputs: parsed.boundary_outputs,
        profiles: parsed.profiles,
        targets: parsed.targets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_rsdl_document() {
        let source = r#"
[package]
name = "robot_demo"
version = "0.1.0"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.imu_sim]
language = "rust"
output = ["imu:Imu"]

[instance.imu_sim]
component = "imu_sim"
process = "main"
target = "linux"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[profile.default]
backend = "inproc"
worker_threads = 3
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[target.linux]
platform = "linux-x86_64"
runtime = ["rust"]
backends = ["inproc"]
"#;

        let document = parse_str(source).expect("document should parse");
        assert_eq!(document.package.name, "robot_demo");
        assert_eq!(document.types["Imu"].fields[0].name, "timestamp");
        assert_eq!(document.components["imu_sim"].output[0].name, "imu");
        assert_eq!(document.instances["imu_sim"].tasks[0].trigger, "periodic");
        assert_eq!(document.profiles["default"].mode, RawGraphMode::Strict);
        assert_eq!(document.profiles["default"].worker_threads, Some(3));
    }

    #[test]
    fn parses_island_mode_and_boundary_endpoints() {
        let source = r#"
[package]
name = "island_demo"
rsdl_version = "0.1"

[type.Scan]
range = "f32"

[type.Command]
speed = "f32"

[component.planner]
language = "rust"
input = ["scan:Scan"]
output = ["cmd:Command"]

[instance.planner]
component = "planner"

[instance.planner.task]
trigger = "on_message"
input = ["scan"]
output = ["cmd"]

[profile.dev]
mode = "island"
backend = "inproc"

[[boundary.input]]
name = "scan_in"
port = "planner.scan"
type = "Scan"

[[boundary.output]]
name = "cmd_out"
port = "planner.cmd"
type = "Command"
"#;

        let document = parse_str(source).expect("island boundary document should parse");
        assert_eq!(document.profiles["dev"].mode, RawGraphMode::Island);
        assert_eq!(document.boundary_inputs.len(), 1);
        assert_eq!(document.boundary_inputs[0].name, "scan_in");
        assert_eq!(document.boundary_inputs[0].port, "planner.scan");
        assert_eq!(document.boundary_inputs[0].ty, "Scan");
        assert_eq!(document.boundary_outputs.len(), 1);
        assert_eq!(document.boundary_outputs[0].name, "cmd_out");
        assert_eq!(document.boundary_outputs[0].port, "planner.cmd");
        assert_eq!(document.boundary_outputs[0].ty, "Command");
    }

    #[test]
    fn rejects_unknown_profile_mode() {
        let source = r#"
[package]
name = "bad_mode"
rsdl_version = "0.1"

[profile.dev]
mode = "legacy"
"#;

        let error = parse_str(source).expect_err("unknown mode should fail");
        let message = error.to_string();
        assert!(
            message.contains("profile.dev.mode")
                && message.contains("profile mode must be `strict` or `island`"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_invalid_boundary_endpoint_tables() {
        let duplicate = r#"
[package]
name = "bad_boundary"
rsdl_version = "0.1"

[[boundary.input]]
name = "scan"
port = "planner.scan"
type = "Scan"

[[boundary.input]]
name = "scan"
port = "planner.other"
type = "Scan"
"#;

        let error = parse_str(duplicate).expect_err("duplicate boundary name should fail");
        assert!(
            error
                .to_string()
                .contains("duplicate `boundary.input` symbol `scan`"),
            "unexpected error: {error}"
        );

        let missing_type = r#"
[package]
name = "bad_boundary"
rsdl_version = "0.1"

[[boundary.output]]
name = "cmd"
port = "planner.cmd"
"#;

        let error = parse_str(missing_type).expect_err("missing boundary type should fail");
        assert!(
            error
                .to_string()
                .contains("missing required field `type` in `boundary.output[0]`"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn parses_multiple_tasks_for_one_instance() {
        let source = r#"
[package]
name = "multi_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
input = ["in:u32"]
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
input = ["in"]
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 100
input = ["in"]
output = ["slow"]
"#;

        let document = parse_str(source).expect("document should parse");
        let tasks = &document.instances["worker"].tasks;

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name.as_deref(), Some("fast_loop"));
        assert_eq!(tasks[1].name.as_deref(), Some("slow_loop"));
        assert_eq!(tasks[0].output, vec!["fast"]);
        assert_eq!(tasks[1].output, vec!["slow"]);
    }

    #[test]
    fn parses_explicit_empty_message_type() {
        let source = r#"
[package]
name = "empty_demo"
rsdl_version = "0.1"

[type.Empty]
empty = true
"#;

        let document = parse_str(source).expect("explicit empty message should parse");
        let ty = &document.types["Empty"];

        assert!(ty.empty);
        assert!(ty.fields.is_empty());
    }

    #[test]
    fn parses_string_field_named_empty_as_regular_field() {
        let source = r#"
[package]
name = "empty_field_demo"
rsdl_version = "0.1"

[type.Sample]
empty = "bool"
"#;

        let document = parse_str(source).expect("field named empty should remain legal");
        let ty = &document.types["Sample"];

        assert!(!ty.empty);
        assert_eq!(ty.fields.len(), 1);
        assert_eq!(ty.fields[0].name, "empty");
        assert_eq!(ty.fields[0].ty, "bool");
    }

    #[test]
    fn parses_external_process_declarations() {
        let source = r#"
[package]
name = "external_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.fake_sensor]
language = "external"
kind = "external"
output = ["sample:Sample"]

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"
target = "linux"

[[process]]
name = "sensor_proc"
readiness = "runtime_ready"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "driver"
args = ["--rate", "50"]
working_dir = "package"
health = "runtime_socket"
required_backends = ["zenoh"]

[profile.default]
backend = "zenoh"

[target.linux]
platform = "linux-arm64"
runtime = ["external"]
backends = ["zenoh"]
"#;

        let document = parse_str(source).expect("external process document should parse");
        let component = &document.components["fake_sensor"];
        assert_eq!(component.language, "external");
        assert_eq!(component.kind.as_deref(), Some("external"));
        assert_eq!(document.external_processes.len(), 1);
        let external = &document.external_processes[0];
        assert_eq!(external.process, "sensor_proc");
        assert_eq!(external.package, "fake_sensor_driver");
        assert_eq!(external.executable, "driver");
        assert_eq!(external.args, vec!["--rate", "50"]);
        assert_eq!(external.working_dir.as_deref(), Some("package"));
        assert_eq!(external.health.as_deref(), Some("runtime_socket"));
        assert_eq!(external.required_backends, vec!["zenoh"]);
    }

    #[test]
    fn parses_component_build_pkg_config_dependencies() {
        let document = parse_str(
            r#"
[package]
name = "cpp_sdk_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.camera]
language = "cpp"
kind = "io_boundary"
output = ["sample:Sample"]

[component.camera.build]
pkg_config = ["vendor_capture", "vendor_codec"]

[instance.camera]
component = "camera"

[instance.camera.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]
"#,
        )
        .unwrap();

        let camera = &document.components["camera"];
        assert_eq!(
            camera.build.pkg_config,
            vec!["vendor_capture", "vendor_codec"]
        );
    }

    #[test]
    fn parses_io_boundary_resource_descriptor_tables() {
        let document = parse_str(
            r#"
[package]
name = "io_boundary_demo"
rsdl_version = "0.1"

[type.FrameHandle]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"
format = "u32"
encoding = "u32"

[component.camera]
language = "rust"
kind = "io_boundary"
io_side_effect = ["device", "read"]
io_readiness = "resource_ready"
io_health = "runtime_reported"
io_shutdown = "cooperative"
output = ["frame:FrameHandle"]

[component.camera.resource.frames]
kind = "shm"
required = true

[component.camera.resource.frames.descriptor]
kind = "frame"
port = "frame"
format = "rgb8"
encoding = "row_major"
metadata = { width = "640", height = "480" }
record_payload = true

[instance.camera]
component = "camera"

[instance.camera.task]
trigger = "periodic"
period_ms = 33
output = ["frame"]
"#,
        )
        .unwrap();

        let camera = &document.components["camera"];
        assert_eq!(camera.kind.as_deref(), Some("io_boundary"));
        assert_eq!(camera.io_side_effect, vec!["device", "read"]);
        assert_eq!(camera.io_readiness.as_deref(), Some("resource_ready"));
        assert_eq!(camera.io_health.as_deref(), Some("runtime_reported"));
        assert_eq!(camera.io_shutdown.as_deref(), Some("cooperative"));
        assert_eq!(camera.resources.len(), 1);
        let resource = &camera.resources[0];
        assert_eq!(resource.name, "frames");
        assert_eq!(resource.kind, "shm");
        assert!(resource.required);
        let descriptor = resource.descriptor.as_ref().unwrap();
        assert_eq!(descriptor.kind, "frame");
        assert_eq!(descriptor.port.as_deref(), Some("frame"));
        assert_eq!(descriptor.format, "rgb8");
        assert_eq!(descriptor.encoding.as_deref(), Some("row_major"));
        assert_eq!(descriptor.metadata["width"], "640");
        assert!(descriptor.record_payload);
    }

    #[test]
    fn parses_scheduler_v2_task_fields() {
        let source = r#"
[package]
name = "scheduler_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
input = ["in:u32"]
output = ["out:u32"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "on_message"
readiness = "all_ready"
lane = "worker_serial"
priority = 7
input = ["in"]
output = ["out"]
"#;

        let document = parse_str(source).expect("document should parse");
        let task = &document.instances["worker"].tasks[0];

        assert_eq!(task.readiness.as_deref(), Some("all_ready"));
        assert_eq!(task.lane.as_deref(), Some("worker_serial"));
        assert_eq!(task.priority, Some(7));
    }

    #[test]
    fn parses_process_orchestration_tables() {
        let source = r#"
[package]
name = "process_demo"
rsdl_version = "0.1"

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
"#;

        let document = parse_str(source).expect("document should parse");

        assert_eq!(document.processes.len(), 2);
        assert_eq!(document.processes[0].name, "sensor_proc");
        assert_eq!(document.processes[0].depends_on, Vec::<String>::new());
        assert_eq!(document.processes[0].restart.as_deref(), Some("on_failure"));
        assert_eq!(document.processes[0].max_restarts, Some(5));
        assert_eq!(document.processes[0].initial_delay_ms, Some(50));
        assert_eq!(document.processes[0].max_delay_ms, Some(500));
        assert_eq!(document.processes[0].failure.as_deref(), Some("propagate"));
        assert_eq!(document.processes[1].name, "control_proc");
        assert_eq!(document.processes[1].depends_on, vec!["sensor_proc"]);
        assert_eq!(document.processes[1].restart.as_deref(), Some("never"));
        assert_eq!(document.processes[1].failure.as_deref(), Some("isolate"));
    }

    #[test]
    fn parses_process_orchestration_with_resource_hints() {
        let source = r#"
[package]
name = "process_resource_demo"
rsdl_version = "0.1"

[[process]]
name = "sensor_proc"
restart = "on_failure"
max_restarts = 5
initial_delay_ms = 50
max_delay_ms = 500
failure = "propagate"
readiness = "runtime_ready"
startup_delay_ms = 200
cpu_affinity = [0, 1]
nice = -5
rt_policy = "fifo"
rt_priority = 50

[[process]]
name = "control_proc"
depends_on = ["sensor_proc"]
restart = "never"
failure = "isolate"
readiness = "service_ready"
env = { FLOWRT_LOG_LEVEL = "debug", APP_MODE = "control" }

[[process]]
name = "idle_proc"
restart = "never"
readiness = "process_started"
"#;

        let document = parse_str(source).expect("document should parse");

        assert_eq!(document.processes.len(), 3);
        assert_eq!(document.processes[0].name, "sensor_proc");
        assert_eq!(
            document.processes[0].readiness.as_deref(),
            Some("runtime_ready")
        );
        assert_eq!(document.processes[0].startup_delay_ms, Some(200));
        assert_eq!(document.processes[0].cpu_affinity, vec![0, 1]);
        assert_eq!(document.processes[0].nice, Some(-5));
        assert_eq!(document.processes[0].rt_policy.as_deref(), Some("fifo"));
        assert_eq!(document.processes[0].rt_priority, Some(50));
        assert_eq!(document.processes[1].name, "control_proc");
        assert_eq!(
            document.processes[1].readiness.as_deref(),
            Some("service_ready")
        );
        assert_eq!(document.processes[1].env["FLOWRT_LOG_LEVEL"], "debug");
        assert_eq!(document.processes[1].env["APP_MODE"], "control");
        assert_eq!(document.processes[2].name, "idle_proc");
        assert_eq!(
            document.processes[2].readiness.as_deref(),
            Some("process_started")
        );
        assert!(document.processes[2].env.is_empty());
        assert!(document.processes[2].cpu_affinity.is_empty());
        assert_eq!(document.processes[2].nice, None);
        assert_eq!(document.processes[2].rt_policy, None);
        assert_eq!(document.processes[2].rt_priority, None);
    }

    #[test]
    fn parses_service_ports_and_binds() {
        let source = r#"
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

[[bind.service]]
client = "client.plan"
server = "server.plan"
"#;

        let document = parse_str(source).expect("document should parse");

        assert_eq!(document.components["client"].service_clients.len(), 1);
        assert_eq!(
            document.components["client"].service_clients[0].name,
            "plan"
        );
        assert_eq!(
            document.components["client"].service_clients[0].request,
            "PlanRequest"
        );
        assert_eq!(
            document.components["client"].service_clients[0].response,
            "PlanResponse"
        );
        assert_eq!(document.components["server"].service_servers.len(), 1);
        assert_eq!(document.service_binds.len(), 1);
        assert_eq!(document.service_binds[0].client, "client.plan");
        assert_eq!(document.service_binds[0].server, "server.plan");
    }

    #[test]
    fn parses_operation_ports_and_binds() {
        let source = r#"
[package]
name = "operation_demo"
rsdl_version = "0.1"

[type.PlanGoal]
target = "u32"

[type.PlanFeedback]
progress = "f32"

[type.PlanResult]
accepted = "bool"

[component.controller]
language = "rust"

[component.controller.operation_client.plan]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

[component.navigator]
language = "rust"

[component.navigator.operation_server.plan]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
backend = "auto"
timeout_ms = 30000
concurrency = "reject"
preempt = "cancel_running"
queue_depth = 8
max_in_flight = 1
feedback = "latest"
result_retention_ms = 60000
"#;

        let document = parse_str(source).expect("document should parse");

        assert_eq!(document.components["controller"].operation_clients.len(), 1);
        assert_eq!(
            document.components["controller"].operation_clients[0].name,
            "plan"
        );
        assert_eq!(
            document.components["controller"].operation_clients[0].goal,
            "PlanGoal"
        );
        assert_eq!(
            document.components["controller"].operation_clients[0].feedback,
            "PlanFeedback"
        );
        assert_eq!(
            document.components["controller"].operation_clients[0].result,
            "PlanResult"
        );
        assert_eq!(document.components["navigator"].operation_servers.len(), 1);
        assert_eq!(document.operation_binds.len(), 1);
        assert_eq!(document.operation_binds[0].client, "controller.plan");
        assert_eq!(document.operation_binds[0].server, "navigator.plan");
        assert_eq!(document.operation_binds[0].backend.as_deref(), Some("auto"));
        assert_eq!(document.operation_binds[0].timeout_ms, Some(30000));
        assert_eq!(
            document.operation_binds[0].concurrency.as_deref(),
            Some("reject")
        );
        assert_eq!(
            document.operation_binds[0].preempt.as_deref(),
            Some("cancel_running")
        );
        assert_eq!(document.operation_binds[0].queue_depth, Some(8));
        assert_eq!(document.operation_binds[0].max_in_flight, Some(1));
        assert_eq!(
            document.operation_binds[0].feedback.as_deref(),
            Some("latest")
        );
        assert_eq!(document.operation_binds[0].result_retention_ms, Some(60000));
    }

    #[test]
    fn rejects_unnamed_task_in_task_array() {
        let source = r#"
[package]
name = "multi_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["fast:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
trigger = "periodic"
period_ms = 5
output = ["fast"]
"#;

        let error = parse_str(source).expect_err("task array entries must be named");
        assert!(error.to_string().contains("missing required field `name`"));
    }

    #[test]
    fn rejects_invalid_port_descriptor() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.bad]
language = "rust"
input = ["odom"]
"#;

        let error = parse_str(source).expect_err("invalid port descriptor should fail");
        assert!(matches!(error, RsdlError::InvalidPortDescriptor { .. }));
    }

    #[test]
    fn rejects_unknown_top_level_sections() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[components.worker]
language = "rust"
"#;

        let error = parse_str(source).expect_err("unknown top-level section should fail");

        assert!(matches!(
            error,
            RsdlError::UnknownTopLevelSection { section } if section == "components"
        ));
    }

    #[test]
    fn rejects_unknown_fields_in_fixed_schema_tables() {
        let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
proces = "main"
"#;

        let error = parse_str(source).expect_err("unknown fixed-schema field should fail");

        assert!(matches!(
            error,
            RsdlError::UnknownField { context, field }
                if context == "instance.worker" && field == "proces"
        ));
    }

    #[test]
    fn parses_component_and_task_concurrency() {
        let source = r#"
[package]
name = "concurrency_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
concurrency = "parallel"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
concurrency = "parallel"
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 10
output = ["slow"]
"#;

        let document = parse_str(source).expect("document should parse");

        assert_eq!(
            document.components["worker"].concurrency.as_deref(),
            Some("parallel")
        );
        assert_eq!(
            document.instances["worker"].tasks[0].concurrency.as_deref(),
            Some("parallel")
        );
        assert_eq!(document.instances["worker"].tasks[1].concurrency, None);
    }

    #[test]
    fn rejects_invalid_component_concurrency() {
        let source = r#"
[package]
name = "bad_concurrency"
rsdl_version = "0.1"

[component.worker]
language = "rust"
concurrency = "shared"
"#;

        let error = parse_str(source).expect_err("invalid component concurrency should fail");
        assert!(error.to_string().contains("component concurrency"));
        assert!(error.to_string().contains("exclusive"));
        assert!(error.to_string().contains("parallel"));
    }

    #[test]
    fn rejects_invalid_task_concurrency() {
        let source = r#"
[package]
name = "bad_concurrency"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["sample:u32"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
concurrency = "shared"
output = ["sample"]
"#;

        let error = parse_str(source).expect_err("invalid task concurrency should fail");
        assert!(error.to_string().contains("task concurrency"));
        assert!(error.to_string().contains("exclusive"));
        assert!(error.to_string().contains("parallel"));
    }

    #[test]
    fn parse_file_expands_package_imports() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("types")).unwrap();
        std::fs::create_dir_all(root.join("components")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/*.rsdl"]
components = ["components/estimator.rsdl"]

[instance.estimator]
component = "estimator"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("types").join("imu.rsdl"),
            r#"
[type.Imu]
timestamp = "u64"
ax = "f32"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("estimator.rsdl"),
            r#"
[component.estimator]
language = "rust"
input = ["imu:Imu"]
"#,
        )
        .unwrap();

        let document = parse_file(root.join("robot.rsdl")).unwrap();

        assert_eq!(document.package.name, "robot_demo");
        assert_eq!(document.package.imports["types"], vec!["types/*.rsdl"]);
        assert_eq!(document.types["Imu"].fields.len(), 2);
        assert_eq!(document.components["estimator"].input[0].ty, "Imu");
        assert_eq!(document.instances["estimator"].component, "estimator");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_expands_graph_fragment_imports() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("types")).unwrap();
        std::fs::create_dir_all(root.join("components")).unwrap();
        std::fs::create_dir_all(root.join("graphs")).unwrap();
        std::fs::create_dir_all(root.join("profiles")).unwrap();
        std::fs::create_dir_all(root.join("targets")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/*.rsdl"]
components = ["components/*.rsdl"]
graphs = ["graphs/*.rsdl"]
profiles = ["profiles/*.rsdl"]
targets = ["targets/*.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("types").join("messages.rsdl"),
            r#"
[type.Imu]
timestamp = "u64"

[type.Odom]
timestamp = "u64"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("imu_sim.rsdl"),
            r#"
[component.imu_sim]
language = "rust"
output = ["imu:Imu"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("estimator.rsdl"),
            r#"
[component.estimator]
language = "rust"
input = ["imu:Imu"]
output = ["odom:Odom"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("graphs").join("default.rsdl"),
            r#"
[instance.imu_sim]
component = "imu_sim"
process = "main"
target = "linux"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.estimator]
component = "estimator"
process = "main"
target = "linux"

[instance.estimator.task]
trigger = "on_message"
input = ["imu"]
output = ["odom"]
deadline_ms = 10

[[bind.dataflow]]
from = "imu_sim.imu"
to = "estimator.imu"
channel = "latest"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("profiles").join("default.rsdl"),
            r#"
[profile.default]
backend = "inproc"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("targets").join("linux.rsdl"),
            r#"
[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
        )
        .unwrap();

        let document = parse_file(root.join("robot.rsdl")).unwrap();

        assert_eq!(document.package.name, "robot_demo");
        assert_eq!(document.types["Imu"].fields[0].name, "timestamp");
        assert_eq!(document.components["imu_sim"].output[0].name, "imu");
        assert_eq!(document.instances["imu_sim"].component, "imu_sim");
        assert_eq!(
            document.instances["estimator"]
                .tasks
                .first()
                .unwrap()
                .trigger,
            "on_message"
        );
        assert_eq!(document.binds[0].from, "imu_sim.imu");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_rejects_import_patterns_without_matches() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/*.rsdl"]
"#,
        )
        .unwrap();

        let error = parse_file(root.join("robot.rsdl")).expect_err("missing import should fail");
        assert!(matches!(error, RsdlError::ImportPatternNoMatches { .. }));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_rejects_absolute_import_paths() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["/tmp/flowrt/secret.rsdl"]
"#,
        )
        .unwrap();

        let error =
            parse_file(root.join("robot.rsdl")).expect_err("absolute import path should fail");

        assert!(matches!(
            error,
            RsdlError::InvalidImportPath { pattern, .. }
                if pattern == "/tmp/flowrt/secret.rsdl"
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_rejects_parent_directory_import_paths() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["../shared/types.rsdl"]
"#,
        )
        .unwrap();

        let error =
            parse_file(root.join("robot.rsdl")).expect_err("parent import path should fail");

        assert!(matches!(
            error,
            RsdlError::InvalidImportPath { pattern, .. }
                if pattern == "../shared/types.rsdl"
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_file_expands_nested_imports_and_records_loaded_sources() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("components").join("common")).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
components = ["components/source.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("source.rsdl"),
            r#"
[package]
name = "source_fragment"
rsdl_version = "0.1"

[package.imports]
types = ["common/*.rsdl"]

[component.source]
language = "rust"
output = ["sample:Sample"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("components").join("common").join("sample.rsdl"),
            r#"
[type.Sample]
value = "u32"
"#,
        )
        .unwrap();

        let loaded = load_file(root.join("robot.rsdl")).unwrap();
        let source_paths = loaded
            .sources
            .iter()
            .map(|source| source.path.as_path())
            .collect::<Vec<_>>();

        assert!(loaded.document.types.contains_key("Sample"));
        assert_eq!(loaded.document.components["source"].output[0].ty, "Sample");
        assert_eq!(
            source_paths,
            vec![
                Path::new("robot.rsdl"),
                Path::new("components/source.rsdl"),
                Path::new("components/common/sample.rsdl"),
            ]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parse_file_rejects_duplicate_imported_symbols() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("types")).unwrap();
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/*.rsdl"]

[type.Imu]
timestamp = "u64"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("types").join("imu.rsdl"),
            r#"
[type.Imu]
timestamp = "u64"
"#,
        )
        .unwrap();

        let error = parse_file(root.join("robot.rsdl")).expect_err("duplicate type should fail");
        assert!(matches!(
            error,
            RsdlError::DuplicateSymbol { kind: "type", .. }
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_ros2_bridge_tables() {
        let document = parse_str(
            r#"
[package]
name = "ros2_bridge_demo"
rsdl_version = "0.1"

[type.TextFrame]
data = "string"

[component.source]
language = "rust"
output = ["text:TextFrame"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 10
output = ["text"]

[[bridge.ros2]]
flowrt = "source.text"
ros2_topic = "/flowrt/text"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"
field = "data"

[profile.default]
backend = "zenoh"
"#,
        )
        .unwrap();

        assert_eq!(document.ros2_bridges.len(), 1);
        assert_eq!(document.ros2_bridges[0].flowrt, "source.text");
        assert_eq!(document.ros2_bridges[0].ros2_topic, "/flowrt/text");
        assert_eq!(document.ros2_bridges[0].ros2_type, "std_msgs/msg/String");
        assert_eq!(document.ros2_bridges[0].direction, "flowrt_to_ros2");
        assert_eq!(document.ros2_bridges[0].field.as_deref(), Some("data"));
    }

    #[test]
    fn parses_ros2_bridge_bidirectional_typed_tables() {
        let document = parse_str(
            r#"
[package]
name = "ros2_bridge_demo"
rsdl_version = "0.1"

[type.Pose]
position = "Point3"
orientation = "Quaternion"

[type.Point3]
x = "f64"
y = "f64"
z = "f64"

[type.Quaternion]
x = "f64"
y = "f64"
z = "f64"
w = "f64"

[component.source]
language = "rust"
output = ["pose:Pose"]

[component.sink]
language = "rust"
input = ["pose:Pose"]

[instance.source]
component = "source"

[instance.sink]
component = "sink"

[[bridge.ros2]]
flowrt = "source.pose"
ros2_topic = "/flowrt/pose"
ros2_type = "geometry_msgs/msg/Pose"
direction = "flowrt_to_ros2"

[[bridge.ros2]]
flowrt = "sink.pose"
ros2_topic = "/ros2/pose"
ros2_type = "geometry_msgs/msg/Pose"
direction = "ros2_to_flowrt"
"#,
        )
        .unwrap();

        assert_eq!(document.ros2_bridges.len(), 2);
        assert_eq!(document.ros2_bridges[0].direction, "flowrt_to_ros2");
        assert_eq!(document.ros2_bridges[0].field, None);
        assert_eq!(document.ros2_bridges[1].direction, "ros2_to_flowrt");
        assert_eq!(document.ros2_bridges[1].field, None);
        assert_eq!(document.ros2_bridges[1].ros2_type, "geometry_msgs/msg/Pose");
    }

    #[test]
    fn load_file_expands_workspace_boundary_endpoints() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("composition")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "workspace_boundary_demo"
rsdl_version = "0.1"

[workspace]
compositions = ["composition/default.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("composition").join("default.rsdl"),
            r#"
[type.Scan]
range = "f32"

[type.Command]
speed = "f32"

[component.planner]
language = "rust"
input = ["scan:Scan"]
output = ["cmd:Command"]

[instance.planner]
component = "planner"

[profile.dev]
mode = "island"

[[boundary.input]]
name = "scan_in"
port = "planner.scan"
type = "Scan"

[[boundary.output]]
name = "cmd_out"
port = "planner.cmd"
type = "Command"
"#,
        )
        .unwrap();

        let loaded = load_file(root.join("robot.rsdl")).expect("workspace should load");
        assert_eq!(loaded.document.boundary_inputs.len(), 1);
        assert_eq!(loaded.document.boundary_inputs[0].name, "scan_in");
        assert_eq!(loaded.document.boundary_outputs.len(), 1);
        assert_eq!(loaded.document.profiles["dev"].mode, RawGraphMode::Island);
        assert_eq!(loaded.compositions[0].boundary_inputs[0].name, "scan_in");
    }

    #[test]
    fn load_file_expands_workspace_modules_and_compositions() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("modules")).unwrap();
        std::fs::create_dir_all(root.join("composition")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "workspace_demo"
rsdl_version = "0.1"

[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules").join("perception.rsdl"),
            r#"
[module]
name = "perception"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.imu_sim]
language = "rust"
output = ["imu:Imu"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules").join("control.rsdl"),
            r#"
[module]
name = "control"

[type.Odom]
timestamp = "u64"

[component.estimator]
language = "rust"
input = ["imu:perception::Imu"]
output = ["odom:Odom"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("composition").join("default.rsdl"),
            r#"
[instance.imu_sim]
component = "perception::imu_sim"
process = "main"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.estimator]
component = "control::estimator"
process = "main"

[instance.estimator.task]
trigger = "on_message"
input = ["imu"]
output = ["odom"]

[[bind.dataflow]]
from = "imu_sim.imu"
to = "estimator.imu"
channel = "latest"
"#,
        )
        .unwrap();

        let loaded = load_file(root.join("robot.rsdl")).unwrap();

        assert_eq!(
            loaded.document.workspace.as_ref().unwrap().modules,
            vec!["modules/*.rsdl"]
        );
        assert_eq!(loaded.modules.len(), 2);
        assert_eq!(loaded.modules[0].module.name, "control");
        assert_eq!(loaded.modules[1].module.name, "perception");
        assert_eq!(loaded.modules[1].types["Imu"].fields[0].name, "timestamp");
        assert_eq!(
            loaded.modules[0].components["estimator"].input[0].ty,
            "perception::Imu"
        );
        assert_eq!(loaded.compositions.len(), 1);
        assert_eq!(
            loaded.document.instances["estimator"].component,
            "control::estimator"
        );
        assert_eq!(loaded.document.binds[0].from, "imu_sim.imu");

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn load_file_rejects_instance_inside_workspace_module() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("modules")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "workspace_demo"
rsdl_version = "0.1"

[workspace]
modules = ["modules/perception.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules").join("perception.rsdl"),
            r#"
[module]
name = "perception"

[component.imu_sim]
language = "rust"

[instance.imu_sim]
component = "imu_sim"
"#,
        )
        .unwrap();

        let error = load_file(root.join("robot.rsdl")).expect_err("module instance should fail");

        assert!(matches!(
            error,
            RsdlError::InvalidModuleSection { module, section, .. }
                if module == "perception" && section == "instance"
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    fn unique_temp_dir() -> std::path::PathBuf {
        let suffix = format!(
            "flowrt-rsdl-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(suffix)
    }
}
