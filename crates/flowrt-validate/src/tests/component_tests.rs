use super::*;

#[test]
fn rejects_external_component_kind_with_native_language() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.external_source]
language = "rust"
kind = "external"
output = ["sample:Sample"]

[instance.external_source]
component = "external_source"

[instance.external_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("external/native mismatch should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "component `external_source` uses kind `external` but language is not `external`",
        )
    }));
}

#[test]
fn accepts_io_boundary_component_with_resource_contract() {
    let source = r#"
[package]
name = "io_boundary_ok"
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
process = "main"
health_source = "provider_health"
readiness_source = "provider_ready"

[instance.sensor]
component = "sensor"

[instance.sensor.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    validate_contract(&ir).unwrap();
    let component = ir
        .components
        .iter()
        .find(|component| component.name == "sensor")
        .unwrap();
    assert_eq!(component.kind, flowrt_ir::ComponentKind::IoBoundary);
    assert_eq!(component.resources[0].name, "lidar_uart");
    assert_eq!(
        component.resources[0].capability.0,
        "perception.lidar.samples"
    );
}

#[test]
fn accepts_cpp_component_build_pkg_config_dependencies() {
    let source = r#"
[package]
name = "cpp_sdk_ok"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.camera]
language = "cpp"
kind = "io_boundary"
output = ["sample:Sample"]
io_side_effect = ["device", "read"]
io_readiness = "resource_ready"
io_health = "runtime_reported"
io_shutdown = "cooperative"

[component.camera.build]
pkg_config = ["vendor_capture", "vendor_codec"]

[instance.camera]
component = "camera"

[instance.camera.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    validate_contract(&ir).unwrap();
}

#[test]
fn accepts_c_native_component_language_semantics() {
    let source = r#"
[package]
name = "c_native_ok"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.controller]
language = "c"
input = ["sample:Sample"]

[component.source]
language = "rust"
output = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "controller.sample"
channel = "latest"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_c_component_v0_unsupported_surface() {
    let source = r#"
[package]
name = "c_native_rejected_surface"
rsdl_version = "0.1"

[type.Sample]
payload = "bytes"

[type.Req]
value = "u32"

[type.Resp]
ok = "bool"

[type.Goal]
target = "u32"

[type.Feedback]
progress = "u32"

[type.Result]
ok = "bool"

[component.controller]
language = "c"
kind = "io_boundary"
input = ["sample:Sample"]
output = ["sample:Sample"]
service_client = ["plan:Req->Resp"]

[component.controller.build]
pkg_config = ["vendor_capture"]

[component.controller.params]
gain = { type = "f32", default = 1.0, update = "startup" }

[component.controller.operation_client.move]
goal = "Goal"
feedback = "Feedback"
result = "Result"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    let report = validate_contract(&ir).expect_err("unsupported C v0 surface must fail");

    for expected in [
        "component `controller` declares pkg-config dependencies but language is not `cpp`",
        "component `controller` uses language `c` but C v0 only supports native components",
        "component `controller` uses language `c` but C v0 does not support params",
        "component `controller` uses language `c` but C v0 does not support service ports",
        "component `controller` uses language `c` but C v0 does not support operation ports",
        "component `controller` port `sample` uses variable frame data but C v0 only supports fixed-size message types",
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
fn rejects_rust_component_build_pkg_config_dependencies() {
    let source = r#"
[package]
name = "rust_sdk_bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[component.worker.build]
pkg_config = ["vendor_capture"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("Rust component pkg-config should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "component `worker` declares pkg-config dependencies but language is not `cpp`",
        )
    }));
}

#[test]
fn rejects_invalid_component_build_pkg_config_name() {
    let source = r#"
[package]
name = "cpp_sdk_bad"
rsdl_version = "0.1"

[component.camera]
language = "cpp"

[component.camera.build]
pkg_config = ["bad/package"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("invalid pkg-config name should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("component `camera` pkg-config dependency `bad/package` is invalid")
    }));
}

#[test]
fn accepts_io_boundary_frame_descriptor_bound_to_fixed_output_port() {
    let source = frame_descriptor_contract_source("FrameDescriptor");
    let raw = parse_str(&source).unwrap();
    let ir = normalize_document(&raw, hash_source(&source)).unwrap();

    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_frame_descriptor_bound_to_missing_output_port() {
    let source = frame_descriptor_contract_source("FrameDescriptor")
        .replace("port = \"frame\"", "port = \"missing_frame\"");
    let raw = parse_str(&source).unwrap();
    let ir = normalize_document(&raw, hash_source(&source)).unwrap();
    let report = validate_contract(&ir).expect_err("descriptor port must exist on outputs");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "component `camera` resource `frames` descriptor port `missing_frame` must reference an output port",
        )
    }));
}

#[test]
fn rejects_frame_descriptor_output_with_non_standard_fixed_shape() {
    let source = frame_descriptor_contract_source("BadFrameDescriptor")
        .replace("size_bytes = \"u64\"", "size_bytes = \"string\"");
    let raw = parse_str(&source).unwrap();
    let ir = normalize_document(&raw, hash_source(&source)).unwrap();
    let report =
        validate_contract(&ir).expect_err("descriptor output message must use standard shape");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "component `camera` resource `frames` descriptor port `frame` message `BadFrameDescriptor`",
        ) && error
            .message
            .contains("field `size_bytes` must be `u64`")
    }));
}

#[test]
fn rejects_frame_descriptor_output_with_variable_field_before_backend_fallback() {
    let source = frame_descriptor_contract_source("BadFrameDescriptor")
        .replace("flags = \"u32\"", "flags = \"bytes\"");
    let raw = parse_str(&source).unwrap();
    let ir = normalize_document(&raw, hash_source(&source)).unwrap();
    let report = validate_contract(&ir)
        .expect_err("descriptor output with variable data must fail validation");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "component `camera` resource `frames` descriptor port `frame` message `BadFrameDescriptor`",
        ) && error
            .message
            .contains("field `flags` must be `u32`, found `bytes`")
    }));
}

#[test]
fn rejects_legacy_adapter_component_kind() {
    let source = r#"
[package]
name = "bad_adapter"
rsdl_version = "0.1"

[component.legacy]
language = "rust"
kind = "adapter"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source)).unwrap_err();

    assert!(error.to_string().contains("component kind"));
    assert!(error.to_string().contains("adapter"));
}

#[test]
fn rejects_io_boundary_without_side_effects() {
    let source = r#"
[package]
name = "bad_io_boundary"
rsdl_version = "0.1"

[component.sensor]
language = "cpp"
kind = "io_boundary"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).unwrap_err();

    assert!(report.to_string().contains("declares no side effects"));
}

#[test]
fn rejects_duplicate_component_params() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![
        test_param("gain", ParamValue::Float(1.0)),
        test_param("gain", ParamValue::Float(2.0)),
    ];

    let report = validate_contract(&ir).expect_err("duplicate component params should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("component `producer` has duplicate param `gain`")
    }));
}

#[test]
fn rejects_missing_and_unknown_instance_params() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![
        test_param("gain", ParamValue::Float(1.0)),
        test_param("mode", ParamValue::String("auto".to_string())),
    ];
    let producer = ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist");
    producer.params = vec![
        ParamValueIr {
            name: "gain".to_string(),
            value: ParamValue::Float(2.0),
        },
        ParamValueIr {
            name: "gain".to_string(),
            value: ParamValue::Float(3.0),
        },
        ParamValueIr {
            name: "mystery".to_string(),
            value: ParamValue::Bool(true),
        },
    ];

    let report =
        validate_contract(&ir).expect_err("missing and unknown instance params should fail");

    for expected in [
        "instance `producer` has duplicate param `gain`",
        "instance `producer` is missing param `mode`",
        "instance `producer` has unknown param `mystery`",
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
fn rejects_incompatible_instance_params() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![test_param("gain", ParamValue::Float(1.0))];
    let producer = ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist");
    producer.params = vec![ParamValueIr {
        name: "gain".to_string(),
        value: ParamValue::String("fast".to_string()),
    }];

    let report = validate_contract(&ir).expect_err("incompatible instance params should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "instance `producer` param `gain` has incompatible value kind `string`; expected `f64`",
        )
    }));
}

#[test]
fn rejects_invalid_params_schema_in_contract_ir() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![ParamIr {
        name: "hot_state".to_string(),
        ty: ParamType::Table,
        default: ParamValue::Table(Default::default()),
        update: ParamUpdatePolicy::OnTick,
        min: Some(ParamValue::Integer(0)),
        max: None,
        choices: vec![ParamValue::String("safe".to_string())],
    }];
    let producer_instance = ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist");
    producer_instance.params = vec![ParamValueIr {
        name: "hot_state".to_string(),
        value: ParamValue::Table(Default::default()),
    }];

    let report = validate_contract(&ir).expect_err("invalid param schema should fail");

    for expected in [
        "component `producer` param `hot_state` uses `on_tick` update with non-scalar type `table`",
        "component `producer` param `hot_state` min has incompatible value kind `integer`; expected `table`",
        "component `producer` param `hot_state` enum choice has incompatible value kind `string`; expected `table`",
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
fn rejects_params_values_outside_declared_type_range_in_contract_ir() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![ParamIr {
        name: "gain".to_string(),
        ty: ParamType::U8,
        default: ParamValue::Integer(256),
        update: ParamUpdatePolicy::OnTick,
        min: None,
        max: None,
        choices: vec![],
    }];
    let producer_instance = ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist");
    producer_instance.params = vec![ParamValueIr {
        name: "gain".to_string(),
        value: ParamValue::Integer(1),
    }];

    let report = validate_contract(&ir).expect_err("out-of-range u8 param should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("component `producer` param `gain` default is outside declared type range")
    }));
}

#[test]
fn rejects_params_enum_choices_outside_declared_constraints_in_contract_ir() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![ParamIr {
        name: "gain".to_string(),
        ty: ParamType::U8,
        default: ParamValue::Integer(1),
        update: ParamUpdatePolicy::OnTick,
        min: Some(ParamValue::Integer(1)),
        max: Some(ParamValue::Integer(10)),
        choices: vec![ParamValue::Integer(1), ParamValue::Integer(20)],
    }];
    let producer_instance = ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist");
    producer_instance.params = vec![ParamValueIr {
        name: "gain".to_string(),
        value: ParamValue::Integer(1),
    }];

    let report = validate_contract(&ir).expect_err("enum choice must obey schema constraints");

    assert!(
        report.errors.iter().any(|error| {
            error
                .message
                .contains("component `producer` param `gain` enum choice is above declared maximum")
        }),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_instance_params_override_outside_schema_constraints() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![ParamIr {
        name: "gain".to_string(),
        ty: ParamType::U8,
        default: ParamValue::Integer(1),
        update: ParamUpdatePolicy::OnTick,
        min: Some(ParamValue::Integer(1)),
        max: Some(ParamValue::Integer(10)),
        choices: vec![ParamValue::Integer(1), ParamValue::Integer(2)],
    }];
    let producer_instance = ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist");
    producer_instance.params = vec![ParamValueIr {
        name: "gain".to_string(),
        value: ParamValue::Integer(256),
    }];

    let report = validate_contract(&ir).expect_err("instance param override must obey schema");

    for expected in [
        "instance `producer` param `gain` is outside declared type range",
        "instance `producer` param `gain` is above declared maximum",
        "instance `producer` param `gain` is not in declared enum choices",
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
fn rejects_non_finite_params_float_in_contract_ir() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![ParamIr {
        name: "gain".to_string(),
        ty: ParamType::F64,
        default: ParamValue::Float(f64::NAN),
        update: ParamUpdatePolicy::OnTick,
        min: None,
        max: None,
        choices: vec![],
    }];
    let producer_instance = ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist");
    producer_instance.params = vec![ParamValueIr {
        name: "gain".to_string(),
        value: ParamValue::Float(1.0),
    }];

    let report = validate_contract(&ir).expect_err("non-finite float param should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("component `producer` param `gain` default must be finite")
    }));
}

#[test]
fn rejects_mixed_integer_float_params_bounds_without_precision_loss() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![ParamIr {
        name: "limit".to_string(),
        ty: ParamType::F64,
        default: ParamValue::Integer(9_007_199_254_740_993),
        update: ParamUpdatePolicy::OnTick,
        min: None,
        max: Some(ParamValue::Float(9_007_199_254_740_992.0)),
        choices: vec![],
    }];
    let producer_instance = ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist");
    producer_instance.params = vec![ParamValueIr {
        name: "limit".to_string(),
        value: ParamValue::Integer(1),
    }];

    let report = validate_contract(&ir).expect_err("wide integer bound compare should be exact");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("component `producer` param `limit` default is above declared maximum")
    }));
}

#[test]
fn rejects_nested_non_finite_params_values_in_contract_ir() {
    let mut ir = valid_reference_contract();
    let producer = ir
        .components
        .iter_mut()
        .find(|component| component.name == "producer")
        .expect("producer component must exist");
    producer.params = vec![ParamIr {
        name: "table".to_string(),
        ty: ParamType::Array,
        default: ParamValue::Array(vec![ParamValue::Float(f64::INFINITY)]),
        update: ParamUpdatePolicy::Startup,
        min: None,
        max: None,
        choices: vec![],
    }];
    let producer_instance = ir.graphs[0]
        .instances
        .iter_mut()
        .find(|instance| instance.name == "producer")
        .expect("producer instance must exist");
    producer_instance.params = vec![ParamValueIr {
        name: "table".to_string(),
        value: ParamValue::Array(vec![]),
    }];

    let report = validate_contract(&ir).expect_err("nested non-finite values should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("component `producer` param `table` default contains non-finite float")
    }));
}

fn frame_descriptor_contract_source(output_type: &str) -> String {
    format!(
        r#"
[package]
name = "frame_descriptor_contract"
rsdl_version = "0.1"

[type.FrameDescriptor]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"
timestamp_unix_ns = "u64"
width = "u32"
height = "u32"
stride_bytes = "u32"
format_id = "u32"
encoding_id = "u32"
flags = "u32"

[type.BadFrameDescriptor]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"
timestamp_unix_ns = "u64"
width = "u32"
height = "u32"
stride_bytes = "u32"
format_id = "u32"
encoding_id = "u32"
flags = "u32"

[component.camera]
language = "rust"
kind = "io_boundary"
io_side_effect = ["device", "read"]
output = ["frame:{output_type}"]

[component.camera.resource.frames]
capability = "payload.frame_buffer"
required = false

[component.camera.resource.frames.descriptor]
kind = "frame"
port = "frame"
format = "rgb8"
encoding = "row_major"
metadata = {{ width = "640", height = "480" }}

[instance.camera]
component = "camera"

[instance.camera.task]
trigger = "periodic"
period_ms = 33
output = ["frame"]
"#,
    )
}
