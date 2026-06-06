use super::*;

#[test]
fn emits_inproc_stale_channel_reads_from_bind_policy() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "drop"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains(
            "flowrt::LatestChannel::with_stale_config(flowrt::StaleConfig::new(Some(20), flowrt::StalePolicy::Drop))"
        ));
    assert!(rust_shell.contains("publish_at(value.clone(), tick_time_ms)"));
    assert!(rust_shell.contains("view_at(tick_time_ms)"));
}

#[test]
fn emits_inproc_fifo_stale_channel_reads_from_bind_policy() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "fifo"
depth = 4
overflow = "drop_oldest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains(
            "flowrt::FifoChannel::with_stale_config(4, flowrt::OverflowPolicy::DropOldest, flowrt::StaleConfig::new(Some(20), flowrt::StalePolicy::Error))"
        ));
    assert!(rust_shell.contains("let imu_read = self.bind_0.pop_at(tick_time_ms);"));
    assert!(rust_shell.contains("let imu = imu_read.view();"));
    assert!(rust_shell.contains("push_at(value.clone(), tick_time_ms)"));
    assert!(rust_shell.contains("if imu.stale() {"));
    assert!(rust_shell.contains("return flowrt::Status::Error;"));
    assert!(rust_shell.find("if imu.stale()").unwrap() < rust_shell.find(".on_tick(imu)").unwrap());
}

#[test]
fn emits_stale_error_guard_before_user_tick() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains("if imu.stale() {"));
    assert!(rust_shell.contains("return flowrt::Status::Error;"));
    assert!(rust_shell.find("if imu.stale()").unwrap() < rust_shell.find(".on_tick(imu)").unwrap());
}

#[test]
fn cpp_shell_emits_stale_channel_reads_from_bind_policy() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "drop"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(cpp_shell.contains(
            "flowrt::LatestChannel<Imu>::with_stale_config(flowrt::StaleConfig{std::chrono::milliseconds{20}, flowrt::StalePolicy::Drop})"
        ));
    assert!(cpp_shell.contains("const auto tick_time_ms = static_cast<std::uint64_t>(tick);"));
    assert!(cpp_shell.contains("publish_at(*value, tick_time_ms)"));
    assert!(cpp_shell.contains("view_at(tick_time_ms)"));
}

#[test]
fn cpp_shell_emits_fifo_stale_channel_reads_from_bind_policy() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "fifo"
depth = 4
overflow = "drop_oldest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(cpp_shell.contains(
            "flowrt::FifoChannel<Imu>::with_stale_config(4, flowrt::OverflowPolicy::DropOldest, flowrt::StaleConfig{std::chrono::milliseconds{20}, flowrt::StalePolicy::Error})"
        ));
    assert!(cpp_shell.contains("auto sink_imu_read = bind_0_.pop_at(tick_time_ms);"));
    assert!(cpp_shell.contains("const auto sink_imu = sink_imu_read.view();"));
    assert!(cpp_shell.contains("push_at(*value, tick_time_ms)"));
    assert!(cpp_shell.contains("if (sink_imu.stale()) {"));
    assert!(cpp_shell.contains("return flowrt::Status::Error;"));
    assert!(
        cpp_shell.find("if (sink_imu.stale())").unwrap()
            < cpp_shell.find("sink_->on_tick(sink_imu)").unwrap()
    );
}

#[test]
fn cpp_shell_emits_stale_error_guard_before_user_tick() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(cpp_shell.contains("if (sink_imu.stale()) {"));
    assert!(cpp_shell.contains("return flowrt::Status::Error;"));
    assert!(
        cpp_shell.find("if (sink_imu.stale())").unwrap()
            < cpp_shell.find("sink_->on_tick(sink_imu)").unwrap()
    );
}
