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

/// fifo 反馈环：controller↔plant，回边 plant.state→controller.state 为 fifo depth 拍延迟。
/// 两端周期与 depth/overflow 可注入以制造单点缺陷。
fn fifo_feedback_source(feedback_edge: &str, controller_period: u64, plant_period: u64) -> String {
    format!(
        r#"
[package]
name = "fifo_feedback_demo"
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

[instance.controller.task]
trigger = "periodic"
period_ms = {controller_period}
input = ["state"]
output = ["cmd"]

[instance.plant]
component = "plant"

[instance.plant.task]
trigger = "periodic"
period_ms = {plant_period}
input = ["cmd"]
output = ["state"]

[[bind.dataflow]]
from = "controller.cmd"
to = "plant.cmd"
channel = "latest"

[[bind.dataflow]]
from = "plant.state"
to = "controller.state"
{feedback_edge}

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#
    )
}

#[test]
fn accepts_feedback_on_fifo_channel_equal_periods() {
    let ir = contract_of(&fifo_feedback_source(
        "channel = \"fifo\"\ndepth = 4\noverflow = \"drop_oldest\"\nfeedback = true",
        10,
        10,
    ));
    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_feedback_fifo_unequal_periods() {
    let ir = contract_of(&fifo_feedback_source(
        "channel = \"fifo\"\ndepth = 4\noverflow = \"drop_oldest\"\nfeedback = true",
        10,
        20,
    ));
    let report = validate_contract(&ir).expect_err("unequal-period fifo feedback should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("equal periodic period"))
    );
}

#[test]
fn rejects_feedback_fifo_without_depth() {
    let ir = contract_of(&fifo_feedback_source(
        "channel = \"fifo\"\noverflow = \"drop_oldest\"\nfeedback = true",
        10,
        10,
    ));
    let report = validate_contract(&ir).expect_err("fifo feedback without depth should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("depth >= 1"))
    );
}

/// 跨进程反馈控制环：controller（ctrl_proc）↔ plant（plant_proc）经 zenoh，回边
/// plant.state→controller.state。`feedback_channel` 注入回边 channel 行（latest/fifo），
/// `determinism` 可注入 `[profile.default.determinism]` 子表。
fn cross_process_feedback_source(feedback_channel: &str, determinism: &str) -> String {
    format!(
        r#"
[package]
name = "xproc_feedback_demo"
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
process = "ctrl_proc"

[instance.controller.task]
trigger = "periodic"
period_ms = 10
input = ["state"]
output = ["cmd"]

[instance.plant]
component = "plant"
process = "plant_proc"

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
{feedback_channel}
feedback = true
init = {{ x = 0.0 }}

[profile.default]
backend = "zenoh"

{determinism}

[target.linux]
runtime = ["rust"]
backends = ["zenoh"]
"#
    )
}

#[test]
fn accepts_cross_process_latest_feedback() {
    // 跨进程 latest 反馈环：seeded latest-snapshot 语义，validator 放行。
    let ir = contract_of(&cross_process_feedback_source("channel = \"latest\"", ""));
    validate_contract(&ir).expect("cross-process latest feedback should validate");
}

#[test]
fn rejects_cross_process_fifo_feedback_without_global_tick() {
    // 非 global_tick profile 无共享 tick，fifo 的 N 拍延迟无意义，拒绝。
    let ir = contract_of(&cross_process_feedback_source(
        "channel = \"fifo\"\ndepth = 2",
        "",
    ));
    let report = validate_contract(&ir).expect_err("cross-process fifo feedback should fail");
    assert!(
        report.errors.iter().any(|error| error
            .message
            .contains("cross-process fifo feedback requires profile determinism mode global_tick")),
        "unexpected errors: {:?}",
        report.errors
    );
}

#[test]
fn allows_cross_process_fifo_feedback_only_in_global_tick_profile() {
    let ir = contract_of(&cross_process_feedback_source(
        "channel = \"fifo\"\ndepth = 2",
        r#"[profile.default.determinism]
mode = "global_tick"
timeout_ms = 1000
on_timeout = "fault_graph""#,
    ));
    validate_contract(&ir).expect("global_tick cross-process fifo feedback should validate");
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

#[test]
fn accepts_feedback_init_matching_type() {
    let ir = contract_of(&feedback_source(
        "feedback = true\ninit = { x = 0.5 }",
        "",
        "",
    ));
    validate_contract(&ir).unwrap();
}

fn nested_feedback_init_source(init: &str) -> String {
    format!(
        r#"
[package]
name = "nested_feedback_demo"
rsdl_version = "0.1"

[type.Pose]
x = "f64"
y = "f64"

[type.State]
pose = "Pose"
covariance = "[f64; 4]"
quality = "u8"

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
init = {init}

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#
    )
}

#[test]
fn accepts_feedback_init_nested_struct_and_fixed_array_sparse_overlay() {
    let ir = contract_of(&nested_feedback_init_source(
        "{ pose = { x = 1.0 }, covariance = [1.0, 0.0, 0.0, 1.0] }",
    ));
    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_feedback_init_fixed_array_length_mismatch() {
    let ir = contract_of(&nested_feedback_init_source(
        "{ covariance = [1.0, 0.0, 0.0] }",
    ));
    let report = validate_contract(&ir).expect_err("short fixed-array init should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("array length")),
        "unexpected errors: {:?}",
        report.errors
    );
}

#[test]
fn rejects_feedback_init_variable_frame_field() {
    let source = r#"
[package]
name = "variable_feedback_demo"
rsdl_version = "0.1"

[type.State]
label = "string<max=8>"

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
init = { label = "boot" }

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let ir = contract_of(source);
    let report = validate_contract(&ir).expect_err("variable-frame init should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("variable frame")),
        "unexpected errors: {:?}",
        report.errors
    );
}

#[test]
fn rejects_feedback_init_unknown_field() {
    let ir = contract_of(&feedback_source(
        "feedback = true\ninit = { y = 0.5 }",
        "",
        "",
    ));
    let report = validate_contract(&ir).expect_err("unknown init field should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("unknown field `y`"))
    );
}

#[test]
fn rejects_feedback_init_type_mismatch() {
    let ir = contract_of(&feedback_source(
        "feedback = true\ninit = { x = true }",
        "",
        "",
    ));
    let report = validate_contract(&ir).expect_err("init type mismatch should fail");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.message.contains("does not match type"))
    );
}
