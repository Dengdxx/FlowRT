use super::*;
use crate::capabilities::expected_bind_capabilities;
use flowrt_ir::{EntityId, LanguageKind, graph_required_capabilities};

#[test]
fn rejects_unknown_backend_names_declared_in_profiles() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[profile.default]
backend = "typo_backend"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("unknown profile backend should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("profile `default` selects unknown backend `typo_backend`")
    }));
}

#[test]
fn rejects_unknown_backend_names_declared_in_targets() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "typo_backend"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("unknown target backend should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("target `linux` declares unknown backend `typo_backend`")
    }));
}

#[test]
fn rejects_implicit_default_backend_when_target_does_not_support_it() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 5

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir)
        .expect_err("implicit default backend unsupported by target should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "target `linux` does not support backend `inproc` selected by profile `default`",
        )
    }));
}

#[test]
fn rejects_iox2_for_cross_target_dataflow_that_requires_multi_host() {
    let source = r#"
[package]
name = "distributed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "producer"
target = "dev_host"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"
process = "consumer"
target = "pi_host"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"

[target.dev_host]
runtime = ["rust"]
backends = ["iox2"]

[target.pi_host]
runtime = ["rust"]
backends = ["iox2"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("iox2 should not satisfy cross-host dataflow");

    assert!(
            report.errors.iter().any(|error| {
                error.message.contains(
                    "backend `iox2` selected by profile `default` cannot satisfy required capabilities for graph `default`",
                )
            }),
            "{:?}",
            report.errors
        );
}

#[test]
fn accepts_zenoh_for_cross_target_dataflow_that_requires_multi_host() {
    let source = r#"
[package]
name = "distributed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "producer"
target = "dev_host"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"
process = "consumer"
target = "pi_host"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "zenoh"

[target.dev_host]
runtime = ["rust"]
backends = ["zenoh"]

[target.pi_host]
runtime = ["rust"]
backends = ["zenoh"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_zenoh_overflow_policy_without_runtime_capability() {
    let source = r#"
[package]
name = "distributed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "producer"
target = "dev_host"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"
process = "consumer"
target = "pi_host"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "fifo"
depth = 2

[profile.default]
backend = "zenoh"
default_overflow = "drop_newest"

[target.dev_host]
runtime = ["rust"]
backends = ["zenoh"]

[target.pi_host]
runtime = ["rust"]
backends = ["zenoh"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir)
        .expect_err("zenoh must reject overflow policies it does not advertise");

    assert!(
            report.errors.iter().any(|error| {
                error.message.contains(
                    "backend `zenoh` selected by profile `default` cannot satisfy required capabilities for graph `default`",
                )
            }),
            "{:?}",
            report.errors
        );
}

#[test]
fn rejects_duplicate_target_runtime_and_backends() {
    let mut ir = valid_reference_contract();
    ir.targets[0].runtime = vec![LanguageKind::Rust, LanguageKind::Rust];
    ir.targets[0].backends = vec![
        flowrt_ir::BackendName("inproc".to_string()),
        flowrt_ir::BackendName("inproc".to_string()),
    ];

    let report = validate_contract(&ir).expect_err("duplicate target lists should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("target `linux` has duplicate runtime `rust`")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("target `linux` has duplicate backend `inproc`")
    }));
}

#[test]
fn rejects_deployment_backend_that_does_not_match_profile() {
    let mut ir = valid_reference_contract();
    ir.deployments[0].backend.0 = "iox2".to_string();
    ir.deployments[0].satisfied = true;

    let report =
        validate_contract(&ir).expect_err("deployment backend/profile mismatch should fail");

    assert!(
        report.errors.iter().any(|error| {
            error.message.contains(
                "deployment backend `iox2` does not match profile `default` backend `inproc`",
            )
        }),
        "{:?}",
        report.errors
    );
    assert!(
        report.errors.iter().any(|error| {
            error
                .message
                .contains("deployment `default / default / linux` has inconsistent satisfied flag")
        }),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_forged_satisfied_deployment() {
    let mut ir = valid_reference_contract();
    ir.targets[0].backends = vec![flowrt_ir::BackendName("iox2".to_string())];
    ir.targets[0].capabilities = backend_capabilities("iox2").unwrap();
    ir.deployments[0].satisfied = true;

    let report = validate_contract(&ir).expect_err("forged satisfied flag should fail");

    assert!(
        report.errors.iter().any(|error| {
            error.message.contains(
                "target `linux` does not support backend `inproc` selected by profile `default`",
            )
        }),
        "{:?}",
        report.errors
    );
    assert!(
        report.errors.iter().any(|error| {
            error
                .message
                .contains("deployment `default / default / linux` has inconsistent satisfied flag")
        }),
        "{:?}",
        report.errors
    );
}

#[test]
fn deployment_capability_validation_reuses_shared_decision() {
    let source = r#"
[package]
name = "wide_demo"
rsdl_version = "0.1"

[type.WideSample]
value = "i128"

[component.producer]
language = "rust"
output = ["sample:WideSample"]

[component.consumer]
language = "rust"
input = ["sample:WideSample"]

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
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    let decision = deployment_capability_decision(
        &ir.deployments[0].backend,
        &ir.targets[0].backends,
        &ir.deployments[0].required_capabilities,
    );

    assert!(decision.selected_backend_known);
    assert!(decision.target_supports_selected_backend);
    assert_eq!(
        decision.missing_required_capabilities,
        vec![CapabilityAtom("abi:int128".to_string())]
    );

    ir.deployments[0].satisfied = true;
    let report = validate_contract(&ir).expect_err("forged satisfied flag should fail");
    assert!(
            report.errors.iter().any(|error| {
                error.message.contains(
                    "backend `inproc` selected by profile `default` cannot satisfy required capabilities for graph `default`",
                )
            }),
            "{:?}",
            report.errors
        );
    assert!(
        report.errors.iter().any(|error| {
            error
                .message
                .contains("deployment `default / default / linux` has inconsistent satisfied flag")
        }),
        "{:?}",
        report.errors
    );

    let mut unknown_ir = valid_reference_contract();
    unknown_ir.deployments[0].backend.0 = "typo_backend".to_string();
    let unknown_decision = deployment_capability_decision(
        &unknown_ir.deployments[0].backend,
        &unknown_ir.targets[0].backends,
        &unknown_ir.deployments[0].required_capabilities,
    );

    assert!(!unknown_decision.selected_backend_known);
    assert!(unknown_decision.missing_required_capabilities.is_empty());

    let report = validate_contract(&unknown_ir).expect_err("unknown backend should fail");
    assert!(
        !report.errors.iter().any(|error| {
            error
                .message
                .contains("cannot satisfy required capabilities")
        }),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_forged_int128_abi_capability_metadata() {
    let source = r#"
[package]
name = "wide_demo"
rsdl_version = "0.1"

[type.WideSample]
value = "i128"

[component.producer]
language = "rust"
output = ["sample:WideSample"]

[component.consumer]
language = "rust"
input = ["sample:WideSample"]

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
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.deployments[0]
        .required_capabilities
        .retain(|capability| capability.0 != "abi:int128");
    ir.deployments[0].satisfied = true;

    let report = validate_contract(&ir).expect_err("forged int128 capability metadata should fail");

    assert!(
            report.errors.iter().any(|error| {
                error.message.contains(
                    "deployment `default / default / linux` required capabilities do not match graph `default`",
                )
            }),
            "{:?}",
            report.errors
        );
    assert!(
            report.errors.iter().any(|error| {
                error.message.contains(
                    "backend `inproc` selected by profile `default` cannot satisfy required capabilities for graph `default`",
                )
            }),
            "{:?}",
            report.errors
        );
    assert!(
        report.errors.iter().any(|error| {
            error
                .message
                .contains("deployment `default / default / linux` has inconsistent satisfied flag")
        }),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_forged_variable_frame_capability_metadata() {
    let mut ir = bounded_variable_contract("inproc");
    validate_contract(&ir).unwrap();

    ir.deployments[0]
        .required_capabilities
        .retain(|capability| capability.0 != "abi:variable_payload_frame");

    let report = validate_contract(&ir).expect_err("forged variable frame capability must fail");
    assert!(
            report.errors.iter().any(|error| {
                error.message.contains(
                    "deployment `default / default / linux` required capabilities do not match graph `default`",
                )
            }),
            "{:?}",
            report.errors
        );
}

#[test]
fn rejects_forged_int128_metadata_for_declared_unreachable_message_type() {
    let source = r#"
[package]
name = "wide_demo"
rsdl_version = "0.1"

[type.UnusedWide]
value = "i128"

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
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.deployments[0]
        .required_capabilities
        .retain(|capability| capability.0 != "abi:int128");
    ir.deployments[0].satisfied = true;

    let report =
        validate_contract(&ir).expect_err("forged unreachable int128 type metadata should fail");

    assert!(
            report.errors.iter().any(|error| {
                error.message.contains(
                    "deployment `default / default / linux` required capabilities do not match graph `default`",
                )
            }),
            "{:?}",
            report.errors
        );
    assert!(
        report.errors.iter().any(|error| {
            error
                .message
                .contains("deployment `default / default / linux` has inconsistent satisfied flag")
        }),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_stale_derived_capability_metadata() {
    let mut ir = valid_reference_contract();
    ir.graphs[0].binds[0].capability_requirements.clear();
    ir.targets[0].capabilities.clear();
    ir.deployments[0].required_capabilities.clear();

    let report = validate_contract(&ir).expect_err("stale derived capabilities should fail");

    for expected in [
        "bind `producer.sample` -> `consumer.sample` capability requirements do not match channel policy",
        "target `linux` capabilities do not match declared backends",
        "deployment `default / default / linux` required capabilities do not match graph `default`",
    ] {
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains(expected)),
            "missing validation error: {expected}; got {:?}",
            report.errors
        );
    }
}

#[test]
fn rejects_non_canonical_derived_capability_ordering() {
    let mut ir = valid_reference_contract();
    validate_contract(&ir).unwrap();

    ir.graphs[0].binds[0].capability_requirements.reverse();
    ir.targets[0].capabilities.reverse();
    ir.deployments[0].required_capabilities.reverse();

    let report = validate_contract(&ir).expect_err("reordered derived capabilities should fail");

    for expected in [
        "bind `producer.sample` -> `consumer.sample` capability requirements do not match channel policy",
        "target `linux` capabilities do not match declared backends",
        "deployment `default / default / linux` required capabilities do not match graph `default`",
    ] {
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains(expected)),
            "missing validation error: {expected}; got {:?}",
            report.errors
        );
    }
}

#[test]
fn rejects_inconsistent_channel_policy_source_metadata() {
    let mut ir = valid_reference_contract();
    ir.graphs[0].binds[0].overflow = OverflowPolicy::Error;
    ir.graphs[0].binds[0].stale = StalePolicy::Drop;
    ir.graphs[0].binds[0].max_age_ms = Some(10);
    ir.graphs[0].binds[0].capability_requirements =
        expected_bind_capabilities(&ir.graphs[0].binds[0]);
    ir.deployments[0].required_capabilities =
        graph_required_capabilities(&ir.graphs[0], &ir.types, &ir.components);

    let report =
        validate_contract(&ir).expect_err("forged channel policy source metadata should fail");

    assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("bind `producer.sample` -> `consumer.sample` policy source metadata is inconsistent")
        }), "{:?}", report.errors);
}

#[test]
fn rejects_inconsistent_channel_policy_source_metadata_before_profile_projection() {
    let source = r#"
[package]
name = "profile_policy_source"
rsdl_version = "0.1"

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
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[profile.safety]
backend = "inproc"
default_overflow = "error"
default_stale_policy = "drop"
max_age_ms = 10

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&ir).unwrap();

    ir.graphs[0].binds[0].overflow = OverflowPolicy::Error;
    ir.graphs[0].binds[0].stale = StalePolicy::Drop;
    ir.graphs[0].binds[0].max_age_ms = Some(10);
    ir.graphs[0].binds[0].capability_requirements =
        expected_bind_capabilities(&ir.graphs[0].binds[0]);
    let required_capabilities =
        graph_required_capabilities(&ir.graphs[0], &ir.types, &ir.components);
    for deployment in &mut ir.deployments {
        deployment.required_capabilities = required_capabilities.clone();
    }

    let report =
        validate_contract(&ir).expect_err("forged unprojected policy metadata should fail");

    assert!(report.errors.iter().any(|error| {
            error
                .message
                .contains("bind `producer.sample` -> `consumer.sample` policy source metadata is inconsistent")
        }), "{:?}", report.errors);
}

#[test]
fn rejects_missing_deployment_matrix_rows() {
    let mut ir = valid_reference_contract();
    ir.deployments.clear();

    let report = validate_contract(&ir).expect_err("missing deployment row should fail");

    assert!(report.errors.iter().any(|error| {
            error.message.contains(
                "contract is missing deployment for graph `default`, profile `default`, target `linux`",
            )
        }), "{:?}", report.errors);
}

#[test]
fn rejects_duplicate_deployment_matrix_rows() {
    let mut ir = valid_reference_contract();
    let mut duplicate = ir.deployments[0].clone();
    duplicate.id = EntityId("deployment_duplicate".to_string());
    ir.deployments.push(duplicate);

    let report = validate_contract(&ir).expect_err("duplicate deployment row should fail");

    assert!(report.errors.iter().any(|error| {
            error.message.contains(
                "contract has duplicate deployment for graph `default`, profile `default`, target `linux`",
            )
        }), "{:?}", report.errors);
}
