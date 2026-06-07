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
fn rust_shell_runs_multiple_tasks_for_one_instance() {
    let ir = contract_from_source(
        r#"
[package]
name = "multi_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 100
output = ["slow"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let scheduler_step = generated_function_block(rust_shell, "fn step(");
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();

    assert_eq!(scheduler_step.matches("self.worker.on_tick(").count(), 2);
    assert!(scheduler_step.contains("let mut fast = flowrt::Output::<u32>::new();"));
    assert!(scheduler_step.contains("let mut slow = flowrt::Output::<u32>::new();"));
    assert_eq!(launch["graphs"][0]["tasks"][0]["name"], "fast_loop");
    assert_eq!(launch["graphs"][0]["tasks"][1]["name"], "slow_loop");
}

#[test]
fn cpp_shell_runs_multiple_tasks_for_one_instance() {
    let ir = contract_from_source(
        r#"
[package]
name = "multi_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 100
output = ["slow"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let scheduler_start = cpp_shell.find("flowrt::Status App::step(").unwrap();
    let scheduler_end = cpp_shell[scheduler_start..]
        .find("flowrt::Status App::step_startup(")
        .map(|offset| scheduler_start + offset)
        .unwrap();
    let scheduler_step = &cpp_shell[scheduler_start..scheduler_end];
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();

    assert_eq!(scheduler_step.matches("worker_->on_tick(").count(), 2);
    assert!(scheduler_step.contains("flowrt::Output<std::uint32_t> worker_fast;"));
    assert!(scheduler_step.contains("flowrt::Output<std::uint32_t> worker_slow;"));
    assert_eq!(selfdesc["graphs"][0]["tasks"][0]["name"], "fast_loop");
    assert_eq!(selfdesc["graphs"][0]["tasks"][1]["name"], "slow_loop");
    assert_eq!(selfdesc["graphs"][0]["scheduler"]["worker_threads"], 1);
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["lanes"][0],
        serde_json::json!({"name": "worker_serial", "kind": "serial", "instance": "worker"})
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["lane"],
        "worker_serial"
    );
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
        .find(
            "self.step_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new())",
        )
        .unwrap();
    let scheduler_call = run
        .find("let mut scheduler = flowrt::DeterministicExecutor")
        .unwrap();
    let shutdown_call = run
        .find("self.step_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new())")
        .unwrap();
    let startup_step = generated_function_block(rust_shell, "fn step_startup");
    let shutdown_step = generated_function_block(rust_shell, "fn step_shutdown");
    let scheduler_step = generated_function_block(rust_shell, "fn step(");

    assert!(startup_call < scheduler_call);
    assert!(scheduler_call < shutdown_call);
    assert!(run.contains(
        "if status == flowrt::Status::Ok {\n            status = self.step_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());\n        }"
    ));
    assert!(run.contains("let shutdown = flowrt::install_signal_shutdown_token();"));
    assert!(run.contains("&& !shutdown.is_requested()"));
    assert!(!run.contains("backend.scheduler().run_ticks_until_shutdown("));
    assert!(startup_step.contains("match self.boot.on_tick()"));
    assert!(shutdown_step.contains("match self.cleanup.on_tick()"));
    assert!(!scheduler_step.contains("match self.boot.on_tick()"));
    assert!(!scheduler_step.contains("match self.cleanup.on_tick()"));
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
        .find("status = step_startup(0, lifecycle_context, introspection_state, scheduler_events, startup_health_map)")
        .unwrap();
    let scheduler_call = run.find("flowrt::DeterministicExecutor scheduler").unwrap();
    let shutdown_call = run
        .find("status = step_shutdown(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map)")
        .unwrap();

    assert!(startup_call < scheduler_call);
    assert!(scheduler_call < shutdown_call);
    assert!(run.contains(
        "if (status == flowrt::Status::Ok) {\n        std::map<std::string, flowrt::IntrospectionTaskHealth> shutdown_health_map;\n        status = step_shutdown(0, lifecycle_context, introspection_state, scheduler_events, shutdown_health_map);\n    }"
    ));
    assert!(run.contains("auto shutdown = flowrt::install_signal_shutdown_token();"));
    assert!(run.contains("!shutdown.is_requested()"));
    assert!(!run.contains("backend.scheduler().run_ticks_until_shutdown("));
    let startup_step = generated_function_block(cpp_shell, "App::step_startup");
    let shutdown_step = generated_function_block(cpp_shell, "App::step_shutdown");
    let scheduler_step = generated_function_block(cpp_shell, "App::step(");
    assert!(startup_step.contains("switch (boot_->on_tick()"));
    assert!(shutdown_step.contains("switch (cleanup_->on_tick()"));
    assert!(!scheduler_step.contains("switch (boot_->on_tick()"));
    assert!(!scheduler_step.contains("switch (cleanup_->on_tick()"));
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
        .find("match self.source.on_tick(&mut sample)")
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
        .find("switch (source_->on_tick(source_sample))")
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

#[test]
fn rust_shell_uses_all_ready_guard_for_on_message_tasks() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source_a]
language = "rust"
output = ["sample:Sample"]

[component.source_b]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["left:Sample", "right:Sample"]

[instance.source_a]
component = "source_a"

[instance.source_a.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.source_b]
component = "source_b"

[instance.source_b.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
readiness = "all_ready"
input = ["left", "right"]

[[bind.dataflow]]
from = "source_a.sample"
to = "sink.left"
channel = "latest"

[[bind.dataflow]]
from = "source_b.sample"
to = "sink.right"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains(
        "if self.bind_0.revision() != bind_0_seen_revision_for_sink_main && self.bind_1.revision() != bind_1_seen_revision_for_sink_main"
    ));
    assert!(rust_shell.contains("if left.present() && right.present() {"));
}

#[test]
fn rust_shell_builds_scheduler_v2_task_plan_and_wakes_on_input_revision() {
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

[profile.default]
backend = "inproc"
worker_threads = 2
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains("let mut scheduler = flowrt::DeterministicExecutor::new(2);"));
    assert!(rust_shell.contains("let scheduler_base_period_ms: u64 = 5;"));
    assert!(rust_shell.contains("let tick_time_ms = scheduler_now_ms;"));
    assert!(rust_shell.contains("scheduler.add_periodic(flowrt::PeriodicSpec"));
    assert!(rust_shell.contains("let mut bind_0_seen_revision_for_sink_main: u64 = 0;"));
    assert!(rust_shell.contains("if self.bind_0.revision() != bind_0_seen_revision_for_sink_main"));
    assert!(rust_shell.contains("scheduler.wake(flowrt::TaskId("));
    assert!(rust_shell.contains("scheduler.run_ready(|task| match task"));
    assert!(rust_shell.contains("let mut woke_on_message = false;"));
    assert!(rust_shell.contains("woke_on_message = true;"));
    assert!(rust_shell.contains("if !woke_on_message && task_statuses.is_empty()"));
    assert!(rust_shell.contains("let mut observed_data_generation: u64;"));
    assert!(rust_shell.contains(
        "loop {\n                observed_data_generation = scheduler_events.data_generation();"
    ));
    assert!(!rust_shell.contains(
        "while status == flowrt::Status::Ok\n            && !shutdown.is_requested()\n            && run_ticks\n                .map(|limit| tick_base < limit)\n                .unwrap_or(true)\n        {\n            let mut observed_data_generation = scheduler_events.data_generation();"
    ));
    assert!(rust_shell.contains("let scheduler_events = flowrt::ScheduleWaiter::new();"));
    assert!(rust_shell.contains("scheduler_events.notify_data();"));
    assert!(rust_shell.contains(
        "scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, &shutdown)"
    ));
    assert!(!rust_shell.contains("backend.scheduler().run_ticks_until_shutdown(1"));
    assert!(rust_shell.contains("flowrt::Status::Retry => return flowrt::Status::Retry,"));
    assert!(rust_shell.contains("if task_status == flowrt::Status::Error"));
}

#[test]
fn rust_shell_reads_cached_transport_sample_after_on_message_wake_probe() {
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
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "fifo"
depth = 4
backend = "zenoh"

[profile.default]
backend = "zenoh"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let sink_step = generated_function_block(rust_shell, "fn step_task_sink_main");

    assert!(rust_shell.contains("self.bind_0.set_schedule_waiter(scheduler_events.clone());"));
    assert!(rust_shell.contains("let _ = self.bind_0.receive_latest_at(tick_time_ms);"));
    assert!(sink_step.contains("let sample = self.bind_0.cached_latest_at(tick_time_ms);"));
    assert!(!sink_step.contains("receive_latest_at(tick_time_ms)"));
}

#[test]
fn cpp_shell_uses_all_ready_guard_for_on_message_tasks() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source_a]
language = "cpp"
output = ["sample:Sample"]

[component.source_b]
language = "cpp"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["left:Sample", "right:Sample"]

[instance.source_a]
component = "source_a"

[instance.source_a.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.source_b]
component = "source_b"

[instance.source_b.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
readiness = "all_ready"
input = ["left", "right"]

[[bind.dataflow]]
from = "source_a.sample"
to = "sink.left"
channel = "latest"

[[bind.dataflow]]
from = "source_b.sample"
to = "sink.right"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(cpp_shell.contains(
        "if (bind_0_.revision() != bind_0_seen_revision_for_sink_main && bind_1_.revision() != bind_1_seen_revision_for_sink_main)"
    ));
    assert!(cpp_shell.contains("if (sink_left.present() && sink_right.present()) {"));
}

#[test]
fn cpp_shell_builds_scheduler_v2_task_plan_and_wakes_on_input_revision() {
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

[profile.default]
backend = "inproc"
worker_threads = 2
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(cpp_shell.contains("flowrt::DeterministicExecutor scheduler{2};"));
    assert!(cpp_shell.contains("const auto scheduler_base_period_ms = std::uint64_t{5};"));
    assert!(cpp_shell.contains("const auto tick_time_ms = scheduler_now_ms;"));
    assert!(cpp_shell.contains("scheduler.add_periodic(flowrt::PeriodicSpec"));
    assert!(cpp_shell.contains("std::uint64_t bind_0_seen_revision_for_sink_main = 0;"));
    assert!(cpp_shell.contains("if (bind_0_.revision() != bind_0_seen_revision_for_sink_main)"));
    assert!(cpp_shell.contains("scheduler.wake(flowrt::TaskId{"));
    assert!(cpp_shell.contains("scheduler.run_ready([this, &lifecycle_context, &introspection_state, &scheduler_events, &health_map, tick_time_ms](flowrt::TaskId task)"));
    assert!(cpp_shell.contains("bool woke_on_message = false;"));
    assert!(cpp_shell.contains("woke_on_message = true;"));
    assert!(cpp_shell.contains("if (!woke_on_message && task_statuses.empty())"));
    assert!(
        cpp_shell.contains(
            "std::uint64_t observed_data_generation = scheduler_events.data_generation();"
        )
    );
    assert!(cpp_shell.contains(
        "while (true) {\n            observed_data_generation = scheduler_events.data_generation();"
    ));
    assert!(!cpp_shell.contains(
        "while (status == flowrt::Status::Ok && !shutdown.is_requested() && (!run_ticks.has_value() || tick_base < *run_ticks)) {\n        const auto observed_data_generation = scheduler_events.data_generation();"
    ));
    assert!(cpp_shell.contains("flowrt::ScheduleWaiter scheduler_events;"));
    assert!(cpp_shell.contains("scheduler_events.notify_data();"));
    assert!(cpp_shell.contains(
        "scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, shutdown)"
    ));
    assert!(!cpp_shell.contains("backend.scheduler().run_ticks_until_shutdown("));
    assert!(cpp_shell.contains("case flowrt::Status::Retry:"));
    assert!(cpp_shell.contains("return flowrt::Status::Retry;"));
    assert!(cpp_shell.contains("if (task_status == flowrt::Status::Error)"));
}

#[test]
fn cpp_shell_reads_cached_transport_sample_after_on_message_wake_probe() {
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
channel = "fifo"
depth = 4
backend = "zenoh"

[profile.default]
backend = "zenoh"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let sink_step = generated_function_block(cpp_shell, "App::step_task_sink_main");

    assert!(cpp_shell.contains("bind_0_.set_schedule_waiter(scheduler_events);"));
    assert!(cpp_shell.contains("(void)bind_0_.receive_latest_at(tick_time_ms);"));
    assert!(sink_step.contains("const auto sink_sample = bind_0_.cached_latest_at(tick_time_ms);"));
    assert!(!sink_step.contains("receive_latest_at(tick_time_ms)"));
}

#[test]
fn rust_shell_maps_fifo_backpressure_to_retry_without_global_error() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
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
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "fifo"
depth = 1
overflow = "block"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let source_step = generated_function_block(rust_shell, "fn step_task_source_main");
    let run_loop = &rust_shell[rust_shell
        .find("let mut scheduler = flowrt::DeterministicExecutor")
        .unwrap()..];

    assert!(source_step.contains("Ok(flowrt::ChannelWriteOutcome::Backpressured) =>"));
    assert!(source_step.contains("backpressure += 1"));
    assert!(source_step.contains("return flowrt::Status::Retry"));
    assert!(run_loop.contains("if task_status == flowrt::Status::Error"));
    assert!(!run_loop.contains("if task_status != flowrt::Status::Ok"));
}

#[test]
fn cpp_shell_maps_fifo_backpressure_to_retry_without_global_error() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["sample:u32"]

[component.sink]
language = "cpp"
input = ["sample:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "fifo"
depth = 1
overflow = "block"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let source_step = generated_function_block(cpp_shell, "App::step_task_source_main");
    let run_loop = &cpp_shell[cpp_shell
        .find("flowrt::DeterministicExecutor scheduler")
        .unwrap()..];

    assert!(source_step.contains("case flowrt::Status::Retry:"));
    assert!(source_step.contains("return flowrt::Status::Retry;"));
    assert!(run_loop.contains("if (task_status == flowrt::Status::Error)"));
    assert!(!run_loop.contains("if (task_status != flowrt::Status::Ok)"));
}
