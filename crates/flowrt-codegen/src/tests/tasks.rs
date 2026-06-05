use super::*;
use flowrt_ir::TriggerKind;

#[test]
fn generated_shells_cleanup_entered_lifecycle_stages_in_reverse_order() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.cpp_alpha]
language = "cpp"

[component.cpp_beta]
language = "cpp"

[component.rust_alpha]
language = "rust"

[component.rust_beta]
language = "rust"

[instance.cpp_alpha]
component = "cpp_alpha"

[instance.cpp_alpha.task]
trigger = "periodic"
period_ms = 5

[instance.cpp_beta]
component = "cpp_beta"

[instance.cpp_beta.task]
trigger = "periodic"
period_ms = 5

[instance.rust_alpha]
component = "rust_alpha"

[instance.rust_alpha.task]
trigger = "periodic"
period_ms = 5

[instance.rust_beta]
component = "rust_beta"

[instance.rust_beta.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(cpp_shell.contains("auto status = flowrt::Status::Ok;"));
    assert!(cpp_shell.contains("bool cpp_alpha_initialized = false;"));
    assert!(cpp_shell.contains("bool cpp_alpha_started = false;"));
    assert!(cpp_shell.contains("if (status == flowrt::Status::Ok) {"));
    assert!(
        cpp_shell
            .contains("if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok)")
    );
    assert!(
        cpp_shell
            .contains("if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok)")
    );
    assert!(cpp_shell.contains("return status;"));
    assert!(!cpp_shell.contains("if (status != flowrt::Status::Ok) {\n        return status;"));
    assert!(
        cpp_shell
            .find("if (cpp_beta_started && cpp_beta_)")
            .unwrap()
            < cpp_shell
                .find("if (cpp_alpha_started && cpp_alpha_)")
                .unwrap()
    );
    assert!(
        cpp_shell
            .find("if (cpp_beta_initialized && cpp_beta_)")
            .unwrap()
            < cpp_shell
                .find("if (cpp_alpha_initialized && cpp_alpha_)")
                .unwrap()
    );

    assert!(rust_shell.contains("let mut status = flowrt::Status::Ok;"));
    assert!(rust_shell.contains("let mut rust_alpha_initialized = false;"));
    assert!(rust_shell.contains("let mut rust_alpha_started = false;"));
    assert!(rust_shell.contains("if status == flowrt::Status::Ok {"));
    assert!(
        rust_shell.contains("if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok")
    );
    assert!(
        rust_shell
            .contains("if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok")
    );
    assert!(rust_shell.contains("        status\n    }\n"));
    assert!(
        !rust_shell
            .contains("if status != flowrt::Status::Ok {\n            return status;\n        }")
    );
    assert!(
        rust_shell.find("if rust_beta_started {").unwrap()
            < rust_shell.find("if rust_alpha_started {").unwrap()
    );
    assert!(
        rust_shell.find("if rust_beta_initialized {").unwrap()
            < rust_shell.find("if rust_alpha_initialized {").unwrap()
    );
}

#[test]
fn mixed_rust_shell_does_not_invent_traits_for_cpp_components() {
    let ir = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "cpp_source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "rust_sink"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let rust_components = artifact_content(&bundle, "rust/src/components.rs");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(!rust_components.contains("pub trait Source"));
    assert!(rust_components.contains("pub trait Sink"));
    assert!(!rust_shell.contains("source: Box<dyn Source>"));
    assert!(rust_shell.contains("sink: Box<dyn Sink>"));
    assert!(!rust_shell.contains("mixed-language runtime shell is not implemented"));
    assert!(rust_shell.contains("flowrt::iox2::Iox2PubSub<u32>"));
    assert!(rust_shell.contains("receive_latest_at(tick_time_ms)"));
    assert!(!cpp_header.contains("std::unique_ptr<SinkInterface>"));
    assert!(cpp_header.contains("std::unique_ptr<SourceInterface> source"));
    assert!(!cpp_shell.contains("return flowrt::ok();"));
    assert!(cpp_shell.contains("flowrt::iox2::Iox2PubSub<std::uint32_t>"));
    assert!(cpp_shell.contains("bind_0_.publish_at(*value, tick_time_ms)"));
}

#[test]
fn rust_shell_accepts_multiple_binds_between_same_instance_pair() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["left:Sample", "right:Sample"]

[component.sink]
language = "rust"
input = ["left:Sample", "right:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["left", "right"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["left", "right"]

[[bind.dataflow]]
from = "source.left"
to = "sink.left"
channel = "latest"

[[bind.dataflow]]
from = "source.right"
to = "sink.right"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains("source: Box<dyn Source>"));
    assert!(rust_shell.contains("sink: Box<dyn Sink>"));
}

#[test]
fn rust_shell_uses_task_port_subset_for_channel_io() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.used_source]
language = "rust"
output = ["used_out:Sample"]

[component.unused_source]
language = "rust"
output = ["unused_out:Sample"]

[component.sink]
language = "rust"
input = ["used_in:Sample", "unused_in:Sample"]
output = ["used_out:Sample", "unused_out:Sample"]

[component.monitor]
language = "rust"
input = ["used_in:Sample", "unused_in:Sample"]

[instance.used_source]
component = "used_source"

[instance.used_source.task]
trigger = "periodic"
period_ms = 5
output = ["used_out"]

[instance.unused_source]
component = "unused_source"

[instance.unused_source.task]
trigger = "periodic"
period_ms = 5
output = ["unused_out"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["used_in"]
output = ["used_out"]

[instance.monitor]
component = "monitor"

[instance.monitor.task]
trigger = "on_message"
input = ["used_in", "unused_in"]

[[bind.dataflow]]
from = "used_source.used_out"
to = "sink.used_in"
channel = "latest"

[[bind.dataflow]]
from = "unused_source.unused_out"
to = "sink.unused_in"
channel = "latest"

[[bind.dataflow]]
from = "sink.used_out"
to = "monitor.used_in"
channel = "latest"

[[bind.dataflow]]
from = "sink.unused_out"
to = "monitor.unused_in"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let bind_index = |to_instance: &str, to_port: &str| {
        ir.graphs[0]
            .binds
            .iter()
            .position(|bind| {
                bind.to.instance.name == to_instance && bind.to.port.as_str() == to_port
            })
            .unwrap()
    };
    let sink_used_bind = bind_index("sink", "used_in");
    let sink_unused_bind = bind_index("sink", "unused_in");
    let monitor_used_bind = bind_index("monitor", "used_in");
    let sink_used_read =
        format!("        let used_in = self.bind_{sink_used_bind}.view_at(tick_time_ms);");
    let monitor_used_read =
        format!("        let used_in = self.bind_{monitor_used_bind}.view_at(tick_time_ms);");
    let sink_step_start = rust_shell.find(&sink_used_read).unwrap();
    let monitor_step_start = rust_shell.find(&monitor_used_read).unwrap();
    let sink_step = &rust_shell[sink_step_start..monitor_step_start];

    assert!(sink_step.contains(&sink_used_read));
    assert!(sink_step.contains("let unused_in = flowrt::Latest::new(None, false);"));
    assert!(sink_step.contains("let mut used_out = flowrt::Output::<Sample>::new();"));
    assert!(sink_step.contains("let mut unused_out = flowrt::Output::<Sample>::new();"));
    assert!(sink_step.contains("if used_in.present() {"));
    assert!(
        sink_step.contains("self.sink.on_tick(used_in, unused_in, &mut used_out, &mut unused_out)")
    );
    assert!(sink_step.contains("if let Some(value) = used_out.as_ref().cloned()"));
    assert!(!sink_step.contains(&format!(
        "self.bind_{sink_unused_bind}.view_at(tick_time_ms)"
    )));
    assert!(!sink_step.contains("if let Some(value) = unused_out.as_ref().cloned()"));
}

#[test]
fn rust_shell_runs_startup_and_shutdown_tasks_outside_tick_loop() {
    let mut ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.boot]
language = "rust"

[component.cleanup]
language = "rust"

[instance.boot]
component = "boot"

[instance.boot.task]
trigger = "periodic"
period_ms = 5

[instance.cleanup]
component = "cleanup"

[instance.cleanup.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    ir.graphs[0].tasks[0].trigger = TriggerKind::Startup;
    ir.graphs[0].tasks[0].period_ms = None;
    ir.graphs[0].tasks[1].trigger = TriggerKind::Shutdown;
    ir.graphs[0].tasks[1].period_ms = None;

    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let run_start = rust_shell.find("    pub fn run(").unwrap();
    let run = &rust_shell[run_start..];
    let startup_call = run
        .find("self.step_startup(0, &mut lifecycle_context, &introspection_state)")
        .unwrap();
    let scheduler_call = run.find("backend.scheduler().run_ticks").unwrap();
    let shutdown_call = run
        .find("self.step_shutdown(0, &mut lifecycle_context, &introspection_state)")
        .unwrap();
    let startup_step = generated_function_block(rust_shell, "fn step_startup");
    let shutdown_step = generated_function_block(rust_shell, "fn step_shutdown");
    let scheduler_step = generated_function_block(rust_shell, "fn step(");

    assert!(startup_call < scheduler_call);
    assert!(scheduler_call < shutdown_call);
    assert!(run.contains("let shutdown = flowrt::install_signal_shutdown_token();"));
    assert!(run.contains("&& !shutdown.is_requested()"));
    assert!(run.contains("backend.scheduler().run_ticks_until_shutdown("));
    assert!(startup_step.contains("if self.boot.on_tick()"));
    assert!(shutdown_step.contains("if self.cleanup.on_tick()"));
    assert!(!scheduler_step.contains("if self.boot.on_tick()"));
    assert!(!scheduler_step.contains("if self.cleanup.on_tick()"));
}

#[test]
fn cpp_shell_runs_startup_and_shutdown_tasks_outside_tick_loop() {
    let mut ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.boot]
language = "cpp"

[component.cleanup]
language = "cpp"

[instance.boot]
component = "boot"

[instance.boot.task]
trigger = "periodic"
period_ms = 5

[instance.cleanup]
component = "cleanup"

[instance.cleanup.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    ir.graphs[0].tasks[0].trigger = TriggerKind::Startup;
    ir.graphs[0].tasks[0].period_ms = None;
    ir.graphs[0].tasks[1].trigger = TriggerKind::Shutdown;
    ir.graphs[0].tasks[1].period_ms = None;

    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let run_start = cpp_shell.find("flowrt::Status App::run(").unwrap();
    let run = &cpp_shell[run_start..];
    let startup_call = run
        .find("status = step_startup(0, lifecycle_context, introspection_state)")
        .unwrap();
    let scheduler_call = run.find("backend.scheduler().run_ticks").unwrap();
    let shutdown_call = run
        .find("status = step_shutdown(0, lifecycle_context, introspection_state)")
        .unwrap();

    assert!(startup_call < scheduler_call);
    assert!(scheduler_call < shutdown_call);
    assert!(run.contains("auto shutdown = flowrt::install_signal_shutdown_token();"));
    assert!(run.contains("!shutdown.is_requested()"));
    assert!(run.contains("backend.scheduler().run_ticks_until_shutdown("));
    assert!(cpp_shell.contains("flowrt::Status App::step_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (boot_ && boot_->on_tick()"));
    assert!(cpp_shell.contains("flowrt::Status App::step_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (cleanup_ && cleanup_->on_tick()"));
    assert!(!cpp_shell.contains("flowrt::Status App::step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (boot_ && boot_->on_tick()"));
    assert!(!cpp_shell.contains("flowrt::Status App::step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (cleanup_ && cleanup_->on_tick()"));
}

#[test]
fn rust_shell_enforces_task_deadline_before_publishing_outputs() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
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
deadline_ms = 10
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
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let deadline_start = rust_shell
        .find("let source_deadline_started_at = std::time::Instant::now();")
        .unwrap();
    let source_call = rust_shell
        .find("self.source.on_tick(&mut sample) != flowrt::Status::Ok")
        .unwrap();
    let deadline_guard = rust_shell
        .find("source_deadline_started_at.elapsed() > std::time::Duration::from_millis(10)")
        .unwrap();
    let publish = rust_shell
        .find("if let Some(value) = sample.as_ref().cloned()")
        .unwrap();

    assert!(deadline_start < source_call);
    assert!(source_call < deadline_guard);
    assert!(deadline_guard < publish);
}

#[test]
fn cpp_shell_enforces_task_deadline_before_publishing_outputs() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "cpp"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
deadline_ms = 10
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
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let deadline_start = cpp_shell
        .find("const auto source_deadline_started_at = std::chrono::steady_clock::now();")
        .unwrap();
    let source_call = cpp_shell
        .find("source_ && source_->on_tick(source_sample) != flowrt::Status::Ok")
        .unwrap();
    let deadline_guard = cpp_shell
            .find("std::chrono::steady_clock::now() - source_deadline_started_at > std::chrono::milliseconds{10}")
            .unwrap();
    let publish = cpp_shell
        .find("if (const auto* value = source_sample.as_ref())")
        .unwrap();

    assert!(deadline_start < source_call);
    assert!(source_call < deadline_guard);
    assert!(deadline_guard < publish);
}

#[test]
fn cpp_shell_gates_on_message_instances_on_present_inputs() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "cpp"
output = ["sample:Sample"]

[component.sink]
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

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let source_call = cpp_shell.find("source_->on_tick(source_sample)").unwrap();
    let gate = cpp_shell.find("if (sink_sample.present()) {").unwrap();
    let sink_call = cpp_shell.find("sink_->on_tick(sink_sample)").unwrap();

    assert!(source_call < gate);
    assert!(gate < sink_call);
}
