use super::*;

fn validate_source(source: &str) -> crate::Result<()> {
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&ir)
}

fn assert_reserved_keyword_error(source: &str, expected: &str) {
    let report = validate_source(source).expect_err("reserved keyword name should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains(expected)),
        "missing validation error: {expected}; got {:?}",
        report.errors
    );
}

#[test]
fn rejects_reserved_keywords_for_identifier_emitting_names() {
    assert_reserved_keyword_error(
        r#"
[package]
name = "keyword_field"
rsdl_version = "0.1"

[type.Packet]
class = "u32"
"#,
        "field name `class` collides with a reserved Rust/C++ keyword",
    );

    assert_reserved_keyword_error(
        r#"
[package]
name = "keyword_port"
rsdl_version = "0.1"

[type.Packet]
value = "u32"

[component.consumer]
language = "rust"
input = ["in:Packet"]
"#,
        "port name `in` collides with a reserved Rust/C++ keyword",
    );

    assert_reserved_keyword_error(
        r#"
[package]
name = "keyword_service"
rsdl_version = "0.1"

[type.Req]
value = "u32"

[type.Resp]
ok = "bool"

[component.client]
language = "rust"
service_client = ["new:Req->Resp"]
"#,
        "service port name `new` collides with a reserved Rust/C++ keyword",
    );

    assert_reserved_keyword_error(
        r#"
[package]
name = "keyword_operation"
rsdl_version = "0.1"

[type.Goal]
target = "u32"

[type.Feedback]
progress = "u32"

[type.Result]
ok = "bool"

[component.controller]
language = "rust"

[component.controller.operation_client.delete]
goal = "Goal"
feedback = "Feedback"
result = "Result"
"#,
        "operation port name `delete` collides with a reserved Rust/C++ keyword",
    );

    assert_reserved_keyword_error(
        r#"
[package]
name = "keyword_instance"
rsdl_version = "0.1"

[type.Packet]
value = "u32"

[component.worker]
language = "rust"
output = ["packet:Packet"]

[instance.type]
component = "worker"

[instance.type.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]
"#,
        "instance name `type` collides with a reserved Rust/C++ keyword",
    );

    assert_reserved_keyword_error(
        r#"
[package]
name = "keyword_task"
rsdl_version = "0.1"

[type.Packet]
value = "u32"

[component.worker]
language = "rust"
output = ["packet:Packet"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "match"
trigger = "periodic"
period_ms = 5
output = ["packet"]
"#,
        "task name `match` collides with a reserved Rust/C++ keyword",
    );
}

#[test]
fn accepts_reserved_keywords_for_non_identifier_names() {
    let source = r#"
[package]
name = "keyword_metadata"
rsdl_version = "0.1"

[type.Packet]
value = "u32"

[component.producer]
language = "rust"
output = ["packet:Packet"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;

    validate_source(source).unwrap();
}
