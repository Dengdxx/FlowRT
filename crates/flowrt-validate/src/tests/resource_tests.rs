use super::*;

#[test]
fn accepts_resource_requirements_satisfied_by_target_process_and_external_package_providers() {
    let ir = abstract_resource_contract(
        r#"
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

[[resource.provider]]
name = "vision_driver_provider"
capabilities = ["compute.vision.inference"]
scope = "external_package"
external_package = "vision_driver_pkg"
health_source = "driver_health"
readiness_source = "driver_ready"
"#,
    );

    validate_contract(&ir).unwrap();

    let graph = &ir.graphs[0];
    assert!(graph.resource_satisfactions.iter().any(|satisfaction| {
        satisfaction.instance.name == "target_consumer"
            && satisfaction
                .provider
                .as_ref()
                .is_some_and(|provider| provider.name == "camera_provider")
            && satisfaction.satisfied
    }));
    assert!(graph.resource_satisfactions.iter().any(|satisfaction| {
        satisfaction.instance.name == "optional_observer"
            && !satisfaction.satisfied
            && satisfaction.provider.is_none()
            && satisfaction
                .diagnostic
                .as_ref()
                .is_some_and(|diagnostic| diagnostic.contains("optional"))
    }));
}

#[test]
fn rejects_required_resource_requirement_without_provider() {
    let ir = abstract_resource_contract(
        r#"
[[resource.provider]]
name = "calibration_provider"
capabilities = ["storage.calibration.cache"]
scope = "process"
process = "control"
health_source = "cache_health"
readiness_source = "cache_ready"
"#,
    );

    let report = validate_contract(&ir).expect_err("required resource must be satisfied");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "instance `target_consumer` resource `frames` requires capability `perception.camera.frames` but no provider satisfies it",
        )
    }));
}

#[test]
fn rejects_exclusive_resource_consumed_by_multiple_instances_in_same_scope() {
    let source = r#"
[package]
name = "exclusive_resource_demo"
rsdl_version = "0.1"

[component.camera_client]
language = "rust"

[component.camera_client.resource.frames]
capability = "perception.camera.frames"
access = "exclusive"

[component.camera_observer]
language = "rust"

[component.camera_observer.resource.frames]
capability = "perception.camera.frames"
access = "read"

[instance.left_client]
component = "camera_client"
process = "control"
target = "edge"

[instance.right_client]
component = "camera_observer"
process = "control"
target = "edge"

[[resource.provider]]
name = "camera_provider"
capabilities = ["perception.camera.frames"]
scope = "target"
target = "edge"
health_source = "target_health"
readiness_source = "target_ready"

[profile.default]
backend = "inproc"

[target.edge]
runtime = ["rust", "external"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("exclusive resource conflict must fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "exclusive resource provider `camera_provider` capability `perception.camera.frames` is shared by multiple active instances in scope `target`",
        )
    }));
}

#[test]
fn rejects_tampered_resource_satisfaction_metadata() {
    let mut ir = abstract_resource_contract(
        r#"
[[resource.provider]]
name = "camera_provider"
capabilities = ["perception.camera.frames"]
scope = "target"
target = "edge"
health_source = "target_health"
readiness_source = "target_ready"
"#,
    );
    ir.graphs[0].resource_satisfactions[0].satisfied =
        !ir.graphs[0].resource_satisfactions[0].satisfied;

    let report = validate_contract(&ir).expect_err("tampered satisfaction metadata must fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("graph `default` resource satisfaction metadata is not canonical")
    }));
}

#[test]
fn rejects_resource_provider_reference_to_unknown_target() {
    let mut ir = abstract_resource_contract(
        r#"
[[resource.provider]]
name = "camera_provider"
capabilities = ["perception.camera.frames"]
scope = "target"
target = "edge"
health_source = "target_health"
readiness_source = "target_ready"
"#,
    );
    let provider = ir
        .graphs
        .get_mut(0)
        .unwrap()
        .resource_providers
        .iter_mut()
        .find(|provider| provider.name == "camera_provider")
        .unwrap();
    provider.target.as_mut().unwrap().name = "missing".to_string();

    let report = validate_contract(&ir).expect_err("unknown provider target must fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("resource provider `camera_provider` target reference references unknown target `missing`")
    }));
}

#[test]
fn rejects_concrete_resource_capability_terms() {
    let source = r#"
[package]
name = "bad_concrete_resource"
rsdl_version = "0.1"

[component.camera]
language = "rust"

[component.camera.resource.stream]
capability = "transport.tcp"
required = false

[instance.camera]
component = "camera"
target = "edge"

[profile.default]
backend = "inproc"

[target.edge]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("concrete capability term must fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "component `camera` resource `stream` capability `transport.tcp` contains concrete hardware/protocol term `tcp`",
        )
    }));
}

fn abstract_resource_contract(providers: &str) -> ContractIr {
    let source = format!(
        r#"
[package]
name = "abstract_resource_demo"
rsdl_version = "0.1"

[component.target_consumer]
language = "rust"

[component.target_consumer.resource.frames]
capability = "perception.camera.frames"
access = "read"
readiness = "before_init"
health = "required"
on_failure = "stop_process"

[component.process_consumer]
language = "rust"

[component.process_consumer.resource.cache]
capability = "storage.calibration.cache"
access = "write"
health = "optional"
on_failure = "restart_process"

[component.external_consumer]
language = "rust"

[component.external_consumer.resource.inference]
capability = "compute.vision.inference"
access = "read_write"
readiness = "lazy"
on_failure = "stop_graph"
required = false

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

{providers}

[profile.default]
backend = "inproc"

[target.edge]
runtime = ["rust", "external"]
backends = ["inproc"]
"#
    );
    let raw = parse_str(&source).unwrap();
    normalize_document(&raw, hash_source(&source)).unwrap()
}
