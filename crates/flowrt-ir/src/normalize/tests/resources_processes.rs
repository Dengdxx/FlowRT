use super::*;

#[test]
fn normalizes_external_component_and_process_contract() {
    let source = r#"
[package]
name = "external_demo"
version = "0.1.0"
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

    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    let component = &ir.components[0];
    assert_eq!(component.language, crate::LanguageKind::External);
    assert_eq!(component.kind, crate::ComponentKind::External);
    assert_eq!(ir.targets[0].runtime, vec![crate::LanguageKind::External]);
    assert_eq!(ir.graphs[0].external_processes.len(), 1);
    let external = &ir.graphs[0].external_processes[0];
    assert_eq!(external.process, "sensor_proc");
    assert_eq!(external.package, "fake_sensor_driver");
    assert_eq!(external.executable, "driver");
    assert_eq!(external.args, vec!["--rate", "50"]);
    assert_eq!(external.working_dir, crate::ExternalWorkingDir::Package);
    assert_eq!(external.health, crate::ExternalHealthKind::RuntimeSocket);
    assert_eq!(
        external.required_backends,
        vec![crate::BackendName("zenoh".to_string())]
    );
}

#[test]
fn normalizes_abstract_resource_providers_and_satisfaction_metadata() {
    let source = r#"
[package]
name = "abstract_resource_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.target_consumer]
language = "rust"
input = ["sample:Sample"]

[component.target_consumer.resource.frames]
capability = "perception.camera.frames"
access = "read"
readiness = "before_init"
health = "required"
on_failure = "stop_process"

[component.process_consumer]
language = "rust"
input = ["sample:Sample"]

[component.process_consumer.resource.cache]
capability = "storage.calibration.cache"
access = "write"
health = "optional"
on_failure = "restart_process"

[component.external_consumer]
language = "rust"
input = ["sample:Sample"]

[component.external_consumer.resource.inference]
capability = "compute.vision.inference"
access = "read_write"
readiness = "lazy"
on_failure = "stop_graph"

[component.optional_observer]
language = "rust"

[component.optional_observer.resource.trace]
capability = "observability.trace"
required = false

[component.vision_driver]
language = "external"
kind = "external"

[instance.target_consumer]
component = "target_consumer"
process = "control"
target = "edge"

[instance.process_consumer]
component = "process_consumer"
process = "control"
target = "edge"

[instance.external_consumer]
component = "external_consumer"
process = "control"
target = "edge"

[instance.optional_observer]
component = "optional_observer"
process = "control"
target = "edge"

[instance.vision_driver]
component = "vision_driver"
process = "vision_driver"
target = "edge"

[[external_process]]
process = "vision_driver"
package = "vision_driver_pkg"
executable = "driver"

[[resource.provider]]
name = "vision_driver_provider"
capabilities = ["compute.vision.inference"]
scope = "external_package"
external_package = "vision_driver_pkg"
health_source = "driver_health"
readiness_source = "driver_ready"

[[resource.provider]]
name = "camera_provider"
capabilities = ["perception.camera.frames"]
scope = "target"
target = "edge"
health_source = "target_health"
readiness_source = "target_ready"

[[resource.provider]]
name = "calibration_provider"
capabilities = ["storage.calibration.cache"]
scope = "process"
process = "control"
health_source = "cache_health"
readiness_source = "cache_ready"

[profile.default]
backend = "inproc"

[target.edge]
runtime = ["rust"]
backends = ["inproc"]
"#;

    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let graph = &ir.graphs[0];

    assert_eq!(
        graph
            .resource_providers
            .iter()
            .map(|provider| provider.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "calibration_provider",
            "camera_provider",
            "vision_driver_provider"
        ]
    );
    assert_eq!(
        graph.resource_satisfactions.len(),
        4,
        "one satisfaction record per active instance requirement"
    );

    let target = graph
        .resource_satisfactions
        .iter()
        .find(|satisfaction| satisfaction.instance.name == "target_consumer")
        .unwrap();
    assert!(target.satisfied);
    assert_eq!(
        target
            .provider
            .as_ref()
            .map(|provider| provider.name.as_str()),
        Some("camera_provider")
    );
    assert_eq!(target.diagnostic, None);

    let optional = graph
        .resource_satisfactions
        .iter()
        .find(|satisfaction| satisfaction.instance.name == "optional_observer")
        .unwrap();
    assert!(!optional.satisfied);
    assert!(optional.provider.is_none());
    assert!(optional.diagnostic.as_deref().unwrap().contains("optional"));

    let json = ir.to_canonical_json().unwrap();
    let roundtrip = crate::ContractIr::from_json_str(&json).unwrap();
    assert_eq!(roundtrip, ir);
}

#[test]
fn rejects_unknown_resource_access_enum() {
    let source = r#"
[package]
name = "bad_resource_access"
rsdl_version = "0.1"

[component.camera]
language = "rust"

[component.camera.resource.frames]
capability = "perception.camera.frames"
access = "shared"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("unknown resource access should fail");

    assert!(
        format!("{error}").contains("invalid `resource access` value `shared`"),
        "unexpected error: {error}"
    );
}

#[test]
fn normalizes_process_orchestration_defaults_and_overrides() {
    let source = r#"
[package]
name = "process_demo"
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
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let processes = &ir.graphs[0].processes;

    assert_eq!(processes.len(), 2);
    assert_eq!(processes[0].name, "control_proc");
    assert_eq!(processes[0].depends_on, vec!["sensor_proc"]);
    assert_eq!(processes[0].restart.policy, ProcessRestartPolicyKind::Never);
    assert_eq!(processes[0].restart.max_restarts, 0);
    assert_eq!(
        processes[0].failure_propagation,
        ProcessFailurePropagation::Isolate
    );
    assert_eq!(processes[1].name, "sensor_proc");
    assert_eq!(
        processes[1].restart.policy,
        ProcessRestartPolicyKind::OnFailure
    );
    assert_eq!(processes[1].restart.max_restarts, 5);
    assert_eq!(processes[1].restart.initial_delay_ms, 50);
    assert_eq!(processes[1].restart.max_delay_ms, 500);
    assert_eq!(
        processes[1].failure_propagation,
        ProcessFailurePropagation::Propagate
    );
}

#[test]
fn normalizes_process_resource_hints() {
    let source = r#"
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
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let processes = &ir.graphs[0].processes;

    assert_eq!(processes.len(), 2);
    let control = processes.iter().find(|p| p.name == "control_proc").unwrap();
    let sensor = processes.iter().find(|p| p.name == "sensor_proc").unwrap();

    assert_eq!(control.readiness, ProcessReadinessGate::ServiceReady);
    assert_eq!(control.startup_delay_ms, 0);
    assert_eq!(control.env["APP_MODE"], "control");
    assert!(control.cpu_affinity.is_empty());
    assert_eq!(control.nice, None);
    assert_eq!(control.rt_policy, None);
    assert_eq!(control.rt_priority, None);

    assert_eq!(sensor.readiness, ProcessReadinessGate::RuntimeReady);
    assert_eq!(sensor.startup_delay_ms, 200);
    assert_eq!(sensor.cpu_affinity, vec![0, 1]);
    assert_eq!(sensor.nice, Some(-5));
    assert_eq!(sensor.rt_policy, Some(RtPolicy::Fifo));
    assert_eq!(sensor.rt_priority, Some(50));
}

#[test]
fn normalizes_process_defaults_when_no_resource_hints() {
    let source = r#"
[package]
name = "default_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let processes = &ir.graphs[0].processes;

    assert_eq!(processes.len(), 1);
    assert_eq!(processes[0].name, "main");
    assert_eq!(processes[0].readiness, ProcessReadinessGate::ProcessStarted);
    assert_eq!(processes[0].startup_delay_ms, 0);
    assert!(processes[0].env.is_empty());
    assert!(processes[0].cpu_affinity.is_empty());
    assert_eq!(processes[0].nice, None);
    assert_eq!(processes[0].rt_policy, None);
    assert_eq!(processes[0].rt_priority, None);
}

#[test]
fn rejects_unknown_process_readiness_gate() {
    let source = r#"
[package]
name = "bad_readiness"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[[process]]
name = "main"
readiness = "custom_health_check"

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("unknown readiness gate should fail");

    assert!(matches!(
        error,
        IrError::InvalidEnum {
            kind: "process readiness gate",
            value,
            ..
        } if value == "custom_health_check"
    ));
}

#[test]
fn rejects_unknown_rt_policy() {
    let source = r#"
[package]
name = "bad_rt"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[[process]]
name = "main"
rt_policy = "deadline"

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let error =
        normalize_document(&raw, hash_source(source)).expect_err("unknown rt_policy should fail");

    assert!(matches!(
        error,
        IrError::InvalidEnum {
            kind: "RT scheduling policy",
            value,
            ..
        } if value == "deadline"
    ));
}

#[test]
fn normalizes_graph_health_default_continue() {
    let source = r#"
[package]
name = "graph_health_default"
rsdl_version = "0.1"

[component.processor]
language = "rust"
output = ["result:u32"]

[instance.processor]
component = "processor"

[instance.processor.task]
trigger = "periodic"
period_ms = 10
output = ["result"]

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    assert_eq!(ir.graphs[0].health.on_faulted, GraphFaultReaction::Continue);
}

#[test]
fn normalizes_redundancy_groups_canonically() {
    let source = r#"
[package]
name = "redundancy_demo"
rsdl_version = "0.1"

[type.Command]
value = "u32"

[component.controller]
language = "rust"
output = ["command:Command"]

[instance.controller_a]
component = "controller"

[instance.controller_a.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_b]
component = "controller"

[instance.controller_b.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_c]
component = "controller"

[instance.controller_c.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[[redundancy.group]]
name = "z_group"
mode = "standby"
primary = "controller_c"
standby = ["controller_b"]
trigger = "critical_fault"

[[redundancy.group]]
name = "controller_ha"
mode = "standby"
primary = "controller_a"
standby = ["controller_b"]
trigger = "critical_fault"

[profile.default]
backend = "inproc"

[profile.default.determinism]
mode = "global_tick"
timeout_ms = 1000

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;

    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    let groups = &ir.graphs[0].redundancy_groups;
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].name, "controller_ha");
    let group = &groups[0];
    assert_eq!(group.mode, RedundancyMode::Standby);
    assert_eq!(group.primary.name, "controller_a");
    assert_eq!(group.standby[0].name, "controller_b");
    assert_eq!(group.trigger, RedundancyTrigger::CriticalFault);
    assert_eq!(groups[1].name, "z_group");
}

#[test]
fn normalizes_graph_health_stop_reaction() {
    let source = r#"
[package]
name = "graph_health_stop"
rsdl_version = "0.1"

[graph.health]
on_faulted = "stop"

[component.processor]
language = "rust"
output = ["result:u32"]

[instance.processor]
component = "processor"
failure_policy = "restart"

[instance.processor.task]
trigger = "periodic"
period_ms = 10
output = ["result"]

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    assert_eq!(ir.graphs[0].health.on_faulted, GraphFaultReaction::Stop);
}

#[test]
fn normalizes_graph_health_critical_instances_canonically() {
    let source = r#"
[package]
name = "graph_health_critical"
rsdl_version = "0.1"

[graph.health]
on_faulted = "stop"
critical = ["controller_b", "controller_a"]

[component.controller]
language = "rust"
output = ["command:u32"]

[instance.controller_a]
component = "controller"
failure_policy = "restart"

[instance.controller_a.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_b]
component = "controller"

[instance.controller_b.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.monitor]
component = "controller"

[instance.monitor.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let critical = &ir.graphs[0].health.critical_instances;
    let names = critical
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["controller_a", "controller_b"]);
    assert!(critical[0].id.0.starts_with("instance_"));
}

#[test]
fn rejects_unknown_graph_health_critical_instance() {
    let source = r#"
[package]
name = "graph_health_unknown_critical"
rsdl_version = "0.1"

[graph.health]
critical = ["ghost"]

[component.controller]
language = "rust"
output = ["command:u32"]

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("unknown critical instance should be rejected");
    assert!(
        error.to_string().contains("unknown instance `ghost`"),
        "{error}"
    );
}

#[test]
fn rejects_unknown_graph_health_reaction() {
    let source = r#"
[package]
name = "graph_health_bad"
rsdl_version = "0.1"

[graph.health]
on_faulted = "halt"

[component.processor]
language = "rust"
output = ["result:u32"]

[instance.processor]
component = "processor"

[instance.processor.task]
trigger = "periodic"
period_ms = 10
output = ["result"]

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("unknown on_faulted value should be rejected");
    assert!(matches!(
        error,
        IrError::InvalidEnum {
            kind: "graph fault reaction",
            ..
        }
    ));
}
