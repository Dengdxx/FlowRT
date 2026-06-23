use super::*;

/// 构造一个 island restart 契约（boundary input 驱动 on_message task `main`）并叠加故障注入。
fn injected_restart_contract(language: &str, backend: &str, runtime: &str) -> ContractIr {
    let source = format!(
        r#"
[package]
name = "fault_injection_codegen_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.flaky]
language = "{language}"
input = ["sample:Sample"]
output = ["echo:Sample"]

[instance.flaky]
component = "flaky"

[instance.flaky.fault]
policy = "restart"
max_restarts = 2
initial_delay_ms = 10
max_delay_ms = 40

[instance.flaky.task]
trigger = "on_message"
input = ["sample"]
output = ["echo"]

[profile.dev]
mode = "island"
backend = "{backend}"

[[boundary.input]]
name = "feed"
port = "flaky.sample"
type = "Sample"

[target.linux]
runtime = ["{runtime}"]
backends = ["{backend}"]
"#,
    );
    let ir = contract_from_source(&source);
    let projected = flowrt_ir::project_contract_to_profile(&ir, None).unwrap();
    flowrt_ir::apply_fault_injection_overlay(
        &projected,
        &flowrt_ir::FaultInjectionScenario {
            points: vec![flowrt_ir::FaultInjectionScenarioPoint {
                kind: flowrt_ir::FaultInjectionKind::StatusError,
                instance: "flaky".to_string(),
                task: "main".to_string(),
                invocations: vec![],
                from_invocation: Some(1),
                reason: "drive restart to terminal".to_string(),
            }],
            generated_by: Default::default(),
        },
    )
    .unwrap()
}

fn injected_lifecycle_contract(
    language: &str,
    runtime: &str,
    kind: flowrt_ir::FaultInjectionKind,
    trigger: &str,
) -> ContractIr {
    let source = format!(
        r#"
[package]
name = "fault_injection_lifecycle_{language}_{trigger}"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.driver]
language = "{language}"
input = ["sample:Sample"]

[component.hook]
language = "{language}"

[instance.driver]
component = "driver"

[instance.driver.task]
trigger = "on_message"
input = ["sample"]

[instance.hook]
component = "hook"

[instance.hook.task]
trigger = "{trigger}"

[profile.dev]
mode = "island"
backend = "inproc"

[[boundary.input]]
name = "feed"
port = "driver.sample"
type = "Sample"

[target.linux]
runtime = ["{runtime}"]
backends = ["inproc"]
"#,
    );
    let ir = contract_from_source(&source);
    let projected = flowrt_ir::project_contract_to_profile(&ir, None).unwrap();
    flowrt_ir::apply_fault_injection_overlay(
        &projected,
        &flowrt_ir::FaultInjectionScenario {
            points: vec![flowrt_ir::FaultInjectionScenarioPoint {
                kind,
                instance: "hook".to_string(),
                task: "main".to_string(),
                invocations: vec![1],
                from_invocation: None,
                reason: "inject lifecycle hook".to_string(),
            }],
            generated_by: Default::default(),
        },
    )
    .unwrap()
}

fn injected_deadline_contract(language: &str, runtime: &str) -> ContractIr {
    let source = format!(
        r#"
[package]
name = "fault_injection_deadline_{language}"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.flaky]
language = "{language}"
input = ["sample:Sample"]
output = ["echo:Sample"]

[instance.flaky]
component = "flaky"

[instance.flaky.task]
trigger = "on_message"
input = ["sample"]
output = ["echo"]
deadline_ms = 5

[profile.dev]
mode = "island"
backend = "inproc"

[[boundary.input]]
name = "feed"
port = "flaky.sample"
type = "Sample"

[target.linux]
runtime = ["{runtime}"]
backends = ["inproc"]
"#,
    );
    let ir = contract_from_source(&source);
    let projected = flowrt_ir::project_contract_to_profile(&ir, None).unwrap();
    flowrt_ir::apply_fault_injection_overlay(
        &projected,
        &flowrt_ir::FaultInjectionScenario {
            points: vec![flowrt_ir::FaultInjectionScenarioPoint {
                kind: flowrt_ir::FaultInjectionKind::DeadlineMiss,
                instance: "flaky".to_string(),
                task: "main".to_string(),
                invocations: vec![1],
                from_invocation: None,
                reason: "force deadline".to_string(),
            }],
            generated_by: Default::default(),
        },
    )
    .unwrap()
}

fn injected_backend_drop_contract(language: &str, runtime: &str) -> ContractIr {
    let source = format!(
        r#"
[package]
name = "fault_injection_backend_drop_{language}"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "{language}"
input = ["sample:Sample"]
output = ["echo:Sample"]

[component.sink]
language = "{language}"
input = ["echo:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "on_message"
input = ["sample"]
output = ["echo"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["echo"]

[[bind.dataflow]]
from = "source.echo"
to = "sink.echo"
channel = "latest"

[profile.dev]
mode = "island"
backend = "zenoh"

[[boundary.input]]
name = "feed"
port = "source.sample"
type = "Sample"

[target.linux]
runtime = ["{runtime}"]
backends = ["zenoh"]
"#,
    );
    let ir = contract_from_source(&source);
    let projected = flowrt_ir::project_contract_to_profile(&ir, None).unwrap();
    flowrt_ir::apply_fault_injection_overlay(
        &projected,
        &flowrt_ir::FaultInjectionScenario {
            points: vec![flowrt_ir::FaultInjectionScenarioPoint {
                kind: flowrt_ir::FaultInjectionKind::BackendDrop,
                instance: "source".to_string(),
                task: "main".to_string(),
                invocations: vec![1],
                from_invocation: None,
                reason: "drop route backend".to_string(),
            }],
            generated_by: Default::default(),
        },
    )
    .unwrap()
}

#[test]
fn rust_shell_emits_injection_gate() {
    let ir = injected_restart_contract("rust", "inproc", "rust");
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    // per-task pre-execution 计数器声明 + 自增。
    assert!(shell.contains("let mut __inject_count_"));
    assert!(shell.contains("+= 1;"));
    // 命中时跳过用户回调、合成 error outcome。
    assert!(shell.contains("let __inject_fault_"));
    assert!(shell.contains(">= 1u64"));
    assert!(shell.contains("flowrt::TaskRunOutcome::error(Vec::<FlowrtOutputCommit>::new())"));
}

#[test]
fn rust_shell_emits_startup_and_shutdown_injection_gates() {
    let startup = injected_lifecycle_contract(
        "rust",
        "rust",
        flowrt_ir::FaultInjectionKind::StartupError,
        "startup",
    );
    let startup_bundle = emit_artifacts(&startup).unwrap();
    let startup_shell = artifact_content(&startup_bundle, "rust/src/runtime_shell.rs");
    let startup_step = generated_function_block(startup_shell, "fn step_startup");
    assert!(startup_step.contains("__flowrt_inject_status_error"));
    assert!(startup_step.contains("return flowrt::Status::Error;"));
    assert!(!startup_step.contains("panic!(\"FlowRT fault injection panic"));

    let shutdown = injected_lifecycle_contract(
        "rust",
        "rust",
        flowrt_ir::FaultInjectionKind::ShutdownError,
        "shutdown",
    );
    let shutdown_bundle = emit_artifacts(&shutdown).unwrap();
    let shutdown_shell = artifact_content(&shutdown_bundle, "rust/src/runtime_shell.rs");
    let shutdown_step = generated_function_block(shutdown_shell, "fn step_shutdown");
    assert!(shutdown_step.contains("__flowrt_inject_status_error"));
    assert!(shutdown_step.contains("return flowrt::Status::Error;"));
}

#[test]
fn cpp_shell_emits_injection_gate() {
    let ir = injected_restart_contract("cpp", "inproc", "cpp");
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(shell.contains("std::uint64_t __inject_count_"));
    assert!(shell.contains("bool flowrt_inject_fault = false;"));
    assert!(shell.contains(">= 1ULL"));
    assert!(shell.contains(
        "flowrt_inject_fault ? FlowrtTaskOutcome::error(std::vector<FlowrtOutputCommit>{})"
    ));
}

#[test]
fn generated_scheduler_keeps_running_until_pending_restart_is_observed() {
    let rust_ir = injected_restart_contract("rust", "inproc", "rust");
    let rust_bundle = emit_artifacts(&rust_ir).unwrap();
    let rust_shell = artifact_content(&rust_bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("|| flaky_next_restart_ms.is_some())"));

    let cpp_ir = injected_restart_contract("cpp", "inproc", "cpp");
    let cpp_bundle = emit_artifacts(&cpp_ir).unwrap();
    let cpp_shell = artifact_content(&cpp_bundle, "cpp/src/runtime_shell.cpp");
    assert!(cpp_shell.contains("|| flaky_next_restart_ms.has_value()))"));
}

#[test]
fn rust_and_cpp_shell_emit_panic_injection_gates() {
    let rust_ir = injected_restart_contract("rust", "inproc", "rust");
    let mut rust_panic = rust_ir.clone();
    rust_panic.artifact.fault_injection.as_mut().unwrap().points[0].kind =
        flowrt_ir::FaultInjectionKind::Panic;
    let rust_bundle = emit_artifacts(&rust_panic).unwrap();
    let rust_shell = artifact_content(&rust_bundle, "rust/src/runtime_shell.rs");
    assert!(
        rust_shell.contains("panic!(\"FlowRT fault injection panic: drive restart to terminal\")")
    );

    let cpp_ir = injected_restart_contract("cpp", "inproc", "cpp");
    let mut cpp_panic = cpp_ir.clone();
    cpp_panic.artifact.fault_injection.as_mut().unwrap().points[0].kind =
        flowrt_ir::FaultInjectionKind::Panic;
    let cpp_bundle = emit_artifacts(&cpp_panic).unwrap();
    let cpp_shell = artifact_content(&cpp_bundle, "cpp/src/runtime_shell.cpp");
    assert!(cpp_shell.contains(
        "throw std::runtime_error(\"FlowRT fault injection panic: drive restart to terminal\")"
    ));
}

#[test]
fn rust_and_cpp_shell_emit_deadline_miss_injection_gates() {
    let rust_ir = injected_deadline_contract("rust", "rust");
    let rust_bundle = emit_artifacts(&rust_ir).unwrap();
    let rust_shell = artifact_content(&rust_bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("__flowrt_inject_deadline_miss: bool"));
    assert!(rust_shell.contains("let __flowrt_inject_deadline_miss_"));
    assert!(rust_shell.contains("_deadline_exceeded = __flowrt_inject_deadline_miss ||"));
    assert!(rust_shell.contains(".deadline_missed += 1;"));

    let cpp_ir = injected_deadline_contract("cpp", "cpp");
    let cpp_bundle = emit_artifacts(&cpp_ir).unwrap();
    let cpp_shell = artifact_content(&cpp_bundle, "cpp/src/runtime_shell.cpp");
    assert!(cpp_shell.contains("bool __flowrt_inject_deadline_miss"));
    assert!(cpp_shell.contains("const bool __flowrt_inject_deadline_miss_"));
    assert!(cpp_shell.contains("_deadline_exceeded = __flowrt_inject_deadline_miss ||"));
    assert!(cpp_shell.contains(".deadline_missed += 1;"));
}

#[test]
fn backend_drop_injection_updates_route_backend_health() {
    let rust_ir = injected_backend_drop_contract("rust", "rust");
    let rust_bundle = emit_artifacts(&rust_ir).unwrap();
    let rust_shell = artifact_content(&rust_bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("flowrt::BackendHealthSnapshot::fault_injection_backend_drop()"));
    assert!(rust_shell.contains(
        "introspection_state.record_route_backend_health(\"source.echo_to_sink.echo\", flowrt::BackendHealthSnapshot::fault_injection_backend_drop());"
    ));
    assert!(
        rust_shell.contains("introspection_state.record_route_drop(\"source.echo_to_sink.echo\");")
    );

    let cpp_ir = injected_backend_drop_contract("cpp", "cpp");
    let cpp_bundle = emit_artifacts(&cpp_ir).unwrap();
    let cpp_shell = artifact_content(&cpp_bundle, "cpp/src/runtime_shell.cpp");
    assert!(cpp_shell.contains("flowrt::BackendHealthSnapshot::fault_injection_backend_drop()"));
    assert!(cpp_shell.contains(
        "introspection_state.record_route_backend_health(\"source.echo_to_sink.echo\", flowrt::BackendHealthSnapshot::fault_injection_backend_drop());"
    ));
    assert!(
        cpp_shell.contains("introspection_state.record_route_drop(\"source.echo_to_sink.echo\");")
    );
}

#[test]
fn non_injected_shell_has_no_injection_gate() {
    let source = r#"
[package]
name = "no_injection_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[instance.producer]
component = "producer"

[instance.producer.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]

[profile.default]
backend = "inproc"
"#;
    let ir = contract_from_source(source);
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    // 非注入产物不携带注入门，字节不漂移由 golden 锁定，这里只断言无注入符号。
    assert!(!shell.contains("__inject_count_"));
    assert!(!shell.contains("__inject_fault_"));
}
