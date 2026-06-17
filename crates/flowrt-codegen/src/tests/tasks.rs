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
        rust_shell
            .find("if rust_beta_started {\n            let stop_status")
            .unwrap()
            < rust_shell
                .find("if rust_alpha_started {\n            let stop_status")
                .unwrap()
    );
    assert!(
        rust_shell
            .find("if rust_beta_initialized {\n            let shutdown_status")
            .unwrap()
            < rust_shell
                .find("if rust_alpha_initialized {\n            let shutdown_status")
                .unwrap()
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
    assert!(!rust_shell.contains("source: Box<dyn Source"));
    assert!(rust_shell.contains("sink: std::sync::Arc<std::sync::Mutex<Box<dyn Sink + Send>>>"));
    assert!(!rust_shell.contains("mixed-language runtime shell is not implemented"));
    assert!(rust_shell.contains("flowrt::iox2::Iox2PubSub<u32>"));
    assert!(rust_shell.contains("receive_latest_at(tick_time_ms)"));
    assert!(!cpp_header.contains("std::unique_ptr<SinkInterface>"));
    assert!(cpp_header.contains("std::unique_ptr<SourceInterface> source"));
    assert!(!cpp_shell.contains("return flowrt::ok();"));
    assert!(cpp_shell.contains("flowrt::iox2::Iox2PubSub<std::uint32_t>"));
    assert!(cpp_shell.contains("flowrt_output_commits.emplace_back"));
    assert!(cpp_shell.contains("app.bind_0_.publish_at(*value, tick_time_ms)"));
}

#[test]
fn rust_iox2_shell_uses_local_scheduler_affinity() {
    let ir = contract_from_source(
        r#"
[package]
name = "iox2_affinity_demo"
rsdl_version = "0.1"

[type.FrameHandle]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"

[component.camera]
language = "rust"
output = ["frame:FrameHandle"]

[component.processor]
language = "rust"
input = ["frame:FrameHandle"]

[instance.camera]
component = "camera"

[instance.camera.task]
trigger = "periodic"
period_ms = 20
output = ["frame"]

[instance.processor]
component = "processor"

[instance.processor.task]
trigger = "on_message"
input = ["frame"]

[[bind.dataflow]]
from = "camera.frame"
to = "processor.frame"
channel = "latest"

[profile.default]
backend = "iox2"
worker_threads = 4
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains("flowrt::iox2::Iox2PubSub<FrameHandle>"));
    assert!(rust_shell.contains("let mut scheduler = flowrt::DeterministicExecutor::new(4);"));
    assert!(rust_shell.contains("let worker_pool = flowrt::WorkerPool::new(4);"));
    assert!(rust_shell.contains(
        "let task_completion_queue = flowrt::WorkerCompletionQueue::<Vec<FlowrtOutputCommit>>::new();"
    ));
    assert!(rust_shell.contains("worker_pool.submit_collect(admission.task"));
    assert!(rust_shell.contains("scheduler.complete_task(task_result.task)"));
    assert!(!rust_shell.contains("let app = self.clone();"));
    assert!(!rust_shell.contains("let app = app.clone();"));
    assert!(!rust_shell.contains("app.step_task_camera_main"));
    assert!(!rust_shell.contains("app.step_task_processor_main"));
    assert!(rust_shell.contains("let __flowrt_component_camera = app.camera.clone();"));
    assert!(rust_shell.contains("let __flowrt_component_processor = app.processor.clone();"));
    assert!(rust_shell.contains(
        "let (__flowrt_input_frame_value, __flowrt_input_frame_stale, __flowrt_input_frame_revision)"
    ));
    assert!(rust_shell.contains("cached_latest_at(tick_time_ms)"));
    assert!(rust_shell.contains("Self::step_task_camera_main(__flowrt_component_camera"));
    assert!(rust_shell.contains(
        "Self::step_task_processor_main(__flowrt_component_processor, __flowrt_input_frame_value"
    ));
    assert!(rust_shell.contains("flowrt::Context::with_timing(flowrt::TaskTiming"));
    assert!(rust_shell.contains("pending_task_admissions.insert(admission.task, admission);"));
    assert!(rust_shell.contains("pending_task_admissions.remove(&task_result.task)"));
    assert!(rust_shell.contains("health.inflight = true;"));
    assert!(rust_shell.contains("health.inflight = false;"));
    assert!(rust_shell.contains("health.scheduled_time_ms = Some(admission.scheduled_time_ms);"));
    assert!(rust_shell.contains("health.observed_time_ms = Some(admission.observed_time_ms);"));
    assert!(rust_shell.contains("health.lateness_ms = Some(admission.lateness_ms);"));
    assert!(rust_shell.contains("health.missed_periods = Some(admission.missed_periods);"));
    assert!(rust_shell.contains("health.overrun = Some(admission.missed_periods > 0"));
    assert!(!rust_shell.contains("ready_batch.run_local_collect(|task|"));
    assert!(!rust_shell.contains("ready_batch.run_collect(&worker_pool, move |task|"));
    assert!(rust_shell.contains("type FlowrtOutputCommit = Box<dyn FnOnce(&App"));
    assert!(rust_shell.contains(
        "__flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events,"
    ));
    assert!(rust_shell.contains(
        "commit(app.as_ref(), &introspection_state, &scheduler_events, &mut health_map)"
    ));
    assert!(!rust_shell.contains("ready_batch.run_local(|task|"));
    assert!(!rust_shell.contains("ready_batch.run(&worker_pool, move |task|"));
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

    assert!(
        rust_shell.contains("source: std::sync::Arc<std::sync::Mutex<Box<dyn Source + Send>>>")
    );
    assert!(rust_shell.contains("sink: std::sync::Arc<std::sync::Mutex<Box<dyn Sink + Send>>>"));
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
        format!("let used_in = __flowrt_bind_{sink_used_bind}_guard.view_at(tick_time_ms);");
    let monitor_used_read =
        format!("let used_in = __flowrt_bind_{monitor_used_bind}_guard.view_at(tick_time_ms);");
    let sink_step_start = rust_shell.find(&sink_used_read).unwrap();
    let monitor_step_start = rust_shell.find(&monitor_used_read).unwrap();
    let sink_step = &rust_shell[sink_step_start..monitor_step_start];

    assert!(sink_step.contains(&sink_used_read));
    assert!(sink_step.contains("let unused_in = flowrt::Latest::new(None, false);"));
    assert!(sink_step.contains("let mut used_out = flowrt::Output::<Sample>::new();"));
    assert!(sink_step.contains("let mut unused_out = flowrt::Output::<Sample>::new();"));
    assert!(sink_step.contains("if used_in.present() {"));
    assert!(
        sink_step.contains("self.sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(used_in, unused_in, &mut used_out, &mut unused_out)")
    );
    assert!(sink_step.contains("if let Some(value) = used_out.as_ref().cloned()"));
    assert!(!sink_step.contains(&format!(
        "__flowrt_bind_{sink_unused_bind}_guard.view_at(tick_time_ms)"
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

    assert_eq!(
        scheduler_step
            .matches("self.worker.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(")
            .count(),
        2
    );
    assert!(scheduler_step.contains("let mut fast = flowrt::Output::<u32>::new();"));
    assert!(scheduler_step.contains("let mut slow = flowrt::Output::<u32>::new();"));
    assert!(rust_shell.contains("health_map.entry(\"worker.fast_loop\".to_string())"));
    assert!(rust_shell.contains("health_map.entry(\"worker.slow_loop\".to_string())"));
    assert!(!rust_shell.contains("health_map.entry(\"worker\".to_string())"));
    assert_eq!(launch["graphs"][0]["tasks"][0]["name"], "fast_loop");
    assert_eq!(launch["graphs"][0]["tasks"][1]["name"], "slow_loop");
}

#[test]
fn concurrency_rust_shell_uses_parallel_dispatch_and_preserves_exclusive_instance_lane() {
    let ir = contract_from_source(
        r#"
[package]
name = "rust_parallel_demo"
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
lane = "fast_lane"
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 100
lane = "slow_lane"
output = ["slow"]

[profile.default]
worker_threads = 4
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();

    assert!(!rust_shell.contains("scheduler.run_ready(|task| match task"));
    assert!(rust_shell.contains("let worker_pool = flowrt::WorkerPool::new(4);"));
    assert!(rust_shell.contains("let ready_batch = scheduler.take_ready_batch();"));
    assert!(rust_shell.contains("worker_pool.submit_collect(admission.task"));
    assert!(rust_shell.contains("pending_task_order.push_back(admission.task)"));
    assert!(rust_shell.contains("scheduler.complete_task(task_result.task)"));
    assert!(
        rust_shell.contains("worker: std::sync::Arc<std::sync::Mutex<Box<dyn Worker + Send>>>")
    );
    assert!(rust_shell.contains("worker: Box<dyn Worker + Send>"));
    assert!(
        rust_shell.contains("let worker = std::sync::Arc::new(std::sync::Mutex::new(worker));")
    );
    assert!(rust_shell.contains(
        "flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 }"
    ));
    assert!(rust_shell.contains(
        "flowrt::TaskSpec { id: flowrt::TaskId(2), lane: flowrt::LaneId(1), priority: 0 }"
    ));
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["concurrency"],
        "exclusive"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["lane"],
        "worker_serial"
    );
}

#[test]
fn concurrency_rust_parallel_component_uses_sync_trait_and_explicit_lanes() {
    let ir = contract_from_source(
        r#"
[package]
name = "rust_parallel_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
concurrency = "parallel"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
lane = "fast_lane"
concurrency = "parallel"
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 100
lane = "slow_lane"
concurrency = "parallel"
output = ["slow"]

[profile.default]
worker_threads = 4
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_components = artifact_content(&bundle, "rust/src/components.rs");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();

    assert!(rust_components.contains("pub trait Worker: Send + Sync {"));
    assert!(rust_components.contains("fn on_tick(\n        &self,"));
    assert!(rust_shell.contains("worker: std::sync::Arc<Box<dyn Worker + Send + Sync>>"));
    assert!(rust_shell.contains("worker: Box<dyn Worker + Send + Sync>"));
    assert!(rust_shell.contains("let worker = std::sync::Arc::new(worker);"));
    assert!(rust_shell.contains(
        "flowrt::TaskSpec { id: flowrt::TaskId(1), lane: flowrt::LaneId(1), priority: 0 }"
    ));
    assert!(rust_shell.contains(
        "flowrt::TaskSpec { id: flowrt::TaskId(2), lane: flowrt::LaneId(2), priority: 0 }"
    ));
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["concurrency"],
        "parallel"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["lane"],
        "fast_lane"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][1]["lane"],
        "slow_lane"
    );
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
    assert!(cpp_shell.contains("health_map[\"worker.fast_loop\"]"));
    assert!(cpp_shell.contains("health_map[\"worker.slow_loop\"]"));
    assert!(
        cpp_shell.contains("pending_task_admissions.insert_or_assign(admission.task, admission);")
    );
    assert!(cpp_shell.contains("pending_task_admissions.find(task_result.task)"));
    assert!(cpp_shell.contains("health.inflight = true;"));
    assert!(cpp_shell.contains("health.inflight = false;"));
    assert!(cpp_shell.contains("health.scheduled_time_ms = admission.scheduled_time_ms;"));
    assert!(cpp_shell.contains("health.observed_time_ms = admission.observed_time_ms;"));
    assert!(cpp_shell.contains("health.lateness_ms = admission.lateness_ms;"));
    assert!(cpp_shell.contains("health.missed_periods = admission.missed_periods;"));
    assert!(cpp_shell.contains("health.overrun = admission.missed_periods > 0U"));
    assert!(!cpp_shell.contains("health_map[\"worker\"].name"));
    assert_eq!(selfdesc["graphs"][0]["tasks"][0]["name"], "fast_loop");
    assert_eq!(selfdesc["graphs"][0]["tasks"][1]["name"], "slow_loop");
    assert_eq!(
        selfdesc["graphs"][0]["tasks"][0]["concurrency"],
        "exclusive"
    );
    assert_eq!(
        selfdesc["graphs"][0]["tasks"][1]["concurrency"],
        "exclusive"
    );
    assert_eq!(selfdesc["graphs"][0]["scheduler"]["worker_threads"], 1);
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["lanes"][0],
        serde_json::json!({"name": "worker_serial", "kind": "serial", "instance": "worker"})
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["lane"],
        "worker_serial"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][1]["lane"],
        "worker_serial"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["concurrency"],
        "exclusive"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][1]["concurrency"],
        "exclusive"
    );
}

#[test]
fn concurrency_cpp_shell_parallel_tasks_keep_explicit_lanes_and_emit_parallel_metadata() {
    let ir = contract_from_source(
        r#"
[package]
name = "parallel_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
concurrency = "parallel"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
concurrency = "parallel"
lane = "fast_lane"
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 10
concurrency = "parallel"
lane = "slow_lane"
output = ["slow"]

[profile.default]
backend = "inproc"
worker_threads = 2
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();

    assert!(cpp_shell.contains("flowrt::WorkerPool worker_pool{2};"));
    assert!(cpp_shell.contains("auto ready_batch = scheduler.take_ready_batch();"));
    assert!(!cpp_shell.contains("const auto ready_batch = scheduler.take_ready_batch();"));
    assert!(cpp_shell.contains(
        "flowrt::WorkerCompletionQueue<std::vector<FlowrtOutputCommit>> task_completion_queue;"
    ));
    assert!(cpp_shell.contains("worker_pool.submit_collect(admission.task, task_completion_queue"));
    assert!(cpp_shell.contains("scheduler.complete_task(task_result.task)"));
    assert!(!cpp_shell.contains(
        "scheduler.run_ready([this, &lifecycle_context, &introspection_state, &scheduler_events, &health_map, tick_time_ms](flowrt::TaskId task)"
    ));
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["lanes"],
        serde_json::json!([
            {"name": "fast_lane", "kind": "serial", "instance": "worker"},
            {"name": "slow_lane", "kind": "serial", "instance": "worker"}
        ])
    );
    assert_eq!(selfdesc["graphs"][0]["tasks"][0]["concurrency"], "parallel");
    assert_eq!(selfdesc["graphs"][0]["tasks"][1]["concurrency"], "parallel");
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["lane"],
        "fast_lane"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][1]["lane"],
        "slow_lane"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["concurrency"],
        "parallel"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][1]["concurrency"],
        "parallel"
    );
}

#[test]
fn concurrency_cpp_shell_parallel_declaration_with_single_worker_thread_still_generates_valid_shell()
 {
    let ir = contract_from_source(
        r#"
[package]
name = "parallel_single_worker"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
concurrency = "parallel"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
concurrency = "parallel"
lane = "fast_lane"
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 10
concurrency = "parallel"
lane = "slow_lane"
output = ["slow"]

[profile.default]
backend = "inproc"
worker_threads = 1
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();

    assert!(cpp_shell.contains("flowrt::DeterministicExecutor scheduler{1};"));
    assert!(cpp_shell.contains("flowrt::WorkerPool worker_pool{1};"));
    assert!(cpp_shell.contains("auto ready_batch = scheduler.take_ready_batch();"));
    assert!(!cpp_shell.contains("const auto ready_batch = scheduler.take_ready_batch();"));
    assert_eq!(selfdesc["graphs"][0]["scheduler"]["worker_threads"], 1);
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][0]["concurrency"],
        "parallel"
    );
    assert_eq!(
        selfdesc["graphs"][0]["scheduler"]["tasks"][1]["concurrency"],
        "parallel"
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
            "app.step_startup(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new())",
        )
        .unwrap();
    let scheduler_call = run
        .find("let mut scheduler = flowrt::DeterministicExecutor")
        .unwrap();
    let shutdown_call = run
        .find("app.step_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new())")
        .unwrap();
    let startup_step = generated_function_block(rust_shell, "fn step_startup");
    let shutdown_step = generated_function_block(rust_shell, "fn step_shutdown");
    let scheduler_step = generated_function_block(rust_shell, "fn step(");

    assert!(startup_call < scheduler_call);
    assert!(scheduler_call < shutdown_call);
    assert!(run.contains(
        "if status == flowrt::Status::Ok {\n            status = app.step_shutdown(0, &mut lifecycle_context, &introspection_state, &scheduler_events, &mut std::collections::BTreeMap::new());\n        }"
    ));
    assert!(run.contains("let shutdown = flowrt::install_signal_shutdown_token();"));
    assert!(run.contains("&& !shutdown.is_requested()"));
    assert!(!run.contains("backend.scheduler().run_ticks_until_shutdown("));
    assert!(startup_step.contains(
        "match self.boot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick()"
    ));
    assert!(shutdown_step.contains(
        "match self.cleanup.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick()"
    ));
    assert!(!scheduler_step.contains(
        "match self.boot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick()"
    ));
    assert!(!scheduler_step.contains(
        "match self.cleanup.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick()"
    ));
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
fn concurrency_rust_shell_enforces_task_deadline_before_publishing_outputs() {
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
        .find("let source_main_deadline_started_at = std::time::Instant::now();")
        .unwrap();
    let source_call = rust_shell
        .find("match self.source.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).on_tick(&mut sample)")
        .unwrap();
    let deadline_guard = rust_shell
        .find("source_main_deadline_started_at.elapsed() > std::time::Duration::from_millis(10)")
        .unwrap();
    let publish = rust_shell
        .find("if let Some(value) = sample.as_ref().cloned()")
        .unwrap();

    assert!(deadline_start < source_call);
    assert!(source_call < deadline_guard);
    assert!(deadline_guard < publish);
}

#[test]
fn concurrency_cpp_shell_enforces_task_deadline_before_publishing_outputs() {
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
        .find("const auto source_main_deadline_started_at = std::chrono::steady_clock::now();")
        .unwrap();
    let source_call = cpp_shell
        .find("switch (source_->on_tick(source_sample))")
        .unwrap();
    let deadline_guard = cpp_shell
            .find("std::chrono::steady_clock::now() - source_main_deadline_started_at > std::chrono::milliseconds{10}")
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
        "if app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_0_seen_revision_for_sink_main && app.bind_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_1_seen_revision_for_sink_main"
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
    assert!(rust_shell.contains("let scheduler_started_at = std::time::Instant::now();"));
    assert!(rust_shell.contains("let scheduler_runtime_now_ms = || -> u64 {"));
    assert!(
        rust_shell.contains("scheduler_now_ms = scheduler_now_ms.max(scheduler_runtime_now_ms());")
    );
    assert!(rust_shell.contains("let tick_time_ms = scheduler_now_ms;"));
    assert!(rust_shell.contains("scheduler.add_periodic(flowrt::PeriodicSpec"));
    assert!(rust_shell.contains("let mut bind_0_seen_revision_for_sink_main: u64 = 0;"));
    assert!(rust_shell.contains("if app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).revision() != bind_0_seen_revision_for_sink_main"));
    assert!(rust_shell.contains("scheduler.wake(flowrt::TaskId("));
    assert!(rust_shell.contains("let ready_batch = scheduler.take_ready_batch();"));
    assert!(rust_shell.contains("worker_pool.submit_collect(admission.task"));
    assert!(rust_shell.contains("task_completion_queue.drain_completed()"));
    assert!(rust_shell.contains("scheduler.complete_task(task_result.task)"));
    assert!(rust_shell.contains("let mut woke_on_message = false;"));
    assert!(rust_shell.contains("woke_on_message = true;"));
    assert!(rust_shell.contains(
        "if committed_task_count == 0 || (!woke_on_message && submitted_task_count == 0)"
    ));
    assert!(rust_shell.contains("let mut observed_data_generation: u64;"));
    assert!(rust_shell.contains(
        "loop {\n                observed_data_generation = scheduler_events.data_generation();"
    ));
    assert!(!rust_shell.contains(
        "while status == flowrt::Status::Ok\n            && !shutdown.is_requested()\n            && run_ticks\n                .map(|limit| tick_base < limit)\n                .unwrap_or(true)\n        {\n            let mut observed_data_generation = scheduler_events.data_generation();"
    ));
    assert!(rust_shell.contains("let scheduler_events = flowrt::ScheduleWaiter::new();"));
    assert!(rust_shell.contains("scheduler_events.notify_data();"));
    assert!(rust_shell.contains("let clock_source = \"realtime\";"));
    assert!(rust_shell.contains("introspection_state.record_tick_at(tick_time_ms, clock_source);"));
    assert!(rust_shell.contains("let _ = scheduler_events.take_data_time_ms();"));
    assert!(!rust_shell.contains("scheduler_now_ms = scheduler_now_ms.max(data_time_ms);"));
    assert!(rust_shell.contains(
        "scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, &shutdown)"
    ));
    assert!(!rust_shell.contains("backend.scheduler().run_ticks_until_shutdown(1"));
    assert!(rust_shell.contains("flowrt::Status::Retry => return flowrt::Status::Retry,"));
    assert!(rust_shell.contains("if task_result.status == flowrt::Status::Error"));
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

    assert!(rust_shell.contains("app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).set_schedule_waiter(scheduler_events.clone());"));
    assert!(rust_shell.contains("let _ = app.bind_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).receive_latest_at(tick_time_ms);"));
    assert!(
        rust_shell.contains(
            "let __flowrt_sample_snapshot_view = __flowrt_bind_0_snapshot_guard.cached_latest_at(tick_time_ms);"
        )
    );
    assert!(
        sink_step.contains(
            "let sample = flowrt::Latest::new(__flowrt_input_sample_value.as_ref(), __flowrt_input_sample_stale);"
        )
    );
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
    assert!(
        cpp_shell.contains("const auto scheduler_started_at = std::chrono::steady_clock::now();")
    );
    assert!(cpp_shell.contains(
        "const auto scheduler_runtime_now_ms = [&scheduler_started_at]() -> std::uint64_t {"
    ));
    assert!(
        cpp_shell
            .contains("scheduler_now_ms = std::max(scheduler_now_ms, scheduler_runtime_now_ms());")
    );
    assert!(cpp_shell.contains("const auto tick_time_ms = scheduler_now_ms;"));
    assert!(cpp_shell.contains("scheduler.add_periodic(flowrt::PeriodicSpec"));
    assert!(cpp_shell.contains("std::uint64_t bind_0_seen_revision_for_sink_main = 0;"));
    assert!(cpp_shell.contains("if (bind_0_.revision() != bind_0_seen_revision_for_sink_main)"));
    assert!(cpp_shell.contains("scheduler.wake(flowrt::TaskId{"));
    assert!(cpp_shell.contains("flowrt::WorkerPool worker_pool{2};"));
    assert!(cpp_shell.contains("auto ready_batch = scheduler.take_ready_batch();"));
    assert!(!cpp_shell.contains("const auto ready_batch = scheduler.take_ready_batch();"));
    assert!(cpp_shell.contains(
        "flowrt::WorkerCompletionQueue<std::vector<FlowrtOutputCommit>> task_completion_queue;"
    ));
    assert!(cpp_shell.contains("worker_pool.submit_collect(admission.task, task_completion_queue"));
    assert!(cpp_shell.contains("scheduler.complete_task(task_result.task)"));
    assert!(!cpp_shell.contains("scheduler.run_ready([this, &lifecycle_context, &introspection_state, &scheduler_events, &health_map, tick_time_ms](flowrt::TaskId task)"));
    assert!(cpp_shell.contains("bool woke_on_message = false;"));
    assert!(cpp_shell.contains("woke_on_message = true;"));
    assert!(cpp_shell.contains(
        "if (committed_task_count == 0U || (!woke_on_message && submitted_task_count == 0U))"
    ));
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
    assert!(cpp_shell.contains("const auto clock_source = std::string_view{\"realtime\"};"));
    assert!(cpp_shell.contains("introspection_state.record_tick(tick_time_ms, clock_source);"));
    assert!(cpp_shell.contains("(void)scheduler_events.take_data_time_ms();"));
    assert!(!cpp_shell.contains("scheduler_now_ms = std::max(scheduler_now_ms, *data_time_ms);"));
    assert!(cpp_shell.contains(
        "scheduler_events.wait_until_after(observed_data_generation, next_wake_deadline, shutdown)"
    ));
    assert!(!cpp_shell.contains("backend.scheduler().run_ticks_until_shutdown("));
    assert!(cpp_shell.contains("case flowrt::Status::Retry:"));
    assert!(cpp_shell.contains("return flowrt::Status::Retry;"));
    assert!(cpp_shell.contains("if (task_result.status == flowrt::Status::Error)"));
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
    assert!(run_loop.contains("if task_result.status == flowrt::Status::Error"));
    assert!(!run_loop.contains("if task_result.status != flowrt::Status::Ok"));
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
    assert!(
        source_step.contains("return FlowrtTaskOutcome::retry(std::vector<FlowrtOutputCommit>{});")
    );
    assert!(run_loop.contains("if (task_result.status == flowrt::Status::Error)"));
    assert!(!run_loop.contains("if (task_status != flowrt::Status::Ok)"));
}

#[test]
fn cpp_output_commits_use_unique_payload_names_for_fanout_targets() {
    let ir = contract_from_source(
        r#"
[package]
name = "fanout_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["sample:u32"]

[component.left_sink]
language = "cpp"
input = ["sample:u32"]

[component.right_sink]
language = "cpp"
input = ["sample:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.left_sink]
component = "left_sink"

[instance.left_sink.task]
trigger = "on_message"
input = ["sample"]

[instance.right_sink]
component = "right_sink"

[instance.right_sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "left_sink.sample"
channel = "latest"

[[bind.dataflow]]
from = "source.sample"
to = "right_sink.sample"
channel = "latest"

[[boundary.output]]
name = "sample_out"
port = "source.sample"
type = "u32"

[profile.dev]
mode = "island"
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let source_step = generated_function_block(cpp_shell, "App::step_task_source_main");

    assert_eq!(
        source_step
            .matches("flowrt_output_commits.emplace_back")
            .count(),
        3
    );
    assert_eq!(source_step.matches("auto payload = *value;").count(), 0);
    assert!(source_step.contains("auto flowrt_payload_0 = *value;"));
    assert!(source_step.contains("auto flowrt_payload_1 = *value;"));
    assert!(source_step.contains("auto flowrt_payload_2 = *value;"));
}

#[test]
fn emit_seeds_feedback_channel_and_breaks_cycle() {
    // 反馈自环：codegen 必须能生成（拓扑剔除反馈边断环），并在 run 启动期播种零初值。
    let ir = contract_from_source(
        r#"
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
"#,
    );
    let bundle = emit_artifacts(&ir).expect("feedback contract should emit");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    // 构造期播种零初值。
    assert!(rust_shell.contains(".publish_at(State::default(), 0);"));
}

#[test]
fn emit_seeds_feedback_literal_init_both_languages() {
    // 反馈自环带 literal 初值：两语言播种字面量而非零值。
    let rust_ir = contract_from_source(
        r#"
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
init = { x = 1.5 }

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let rust_bundle = emit_artifacts(&rust_ir).expect("rust feedback init should emit");
    let rust_shell = artifact_content(&rust_bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains(".publish_at(State { x: 1.5f64 }, 0);"));

    let cpp_ir = contract_from_source(
        r#"
[package]
name = "accumulator"
rsdl_version = "0.1"

[type.State]
x = "f64"

[component.accumulator]
language = "cpp"
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
init = { x = 1.5 }

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let cpp_bundle = emit_artifacts(&cpp_ir).expect("cpp feedback init should emit");
    let cpp_shell = artifact_content(&cpp_bundle, "cpp/src/runtime_shell.cpp");
    assert!(cpp_shell.contains("__seed.x = 1.5; return __seed; }(), 0);"));
}

#[test]
fn emit_seeds_feedback_fifo_depth_times() {
    // fifo 反馈环按 depth 拍延迟：每个 run 函数 push 恰 depth 次。run 函数个数从同形 latest
    // 契约（每个 run 播种 1 次）推出，避免耦合 shell 的具体 run 函数数量。
    let fifo_body = |channel_block: &str| {
        format!(
            r#"
[package]
name = "delay_line"
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
{channel_block}
feedback = true

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#
        )
    };

    let latest_ir = contract_from_source(&fifo_body("channel = \"latest\""));
    let latest_bundle = emit_artifacts(&latest_ir).expect("latest feedback should emit");
    let latest_shell = artifact_content(&latest_bundle, "rust/src/runtime_shell.rs");
    let run_functions = latest_shell
        .matches(".publish_at(State::default(), 0);")
        .count();
    assert!(run_functions > 0, "latest 反馈应至少播种一次");

    let fifo_ir = contract_from_source(&fifo_body(
        "channel = \"fifo\"\ndepth = 3\noverflow = \"drop_oldest\"",
    ));
    let fifo_bundle = emit_artifacts(&fifo_ir).expect("fifo feedback should emit");
    let fifo_shell = artifact_content(&fifo_bundle, "rust/src/runtime_shell.rs");
    let push_count = fifo_shell.matches(".push_at(State::default(), 0);").count();
    assert_eq!(
        push_count,
        run_functions * 3,
        "fifo depth=3 每个 run 函数应播种 3 次"
    );
}

#[test]
fn emit_records_lifecycle_states() {
    let ir = contract_from_source(
        r#"
[package]
name = "lifecycle_emit"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["tick:u32"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 10
output = ["tick"]

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(
        rust_shell.contains(
            "introspection_state.record_lifecycle_state(\"worker\", flowrt::LifecycleState::Uninitialized);"
        ),
        "missing uninitialized record"
    );
    assert!(
        rust_shell.contains("flowrt::LifecycleState::Initialized"),
        "missing initialized record"
    );
    assert!(
        rust_shell.contains("flowrt::LifecycleState::Running"),
        "missing running record"
    );
    assert!(
        rust_shell.contains("flowrt::LifecycleState::ShutDown"),
        "missing shutdown record"
    );
}
