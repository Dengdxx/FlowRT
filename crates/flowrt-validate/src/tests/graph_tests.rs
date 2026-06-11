use super::*;

#[test]
fn accepts_external_component_with_external_process_and_zenoh_route() {
    let source = r#"
[package]
name = "external_ok"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.fake_sensor]
language = "external"
kind = "external"
output = ["sample:Sample"]

[component.monitor]
language = "rust"
input = ["sample:Sample"]

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"
target = "linux"

[instance.monitor]
component = "monitor"
process = "monitor_proc"
target = "linux"

[instance.monitor.task]
trigger = "on_message"
input = ["sample"]

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "driver"
required_backends = ["zenoh"]

[[bind.dataflow]]
from = "fake_sensor.sample"
to = "monitor.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust", "external"]
backends = ["inproc", "zenoh"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_external_instance_without_external_process_metadata() {
    let source = r#"
[package]
name = "external_bad"
rsdl_version = "0.1"

[component.fake_sensor]
language = "external"
kind = "external"

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("external metadata should be required");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "external instance `fake_sensor` uses process `sensor_proc` without external_process metadata",
        )
    }));
}

#[test]
fn rejects_native_instance_inside_external_process() {
    let source = r#"
[package]
name = "external_bad"
rsdl_version = "0.1"

[component.fake_sensor]
language = "external"
kind = "external"

[component.monitor]
language = "rust"

[instance.fake_sensor]
component = "fake_sensor"
process = "shared_proc"

[instance.monitor]
component = "monitor"
process = "shared_proc"

[[external_process]]
process = "shared_proc"
package = "fake_sensor_driver"
executable = "driver"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report =
        validate_contract(&ir).expect_err("external process must not mix native instances");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("native instance `monitor` cannot run inside external process `shared_proc`")
    }));
}

#[test]
fn rejects_external_process_executable_escape_paths() {
    for executable in ["/bin/sh", "../driver", "bin/../driver", "./driver"] {
        let source = format!(
            r#"
[package]
name = "external_bad"
rsdl_version = "0.1"

[component.fake_sensor]
language = "external"
kind = "external"

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "{executable}"
"#
        );
        let raw = parse_str(&source).unwrap();
        let ir = normalize_document(&raw, hash_source(&source)).unwrap();
        let report = validate_contract(&ir).expect_err("escape executable should be rejected");

        assert!(
            report.errors.iter().any(|error| error.message.contains(
                "external_process `sensor_proc` executable must be a package-relative path"
            )),
            "missing executable path error for {executable}: {report:?}"
        );
    }
}

#[test]
fn rejects_wrong_bind_direction() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"

[component.producer]
language = "rust"
input = ["imu:Imu"]

[component.consumer]
language = "rust"
input = ["imu:Imu"]

[instance.producer]
component = "producer"

[instance.consumer]
component = "consumer"

[[bind.dataflow]]
from = "producer.imu"
to = "consumer.imu"
channel = "latest"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("wrong direction should fail validation");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("has no Output port"))
    );
}

#[test]
fn rejects_task_input_without_incoming_bind() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("missing incoming bind should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("task input `consumer.sample` has no incoming bind")
    }));
}

#[test]
fn accepts_island_boundary_input_as_task_source() {
    let source = r#"
[package]
name = "island_ok"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[profile.dev]
mode = "island"

[[boundary.input]]
name = "sample_in"
port = "consumer.sample"
type = "Sample"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    validate_contract(&ir).expect("island boundary input should satisfy task input");
}

#[test]
fn rejects_boundary_endpoints_in_strict_profile() {
    let source = r#"
[package]
name = "strict_bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.consumer]
component = "consumer"

[profile.default]
mode = "strict"

[[boundary.input]]
name = "sample_in"
port = "consumer.sample"
type = "Sample"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("strict profile must reject boundary endpoints");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("strict profile `default` cannot be used with boundary endpoints")
    }));
}

#[test]
fn rejects_boundary_input_that_duplicates_dataflow_bind() {
    let source = r#"
[package]
name = "island_bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[profile.dev]
mode = "island"

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[[boundary.input]]
name = "sample_in"
port = "consumer.sample"
type = "Sample"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir)
        .expect_err("boundary input must not duplicate an ordinary dataflow bind");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "input port `consumer.sample` is satisfied by both a dataflow bind and boundary input",
        )
    }));
}

#[test]
fn rejects_boundary_output_bound_to_input_port() {
    let source = r#"
[package]
name = "island_bad_output"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.consumer]
component = "consumer"

[profile.dev]
mode = "island"

[[boundary.output]]
name = "sample_out"
port = "consumer.sample"
type = "Sample"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("boundary output must bind an output port");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("instance `consumer` component `consumer` has no Output port `sample`")
    }));
}

#[test]
fn rejects_duplicate_task_inputs() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample", "sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("duplicate task inputs should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("task on instance `sink` lists input port `sample` more than once")
    }));
}

#[test]
fn rejects_duplicate_task_outputs() {
    let source = r#"
[package]
name = "bad"
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
period_ms = 5
output = ["sample", "sample"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("duplicate task outputs should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("task on instance `source` lists output port `sample` more than once")
    }));
}

#[test]
fn rejects_period_ms_on_non_periodic_task() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
period_ms = 10
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("non-periodic period_ms should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "task on instance `consumer` must not set period_ms unless trigger is periodic",
        )
    }));
}

#[test]
fn rejects_zero_period_ms_on_periodic_task() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 0
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("zero period_ms should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("periodic task on instance `worker` must set period_ms greater than zero")
    }));
}

#[test]
fn accepts_multiple_named_tasks_for_one_instance() {
    let source = r#"
[package]
name = "multi_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
output = ["fast"]
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    let mut second_task = ir.graphs[0].tasks[0].clone();
    second_task.id.0 = "task_1111111111111111".to_string();
    second_task.name = "slow_loop".to_string();
    second_task.period_ms = Some(10);
    second_task.outputs = vec!["slow".to_string()];
    ir.graphs[0].tasks.push(second_task);

    validate_contract(&ir).expect("multiple named tasks should validate");
}

#[test]
fn rejects_duplicate_task_names_for_one_instance() {
    let source = r#"
[package]
name = "bad_tasks"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "loop"
trigger = "periodic"
period_ms = 5
output = ["fast"]

[[instance.worker.task]]
name = "loop"
trigger = "periodic"
period_ms = 100
output = ["slow"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("duplicate task names should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("instance `worker` has duplicate task name `loop`")
    }));
}

#[test]
fn accepts_scheduler_v2_task_fields() {
    let source = r#"
[package]
name = "scheduler_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["sample:u32"]

[component.sink]
language = "rust"
input = ["sample:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
readiness = "all_ready"
lane = "sink_serial"
priority = 4
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    validate_contract(&ir).expect("scheduler v2 task fields should validate");
}

#[test]
fn rejects_readiness_on_non_on_message_task() {
    let source = r#"
[package]
name = "bad_readiness"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["sample:u32"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
readiness = "all_ready"
output = ["sample"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("readiness on periodic task should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "task on instance `worker` must not set readiness unless trigger is on_message",
        )
    }));
}

#[test]
fn rejects_process_spanning_multiple_targets() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "main"
target = "linux_a"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "main"
target = "linux_b"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[target.linux_a]
runtime = ["rust"]
backends = ["inproc"]

[target.linux_b]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("process target mismatch should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("process `main` spans multiple targets")
    }));
}

#[test]
fn rejects_invalid_process_names() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "Control-Loop"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("invalid process names should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("process name `Control-Loop` must be snake_case")
    }));
}

#[test]
fn rejects_reserved_process_name_prefix() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "flowrt_supervisor"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report =
        validate_contract(&ir).expect_err("reserved process prefix should fail validation");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("process name `flowrt_supervisor` uses reserved `flowrt` prefix")
    }));
}

#[test]
fn rejects_process_dependency_cycles_in_contract_ir() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensor_proc"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control_proc"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[[process]]
name = "sensor_proc"
depends_on = ["control_proc"]

[[process]]
name = "control_proc"
depends_on = ["sensor_proc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("process dependency cycle should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("process dependency graph contains a cycle")
    }));
}

#[test]
fn rejects_service_bind_with_mismatched_response_type() {
    let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[type.BadResponse]
code = "u32"

[component.client]
language = "rust"
service_client = ["plan:PlanRequest->PlanResponse"]

[component.server]
language = "rust"
service_server = ["plan:PlanRequest->BadResponse"]

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
    let report = validate_contract(&ir).expect_err("service response mismatch should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("service bind `client.plan -> server.plan` has mismatched response type")
    }));
}

#[test]
fn rejects_operation_bind_with_mismatched_result_type() {
    let source = r#"
[package]
name = "bad_operation"
rsdl_version = "0.1"

[type.PlanGoal]
target = "u32"

[type.PlanFeedback]
progress = "f32"

[type.PlanResult]
accepted = "bool"

[type.BadResult]
code = "u32"

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
result = "BadResult"

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
    let report = validate_contract(&ir).expect_err("operation result mismatch should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "operation bind `controller.plan -> navigator.plan` has mismatched result type",
        )
    }));
}

#[test]
fn rejects_operation_client_bound_more_than_once() {
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

[component.navigator_a]
language = "rust"

[component.navigator_a.operation_server.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[component.navigator_b]
language = "rust"

[component.navigator_b.operation_server.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[instance.controller]
component = "controller"

[instance.navigator_a]
component = "navigator_a"

[instance.navigator_b]
component = "navigator_b"

[[bind.operation]]
client = "controller.plan"
server = "navigator_a.plan"

[[bind.operation]]
client = "controller.plan"
server = "navigator_b.plan"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("duplicate operation client should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("operation client `controller.plan` is bound more than once")
    }));
}

#[test]
fn rejects_operation_zero_policy_fields_at_validation() {
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
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.graphs[0].operations[0].policy.timeout_ms = 0;
    ir.graphs[0].operations[0].policy.queue_depth = 0;
    ir.graphs[0].operations[0].policy.max_in_flight = 0;
    let report = validate_contract(&ir).expect_err("zero operation policy should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("operation bind `controller.plan -> navigator.plan` has zero timeout_ms")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("operation bind `controller.plan -> navigator.plan` has zero queue_depth")
    }));
    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("operation bind `controller.plan -> navigator.plan` has zero max_in_flight")
    }));
}

#[test]
fn rejects_operation_policies_not_supported_by_generated_runtime() {
    let source = r#"
[package]
name = "bad_operation_policy"
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
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    let policy = &mut ir.graphs[0].operations[0].policy;
    policy.concurrency = flowrt_ir::OperationConcurrencyPolicy::Queue;
    policy.preempt = flowrt_ir::OperationPreemptPolicy::CancelRunning;
    policy.max_in_flight = 2;

    let report =
        validate_contract(&ir).expect_err("unsupported generated operation policy should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "operation bind `controller.plan -> navigator.plan` uses unsupported concurrency policy `queue`",
        )
    }));
    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "operation bind `controller.plan -> navigator.plan` uses unsupported preempt policy `cancel_running`",
        )
    }));
    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "operation bind `controller.plan -> navigator.plan` uses unsupported max_in_flight `2`",
        )
    }));
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
        .expect_err("iox2 service backend should fail at normalization");

    assert!(
        error.to_string().contains("iox2"),
        "error should mention iox2: {error}"
    );
}

#[test]
fn rejects_auto_resolved_inproc_service_that_spans_processes() {
    let source = r#"
[package]
name = "bad_service_ir"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"
process = "client_proc"

[instance.server]
component = "server"
process = "server_proc"

[[bind.service]]
client = "client.plan"
server = "server.plan"

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "zenoh"]
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.graphs[0].services[0].backend = flowrt_ir::BackendName("inproc".into());
    ir.graphs[0].services[0].backend_source = flowrt_ir::ServiceBackendSource::AutoResolved;

    let report = validate_contract(&ir).expect_err("tampered inproc service route should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "service bind `client.plan -> server.plan` uses `inproc` but spans process or target boundaries",
        )
    }));
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
        .expect_err("unknown service backend should fail at normalization");

    assert!(
        error.to_string().contains("grpc"),
        "error should mention grpc: {error}"
    );
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

    assert!(
        error.to_string().contains("inproc"),
        "error should mention inproc: {error}"
    );
}

#[test]
fn rejects_service_zero_timeout_at_validation() {
    // 手工构造一个 timeout_ms=0 的 IR，验证 validator 也能拦截。
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
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    // 手工篡改 policy 字段
    ir.graphs[0].services[0].policy.timeout_ms = 0;
    let report = validate_contract(&ir).expect_err("zero timeout should fail validation");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("service bind `client.plan -> server.plan` has zero timeout_ms")
    }));
}

#[test]
fn rejects_service_zero_queue_depth_at_validation() {
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
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.graphs[0].services[0].policy.queue_depth = 0;
    let report = validate_contract(&ir).expect_err("zero queue_depth should fail validation");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("service bind `client.plan -> server.plan` has zero queue_depth")
    }));
}

#[test]
fn rejects_service_zero_max_in_flight_at_validation() {
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
"#;
    let raw = parse_str(source).unwrap();
    let mut ir = normalize_document(&raw, hash_source(source)).unwrap();
    ir.graphs[0].services[0].policy.max_in_flight = 0;
    let report = validate_contract(&ir).expect_err("zero max_in_flight should fail validation");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("service bind `client.plan -> server.plan` has zero max_in_flight")
    }));
}

#[test]
fn rejects_service_lane_that_is_not_snake_case() {
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
lane = "RpcLane"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("invalid service lane should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("service lane name `RpcLane` must be snake_case")
    }));
}

#[test]
fn rejects_reserved_name_prefix_case_insensitively() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.FlowrtSample]
value = "u32"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("reserved prefix with PascalCase should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("type name `FlowrtSample` uses reserved `flowrt` prefix")
    }));
}

#[test]
fn rejects_dataflow_cycle_between_instances() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.alpha]
language = "rust"
input = ["feedback:Sample"]
output = ["forward:Sample"]

[component.beta]
language = "rust"
input = ["forward:Sample"]
output = ["feedback:Sample"]

[instance.alpha]
component = "alpha"

[instance.alpha.task]
trigger = "on_message"
input = ["feedback"]
output = ["forward"]

[instance.beta]
component = "beta"

[instance.beta.task]
trigger = "on_message"
input = ["forward"]
output = ["feedback"]

[[bind.dataflow]]
from = "alpha.forward"
to = "beta.forward"
channel = "latest"

[[bind.dataflow]]
from = "beta.feedback"
to = "alpha.feedback"
channel = "latest"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("dataflow cycle should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("graph `default` has a dataflow cycle involving `alpha`")
    }));
}

#[test]
fn rejects_dataflow_self_loop() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.echo]
language = "rust"
input = ["in_value:Sample"]
output = ["out_value:Sample"]

[instance.echo]
component = "echo"

[instance.echo.task]
trigger = "on_message"
input = ["in_value"]
output = ["out_value"]

[[bind.dataflow]]
from = "echo.out_value"
to = "echo.in_value"
channel = "latest"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("dataflow self-loop should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("graph `default` has a dataflow self-loop on instance `echo`")
    }));
}

#[test]
fn rejects_latest_channel_depth_greater_than_one() {
    let source = r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
depth = 2
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report =
        validate_contract(&ir).expect_err("latest channel depth greater than one should fail");

    assert!(report.errors.iter().any(|error| {
        error
            .message
            .contains("latest channel to `sink.sample` must omit depth or set depth = 1")
    }));
}

#[test]
fn rejects_invalid_nice_value() {
    let source = r#"
[package]
name = "bad_nice"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[[process]]
name = "main"
nice = -21

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("invalid nice value should fail");

    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("invalid nice value -21"))
    );
}

#[test]
fn rejects_rt_priority_without_rt_policy() {
    let source = r#"
[package]
name = "bad_rt"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[[process]]
name = "main"
rt_priority = 50

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("rt_priority without rt_policy should fail");

    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("rt_priority without rt_policy"))
    );
}

#[test]
fn rejects_invalid_rt_priority_range() {
    let source = r#"
[package]
name = "bad_rt_priority"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[[process]]
name = "main"
rt_policy = "fifo"
rt_priority = 100

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("rt_priority out of range should fail");

    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("invalid rt_priority 100"))
    );
}

#[test]
fn rejects_duplicate_cpu_affinity_entry() {
    let source = r#"
[package]
name = "bad_cpu"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[[process]]
name = "main"
cpu_affinity = [0, 1, 0]

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let report = validate_contract(&ir).expect_err("duplicate cpu_affinity should fail");

    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("duplicate cpu_affinity entry 0"))
    );
}

#[test]
fn accepts_valid_process_resource_hints() {
    let source = r#"
[package]
name = "good_resource"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[[process]]
name = "main"
readiness = "service_ready"
startup_delay_ms = 100
cpu_affinity = [0, 1, 2]
nice = -10
rt_policy = "fifo"
rt_priority = 99
env = { APP_MODE = "control" }

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    validate_contract(&ir).expect("valid resource hints should pass");
}
