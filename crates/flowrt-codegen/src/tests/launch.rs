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
fn launch_manifest_exposes_process_resource_hints() {
    let ir = contract_from_source(
        r#"
[package]
name = "resource_demo"
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
env = { APP_MODE = "control" }

[profile.default]
backend = "iox2"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let processes = launch["graphs"][0]["processes"].as_array().unwrap();
    let sensors = processes
        .iter()
        .find(|process| process["name"] == "sensor_proc")
        .unwrap();
    let control = processes
        .iter()
        .find(|process| process["name"] == "control_proc")
        .unwrap();

    assert_eq!(sensors["readiness"], "runtime_ready");
    assert_eq!(sensors["startup_delay_ms"], 200);
    assert_eq!(
        sensors["resource_placement"]["cpu_affinity"],
        serde_json::json!([0, 1])
    );
    assert_eq!(sensors["resource_placement"]["nice"], -5);
    assert_eq!(sensors["resource_placement"]["rt_policy"], "fifo");
    assert_eq!(sensors["resource_placement"]["rt_priority"], 50);
    assert_eq!(sensors["env"], serde_json::json!({}));

    assert_eq!(control["readiness"], "service_ready");
    assert_eq!(control["startup_delay_ms"], 0);
    assert_eq!(
        control["resource_placement"]["cpu_affinity"],
        serde_json::json!([])
    );
    assert_eq!(
        control["resource_placement"]["nice"],
        serde_json::Value::Null
    );
    assert_eq!(
        control["resource_placement"]["rt_policy"],
        serde_json::Value::Null
    );
    assert_eq!(
        control["resource_placement"]["rt_priority"],
        serde_json::Value::Null
    );
    assert_eq!(control["env"], serde_json::json!({ "APP_MODE": "control" }));
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
    assert_eq!(services[0]["name"], "client.plan");
    assert_eq!(services[0]["client"], "client.plan");
    assert_eq!(services[0]["client_instance"], "client");
    assert_eq!(services[0]["client_port"], "plan");
    assert_eq!(services[0]["server"], "server.plan");
    assert_eq!(services[0]["server_instance"], "server");
    assert_eq!(services[0]["server_port"], "plan");
    assert_eq!(services[0]["request"], "PlanRequest");
    assert_eq!(services[0]["response"], "PlanResponse");
    assert_eq!(services[0]["backend"], "inproc");
    assert_eq!(services[0]["timeout_ms"], 5000);
    assert_eq!(services[0]["queue_depth"], 32);
    assert_eq!(services[0]["overflow"], "busy");
    assert!(services[0]["lane"].is_null());
    assert_eq!(services[0]["max_in_flight"], 64);
}

#[test]
fn launch_manifest_rejects_service_type_mismatch_in_release_path() {
    let mut ir = contract_from_source(
        r#"
[package]
name = "service_demo"
rsdl_version = "0.1"

[type.PlanRequest]
goal_id = "u32"

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
    let server = ir
        .components
        .iter_mut()
        .find(|component| component.name == "server")
        .unwrap();
    server.service_servers[0].response = flowrt_ir::TypeExpr::Named {
        name: "PlanRequest".to_string(),
    };

    let error = crate::launch_manifest::emit_launch_manifest(&ir).unwrap_err();

    assert!(error.to_string().contains("response type mismatch"));
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
fn launch_manifest_marks_c_process_runtime_kind() {
    let ir = contract_from_source(
        r#"
[package]
name = "c_demo"
rsdl_version = "0.1"

[component.source]
language = "c"
output = ["value:u32"]

[instance.source]
component = "source"
process = "main"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["c"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let process = &launch["graphs"][0]["processes"][0];

    assert_eq!(process["name"], "main");
    assert_eq!(process["runtimes"], serde_json::json!(["c"]));
    assert_eq!(process["runtime_kind"], "c");
}

#[test]
fn launch_manifest_exposes_external_process_package_metadata() {
    let ir = contract_from_source(
        r#"
[package]
name = "external_demo"
rsdl_version = "0.1"

[type.Frame]
seq = "u32"

[component.camera]
language = "external"
kind = "external"
output = ["frame:Frame"]

[component.viewer]
language = "rust"
input = ["frame:Frame"]

[instance.camera]
component = "camera"
process = "camera_proc"
target = "edge"

[instance.viewer]
component = "viewer"
process = "viewer_proc"
target = "edge"

[instance.viewer.task]
trigger = "on_message"
input = ["frame"]

[[bind.dataflow]]
from = "camera.frame"
to = "viewer.frame"
channel = "latest"

[[external_process]]
process = "camera_proc"
package = "camera_driver"
executable = "camera-node"
args = ["--device", "/dev/video0"]
working_dir = "workspace"
health = "runtime_socket"
required_backends = ["zenoh"]

[profile.default]
backend = "zenoh"

[target.edge]
runtime = ["external", "rust"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let processes = launch["graphs"][0]["processes"].as_array().unwrap();
    let camera = processes
        .iter()
        .find(|process| process["name"] == "camera_proc")
        .unwrap();
    let channel = &launch["graphs"][0]["channels"][0];

    assert_eq!(camera["runtime_kind"], "external");
    assert_eq!(camera["runtimes"], serde_json::json!(["external"]));
    assert_eq!(camera["backend"], "zenoh");
    assert_eq!(
        camera["external"],
        serde_json::json!({
            "package": "camera_driver",
            "executable": "camera-node",
            "args": ["--device", "/dev/video0"],
            "working_dir": "workspace",
            "health": "runtime_socket",
            "required_backends": ["zenoh"]
        })
    );
    assert_eq!(channel["backend"], "zenoh");
    assert!(
        channel["key_expr"]
            .as_str()
            .unwrap()
            .contains("camera_frame")
    );

    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    assert_eq!(
        selfdesc["graphs"][0]["external_processes"][0],
        serde_json::json!({
            "process": "camera_proc",
            "package": "camera_driver",
            "executable": "camera-node",
            "args": ["--device", "/dev/video0"],
            "working_dir": "workspace",
            "health": "runtime_socket",
            "required_backends": ["zenoh"]
        })
    );
    assert_eq!(
        selfdesc["component_types"][0]["kind"],
        serde_json::json!("external")
    );
}

#[test]
fn launch_manifest_and_selfdesc_expose_resource_contract() {
    let ir = contract_from_source(
        r#"
[package]
name = "io_boundary_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.sensor]
language = "cpp"
kind = "io_boundary"
output = ["sample:Sample"]
io_side_effect = ["read"]
io_readiness = "resource_ready"
io_health = "runtime_reported"
io_shutdown = "cooperative"

[component.sensor.resource.lidar_uart]
capability = "perception.lidar.samples"
required = true

[[resource.provider]]
name = "lidar_provider"
capabilities = ["perception.lidar.samples"]
scope = "process"
process = "sensor_proc"
health_source = "provider_health"
readiness_source = "provider_ready"

[instance.sensor]
component = "sensor"
process = "sensor_proc"
target = "edge"

[instance.sensor.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]

[profile.default]
backend = "inproc"

[target.edge]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let process = &launch["graphs"][0]["processes"][0];
    let resource_contract = &launch["graphs"][0]["resource_contract"];

    assert_eq!(resource_contract["resource_contract_version"], "0.1");
    assert_eq!(
        resource_contract["providers"][0],
        serde_json::json!({
            "name": "lidar_provider",
            "scope": "process",
            "capabilities": ["perception.lidar.samples"],
            "target": null,
            "process": "sensor_proc",
            "external_package": null,
            "readiness_source": "provider_ready",
            "health_source": "provider_health"
        })
    );
    assert_eq!(
        resource_contract["requirements"][0],
        serde_json::json!({
            "instance": "sensor",
            "component": "sensor",
            "name": "lidar_uart",
            "capability": "perception.lidar.samples",
            "access": "read_write",
            "required": true,
            "readiness": "before_start",
            "health": "required",
            "on_failure": "stop_process",
            "satisfaction": "satisfied",
            "provider": "lidar_provider",
            "diagnostic": null
        })
    );
    assert_eq!(
        resource_contract["satisfactions"][0],
        serde_json::json!({
            "instance": "sensor",
            "component": "sensor",
            "resource": "lidar_uart",
            "capability": "perception.lidar.samples",
            "access": "read_write",
            "required": true,
            "readiness": "before_start",
            "health": "required",
            "on_failure": "stop_process",
            "status": "satisfied",
            "satisfied": true,
            "provider": "lidar_provider",
            "diagnostic": null
        })
    );

    assert_eq!(
        process["io_boundaries"][0],
        serde_json::json!({
            "instance": "sensor",
            "component": "sensor",
            "side_effects": ["read"],
            "readiness": "resource_ready",
            "health": "runtime_reported",
            "shutdown": "cooperative",
            "resources": [{
                "name": "lidar_uart",
                "capability": "perception.lidar.samples",
                "access": "read_write",
                "required": true,
                "readiness": "before_start",
                "health": "required",
                "on_failure": "stop_process"
            }]
        })
    );

    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    assert_eq!(
        selfdesc["component_types"][0]["kind"],
        serde_json::json!("io_boundary")
    );
    assert_eq!(
        selfdesc["component_types"][0]["resources"][0],
        serde_json::json!({
            "name": "lidar_uart",
            "capability": "perception.lidar.samples",
            "access": "read_write",
            "required": true,
            "readiness": "before_start",
            "health": "required",
            "on_failure": "stop_process"
        })
    );
    assert_eq!(
        selfdesc["graphs"][0]["resource_contract"],
        *resource_contract
    );
}

#[test]
fn internal_resource_contract_projection_uses_rederived_facts() {
    let mut ir = contract_from_source(
        r#"
[package]
name = "resource_facts_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.sensor]
language = "rust"
kind = "io_boundary"
output = ["sample:Sample"]

[component.sensor.resource.lidar]
capability = "perception.lidar.samples"
required = true

[[resource.provider]]
name = "lidar_provider"
capabilities = ["perception.lidar.samples"]
scope = "process"
process = "main"
health_source = "provider_health"
readiness_source = "provider_ready"

[instance.sensor]
component = "sensor"

[instance.sensor.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]
"#,
    );
    ir.graphs[0].resource_satisfactions.clear();

    let launch: serde_json::Value =
        serde_json::from_str(&crate::launch_manifest::emit_launch_manifest(&ir).unwrap()).unwrap();
    let launch_resource_contract = &launch["graphs"][0]["resource_contract"];
    assert_eq!(
        launch_resource_contract["requirements"][0]["capability"],
        "perception.lidar.samples"
    );
    assert_eq!(
        launch_resource_contract["requirements"][0]["satisfaction"],
        "satisfied"
    );
    assert_eq!(
        launch_resource_contract["satisfactions"][0]["provider"],
        "lidar_provider"
    );

    let selfdesc: serde_json::Value =
        serde_json::from_str(&crate::selfdesc::emit_self_description(&ir).unwrap()).unwrap();
    let selfdesc_resource_contract = &selfdesc["graphs"][0]["resource_contract"];
    assert_eq!(
        selfdesc_resource_contract["requirements"][0]["capability"],
        "perception.lidar.samples"
    );
    assert_eq!(
        selfdesc_resource_contract["requirements"][0]["satisfaction"],
        "satisfied"
    );
    assert_eq!(
        selfdesc_resource_contract["satisfactions"][0]["provider"],
        "lidar_provider"
    );
}

#[test]
fn launch_manifest_and_selfdesc_expose_island_boundary_contract() {
    let ir = contract_from_source(
        r#"
[package]
name = "island_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[component.producer]
language = "rust"
output = ["sample:Sample"]

[instance.consumer]
component = "consumer"

[instance.producer]
component = "producer"

[profile.dev]
mode = "island"

[[boundary.input]]
name = "sample_in"
port = "consumer.sample"
type = "Sample"

[[boundary.output]]
name = "sample_out"
port = "producer.sample"
type = "Sample"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();

    assert_eq!(launch["profile_modes"][0]["name"], "dev");
    assert_eq!(launch["profile_modes"][0]["mode"], "island");
    assert_eq!(
        launch["graphs"][0]["boundary_endpoints"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        launch["graphs"][0]["boundary_endpoints"][0]["name"],
        "sample_in"
    );
    assert_eq!(
        launch["graphs"][0]["boundary_endpoints"][0]["direction"],
        "input"
    );
    assert_eq!(
        launch["graphs"][0]["boundary_endpoints"][0]["endpoint"],
        "consumer.sample"
    );
    assert_eq!(
        launch["graphs"][0]["boundary_endpoints"][0]["message_type"],
        "Sample"
    );

    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert_eq!(launch["artifact"]["clock"]["source"], "realtime");
    assert_eq!(selfdesc["artifact"]["clock"]["source"], "realtime");
    assert!(rust_shell.contains("let clock_source = \"realtime\";"));
    assert!(!rust_shell.contains("let clock_source = \"simulated_replay\";"));
    assert_eq!(selfdesc["profiles"][0]["mode"], "island");
    assert_eq!(
        selfdesc["graphs"][0]["boundary_endpoints"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        selfdesc["graphs"][0]["boundary_endpoints"][1]["name"],
        "sample_out"
    );
    assert_eq!(
        selfdesc["graphs"][0]["boundary_endpoints"][1]["direction"],
        "output"
    );
    assert_eq!(
        selfdesc["graphs"][0]["boundary_endpoints"][1]["endpoint"],
        "producer.sample"
    );
    assert_eq!(
        selfdesc["graphs"][0]["boundary_endpoints"][1]["message_type"],
        "Sample"
    );
}

#[test]
fn launch_manifest_and_selfdesc_expose_temporary_island_artifact_metadata() {
    let strict = contract_from_source(
        r#"
[package]
name = "temporary_island_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[profile.default]
backend = "inproc"
"#,
    );
    let projected = flowrt_ir::project_contract_to_profile(&strict, None).unwrap();
    let island = flowrt_ir::apply_temporary_island_overlay(
        &projected,
        &flowrt_ir::TemporaryIslandOverlay {
            boundary_inputs: vec![flowrt_ir::TemporaryBoundaryMapping {
                name: "sample_in".to_string(),
                endpoint: "consumer.sample".to_string(),
            }],
            boundary_outputs: vec![],
            generated_by: Default::default(),
        },
    )
    .unwrap();
    let bundle = emit_artifacts(&island).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert_eq!(launch["artifact"]["mode"], "island");
    assert_eq!(launch["artifact"]["temporary_island"], true);
    assert_eq!(launch["artifact"]["test_only"], true);
    assert_eq!(
        launch["artifact"]["temporary_overlay"]["kind"],
        "temporary_island"
    );
    assert_eq!(
        launch["artifact"]["temporary_overlay"]["original_profile_mode"],
        "strict"
    );
    assert_eq!(
        launch["artifact"]["temporary_overlay"]["generated_by"]["command"],
        "flowrt prepare"
    );
    assert_eq!(
        launch["artifact"]["temporary_overlay"]["boundary_mappings"][0]["source"],
        "--boundary-input"
    );
    assert_eq!(launch["artifact"]["clock"]["source"], "simulated_replay");
    assert_eq!(launch["artifact"]["clock"]["unit"], "ms");
    assert_eq!(launch["artifact"]["clock"]["field"], "tick_time_ms");
    assert_eq!(selfdesc["artifact"]["mode"], "island");
    assert_eq!(selfdesc["artifact"]["temporary_island"], true);
    assert_eq!(selfdesc["artifact"]["test_only"], true);
    assert_eq!(
        selfdesc["artifact"]["temporary_overlay"]["original_profile_mode"],
        "strict"
    );
    assert_eq!(selfdesc["artifact"]["clock"]["source"], "simulated_replay");
    assert_eq!(selfdesc["artifact"]["clock"]["unit"], "ms");
    assert_eq!(selfdesc["artifact"]["clock"]["field"], "tick_time_ms");
    assert!(rust_shell.contains("let clock_source = \"simulated_replay\";"));
    assert!(!rust_shell.contains("let scheduler_started_at = std::time::Instant::now();"));
    assert!(
        rust_shell.contains("if let Some(data_time_ms) = scheduler_events.take_data_time_ms()")
    );
    assert!(rust_shell.contains("scheduler_now_ms = scheduler_now_ms.max(data_time_ms);"));
    // T3：simulated_replay 调度逻辑时钟由注入事件驱动，唤醒只等下一个数据事件或关停，
    // 不计算 wall-clock deadline、不被节拍绑死，回放结果只取决于事件序列。
    assert!(
        rust_shell.contains(
            "scheduler_events.wait_until_after(observed_data_generation, None, &shutdown)"
        )
    );
    assert!(!rust_shell.contains("let next_wake_deadline ="));
}

#[test]
fn cpp_simulated_replay_shell_drops_wall_clock_wake() {
    let strict = contract_from_source(
        r#"
[package]
name = "cpp_temporary_island_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "cpp"
input = ["sample:Sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[profile.default]
backend = "inproc"
"#,
    );
    let projected = flowrt_ir::project_contract_to_profile(&strict, None).unwrap();
    let island = flowrt_ir::apply_temporary_island_overlay(
        &projected,
        &flowrt_ir::TemporaryIslandOverlay {
            boundary_inputs: vec![flowrt_ir::TemporaryBoundaryMapping {
                name: "sample_in".to_string(),
                endpoint: "consumer.sample".to_string(),
            }],
            boundary_outputs: vec![],
            generated_by: Default::default(),
        },
    )
    .unwrap();
    let bundle = emit_artifacts(&island).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    // simulated_replay 的 C++ 调度同样不读 wall-clock：唤醒传 std::nullopt deadline，
    // 不计算 steady_clock 节拍，逻辑时钟由注入事件 data_time 推进。
    assert!(
        cpp_shell.contains("const auto clock_source = std::string_view{\"simulated_replay\"};")
    );
    assert!(cpp_shell.contains(
        "scheduler_events.wait_until_after(observed_data_generation, std::nullopt, shutdown)"
    ));
    assert!(!cpp_shell.contains("const auto next_wake_deadline ="));
    assert!(cpp_shell.contains("scheduler_now_ms = std::max(scheduler_now_ms, *data_time_ms);"));
}

#[test]
fn rust_and_cpp_shells_register_resource_surface_and_wire_io_boundary_contexts() {
    let rust_ir = contract_from_source(
        r#"
[package]
name = "rust_io_boundary_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.sensor]
language = "rust"
kind = "io_boundary"
output = ["sample:Sample"]
io_side_effect = ["read"]
io_readiness = "component_started"
io_health = "runtime_reported"
io_shutdown = "cooperative"

[component.sensor.resource.lidar_uart]
capability = "perception.lidar.samples"
required = true

[[resource.provider]]
name = "lidar_provider"
capabilities = ["perception.lidar.samples"]
scope = "process"
process = "main"
health_source = "provider_health"
readiness_source = "provider_ready"

[instance.sensor]
component = "sensor"

[instance.sensor.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]
"#,
    );
    let rust_bundle = emit_artifacts(&rust_ir).unwrap();
    let rust_shell = artifact_content(&rust_bundle, "rust/src/runtime_shell.rs");
    assert!(
        rust_shell
            .contains("introspection_state.register_resource(flowrt::IntrospectionResourceStatus")
    );
    assert!(rust_shell.contains("name: \"sensor.lidar_uart\".to_string()"));
    assert!(rust_shell.contains("state: \"unknown\".to_string()"));
    assert!(rust_shell.contains("owner_process: Some(\"main\".to_string())"));
    assert!(
        rust_shell
            .contains("introspection_state.register_io_boundary(\"sensor\", \"sensor\", vec![")
    );
    assert!(rust_shell.contains("name: \"lidar_uart\".to_string()"));
    assert!(rust_shell.contains("kind: \"perception.lidar.samples\".to_string()"));
    assert!(rust_shell.contains("let mut sensor_boundary_context = flowrt::Context::for_boundary(flowrt::BoundaryContext::new(\"sensor\", \"sensor\", introspection_state.clone()));"));
    assert!(rust_shell.contains("boundary.mark_ready();"));

    let cpp_ir = contract_from_source(
        r#"
[package]
name = "cpp_io_boundary_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.sensor]
language = "cpp"
kind = "io_boundary"
output = ["sample:Sample"]
io_side_effect = ["read"]
io_readiness = "component_started"
io_health = "runtime_reported"
io_shutdown = "cooperative"

[component.sensor.resource.lidar_uart]
capability = "perception.lidar.samples"
required = true

[[resource.provider]]
name = "lidar_provider"
capabilities = ["perception.lidar.samples"]
scope = "process"
process = "main"
health_source = "provider_health"
readiness_source = "provider_ready"

[instance.sensor]
component = "sensor"

[instance.sensor.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]
"#,
    );
    let cpp_bundle = emit_artifacts(&cpp_ir).unwrap();
    let cpp_shell = artifact_content(&cpp_bundle, "cpp/src/runtime_shell.cpp");
    assert!(
        cpp_shell
            .contains("introspection_state.register_resource(flowrt::IntrospectionResourceStatus{")
    );
    assert!(cpp_shell.contains(".name = \"sensor.lidar_uart\""));
    assert!(cpp_shell.contains(".state = \"unknown\""));
    assert!(cpp_shell.contains(".owner_process = std::optional<std::string>{\"main\"}"));
    assert!(cpp_shell.contains("introspection_state.register_io_boundary(\"sensor\", \"sensor\", std::vector<flowrt::BoundaryResourceStatus>{"));
    assert!(
        cpp_shell
            .contains("flowrt::BoundaryResourceStatus{.name = \"lidar_uart\", .kind = \"perception.lidar.samples\"}")
    );
    assert!(cpp_shell.contains("auto sensor_boundary_context = flowrt::Context::for_boundary(flowrt::BoundaryContext{\"sensor\", \"sensor\""));
    assert!(
        cpp_shell.contains("introspection_state.record_io_boundary_health(std::move(status));")
    );
    assert!(cpp_shell.contains(
        "introspection_state.record_frame_descriptor_event(name, descriptor, status, payload_recording);"
    ));
    assert!(cpp_shell.contains(
        "return flowrt::BoundaryRecordOutcome{.recorded = record.recorded, .dropped = record.dropped};"
    ));
    assert!(cpp_shell.contains("boundary->mark_ready();"));
}

#[test]
fn launch_manifest_uses_external_required_backend_without_routes() {
    let ir = contract_from_source(
        r#"
[package]
name = "external_only_demo"
rsdl_version = "0.1"

[component.sensor]
language = "external"
kind = "external"
output = ["value:u32"]

[instance.sensor]
component = "sensor"
process = "sensor_proc"
target = "edge"

[[external_process]]
process = "sensor_proc"
package = "sensor_driver"
executable = "bin/driver"
required_backends = ["zenoh"]

[profile.default]
backend = "zenoh"

[target.edge]
runtime = ["external"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let process = &launch["graphs"][0]["processes"][0];

    assert_eq!(process["runtime_kind"], "external");
    assert_eq!(process["backend"], "zenoh");
    assert_eq!(
        process["external"]["required_backends"],
        serde_json::json!(["zenoh"])
    );
}

#[test]
fn launch_manifest_marks_service_zenoh_processes_without_dataflow_routes() {
    let ir = contract_from_source(
        r#"
[package]
name = "service_backend_demo"
rsdl_version = "0.1"

[component.client]
language = "external"
kind = "external"
service_client = ["plan:u32->bool"]

[component.server]
language = "external"
kind = "external"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"
process = "client_proc"
target = "linux"

[instance.server]
component = "server"
process = "server_proc"
target = "linux"

[[bind.service]]
client = "client.plan"
server = "server.plan"

[[external_process]]
process = "client_proc"
package = "client_pkg"
executable = "client-node"
required_backends = ["zenoh"]

[[external_process]]
process = "server_proc"
package = "server_pkg"
executable = "server-node"
required_backends = ["zenoh"]

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["external"]
backends = ["inproc", "zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let processes = launch["graphs"][0]["processes"].as_array().unwrap();

    assert!(
        processes
            .iter()
            .all(|process| process["backend"] == "zenoh")
    );
}

#[test]
fn launch_manifest_marks_operation_zenoh_processes_without_dataflow_routes() {
    let ir = contract_from_source(
        r#"
[package]
name = "operation_backend_demo"
rsdl_version = "0.1"

[component.client]
language = "external"
kind = "external"

[component.client.operation_client.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[component.server]
language = "external"
kind = "external"

[component.server.operation_server.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[instance.client]
component = "client"
process = "client_proc"
target = "linux"

[instance.server]
component = "server"
process = "server_proc"
target = "linux"

[[bind.operation]]
client = "client.plan"
server = "server.plan"

[[external_process]]
process = "client_proc"
package = "client_pkg"
executable = "client-node"
required_backends = ["zenoh"]

[[external_process]]
process = "server_proc"
package = "server_pkg"
executable = "server-node"
required_backends = ["zenoh"]

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["external"]
backends = ["inproc", "zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let processes = launch["graphs"][0]["processes"].as_array().unwrap();

    assert!(
        processes
            .iter()
            .all(|process| process["backend"] == "zenoh")
    );
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
        "flowrt::Status step_process_control(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state, flowrt::ScheduleWaiter& scheduler_events, std::map<std::string, flowrt::IntrospectionTaskHealth>& health_map);"
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
    assert!(supervisor.contains("const LAUNCH_MANIFEST_HASH: &str = "));
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
