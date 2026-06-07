//! Service codegen 测试：验证 Rust/C++ service client/server 代码生成。

use super::*;

const SERVICE_RSDL: &str = r#"
[package]
name = "service_demo"
rsdl_version = "0.1"

[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[component.plan_service]
language = "rust"
service_server = ["plan:PlanRequest->PlanResponse"]

[component.planner]
language = "rust"
service_client = ["plan:PlanRequest->PlanResponse"]

[instance.plan_svc]
component = "plan_service"

[instance.plan_svc.task]
trigger = "periodic"
period_ms = 1000

[instance.plan_client]
component = "planner"

[instance.plan_client.task]
trigger = "periodic"
period_ms = 100

[[bind.service]]
client = "plan_client.plan"
server = "plan_svc.plan"
backend = "inproc"
timeout_ms = 1000
queue_depth = 16
overflow = "busy"
"#;

/// service client handle struct 出现在 runtime_shell 文件中。
#[test]
fn rust_service_client_handle_struct_is_generated() {
    let contract = contract_from_source(SERVICE_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(
        shell.contains("ServiceClient_plan_client_plan"),
        "runtime shell must contain service client handle struct.\n\n{shell}"
    );
    assert!(
        shell.contains("fn call("),
        "service client handle must expose call() method.\n\n{shell}"
    );
    assert!(
        shell.contains("fn start_call("),
        "service client handle must expose start_call() method.\n\n{shell}"
    );
}

/// service server handler 方法出现在 component trait 中。
#[test]
fn rust_service_server_handler_method_in_trait() {
    let contract = contract_from_source(SERVICE_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let components = artifact_content(&bundle, "rust/src/components.rs");
    assert!(
        components.contains("fn on_plan_request("),
        "plan_service trait must contain on_plan_request handler method.\n\n{components}"
    );
    assert!(
        components.contains("ServiceResult<PlanResponse>"),
        "handler must return ServiceResult<PlanResponse>.\n\n{components}"
    );
}

/// runtime shell 包含 service 注册和 client/server 字段。
#[test]
fn rust_runtime_shell_registers_service() {
    let contract = contract_from_source(SERVICE_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(
        shell.contains("ServiceRegistry::new()"),
        "runtime shell must create ServiceRegistry.\n\n{shell}"
    );
    assert!(
        shell.contains("register_result_with_config"),
        "runtime shell must register service with config.\n\n{shell}"
    );
    assert!(
        shell.contains("service_client_plan_client_plan"),
        "App struct must have client field.\n\n{shell}"
    );
    assert!(
        shell.contains("service_server_plan_svc_plan"),
        "App struct must have server field.\n\n{shell}"
    );
}

/// runtime shell 包含 hidden service task step 函数。
#[test]
fn rust_runtime_shell_has_service_step_function() {
    let contract = contract_from_source(SERVICE_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(
        shell.contains("fn step_service_plan_svc_plan("),
        "runtime shell must have hidden service step function.\n\n{shell}"
    );
}

/// runtime shell scheduler 包含 service task dispatch。
#[test]
fn rust_scheduler_dispatches_service_tasks() {
    let contract = contract_from_source(SERVICE_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(
        shell.contains("step_service_plan_svc_plan()"),
        "scheduler must dispatch service task.\n\n{shell}"
    );
}

/// service policy 参数正确读取。
#[test]
fn service_policy_values_are_read() {
    let contract = contract_from_source(SERVICE_RSDL);
    let graph = contract.graphs.first().unwrap();
    let service = graph.services.first().unwrap();
    assert_eq!(service.policy.timeout_ms, 1000);
    assert_eq!(service.policy.queue_depth, 16);
    assert!(matches!(
        service.policy.overflow,
        flowrt_ir::ServiceOverflowPolicy::Busy
    ));
}

/// 没有 service 时不影响现有生成。
#[test]
fn no_service_does_not_affect_generation() {
    let source = r#"
[package]
name = "no_service"
rsdl_version = "0.1"

[component.c]
language = "rust"

[instance.c]
component = "c"

[instance.c.task]
trigger = "periodic"
period_ms = 10
"#;
    let contract = contract_from_source(source);
    let bundle = emit_artifacts(&contract).unwrap();
    let components = artifact_content(&bundle, "rust/src/components.rs");
    assert!(
        !components.contains("ServiceClient"),
        "no service client handle when no service bind.\n\n{components}"
    );
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(
        !shell.contains("ServiceRegistry"),
        "no ServiceRegistry when no service bind.\n\n{shell}"
    );
}

/// C++ service 组件生成：components header 包含 service handler 声明。
#[test]
fn cpp_service_components_are_generated() {
    let source = r#"
[package]
name = "cpp_service"
rsdl_version = "0.1"

[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[component.plan_service]
language = "cpp"
service_server = ["plan:PlanRequest->PlanResponse"]

[component.planner]
language = "cpp"
service_client = ["plan:PlanRequest->PlanResponse"]

[instance.plan_svc]
component = "plan_service"

[instance.plan_svc.task]
trigger = "periodic"
period_ms = 1000

[instance.plan_client]
component = "planner"

[instance.plan_client.task]
trigger = "periodic"
period_ms = 100

[[bind.service]]
client = "plan_client.plan"
server = "plan_svc.plan"
backend = "inproc"
timeout_ms = 2000
"#;
    let contract = contract_from_source(source);
    let bundle = emit_artifacts(&contract).unwrap();
    let components = artifact_content(&bundle, "cpp/include/flowrt_app/components.hpp");
    // C++ service handler 应该在 interface 中声明
    assert!(
        components.contains("on_plan_request"),
        "C++ plan_service interface must declare on_plan_request handler.\n\n{components}"
    );
    // C++ runtime shell 应该有 service client wrapper 和 server 字段
    let shell_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    assert!(
        shell_header.contains("ServiceClient_plan_client_plan"),
        "C++ runtime shell header must have service client wrapper.\n\n{shell_header}"
    );
    assert!(
        shell_header.contains("service_server_plan_svc_plan"),
        "C++ runtime shell header must have service server field.\n\n{shell_header}"
    );
    assert!(
        shell_header.contains("step_service_plan_svc_plan"),
        "C++ runtime shell header must have service step function declaration.\n\n{shell_header}"
    );
    // C++ runtime shell 应该有 service registration
    let shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    assert!(
        shell.contains("InprocServiceServer"),
        "C++ runtime shell must register service server.\n\n{shell}"
    );
    assert!(
        shell.contains("InprocServiceClient"),
        "C++ runtime shell must create service client.\n\n{shell}"
    );
    assert!(
        shell.contains("process_pending()"),
        "C++ runtime shell must call process_pending in service step.\n\n{shell}"
    );
}

/// inproc backend service 生成正确的配置。
#[test]
fn inproc_service_generates_correct_config() {
    let contract = contract_from_source(SERVICE_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(
        shell.contains("queue_depth: 16"),
        "must use configured queue_depth.\n\n{shell}"
    );
    assert!(
        shell.contains("ServiceOverflowPolicy::Busy"),
        "must use configured overflow policy.\n\n{shell}"
    );
}

/// 多个 service bind 正确生成。
#[test]
fn multiple_service_binds_generate_multiple_handles() {
    let source = r#"
[package]
name = "multi_service"
rsdl_version = "0.1"

[type.ReqA]
value = "i32"

[type.RespA]
result = "i32"

[type.ReqB]
data = "f32"

[type.RespB]
status = "bool"

[component.svc_a]
language = "rust"
service_server = ["s1:ReqA->RespA"]

[component.svc_b]
language = "rust"
service_server = ["s2:ReqB->RespB"]

[component.client]
language = "rust"
service_client = ["s1:ReqA->RespA", "s2:ReqB->RespB"]

[instance.svc_a]
component = "svc_a"

[instance.svc_a.task]
trigger = "periodic"
period_ms = 1000

[instance.svc_b]
component = "svc_b"

[instance.svc_b.task]
trigger = "periodic"
period_ms = 1000

[instance.client]
component = "client"

[instance.client.task]
trigger = "periodic"
period_ms = 100

[[bind.service]]
client = "client.s1"
server = "svc_a.s1"
backend = "inproc"

[[bind.service]]
client = "client.s2"
server = "svc_b.s2"
backend = "inproc"
"#;
    let contract = contract_from_source(source);
    let graph = contract.graphs.first().unwrap();
    assert_eq!(graph.services.len(), 2, "must have 2 service binds");
    let bundle = emit_artifacts(&contract).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(
        shell.contains("ServiceClient_client_s1"),
        "must have first client handle.\n\n{shell}"
    );
    assert!(
        shell.contains("ServiceClient_client_s2"),
        "must have second client handle.\n\n{shell}"
    );
}
