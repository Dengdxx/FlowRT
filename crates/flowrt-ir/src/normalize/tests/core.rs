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
