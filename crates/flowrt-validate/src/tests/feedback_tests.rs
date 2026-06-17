use super::*;

/// 反馈控制环：controller→plant 前向，plant.state→controller.state 为反馈回边。
/// `feedback` 参数注入到反馈边，其余测试覆写以制造单点缺陷。
fn feedback_source(feedback_line: &str, controller_process: &str, plant_process: &str) -> String {
    format!(
        r#"
[package]
name = "feedback_demo"
rsdl_version = "0.1"

[type.Cmd]
u = "f64"

[type.State]
x = "f64"

[component.controller]
language = "rust"
input = ["state:State"]
output = ["cmd:Cmd"]

[component.plant]
language = "rust"
input = ["cmd:Cmd"]
output = ["state:State"]

[instance.controller]
component = "controller"
{controller_process}

[instance.controller.task]
trigger = "periodic"
period_ms = 10
input = ["state"]
output = ["cmd"]

[instance.plant]
component = "plant"
{plant_process}

[instance.plant.task]
trigger = "periodic"
period_ms = 10
input = ["cmd"]
output = ["state"]

[[bind.dataflow]]
from = "controller.cmd"
to = "plant.cmd"
channel = "latest"

[[bind.dataflow]]
from = "plant.state"
to = "controller.state"
channel = "latest"
{feedback_line}

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#
    )
}

fn contract_of(source: &str) -> ContractIr {
    let raw = parse_str(source).unwrap();
    normalize_document(&raw, hash_source(source)).unwrap()
}

#[test]
fn accepts_cycle_broken_by_feedback_edge() {
    let ir = contract_of(&feedback_source("feedback = true", "", ""));
    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_cycle_without_feedback_edge() {
    let ir = contract_of(&feedback_source("", "", ""));
    let report = validate_contract(&ir).expect_err("plain cycle should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("cycle"))
    );
}

#[test]
fn rejects_feedback_on_fifo_channel() {
    let source = feedback_source("feedback = true", "", "").replace(
        "to = \"controller.state\"\nchannel = \"latest\"\nfeedback = true",
        "to = \"controller.state\"\nchannel = \"fifo\"\ndepth = 4\nfeedback = true",
    );
    let ir = contract_of(&source);
    let report = validate_contract(&ir).expect_err("fifo feedback should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("must use `latest` channel"))
    );
}

#[test]
fn rejects_feedback_across_processes() {
    let ir = contract_of(&feedback_source(
        "feedback = true",
        "process = \"ctrl_proc\"",
        "process = \"plant_proc\"",
    ));
    let report = validate_contract(&ir).expect_err("cross-process feedback should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("must stay within one process"))
    );
}

#[test]
fn rejects_spurious_feedback_not_closing_cycle() {
    // 直链 producer -> consumer，无环；把唯一边标 feedback 即多余。
    let source = r#"
[package]
name = "spurious_feedback"
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
trigger = "periodic"
period_ms = 5
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"
feedback = true

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let ir = contract_of(source);
    let report = validate_contract(&ir).expect_err("spurious feedback should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("does not close a cycle"))
    );
}

#[test]
fn accepts_feedback_self_loop() {
    // 累加器：自身上一拍输出经反馈回边喂回自身输入。
    let source = r#"
[package]
name = "accumulator"
rsdl_version = "0.1"

[type.State]
x = "f64"

[component.accumulator]
language = "rust"
input = ["prev:State"]
output = ["next:State"]

[instance.accumulator]
component = "accumulator"

[instance.accumulator.task]
trigger = "periodic"
period_ms = 10
input = ["prev"]
output = ["next"]

[[bind.dataflow]]
from = "accumulator.next"
to = "accumulator.prev"
channel = "latest"
feedback = true

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let ir = contract_of(source);
    validate_contract(&ir).unwrap();
}
