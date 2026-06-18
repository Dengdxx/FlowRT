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
