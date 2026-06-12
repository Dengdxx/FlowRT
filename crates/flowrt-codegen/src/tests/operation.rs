//! Operation codegen 测试：验证 typed 用户 API 和内部 Service/Channel lowering。

use super::*;

const RUST_OPERATION_RSDL: &str = r#"
[package]
name = "operation_demo"
rsdl_version = "0.1"

[type.PlanGoal]
target = "u32"

[type.PlanFeedback]
progress = "f32"

[type.PlanResult]
accepted = "bool"

[component.controller]
language = "rust"

[component.controller.operation_client.plan]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

[component.navigator]
language = "rust"

[component.navigator.operation_server.plan]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 100

[instance.navigator]
component = "navigator"

[instance.navigator.task]
trigger = "periodic"
period_ms = 1000

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
backend = "inproc"
timeout_ms = 5000
queue_depth = 4
max_in_flight = 1
concurrency = "reject"
preempt = "reject"
feedback = "latest"
result_retention_ms = 60000
"#;

/// Rust components 应暴露 Operation typed client，而不是把 Service API 泄漏给用户。
#[test]
fn rust_operation_client_handle_is_generated() {
    let contract = contract_from_source(RUST_OPERATION_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let components = artifact_content(&bundle, "rust/src/components.rs");
    assert!(
        components.contains("pub struct OperationClient_controller_plan"),
        "components module must contain operation client handle.\n\n{components}"
    );
    assert!(
        components.contains("fn start("),
        "operation client handle must expose start().\n\n{components}"
    );
    assert!(
        components.contains("fn cancel("),
        "operation client handle must expose cancel().\n\n{components}"
    );
    assert!(
        components.contains("fn status("),
        "operation client handle must expose status().\n\n{components}"
    );
    assert!(
        !components.contains("ServiceClient_controller_plan"),
        "operation user API must not be exposed as a service client.\n\n{components}"
    );
}

/// Rust server trait 应暴露 goal/cancel/progress/result 形态的 Operation handler。
#[test]
fn rust_operation_server_handler_method_is_generated() {
    let contract = contract_from_source(RUST_OPERATION_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let components = artifact_content(&bundle, "rust/src/components.rs");
    assert!(
        components.contains("fn on_plan_operation("),
        "navigator trait must contain operation handler method.\n\n{components}"
    );
    assert!(
        components.contains("_goal: &PlanGoal"),
        "operation handler must receive typed goal by reference.\n\n{components}"
    );
    assert!(
        components.contains("_cancel: flowrt::OperationCancelToken"),
        "operation handler must receive cooperative cancel token.\n\n{components}"
    );
    assert!(
        components.contains("_progress: &mut flowrt::OperationProgressPublisher<PlanFeedback>"),
        "operation handler must receive typed progress publisher.\n\n{components}"
    );
    assert!(
        components.contains("flowrt::OperationHandlerResult<PlanResult>"),
        "operation handler must return typed result wrapper.\n\n{components}"
    );
}

/// Runtime shell 必须把 Operation lower 成稳定命名的内部 endpoint，并注入 client handle。
#[test]
fn rust_runtime_shell_lowers_operation_to_internal_endpoints() {
    let contract = contract_from_source(RUST_OPERATION_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(
        shell.contains("__flowrt_operation_controller_plan_start"),
        "runtime shell must contain internal start endpoint name.\n\n{shell}"
    );
    assert!(
        shell.contains("__flowrt_operation_controller_plan_cancel"),
        "runtime shell must contain internal cancel endpoint name.\n\n{shell}"
    );
    assert!(
        shell.contains("__flowrt_operation_controller_plan_status"),
        "runtime shell must contain internal status endpoint name.\n\n{shell}"
    );
    assert!(
        shell.contains("__flowrt_operation_controller_plan_feedback"),
        "runtime shell must contain internal feedback endpoint name.\n\n{shell}"
    );
    assert!(
        shell.contains("__flowrt_operation_controller_plan_result"),
        "runtime shell must contain internal result endpoint name.\n\n{shell}"
    );
    assert!(
        shell.contains("operation_client_controller_plan"),
        "App struct must store operation client handle.\n\n{shell}"
    );
    assert!(
        shell.contains("on_tick(&self.operation_client_controller_plan)"),
        "runtime shell must pass operation client handle into on_tick.\n\n{shell}"
    );
    assert!(
        shell.contains(
            "self.navigator.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick()"
        ),
        "operation server component must be locked for ordinary task callbacks because the handler also owns it.\n\n{shell}"
    );
}

/// Operation start RPC 不能同步执行长 handler，否则 cancel/status 会被 hidden task 阻塞。
#[test]
fn rust_operation_start_handler_spawns_background_invocation() {
    let contract = contract_from_source(RUST_OPERATION_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(
        shell.contains("let operation_cancel_token_0"),
        "runtime shell must keep a shared cancel token slot for cancel/status handlers.\n\n{shell}"
    );
    assert!(
        shell.contains("std::thread::Builder::new()"),
        "operation start handler must launch the long-running invocation outside the service task.\n\n{shell}"
    );
    assert!(
        shell.contains("flowrt-operation-0"),
        "operation worker thread should have a stable generated name for diagnostics.\n\n{shell}"
    );
    let start_handler = generated_function_block(shell, "let operation_start_handler_0");
    let spawn_index = start_handler
        .find("std::thread::Builder::new()")
        .expect("start handler must spawn worker");
    let ack_index = start_handler
        .find("flowrt::ServiceResult::ok(flowrt::OperationStartAck::accepted(id))")
        .expect("start handler must return accepted ack");
    assert!(
        ack_index > spawn_index,
        "start handler must acknowledge after worker launch is requested.\n\n{start_handler}"
    );
    assert!(
        !start_handler.contains(".on_plan_operation(&goal,"),
        "start handler itself must not call the user operation handler synchronously.\n\n{start_handler}"
    );
}

/// 当前 generated Operation runtime 只支持单个运行中的 invocation，第二个 start 必须被拒绝。
#[test]
fn rust_operation_start_handler_rejects_second_active_invocation() {
    let contract = contract_from_source(RUST_OPERATION_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let start_handler = generated_function_block(shell, "let operation_start_handler_0");

    assert!(
        shell.contains("let operation_active_0"),
        "runtime shell must keep an active flag for reject concurrency.\n\n{shell}"
    );
    assert!(
        start_handler.contains(".compare_exchange(false, true"),
        "start handler must atomically reserve the single active invocation.\n\n{start_handler}"
    );
    assert!(
        start_handler.contains("flowrt::ServiceError::Busy"),
        "start handler must reject a second start while one invocation is active.\n\n{start_handler}"
    );
    assert!(
        start_handler.contains("operation_worker_active.store(false"),
        "operation worker must release the active flag after terminal state.\n\n{start_handler}"
    );
}

/// Self-description 必须保留 Operation 用户语义和调试用 lowering refs。
#[test]
fn self_description_contains_operation_topology_and_lowering_refs() {
    let contract = contract_from_source(RUST_OPERATION_RSDL);
    let bundle = emit_artifacts(&contract).unwrap();
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    let operation = &selfdesc["graphs"][0]["operations"][0];

    assert_eq!(operation["name"], "controller.plan");
    assert_eq!(operation["client_instance"], "controller");
    assert_eq!(operation["client_port"], "plan");
    assert_eq!(operation["server_instance"], "navigator");
    assert_eq!(operation["server_port"], "plan");
    assert_eq!(operation["goal_type"], "PlanGoal");
    assert_eq!(operation["feedback_type"], "PlanFeedback");
    assert_eq!(operation["result_type"], "PlanResult");
    assert_eq!(operation["backend"], "inproc");
    assert_eq!(operation["timeout_ms"], 5000);
    assert_eq!(operation["queue_depth"], 4);
    assert_eq!(operation["max_in_flight"], 1);
    assert_eq!(operation["concurrency"], "reject");
    assert_eq!(operation["preempt"], "reject");
    assert_eq!(operation["feedback"], "latest");
    assert_eq!(operation["result_retention_ms"], 60000);
    assert_eq!(
        operation["lowering"]["start_service"],
        "__flowrt_operation_controller_plan_start"
    );
    assert_eq!(
        operation["lowering"]["cancel_service"],
        "__flowrt_operation_controller_plan_cancel"
    );
    assert_eq!(
        operation["lowering"]["status_service"],
        "__flowrt_operation_controller_plan_status"
    );
    assert_eq!(
        operation["lowering"]["feedback_channel"],
        "__flowrt_operation_controller_plan_feedback"
    );
    assert_eq!(
        operation["lowering"]["result_channel"],
        "__flowrt_operation_controller_plan_result"
    );

    let component = &selfdesc["component_types"][0];
    assert_eq!(component["operation_clients"][0]["name"], "plan");
    assert_eq!(component["operation_clients"][0]["goal_type"], "PlanGoal");
}

/// zenoh Operation runtime 语义未接入 generated app 时必须 fail-fast，不能生成 placeholder。
#[test]
fn rust_zenoh_operation_codegen_rejects_placeholder_output() {
    let source = RUST_OPERATION_RSDL.replace("backend = \"inproc\"", "backend = \"zenoh\"");
    let contract = contract_from_source(&source);

    let error = emit_artifacts(&contract).expect_err("zenoh operation codegen must fail fast");

    assert!(
        error.to_string().contains("generated Operation codegen"),
        "unexpected error: {error}"
    );
    assert!(error.to_string().contains("backend `zenoh`"));
}

/// C++ components 应生成和 Rust 等价的 Operation typed API。
#[test]
fn cpp_operation_components_are_generated() {
    let source = RUST_OPERATION_RSDL.replace("language = \"rust\"", "language = \"cpp\"");
    let contract = contract_from_source(&source);
    let bundle = emit_artifacts(&contract).unwrap();
    let components = artifact_content(&bundle, "cpp/include/flowrt_app/components.hpp");
    assert!(
        components.contains("class OperationClient_controller_plan"),
        "C++ components header must expose operation client wrapper.\n\n{components}"
    );
    assert!(
        components.contains("OperationClient_controller_plan& plan"),
        "controller interface on_tick must receive operation client handle.\n\n{components}"
    );
    assert!(
        components.contains("std::uint64_t timeout_ms = 5000"),
        "C++ operation client wrapper must default to the RSDL policy timeout, not zero.\n\n{components}"
    );
    assert!(
        components.contains("on_plan_operation"),
        "navigator interface must declare operation handler.\n\n{components}"
    );
    assert!(
        components.contains("flowrt::OperationProgressPublisher<PlanFeedback>& progress"),
        "operation handler must receive typed progress publisher.\n\n{components}"
    );

    let shell_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    assert!(
        shell_header.contains("OperationClient_controller_plan operation_client_controller_plan_"),
        "C++ runtime shell header must have operation client field.\n\n{shell_header}"
    );
    assert!(
        shell_header.contains("step_operation_navigator_plan"),
        "C++ runtime shell header must declare operation step function.\n\n{shell_header}"
    );

    let shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    assert!(
        shell.contains("__flowrt_operation_controller_plan_start"),
        "C++ runtime shell must contain internal start endpoint name.\n\n{shell}"
    );
    assert!(
        shell.contains("controller_->on_tick(operation_client_controller_plan_)"),
        "C++ runtime shell must pass operation client handle into on_tick.\n\n{shell}"
    );
    assert!(
        shell.contains("std::thread"),
        "C++ operation lowering must include a background worker for long handlers.\n\n{shell}"
    );
    assert!(
        shell.contains("operation_cancel_token_0"),
        "C++ operation lowering must keep a shared cancel token slot.\n\n{shell}"
    );
    assert!(
        !shell.contains("const auto result = this->navigator_->on_plan_operation(goal,"),
        "C++ start handler itself must not call user operation handler synchronously.\n\n{shell}"
    );
    assert!(
        shell.contains("std::atomic_bool"),
        "C++ operation lowering must keep an active flag for reject concurrency.\n\n{shell}"
    );
    assert!(
        shell.contains("compare_exchange_strong"),
        "C++ start handler must atomically reserve the single active invocation.\n\n{shell}"
    );
    assert!(
        shell.contains("flowrt::ServiceError::Busy"),
        "C++ start handler must reject a second start while one invocation is active.\n\n{shell}"
    );
    assert!(
        shell.contains("operation_worker_active->store(false"),
        "C++ operation worker must release the active flag after terminal state.\n\n{shell}"
    );
}

/// C++ zenoh Operation 也必须 fail-fast，不能生成 Backend placeholder。
#[test]
fn cpp_zenoh_operation_codegen_rejects_placeholder_output() {
    let source = RUST_OPERATION_RSDL
        .replace("language = \"rust\"", "language = \"cpp\"")
        .replace("backend = \"inproc\"", "backend = \"zenoh\"");
    let contract = contract_from_source(&source);

    let error = emit_artifacts(&contract).expect_err("C++ zenoh operation codegen must fail fast");

    assert!(
        error.to_string().contains("generated Operation codegen"),
        "unexpected error: {error}"
    );
    assert!(error.to_string().contains("backend `zenoh`"));
}
