use super::*;

#[test]
fn external_service_auto_backend_resolves_to_zenoh() {
    let source = r#"
[package]
name = "external_service_demo"
rsdl_version = "0.1"

[type.Request]
value = "u32"

[type.Response]
accepted = "bool"

[component.client]
language = "rust"
service_client = ["plan:Request->Response"]

[component.external_planner]
language = "external"
kind = "external"
service_server = ["plan:Request->Response"]

[instance.client]
component = "client"
process = "client_proc"

[instance.external_planner]
component = "external_planner"
process = "planner_proc"

[[external_process]]
process = "planner_proc"
package = "planner_driver"
executable = "planner"

[[bind.service]]
client = "client.plan"
server = "external_planner.plan"

[profile.default]
backend = "inproc"
"#;

    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(ir.graphs[0].services[0].backend.0, "zenoh");
    assert_eq!(
        ir.graphs[0].services[0].backend_source,
        ServiceBackendSource::AutoResolved
    );
}

#[test]
fn rejects_explicit_inproc_for_external_operation() {
    let source = r#"
[package]
name = "external_operation_demo"
rsdl_version = "0.1"

[type.Goal]
target = "u32"

[type.Feedback]
progress = "u32"

[type.Result]
ok = "bool"

[component.client]
language = "rust"

[component.client.operation_client.plan]
goal = "Goal"
feedback = "Feedback"
result = "Result"

[component.external_planner]
language = "external"
kind = "external"

[component.external_planner.operation_server.plan]
goal = "Goal"
feedback = "Feedback"
result = "Result"

[instance.client]
component = "client"
process = "client_proc"

[instance.external_planner]
component = "external_planner"
process = "planner_proc"

[[external_process]]
process = "planner_proc"
package = "planner_driver"
executable = "planner"

[[bind.operation]]
client = "client.plan"
server = "external_planner.plan"
backend = "inproc"
"#;

    let raw = parse_str(source).unwrap();
    let err = normalize_document(&raw, hash_source(source))
        .expect_err("external operation must not use inproc");
    assert!(format!("{err}").contains("external operation route cannot use `inproc`"));
}

#[test]
fn normalizes_service_ports_and_binds() {
    let source = r#"
[package]
name = "service_demo"
rsdl_version = "0.1"

[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[component.client]
language = "rust"
service_client = ["plan:PlanRequest->PlanResponse"]

[component.server]
language = "rust"
service_server = ["plan:PlanRequest->PlanResponse"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let client = ir
        .components
        .iter()
        .find(|component| component.name == "client")
        .unwrap();
    let service = &ir.graphs[0].services[0];

    assert_eq!(client.service_clients[0].name, "plan");
    assert_eq!(
        client.service_clients[0].request.canonical_syntax(),
        "PlanRequest"
    );
    assert_eq!(
        client.service_clients[0].response.canonical_syntax(),
        "PlanResponse"
    );
    assert_eq!(service.client.instance.name, "client");
    assert_eq!(service.client.port, "plan");
    assert_eq!(service.server.instance.name, "server");
    assert_eq!(service.server.port, "plan");
    // 默认 policy
    assert_eq!(service.backend.0, "inproc");
    assert_eq!(service.backend_source, ServiceBackendSource::AutoResolved);
    assert_eq!(service.policy.timeout_ms, 5000);
    assert_eq!(service.policy.queue_depth, 32);
    assert_eq!(service.policy.overflow, ServiceOverflowPolicy::Busy);
    assert_eq!(service.policy.lane, None);
    assert_eq!(service.policy.max_in_flight, 64);
}

#[test]
fn normalizes_operation_ports_and_binds() {
    let source = r#"
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

[instance.navigator]
component = "navigator"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let client = ir
        .components
        .iter()
        .find(|component| component.name == "controller")
        .unwrap();
    let operation = &ir.graphs[0].operations[0];

    assert_eq!(client.operation_clients[0].name, "plan");
    assert_eq!(
        client.operation_clients[0].goal.canonical_syntax(),
        "PlanGoal"
    );
    assert_eq!(
        client.operation_clients[0].feedback.canonical_syntax(),
        "PlanFeedback"
    );
    assert_eq!(
        client.operation_clients[0].result.canonical_syntax(),
        "PlanResult"
    );
    assert_eq!(operation.client.instance.name, "controller");
    assert_eq!(operation.client.port, "plan");
    assert_eq!(operation.server.instance.name, "navigator");
    assert_eq!(operation.server.port, "plan");
    assert_eq!(operation.backend.0, "inproc");
    assert_eq!(
        operation.backend_source,
        OperationBackendSource::AutoResolved
    );
    assert_eq!(operation.policy.timeout_ms, 30000);
    assert_eq!(
        operation.policy.concurrency,
        OperationConcurrencyPolicy::Reject
    );
    assert_eq!(operation.policy.preempt, OperationPreemptPolicy::Reject);
    assert_eq!(operation.policy.queue_depth, 8);
    assert_eq!(operation.policy.max_in_flight, 1);
    assert_eq!(operation.policy.feedback, OperationFeedbackPolicy::Latest);
    assert_eq!(operation.policy.result_retention_ms, 60000);
}

#[test]
fn operation_auto_backend_resolves_to_zenoh_for_cross_process() {
    let source = r#"
[package]
name = "operation_cross_process"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.operation_client.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[component.navigator]
language = "rust"

[component.navigator.operation_server.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[instance.controller]
component = "controller"
process = "control_proc"

[instance.navigator]
component = "navigator"
process = "nav_proc"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let operation = &ir.graphs[0].operations[0];

    assert_eq!(operation.backend.0, "zenoh");
    assert_eq!(
        operation.backend_source,
        OperationBackendSource::AutoResolved
    );
}

#[test]
fn operation_bind_with_explicit_policy_fields() {
    let source = r#"
[package]
name = "operation_policy_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.operation_client.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[component.navigator]
language = "rust"

[component.navigator.operation_server.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[instance.controller]
component = "controller"

[instance.navigator]
component = "navigator"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
backend = "zenoh"
timeout_ms = 1000
concurrency = "queue"
preempt = "cancel_running"
queue_depth = 16
max_in_flight = 4
feedback = "fifo"
result_retention_ms = 2000
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let operation = &ir.graphs[0].operations[0];

    assert_eq!(operation.backend.0, "zenoh");
    assert_eq!(operation.backend_source, OperationBackendSource::Explicit);
    assert_eq!(operation.policy.timeout_ms, 1000);
    assert_eq!(
        operation.policy.concurrency,
        OperationConcurrencyPolicy::Queue
    );
    assert_eq!(
        operation.policy.preempt,
        OperationPreemptPolicy::CancelRunning
    );
    assert_eq!(operation.policy.queue_depth, 16);
    assert_eq!(operation.policy.max_in_flight, 4);
    assert_eq!(operation.policy.feedback, OperationFeedbackPolicy::Fifo);
    assert_eq!(operation.policy.result_retention_ms, 2000);
    assert_eq!(operation.policy_source.backend, PolicyValueSource::Explicit);
    assert_eq!(
        operation.policy_source.concurrency,
        PolicyValueSource::Explicit
    );
}

#[test]
fn rejects_operation_bind_with_iox2_backend() {
    let source = r#"
[package]
name = "bad_operation"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.operation_client.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[component.navigator]
language = "rust"

[component.navigator.operation_server.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[instance.controller]
component = "controller"

[instance.navigator]
component = "navigator"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
backend = "iox2"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("iox2 operation backend should fail");

    assert!(matches!(
        error,
        IrError::InvalidEnum {
            kind: "operation backend",
            value,
            ..
        } if value == "iox2"
    ));
}

#[test]
fn service_bind_with_explicit_policy_fields() {
    let source = r#"
[package]
name = "service_policy_demo"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "zenoh"
timeout_ms = 1000
queue_depth = 16
overflow = "error"
lane = "rpc_lane"
max_in_flight = 8
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let service = &ir.graphs[0].services[0];

    assert_eq!(service.backend.0, "zenoh");
    assert_eq!(service.backend_source, ServiceBackendSource::Explicit);
    assert_eq!(service.policy.timeout_ms, 1000);
    assert_eq!(service.policy.queue_depth, 16);
    assert_eq!(service.policy.overflow, ServiceOverflowPolicy::Error);
    assert_eq!(service.policy.lane.as_deref(), Some("rpc_lane"));
    assert_eq!(service.policy.max_in_flight, 8);
    assert_eq!(service.policy_source.backend, PolicyValueSource::Explicit);
    assert_eq!(
        service.policy_source.timeout_ms,
        PolicyValueSource::Explicit
    );
}

#[test]
fn service_auto_backend_resolves_to_zenoh_for_cross_process() {
    let source = r#"
[package]
name = "service_cross_process"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"
process = "proc_a"

[instance.server]
component = "server"
process = "proc_b"

[[bind.service]]
client = "client.plan"
server = "server.plan"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let service = &ir.graphs[0].services[0];

    assert_eq!(service.backend.0, "zenoh");
    assert_eq!(service.backend_source, ServiceBackendSource::AutoResolved);
}

#[test]
fn service_explicit_inproc_same_process() {
    let source = r#"
[package]
name = "service_inproc"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"
process = "main"

[instance.server]
component = "server"
process = "main"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let service = &ir.graphs[0].services[0];

    assert_eq!(service.backend.0, "inproc");
    assert_eq!(service.backend_source, ServiceBackendSource::Explicit);
}

#[test]
fn rejects_service_bind_with_iox2_backend() {
    let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "iox2"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("iox2 service backend should fail");

    assert!(matches!(
        error,
        IrError::InvalidEnum {
            kind: "service backend",
            value,
            ..
        } if value == "iox2"
    ));
}

#[test]
fn rejects_service_bind_with_unknown_backend() {
    let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "grpc"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("unknown service backend should fail");

    assert!(matches!(
        error,
        IrError::InvalidEnum {
            kind: "service backend",
            value,
            ..
        } if value == "grpc"
    ));
}

#[test]
fn rejects_explicit_inproc_across_processes() {
    let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"
process = "proc_a"

[instance.server]
component = "server"
process = "proc_b"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("cross-process inproc should fail");

    assert!(matches!(error, IrError::InvalidValue { .. }));
}

#[test]
fn rejects_service_zero_timeout() {
    let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
timeout_ms = 0
"#;
    let raw = parse_str(source).unwrap();
    let error =
        normalize_document(&raw, hash_source(source)).expect_err("zero timeout should fail");

    assert!(matches!(error, IrError::InvalidValue { .. }));
}

#[test]
fn rejects_service_zero_queue_depth() {
    let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
queue_depth = 0
"#;
    let raw = parse_str(source).unwrap();
    let error =
        normalize_document(&raw, hash_source(source)).expect_err("zero queue_depth should fail");

    assert!(matches!(error, IrError::InvalidValue { .. }));
}

#[test]
fn rejects_service_zero_max_in_flight() {
    let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
max_in_flight = 0
"#;
    let raw = parse_str(source).unwrap();
    let error =
        normalize_document(&raw, hash_source(source)).expect_err("zero max_in_flight should fail");

    assert!(matches!(error, IrError::InvalidValue { .. }));
}

#[test]
fn rejects_service_unknown_overflow_policy() {
    let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
overflow = "drop_oldest"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("unknown overflow policy should fail");

    assert!(matches!(
        error,
        IrError::InvalidEnum {
            kind: "service overflow policy",
            value,
            ..
        } if value == "drop_oldest"
    ));
}
