use super::*;

fn standby_failover_source(language: &str) -> String {
    format!(
        r#"
[package]
name = "standby_failover_{language}"
rsdl_version = "0.1"

[type.Command]
value = "u32"

[component.controller]
language = "{language}"
output = ["command:Command"]

[component.sink]
language = "{language}"
input = ["command:Command"]

[instance.controller_a]
component = "controller"
failure_policy = "isolate"

[instance.controller_a.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.controller_b]
component = "controller"

[instance.controller_b.task]
trigger = "periodic"
period_ms = 10
output = ["command"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["command"]

[[bind.dataflow]]
from = "controller_a.command"
to = "sink.command"
channel = "latest"

[[redundancy.group]]
name = "controller_ha"
mode = "standby"
primary = "controller_a"
standby = ["controller_b"]
trigger = "critical_fault"

[profile.default]
backend = "inproc"

[profile.default.determinism]
mode = "global_tick"
timeout_ms = 1000

[target.linux]
runtime = ["{language}"]
backends = ["inproc"]
"#
    )
}

#[test]
fn standby_failover_rust_shell_emits_active_route_and_event() {
    let source = standby_failover_source("rust");
    let ir = contract_from_source(&source);
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains(
        "let mut flowrt_active_redundancy_controller_ha = \"controller_a\".to_string();"
    ));
    assert!(rust_shell.contains("flowrt_active_redundancy_controller_ha == \"controller_a\""));
    assert!(rust_shell.contains("flowrt_active_redundancy_controller_ha == \"controller_b\""));
    assert!(
        rust_shell
            .contains("introspection_state.record_failover(flowrt::IntrospectionFailoverEvent")
    );
    assert!(rust_shell.contains("new_active: \"controller_b\".to_string()"));
}

#[test]
fn standby_failover_cpp_shell_emits_active_route_and_event() {
    let source = standby_failover_source("cpp");
    let ir = contract_from_source(&source);
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(
        cpp_shell
            .contains("std::string flowrt_active_redundancy_controller_ha = \"controller_a\";")
    );
    assert!(cpp_shell.contains("flowrt_active_redundancy_controller_ha == \"controller_a\""));
    assert!(cpp_shell.contains("flowrt_active_redundancy_controller_ha == \"controller_b\""));
    assert!(
        cpp_shell
            .contains("introspection_state.record_failover(flowrt::IntrospectionFailoverEvent{")
    );
    assert!(cpp_shell.contains(".new_active = \"controller_b\""));
}
