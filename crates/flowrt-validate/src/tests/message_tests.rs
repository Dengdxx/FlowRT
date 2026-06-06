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
fn accepts_top_level_variable_frame_fields_with_inproc() {
    validate_contract(&variable_frame_contract("inproc")).unwrap();
}

#[test]
fn variable_frame_fields_follow_selected_backend_capabilities() {
    validate_contract(&variable_frame_contract("iox2")).unwrap();
    validate_contract(&variable_frame_contract("zenoh")).unwrap();
}

#[test]
fn rejects_variable_frame_data_below_top_level_message_fields() {
    let source = r#"
[package]
name = "bad_variable_shapes"
rsdl_version = "0.1"

[type.ArrayHolder]
items = "[bytes; 2]"

[type.Inner]
payload = "bytes"

[type.Outer]
inner = "Inner"

[type.SequenceHolder]
items = "sequence<bytes>"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("nested variable data must be rejected");

    for expected in [
        "type `ArrayHolder` field `items` nests variable data; variable data is only supported as a top-level message field",
        "type `Outer` field `inner` nests variable data; variable data is only supported as a top-level message field",
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
payload = "bytes"

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
fn rejects_variable_frame_data_used_directly_as_component_port_type() {
    let source = r#"
[package]
name = "bad_variable_port"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["payload:bytes"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report =
        validate_contract(&ir).expect_err("variable data must be wrapped in a named message type");

    assert!(
            report.errors.iter().any(|error| {
                error.message.contains(
                    "component `producer` port `payload` uses variable data directly; variable data must be declared as a top-level field of a named message type",
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
