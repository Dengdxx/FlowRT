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
fn rejects_invalid_parameter_schema_in_contract_ir() {
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
fn rejects_param_values_outside_declared_type_range_in_contract_ir() {
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
fn rejects_instance_param_override_outside_schema_constraints() {
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
fn rejects_non_finite_param_float_in_contract_ir() {
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
fn rejects_mixed_integer_float_param_bounds_without_precision_loss() {
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
fn rejects_nested_non_finite_param_values_in_contract_ir() {
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
