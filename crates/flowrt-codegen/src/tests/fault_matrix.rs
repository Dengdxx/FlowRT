use super::*;

#[test]
fn rust_generated_run_writes_flowrt_status_out() {
    let source = r#"
[package]
name = "status_out_rust"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]

[profile.default]
backend = "inproc"
"#;
    let ir = contract_from_source(source);
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(shell.contains("std::env::var(\"FLOWRT_STATUS_OUT\")"));
    assert!(shell.contains("serde_json::to_string_pretty(&introspection_state.status())"));
}

#[test]
fn cpp_generated_run_writes_flowrt_status_out() {
    let source = r#"
[package]
name = "status_out_cpp"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "cpp"
output = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]

[profile.default]
backend = "inproc"
"#;
    let ir = contract_from_source(source);
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    assert!(shell.contains("std::getenv(\"FLOWRT_STATUS_OUT\")"));
    assert!(shell.contains("flowrt::introspection_status_json(introspection_state.status())"));
}
