use super::*;

#[test]
fn normalizes_minimal_document() {
    let source = r#"
[package]
name = "robot_demo"
version = "0.1.0"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.imu_sim]
language = "rust"
output = ["imu:Imu"]

[instance.imu_sim]
component = "imu_sim"
process = "main"
target = "linux"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[profile.default]
backend = "inproc"
worker_threads = 3
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[target.linux]
platform = "linux-x86_64"
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(ir.package.name, "robot_demo");
    assert_eq!(
        ir.types[0].fields[0].ty,
        TypeExpr::Primitive {
            name: PrimitiveType::U64
        }
    );
    assert_eq!(ir.graphs[0].tasks[0].period_ms, Some(5));
    assert_eq!(ir.profiles[0].scheduler.worker_threads, 3);
    assert_eq!(
        ir.targets[0].platform,
        Some(crate::TargetPlatform::LinuxAmd64)
    );
}

#[test]
fn normalizes_c_component_language_and_target_runtime() {
    let source = r#"
[package]
name = "c_demo"
rsdl_version = "0.1"

[component.controller]
language = "c"

[target.linux]
runtime = ["rust", "c", "cpp"]
backends = ["inproc"]
"#;

    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(ir.components[0].language, crate::LanguageKind::C);
    assert_eq!(
        ir.targets[0].runtime,
        vec![
            crate::LanguageKind::C,
            crate::LanguageKind::Cpp,
            crate::LanguageKind::Rust
        ]
    );
    let json = ir.to_canonical_json().unwrap();
    assert!(json.contains("\"language\": \"c\""));
    assert!(
        json.contains(
            "\"runtime\": [\n        \"c\",\n        \"cpp\",\n        \"rust\"\n      ]"
        )
    );
}

#[test]
fn normalizes_frame_descriptor_resource_schema_with_payload_recording_opt_in() {
    let source = r#"
[package]
name = "descriptor_demo"
rsdl_version = "0.1"

[type.FrameHandle]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"
format = "u32"
encoding = "u32"

[component.camera]
language = "rust"
kind = "io_boundary"
io_side_effect = ["device", "read"]
output = ["frame:FrameHandle"]

[component.camera.resource.frames]
capability = "payload.frame_buffer"

[component.camera.resource.frames.descriptor]
kind = "frame"
port = "frame"
format = "rgb8"
encoding = "row_major"
metadata = { width = "640", height = "480" }
record_payload = true

[instance.camera]
component = "camera"

[instance.camera.task]
trigger = "periodic"
period_ms = 33
output = ["frame"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let resource = &ir.components[0].resources[0];
    let descriptor = resource
        .descriptor
        .as_ref()
        .expect("resource should carry descriptor schema");

    assert_eq!(resource.name, "frames");
    assert_eq!(descriptor.kind, crate::ResourceDescriptorKind::Frame);
    assert_eq!(descriptor.port, "frame");
    assert_eq!(descriptor.format, "rgb8");
    assert_eq!(descriptor.encoding.as_deref(), Some("row_major"));
    assert_eq!(
        descriptor.metadata,
        BTreeMap::from([
            ("height".to_string(), "480".to_string()),
            ("width".to_string(), "640".to_string())
        ])
    );
    assert!(descriptor.record_payload);
}

#[test]
fn rejects_frame_descriptor_resource_schema_without_output_port() {
    let source = r#"
[package]
name = "descriptor_demo"
rsdl_version = "0.1"

[type.FrameHandle]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"

[component.camera]
language = "rust"
kind = "io_boundary"
io_side_effect = ["device", "read"]
output = ["frame:FrameHandle"]

[component.camera.resource.frames]
capability = "payload.frame_buffer"

[component.camera.resource.frames.descriptor]
kind = "frame"
format = "rgb8"

[instance.camera]
component = "camera"

[instance.camera.task]
trigger = "periodic"
period_ms = 33
output = ["frame"]
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("descriptor schema must bind to an output port");

    assert!(format!("{error}").contains("component.resource.frames.descriptor.port"));
}

#[test]
fn rejects_unknown_target_platform() {
    let source = r#"
[package]
name = "bad_platform"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"

[target.edge]
platform = "linux-riscv64"
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source)).unwrap_err();

    assert!(error.to_string().contains("unsupported target platform"));
    assert!(error.to_string().contains("linux-riscv64"));
}

#[test]
fn normalizes_named_task_array_for_one_instance() {
    let source = r#"
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
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let tasks = &ir.graphs[0].tasks;

    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].name, "fast_loop");
    assert_eq!(tasks[1].name, "slow_loop");
    assert_ne!(tasks[0].id, tasks[1].id);
    assert_eq!(tasks[0].outputs, vec!["fast"]);
    assert_eq!(tasks[1].outputs, vec!["slow"]);
}

#[test]
fn normalizes_scheduler_v2_task_fields() {
    let source = r#"
[package]
name = "scheduler_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
input = ["in:u32"]
output = ["out:u32"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "on_message"
readiness = "all_ready"
lane = "worker_serial"
priority = 7
input = ["in"]
output = ["out"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let task = &ir.graphs[0].tasks[0];

    assert_eq!(task.readiness, TaskReadiness::AllReady);
    assert_eq!(task.lane.as_deref(), Some("worker_serial"));
    assert_eq!(task.priority, Some(7));
}

#[test]
fn normalizes_explicit_empty_message_flag() {
    let source = r#"
[package]
name = "empty_demo"
rsdl_version = "0.1"

[type.Empty]
empty = true
"#;

    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(ir.types[0].name, "Empty");
    assert!(ir.types[0].empty);
    assert!(ir.types[0].fields.is_empty());
    assert!(ir.to_canonical_json().unwrap().contains("\"empty\": true"));
}

#[test]
fn task_concurrency_defaults_to_exclusive() {
    let source = r#"
[package]
name = "concurrency_defaults"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["sample:u32"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(
        ir.components[0].concurrency,
        crate::TaskConcurrency::Exclusive
    );
    assert_eq!(
        ir.graphs[0].tasks[0].concurrency,
        crate::TaskConcurrency::Exclusive
    );
    assert_eq!(ir.components[0].declared_concurrency, None);
    assert_eq!(ir.graphs[0].tasks[0].declared_concurrency, None);
}

#[test]
fn task_parallel_concurrency_enters_canonical_json() {
    let source = r#"
[package]
name = "concurrency_parallel"
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
concurrency = "parallel"
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 10
output = ["slow"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let json = ir.to_canonical_json().unwrap();

    assert_eq!(
        ir.components[0].concurrency,
        crate::TaskConcurrency::Parallel
    );
    assert_eq!(
        ir.graphs[0].tasks[0].concurrency,
        crate::TaskConcurrency::Parallel
    );
    assert_eq!(
        ir.components[0].declared_concurrency,
        Some(crate::TaskConcurrency::Parallel)
    );
    assert_eq!(
        ir.graphs[0].tasks[0].declared_concurrency,
        Some(crate::TaskConcurrency::Parallel)
    );
    assert!(json.contains("\"concurrency\": \"parallel\""));
}

#[test]
fn normalizes_sync_groups_canonically_and_links_task() {
    let source = r#"
[package]
name = "fusion_demo"
rsdl_version = "0.1"

[type.Imu]
ax = "f64"

[type.Odom]
vx = "f64"

[component.fusion]
language = "rust"
input = ["imu:Imu", "odom:Odom"]

[instance.fusion]
component = "fusion"
target = "linux"

[instance.fusion.task]
trigger = "on_synchronized"
sync = "alpha"

[[sync]]
name = "zoom"
instance = "fusion"
inputs = ["imu", "odom"]
tolerance_ms = 5

[[sync]]
name = "alpha"
instance = "fusion"
inputs = ["odom", "imu"]
tolerance_ms = 7

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let graph = &ir.graphs[0];

    // 集合按 name 归一化排序，inputs 保留声明顺序（不排序）。
    assert_eq!(graph.sync_groups.len(), 2);
    assert_eq!(graph.sync_groups[0].name, "alpha");
    assert_eq!(graph.sync_groups[1].name, "zoom");
    let alpha = &graph.sync_groups[0];
    assert_eq!(alpha.instance.name, "fusion");
    assert_eq!(alpha.inputs, vec!["odom", "imu"]);
    assert_eq!(alpha.tolerance_ms, 7);
    assert_eq!(alpha.late_policy, crate::SyncLatePolicy::DropLate);
    assert!(alpha.id.0.starts_with("sync_"));

    // on_synchronized task 的 sync_group 引用解析到同一实体（id 确定性一致）。
    let task = &graph.tasks[0];
    assert_eq!(task.trigger, crate::TriggerKind::OnSynchronized);
    let task_ref = task.sync_group.as_ref().expect("sync_group linked");
    assert_eq!(task_ref.name, "alpha");
    assert_eq!(task_ref.id, alpha.id);
}

#[test]
fn normalizes_instance_failure_policy_default_and_explicit() {
    let source = r#"
[package]
name = "failure_policy_ir"
rsdl_version = "0.1"

[component.processor]
language = "rust"
output = ["result:u32"]

[instance.implicit]
component = "processor"

[instance.implicit.task]
trigger = "periodic"
period_ms = 10
output = ["result"]

[instance.explicit]
component = "processor"
failure_policy = "isolate"

[instance.explicit.task]
trigger = "periodic"
period_ms = 10
output = ["result"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let fault = |n: &str| {
        ir.graphs[0]
            .instances
            .iter()
            .find(|i| i.name == n)
            .unwrap()
            .fault
    };
    assert_eq!(
        fault("implicit").policy,
        crate::InstanceFailurePolicy::FailFast
    );
    assert!(fault("implicit").restart.is_none());
    assert_eq!(
        fault("explicit").policy,
        crate::InstanceFailurePolicy::Isolate
    );
}

#[test]
fn normalizes_instance_fault_restart_table_fills_defaults() {
    let source = r#"
[package]
name = "fault_table_ir"
rsdl_version = "0.1"

[component.processor]
language = "rust"
output = ["result:u32"]

[instance.worker]
component = "processor"

[instance.worker.fault]
policy = "restart"
initial_delay_ms = 10

[instance.worker.task]
trigger = "periodic"
period_ms = 10
output = ["result"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let fault = &ir.graphs[0].instances[0].fault;
    assert_eq!(fault.policy, crate::InstanceFailurePolicy::Restart);
    let restart = fault.restart.expect("restart params");
    assert_eq!(restart.initial_delay_ms, 10);
    assert_eq!(
        restart.max_restarts,
        crate::DEFAULT_INSTANCE_RESTART.max_restarts
    );
    assert_eq!(
        restart.max_delay_ms,
        crate::DEFAULT_INSTANCE_RESTART.max_delay_ms
    );
}

#[test]
fn rejects_instance_fault_double_spec() {
    let source = r#"
[package]
name = "fault_double_ir"
rsdl_version = "0.1"

[component.processor]
language = "rust"
output = ["result:u32"]

[instance.worker]
component = "processor"
failure_policy = "isolate"

[instance.worker.fault]
policy = "restart"

[instance.worker.task]
trigger = "periodic"
period_ms = 10
output = ["result"]
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source)).unwrap_err();
    assert!(format!("{error}").contains("互斥"), "{error}");
}

#[test]
fn rejects_restart_params_on_non_restart_policy() {
    let source = r#"
[package]
name = "fault_params_ir"
rsdl_version = "0.1"

[component.processor]
language = "rust"
output = ["result:u32"]

[instance.worker]
component = "processor"

[instance.worker.fault]
policy = "isolate"
max_restarts = 2

[instance.worker.task]
trigger = "periodic"
period_ms = 10
output = ["result"]
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source)).unwrap_err();
    assert!(format!("{error}").contains("restart 参数"), "{error}");
}

#[test]
fn rejects_instance_failure_policy_unknown_string() {
    let source = r#"
[package]
name = "failure_policy_bad"
rsdl_version = "0.1"

[component.processor]
language = "rust"
output = ["result:u32"]

[instance.processor]
component = "processor"
failure_policy = "nonsense"

[instance.processor.task]
trigger = "periodic"
period_ms = 10
output = ["result"]
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source)).unwrap_err();
    assert!(format!("{error}").contains("failure_policy"), "{error}");
}
