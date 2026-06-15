use super::*;

#[test]
fn external_dataflow_auto_backend_resolves_to_zenoh() {
    let source = r#"
[package]
name = "external_route_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.fake_sensor]
language = "external"
kind = "external"
output = ["sample:Sample"]

[component.monitor]
language = "rust"
input = ["sample:Sample"]

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"
target = "linux"

[instance.monitor]
component = "monitor"
process = "monitor_proc"
target = "linux"

[instance.monitor.task]
trigger = "on_message"
input = ["sample"]

[[process]]
name = "sensor_proc"

[[process]]
name = "monitor_proc"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "driver"

[[bind.dataflow]]
from = "fake_sensor.sample"
to = "monitor.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust", "external"]
backends = ["inproc", "zenoh"]
"#;

    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(ir.graphs[0].binds[0].backend.0, "zenoh");
    assert_eq!(
        ir.graphs[0].binds[0].backend_source,
        ChannelBackendSource::AutoFallback
    );
}

#[test]
fn external_dataflow_explicit_zenoh_keeps_explicit_backend_source() {
    let source = r#"
[package]
name = "external_route_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.fake_sensor]
language = "external"
kind = "external"
output = ["sample:Sample"]

[component.monitor]
language = "rust"
input = ["sample:Sample"]

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"
target = "linux"

[instance.monitor]
component = "monitor"
process = "monitor_proc"
target = "linux"

[instance.monitor.task]
trigger = "on_message"
input = ["sample"]

[[process]]
name = "sensor_proc"

[[process]]
name = "monitor_proc"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "driver"

[[bind.dataflow]]
from = "fake_sensor.sample"
to = "monitor.sample"
channel = "latest"
backend = "zenoh"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust", "external"]
backends = ["inproc", "zenoh"]
"#;

    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(ir.graphs[0].binds[0].backend.0, "zenoh");
    assert_eq!(
        ir.graphs[0].binds[0].backend_source,
        ChannelBackendSource::Explicit
    );
}

#[test]
fn rejects_explicit_inproc_for_external_dataflow() {
    let source = r#"
[package]
name = "external_route_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.fake_sensor]
language = "external"
kind = "external"
output = ["sample:Sample"]

[component.monitor]
language = "rust"
input = ["sample:Sample"]

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"

[instance.monitor]
component = "monitor"
process = "monitor_proc"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "driver"

[[bind.dataflow]]
from = "fake_sensor.sample"
to = "monitor.sample"
channel = "latest"
backend = "inproc"
"#;

    let raw = parse_str(source).unwrap();
    let err = normalize_document(&raw, hash_source(source))
        .expect_err("external dataflow must not use inproc");
    assert!(format!("{err}").contains("external dataflow route cannot use `inproc`"));
}

#[test]
fn expands_dataflow_binds() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"

[component.producer]
language = "rust"
output = ["imu:Imu"]

[component.consumer]
language = "rust"
input = ["imu:Imu"]

[instance.producer]
component = "producer"

[instance.consumer]
component = "consumer"

[[bind.dataflow]]
from = "producer.imu"
to = "consumer.imu"
channel = "latest"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(ir.graphs[0].binds[0].channel, ChannelKind::Latest);
    assert_eq!(ir.graphs[0].binds[0].depth, Some(1));
}

#[test]
fn canonicalizes_bind_order_independent_of_source_order() {
    let source_a = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.alpha]
language = "rust"
input = ["sample:Sample"]

[component.beta]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"

[instance.alpha]
component = "alpha"

[instance.beta]
component = "beta"

[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest"
"#;
    let source_b = source_a.replace(
        r#"[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest""#,
        r#"[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest""#,
    );
    let raw_a = parse_str(source_a).unwrap();
    let raw_b = parse_str(&source_b).unwrap();
    let source_hash = hash_source("same logical source");

    let ir_a = normalize_document(&raw_a, source_hash.clone()).unwrap();
    let ir_b = normalize_document(&raw_b, source_hash).unwrap();

    assert_eq!(ir_a, ir_b);
}

#[test]
fn canonicalizes_target_set_order_independent_of_source_order() {
    let source_a = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["iox2", "inproc"]
"#;
    let source_b = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["inproc", "iox2"]
"#;
    let raw_a = parse_str(source_a).unwrap();
    let raw_b = parse_str(source_b).unwrap();
    let source_hash = hash_source("same logical source");

    let ir_a = normalize_document(&raw_a, source_hash.clone()).unwrap();
    let ir_b = normalize_document(&raw_b, source_hash).unwrap();

    assert_eq!(ir_a, ir_b);
}

#[test]
fn canonicalizes_import_pattern_order_independent_of_source_order() {
    let source_a = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/b.rsdl", "types/a.rsdl"]
components = ["components/b.rsdl", "components/a.rsdl"]
"#;
    let source_b = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/a.rsdl", "types/b.rsdl"]
components = ["components/a.rsdl", "components/b.rsdl"]
"#;
    let raw_a = parse_str(source_a).unwrap();
    let raw_b = parse_str(source_b).unwrap();
    let source_hash = hash_source("same logical source");

    let ir_a = normalize_document(&raw_a, source_hash.clone()).unwrap();
    let ir_b = normalize_document(&raw_b, source_hash).unwrap();

    assert_eq!(ir_a, ir_b);
}

#[test]
fn deadline_tasks_require_deadline_aware_backend_capability() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
deadline_ms = 2

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert!(
        ir.deployments[0]
            .required_capabilities
            .contains(&CapabilityAtom("timing:deadline_aware".to_string()))
    );
    assert!(ir.deployments[0].satisfied);
}

#[test]
fn int128_component_ports_require_route_abi_capability() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["sample:u128"]

[component.consumer]
language = "rust"
input = ["sample:u128"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert!(
        ir.graphs[0].binds[0]
            .capability_requirements
            .contains(&CapabilityAtom("abi:int128".to_string()))
    );
    assert!(ir.deployments[0].satisfied);
}

#[test]
fn iox2_route_derives_scheduler_local_commit_affinity_metadata() {
    let source = r#"
[package]
name = "affinity_demo"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["sample:u32"]

[component.consumer]
language = "rust"
input = ["sample:u32"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(
        ir.graphs[0].binds[0].thread_affinity,
        Some(BackendThreadAffinity::SchedulerLocalCommit)
    );
    assert!(
        ir.to_canonical_json()
            .unwrap()
            .contains("\"thread_affinity\": \"scheduler_local_commit\"")
    );
}

#[test]
fn inproc_and_zenoh_routes_derive_send_safe_affinity_metadata() {
    for backend in ["inproc", "zenoh"] {
        let source = format!(
            r#"
[package]
name = "affinity_demo"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["sample:u32"]

[component.consumer]
language = "rust"
input = ["sample:u32"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "{backend}"

[target.linux]
runtime = ["rust"]
backends = ["{backend}"]
"#
        );
        let raw = parse_str(&source).unwrap();
        let ir = normalize_document(&raw, hash_source(&source)).unwrap();

        assert_eq!(
            ir.graphs[0].binds[0].thread_affinity,
            Some(BackendThreadAffinity::SendSafe),
            "{backend} route should be send-safe"
        );
    }
}

#[test]
fn declared_int128_message_types_do_not_affect_unused_routes() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.UnusedWide]
value = "u128"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert!(
        !ir.graphs[0].binds[0]
            .capability_requirements
            .contains(&CapabilityAtom("abi:int128".to_string()))
    );
    assert!(ir.deployments[0].satisfied);
}

#[test]
fn iox2_route_records_int128_abi_capability() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["sample:i128"]

[component.consumer]
language = "rust"
input = ["sample:i128"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert!(
        ir.graphs[0].binds[0]
            .capability_requirements
            .contains(&CapabilityAtom("abi:int128".to_string()))
    );
    assert!(ir.deployments[0].satisfied);
}

#[test]
fn normalized_deployment_satisfied_matches_shared_capability_decision() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.WideSample]
value = "i128"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 5

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let deployment = &ir.deployments[0];
    let decision = deployment_capability_decision(
        &deployment.backend,
        &ir.targets[0].backends,
        &deployment.required_capabilities,
    );

    assert!(decision.selected_backend_known);
    assert!(decision.target_supports_selected_backend);
    assert!(decision.missing_required_capabilities.is_empty());
    assert_eq!(deployment.satisfied, decision.satisfied);
    assert!(deployment.satisfied);
}
