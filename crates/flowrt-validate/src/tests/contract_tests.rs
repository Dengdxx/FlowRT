use super::*;
use flowrt_ir::EntityId;

#[test]
fn accepts_valid_minimal_contract() {
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

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "producer.imu"
to = "consumer.imu"
channel = "latest"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_contract_without_graphs() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.graphs.clear();

    let report = validate_contract(&ir).expect_err("v0.1 contract without graphs should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("Contract IR v0.1 must contain exactly one graph; found 0")
    }));
}

#[test]
fn rejects_contract_without_profiles() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.profiles.clear();
    ir.deployments.clear();

    let report = validate_contract(&ir).expect_err("v0.1 contract without profiles should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("Contract IR v0.1 must contain at least one profile; found 0")
    }));
}

#[test]
fn rejects_contract_with_multiple_graphs() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    let mut second_graph = ir.graphs[0].clone();
    second_graph.name = "secondary".to_string();
    ir.graphs.push(second_graph);

    let report =
        validate_contract(&ir).expect_err("v0.1 contract with multiple graphs should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("Contract IR v0.1 must contain exactly one graph; found 2")
    }));
}

#[test]
fn rejects_unsupported_contract_versions() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.ir_version = "9.9".to_string();
    ir.schema_version = "8.8".to_string();
    ir.package.rsdl_version = "7.7".to_string();

    let report = validate_contract(&ir).expect_err("unsupported versions should fail");

    for expected in [
        "unsupported Contract IR version `9.9`; expected `0.1`",
        "unsupported Contract IR schema version `8.8`; expected `0.1`",
        "unsupported RSDL version `7.7`; expected `0.1`",
    ] {
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains(expected)),
            "missing validation error: {expected}"
        );
    }
}

#[test]
fn rejects_non_canonical_source_hash_and_entity_ids() {
    let mut ir = valid_reference_contract();
    ir.source_hash = "bad".to_string();
    ir.package_id = EntityId("package_bad".to_string());

    let report = validate_contract(&ir).expect_err("non-canonical metadata should fail validation");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("source_hash `bad` must be a 64-character lowercase hex digest")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("package id `package_bad` must use the `package_<hex>` canonical format")
    }));
}

#[test]
fn rejects_non_canonical_collection_ordering() {
    let source = r#"
[package]
name = "ordering_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/a.rsdl", "types/b.rsdl"]

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
target = "linux"

[instance.alpha]
component = "alpha"
target = "linux"

[instance.beta]
component = "beta"
target = "linux"

[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["inproc", "iox2"]
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&ir).unwrap();

    ir.package.imports[0].patterns.reverse();
    ir.graphs[0].binds.reverse();
    ir.targets[0].runtime.reverse();
    ir.targets[0].backends.reverse();

    let report = validate_contract(&ir).expect_err("non-canonical collection ordering should fail");
    for expected in [
        "package import `types` patterns must use canonical sorted order",
        "graph `default` binds must use canonical endpoint order",
        "target `linux` runtime must use canonical sorted order",
        "target `linux` backends must use canonical sorted order",
    ] {
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains(expected)),
            "missing error containing `{expected}`"
        );
    }
}

#[test]
fn rejects_duplicate_package_import_collections_in_contract_ir() {
    let source = r#"
[package]
name = "import_demo"
rsdl_version = "0.1"

[package.imports]
components = ["components/a.rsdl"]
types = ["types/a.rsdl"]
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&ir).unwrap();

    let duplicate_pattern = ir.package.imports[0].patterns[0].clone();
    ir.package.imports[0].patterns.push(duplicate_pattern);
    ir.package.imports.push(ir.package.imports[0].clone());

    let report = validate_contract(&ir)
        .expect_err("duplicate package import collections should fail validation");

    for expected in [
        "package imports have duplicate kind",
        "package import `components` has duplicate pattern",
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
fn rejects_unknown_package_import_kind_in_contract_ir() {
    let source = r#"
[package]
name = "import_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/a.rsdl"]
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&ir).unwrap();

    ir.package.imports[0].kind = "widgets".to_string();

    let report = validate_contract(&ir).expect_err("unknown package import kind should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("package import kind `widgets` is not supported")
    }));
}

#[test]
fn rejects_non_canonical_entity_collection_ordering() {
    let source = r#"
[package]
name = "entity_ordering_demo"
rsdl_version = "0.1"

[package.imports]
components = ["components/a.rsdl"]
types = ["types/a.rsdl"]

[type.Extra]
value = "u32"

[type.Sample]
value = "u32"

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.producer.params]
alpha = 1
beta = 2

[instance.consumer]
component = "consumer"
target = "alpha_target"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[instance.producer]
component = "producer"
target = "alpha_target"

[instance.producer.params]
alpha = 3

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.alpha_target]
runtime = ["rust"]
backends = ["inproc", "iox2"]

[target.beta_target]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&ir).unwrap();

    ir.package.imports.reverse();
    ir.types.reverse();
    ir.components.reverse();
    ir.components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist")
        .params
        .reverse();
    ir.graphs[0].instances.reverse();
    ir.graphs[0].tasks.reverse();
    ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist")
        .params
        .reverse();
    ir.profiles.reverse();
    ir.targets.reverse();
    ir.deployments.reverse();

    let report =
        validate_contract(&ir).expect_err("non-canonical entity collection ordering should fail");
    for expected in [
        "package imports must use canonical kind order",
        "contract types must use canonical name order",
        "contract components must use canonical name order",
        "component `producer` params must use canonical name order",
        "graph `default` instances must use canonical name order",
        "graph `default` tasks must use canonical instance/name order",
        "instance `producer` params must use canonical name order",
        "contract profiles must use canonical name order",
        "contract targets must use canonical name order",
        "contract deployments must use canonical graph/profile/target order",
    ] {
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains(expected)),
            "missing error containing `{expected}`"
        );
    }
}

#[test]
fn rejects_duplicate_entity_names_in_contract_ir_scopes() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.worker]
language = "rust"
output = ["sample:Sample"]

[instance.worker]
component = "worker"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.types.push(ir.types[0].clone());
    ir.components.push(ir.components[0].clone());
    ir.profiles.push(ir.profiles[0].clone());
    ir.targets.push(ir.targets[0].clone());
    let duplicate_graph = ir.graphs[0].clone();
    let duplicate_instance = ir.graphs[0].instances[0].clone();
    ir.graphs[0].instances.push(duplicate_instance);
    ir.graphs.push(duplicate_graph);

    let report = validate_contract(&ir).expect_err("duplicate entity names should fail");

    for expected in [
        "contract has duplicate type name `Sample`",
        "contract has duplicate component name `worker`",
        "contract has duplicate profile name `default`",
        "contract has duplicate target name `linux`",
        "contract has duplicate graph name `default`",
        "graph `default` has duplicate instance name `worker`",
    ] {
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains(expected)),
            "missing validation error: {expected}"
        );
    }
}

#[test]
fn rejects_invalid_rsdl_names() {
    let source = r#"
[package]
name = "RobotDemo"
rsdl_version = "0.1"

[type.imu_sample]
timestamp = "u64"

[component.BadComponent]
language = "rust"
output = ["ImuOut:imu_sample"]

[instance.BadInstance]
component = "BadComponent"
target = "Linux"

[instance.BadInstance.task]
trigger = "periodic"
period_ms = 5
output = ["ImuOut"]

[profile.Default]
backend = "inproc"

[target.Linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("invalid RSDL names should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("package name `RobotDemo` must be snake_case")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("type name `imu_sample` must be PascalCase")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("component name `BadComponent` must be snake_case")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("port name `ImuOut` must be snake_case")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("profile name `Default` must be snake_case")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("target name `Linux` must be snake_case")
    }));
}

#[test]
fn rejects_invalid_generated_names_in_contract_ir() {
    let mut ir = valid_reference_contract();
    ir.types[0].generated_name.clear();
    ir.components[0].generated_name = "1bad".to_string();

    let report = validate_contract(&ir).expect_err("invalid generated names should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("type generated name `` must be a non-empty generated identifier")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("component generated name `1bad` must be a non-empty generated identifier")
    }));
}

#[test]
fn rejects_non_canonical_generated_names_in_contract_ir() {
    let mut ir = valid_reference_contract();
    ir.types[0].generated_name = "OtherSample".to_string();
    ir.components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist")
        .generated_name = "OtherProducer".to_string();

    let report = validate_contract(&ir).expect_err("forged generated names should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "type `Sample` generated name `OtherSample` does not match canonical `Sample`",
        )
    }));
    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "component `producer` generated name `OtherProducer` does not match canonical `producer`",
        )
    }));
}

#[test]
fn rejects_duplicate_generated_symbols_in_contract_ir() {
    let mut ir = valid_reference_contract();
    let mut first = ir.types[0].clone();
    first.id = flowrt_ir::EntityId("type:foo_bar::Baz".to_string());
    first.module = Some("foo_bar".to_string());
    first.name = "Baz".to_string();
    first.qualified_name = "foo_bar::Baz".to_string();
    first.generated_name = "FooBarBaz".to_string();

    let mut second = ir.types[0].clone();
    second.id = flowrt_ir::EntityId("type:foo::BarBaz".to_string());
    second.module = Some("foo".to_string());
    second.name = "BarBaz".to_string();
    second.qualified_name = "foo::BarBaz".to_string();
    second.generated_name = "FooBarBaz".to_string();

    ir.types.push(first);
    ir.types.push(second);

    let report = validate_contract(&ir).expect_err("generated symbol collision should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("contract has duplicate type generated symbol `FooBarBaz`")
    }));
}

#[test]
fn rejects_duplicate_entity_ids_in_contract_ir_scopes() {
    let mut ir = valid_reference_contract();
    let duplicate_id = ir.types[0].id.clone();
    ir.components[0].id = duplicate_id.clone();
    ir.graphs[0].id = duplicate_id.clone();
    ir.graphs[0].tasks[0].id = duplicate_id.clone();
    ir.graphs[0].binds[0].id = duplicate_id.clone();
    ir.deployments[0].id = duplicate_id.clone();

    let report = validate_contract(&ir).expect_err("duplicate entity IDs should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains(&format!("duplicate entity ID `{}`", duplicate_id.0))
    }));
}

#[test]
fn rejects_inconsistent_entity_references_in_contract_ir() {
    let mut ir = valid_reference_contract();
    let consumer_component_id = ir
        .components
        .iter()
        .find(|component| component.name == "consumer")
        .expect("consumer component must exist")
        .id
        .clone();
    let consumer_instance_id = ir.graphs[0]
        .instances
        .iter()
        .find(|instance| instance.name == "consumer")
        .expect("consumer instance must exist")
        .id
        .clone();
    let consumer_target_id = ir.targets[0].id.clone();

    ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist")
        .component
        .id = consumer_component_id;
    ir.graphs[0]
        .tasks
        .iter_mut()
        .find(|task| task.instance.name == "producer")
        .expect("producer task must exist")
        .instance
        .id = consumer_instance_id.clone();
    ir.graphs[0]
        .binds
        .iter_mut()
        .find(|bind| bind.from.instance.name == "producer")
        .expect("producer bind must exist")
        .from
        .instance
        .id = consumer_instance_id;
    ir.deployments[0].profile.id = consumer_target_id;

    let report = validate_contract(&ir).expect_err("inconsistent entity references should fail");

    for expected in [
        "instance `producer` component reference",
        "task `main` on instance `producer` instance reference",
        "bind source instance reference",
        "deployment profile reference",
    ] {
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains(expected)),
            "missing validation error: {expected}"
        );
    }
}
