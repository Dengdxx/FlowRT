use super::*;

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
