use super::*;

#[test]
fn rust_shell_wires_island_boundary_endpoints() {
    let ir = contract_from_source(
        r#"
[package]
name = "island_rust_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "rust"
input = ["sample:Sample"]
output = ["echo:Sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]
output = ["echo"]

[profile.dev]
mode = "island"
backend = "inproc"

[[boundary.input]]
name = "sample_in"
port = "consumer.sample"
type = "Sample"

[[boundary.output]]
name = "echo_out"
port = "consumer.echo"
type = "Sample"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");

    assert!(rust_shell.contains("boundary_input_sample_in: flowrt::BoundaryInput<Sample>"));
    assert!(rust_shell.contains("boundary_output_echo_out: flowrt::BoundaryOutput<Sample>"));
    assert!(rust_shell.contains("boundary_input_sample_in: flowrt::BoundaryInput::new()"));
    assert!(rust_shell.contains(
        "introspection_state.register_boundary_input::<Sample>(\"sample_in\", \"Sample\", self.boundary_input_sample_in.clone());"
    ));
    assert!(
        rust_shell.contains(
            "self.boundary_input_sample_in.set_schedule_waiter(scheduler_events.clone());"
        )
    );
    assert!(
        rust_shell
            .contains("let sample_read = self.boundary_input_sample_in.read_at(tick_time_ms);")
    );
    assert!(rust_shell.contains("let __flowrt_sample_revision = sample_read.revision();"));
    assert!(rust_shell.contains("let sample = sample_read.view();"));
    assert!(rust_shell.contains("self.boundary_input_sample_in.revision() != boundary_input_sample_in_seen_revision_for_consumer_main"));
    assert!(rust_shell.contains("self.boundary_output_echo_out.publish_at(&value, tick_time_ms);"));
    assert!(rust_shell.contains(
        "let _boundary_output_echo_out_probe = self.boundary_output_echo_out.register_sink"
    ));
    assert!(rust_shell.contains("introspection_state.record_channel_publish_bytes(\"echo_out\""));
    assert!(rust_messages.contains("impl flowrt::WireCodec for Sample"));
}

#[test]
fn cpp_shell_wires_island_boundary_endpoints() {
    let ir = contract_from_source(
        r#"
[package]
name = "island_cpp_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.consumer]
language = "cpp"
input = ["sample:Sample"]
output = ["echo:Sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]
output = ["echo"]

[profile.dev]
mode = "island"
backend = "inproc"

[[boundary.input]]
name = "sample_in"
port = "consumer.sample"
type = "Sample"

[[boundary.output]]
name = "echo_out"
port = "consumer.echo"
type = "Sample"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");

    assert!(cpp_header.contains("flowrt::BoundaryInput<Sample> boundary_input_sample_in_;"));
    assert!(cpp_header.contains("flowrt::BoundaryOutput<Sample> boundary_output_echo_out_;"));
    assert!(cpp_shell.contains(
        "introspection_state.register_boundary_input(\"sample_in\", \"Sample\", boundary_input_sample_in_);"
    ));
    assert!(cpp_shell.contains("boundary_input_sample_in_.set_schedule_waiter(scheduler_events);"));
    assert!(cpp_shell.contains(
        "const auto consumer_sample_read = boundary_input_sample_in_.read_at(tick_time_ms);"
    ));
    assert!(cpp_shell.contains("const auto consumer_sample = consumer_sample_read.view();"));
    assert!(cpp_shell.contains("boundary_input_sample_in_.revision() != boundary_input_sample_in_seen_revision_for_consumer_main"));
    assert!(cpp_shell.contains("boundary_output_echo_out_.publish_at(*value, tick_time_ms);"));
    assert!(
        cpp_shell.contains(
            "auto boundary_output_echo_out_probe = boundary_output_echo_out_.register_sink"
        )
    );
    assert!(cpp_shell.contains("introspection_state.record_channel_publish_bytes(\"echo_out\""));
    assert!(cpp_messages.contains("static constexpr std::size_t wire_size() noexcept"));
}

#[test]
fn strict_shells_do_not_emit_boundary_primitives() {
    let ir = contract_from_source(
        r#"
[package]
name = "strict_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[component.cpp_source]
language = "cpp"
output = ["sample:Sample"]

[component.cpp_sink]
language = "cpp"
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

[instance.cpp_source]
component = "cpp_source"

[instance.cpp_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.cpp_sink]
component = "cpp_sink"

[instance.cpp_sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[[bind.dataflow]]
from = "cpp_source.sample"
to = "cpp_sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let cpp_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(!rust_shell.contains("BoundaryInput"));
    assert!(!rust_shell.contains("BoundaryOutput"));
    assert!(!cpp_header.contains("BoundaryInput"));
    assert!(!cpp_header.contains("BoundaryOutput"));
    assert!(!cpp_shell.contains("BoundaryInput"));
    assert!(!cpp_shell.contains("BoundaryOutput"));
}

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
