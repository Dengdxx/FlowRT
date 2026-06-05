use super::*;
use flowrt_ir::TypeExpr;

#[test]
fn rejects_recursive_message_type() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Node]
next = "Node"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("recursive type should fail validation");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("recursive message type"))
    );
}

#[test]
fn rejects_zero_length_arrays_in_contract_ir() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Packet]
payload = "[u8; 1]"
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.types[0].fields[0].ty = TypeExpr::Array {
        element: Box::new(TypeExpr::Primitive {
            name: flowrt_ir::PrimitiveType::U8,
        }),
        len: 0,
    };

    let report = validate_contract(&ir).expect_err("zero-length arrays should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("type `Packet` field `payload` has zero-length array")
    }));
}

#[test]
fn accepts_top_level_bounded_variable_fields_with_inproc() {
    validate_contract(&bounded_variable_contract("inproc")).unwrap();
}

#[test]
fn bounded_variable_fields_follow_selected_backend_capabilities() {
    validate_contract(&bounded_variable_contract("iox2")).unwrap();
    validate_contract(&bounded_variable_contract("zenoh")).unwrap();
}

#[test]
fn rejects_bounded_variable_data_below_top_level_message_fields() {
    let source = r#"
[package]
name = "bad_variable_shapes"
rsdl_version = "0.1"

[type.ArrayHolder]
items = "[bytes<max=8>; 2]"

[type.Inner]
payload = "bytes<max=8>"

[type.Outer]
inner = "Inner"

[type.SequenceHolder]
items = "sequence<bytes<max=8>,max=2>"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("nested bounded variable data must be rejected");

    for expected in [
        "type `ArrayHolder` field `items` nests bounded variable data; variable data is only supported as a top-level message field",
        "type `Outer` field `inner` nests bounded variable data; variable data is only supported as a top-level message field",
        "type `SequenceHolder` field `items` has a variable-length sequence element; sequence elements must be fixed-size",
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
fn rejects_recursive_message_type_when_variable_frame_types_are_present() {
    let source = r#"
[package]
name = "bad_recursive_variable"
rsdl_version = "0.1"

[type.APacket]
payload = "bytes<max=8>"

[type.ZNode]
next = "ZNode"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir)
        .expect_err("variable frame types must not mask recursive message types");

    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("recursive message type `ZNode`")),
        "{:?}",
        report.errors
    );
}

#[test]
fn rejects_zero_bounded_variable_lengths_in_contract_ir() {
    let mut ir = bounded_variable_contract("inproc");
    for field in &mut ir.types[0].fields {
        match &mut field.ty {
            TypeExpr::VarBytes { max_len }
            | TypeExpr::VarString { max_len, .. }
            | TypeExpr::VarSequence { max_len, .. } => *max_len = 0,
            _ => {}
        }
    }

    let report =
        validate_contract(&ir).expect_err("zero bounded variable lengths must be rejected");
    for field_name in ["payload", "label", "samples"] {
        let expected = format!(
            "type `Packet` field `{field_name}` has zero maximum length for bounded variable type"
        );
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.message.contains(&expected)),
            "missing validation error: {expected}; got {:?}",
            report.errors
        );
    }
}

#[test]
fn rejects_bounded_variable_data_used_directly_as_component_port_type() {
    let source = r#"
[package]
name = "bad_variable_port"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["payload:bytes<max=8>"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir)
        .expect_err("bounded variable data must be wrapped in a named message type");

    assert!(
            report.errors.iter().any(|error| {
                error.message.contains(
                    "component `producer` port `payload` uses bounded variable data directly; variable data must be declared as a top-level field of a named message type",
                )
            }),
            "{:?}",
            report.errors
        );
}

#[test]
fn rejects_empty_message_types() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Empty]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("empty message type should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("type `Empty` must declare at least one field")
    }));
}
