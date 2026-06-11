use super::*;

#[test]
fn build_plan_selects_cargo_for_rust_contract() {
    let contract = contract_from_source(
        r#"
[package]
name = "rust_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
    );

    assert_eq!(build_steps(&contract, false), vec![BuildStep::CargoApp]);
    assert_eq!(
        build_steps(&contract, true),
        vec![BuildStep::CargoApp, BuildStep::CargoSupervisor]
    );
}

#[test]
fn build_model_cargo_invocation_uses_target_triple_path() {
    let root = temp_test_dir("cargo-invocation-target-triple");
    let package_dir = root.join("generated");
    std::fs::create_dir_all(package_dir.join("src")).unwrap();
    let manifest = package_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        "[package]\nname = \"robot\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[[bin]]\nname = \"robot-flowrt-app\"\npath = \"src/main.rs\"\n",
    )
    .unwrap();

    let invocation = cargo_build_invocation(
        &manifest,
        "robot-flowrt-app",
        BuildMode::Release,
        &root.join("target"),
        Some("aarch64-unknown-linux-gnu"),
        Some("aarch64-linux-gnu-gcc"),
    )
    .unwrap();

    assert!(
        invocation
            .args
            .windows(2)
            .any(|args| args == ["--target", "aarch64-unknown-linux-gnu"])
    );
    assert_eq!(
        invocation.env,
        vec![(
            "CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER".to_string(),
            "aarch64-linux-gnu-gcc".to_string()
        )]
    );
    assert_eq!(
        invocation.executable_path(),
        root.join("target")
            .join("aarch64-unknown-linux-gnu")
            .join("release")
            .join(format!("robot-flowrt-app{}", std::env::consts::EXE_SUFFIX))
    );
}

#[test]
fn build_plan_selects_cmake_for_cpp_contract() {
    let contract = contract_from_source(
        r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
    );

    assert_eq!(build_steps(&contract, false), vec![BuildStep::CmakeApp]);
    assert_eq!(
        build_steps(&contract, true),
        vec![BuildStep::CmakeApp, BuildStep::CargoSupervisor]
    );
}

#[test]
fn default_build_plan_does_not_build_launcher() {
    let contract = contract_from_source(
        r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
    );

    assert!(!build_steps(&contract, false).contains(&BuildStep::CargoSupervisor));
    assert!(build_steps(&contract, true).contains(&BuildStep::CargoSupervisor));
}

#[test]
fn build_plan_selects_cargo_and_cmake_for_mixed_contract() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.cpp_worker]
language = "cpp"

[component.rust_worker]
language = "rust"
"#,
    );

    assert_eq!(
        build_steps(&contract, false),
        vec![BuildStep::CargoApp, BuildStep::CmakeApp]
    );
    assert_eq!(
        build_steps(&contract, true),
        vec![
            BuildStep::CargoApp,
            BuildStep::CmakeApp,
            BuildStep::CargoSupervisor
        ]
    );
}

#[test]
fn run_mode_selects_cmake_app_only_for_cpp_only_contracts() {
    let cpp_contract = contract_from_source(
        r#"
[package]
name = "cpp_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"
"#,
    );
    assert_eq!(run_mode(&cpp_contract), Some(RunMode::CmakeApp));

    let rust_contract = contract_from_source(
        r#"
[package]
name = "rust_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
    );
    assert_eq!(run_mode(&rust_contract), Some(RunMode::CargoApp));

    let mixed_contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.cpp_worker]
language = "cpp"

[component.rust_worker]
language = "rust"
"#,
    );
    assert_eq!(run_mode(&mixed_contract), None);
    assert!(is_mixed_language_contract(&mixed_contract));
    ensure_direct_runtime_supported(&mixed_contract, "run").unwrap();
}

#[test]
fn run_mode_selects_app_by_process_for_mixed_iox2_contracts() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    assert_eq!(
        run_mode_for_process(&contract, Some("rust_main")).unwrap(),
        RunMode::CargoApp
    );
    assert_eq!(
        run_mode_for_process(&contract, Some("cpp_main")).unwrap(),
        RunMode::CmakeApp
    );
    assert!(run_mode_for_process(&contract, None).is_err());
}

#[test]
fn mixed_runtime_readiness_rejects_same_process_mixed_components() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let error = ensure_direct_runtime_supported(&contract, "launch").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("process `main`"));
    assert!(message.contains("contains both C++ and Rust components"));
    assert!(message.contains("split them into language-specific RSDL process groups"));
}

#[test]
fn mixed_runtime_readiness_rejects_inproc_cross_process_components() {
    let contract = unchecked_contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let error = ensure_direct_runtime_supported(&contract, "launch").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("mixed-language `launch` cannot carry dataflow"));
    assert!(message.contains("backend `inproc`"));
    assert!(message.contains("source.sample"));
    assert!(message.contains("sink.sample"));
}

#[test]
fn mixed_runtime_readiness_allows_iox2_cross_process_components() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    ensure_direct_runtime_supported(&contract, "launch").unwrap();
}

#[test]
fn mixed_runtime_readiness_allows_zenoh_cross_process_components() {
    let contract = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"
process = "rust_main"
target = "dev_host"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "cpp_main"
target = "pi_host"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "zenoh"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[target.dev_host]
runtime = ["rust"]
backends = ["zenoh"]

[target.pi_host]
runtime = ["cpp"]
backends = ["zenoh"]
"#,
    );

    ensure_direct_runtime_supported(&contract, "launch").unwrap();
    ensure_launch_process_boundaries_supported(&contract).unwrap();
}

#[test]
fn launch_readiness_rejects_inproc_dataflow_across_process_groups() {
    let contract = unchecked_contract_from_source(
        r#"
[package]
name = "split_rust_demo"
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
process = "source_process"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "sink_process"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let error = ensure_launch_process_boundaries_supported(&contract).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("backend `inproc`"));
    assert!(message.contains("source_process"));
    assert!(message.contains("sink_process"));
}

#[test]
fn launch_readiness_allows_zenoh_route_across_process_groups_with_inproc_profile() {
    let contract = contract_from_source(
        r#"
[package]
name = "split_rust_demo"
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
process = "source_process"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "sink_process"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
backend = "zenoh"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    ensure_launch_process_boundaries_supported(&contract).unwrap();
    ensure_run_process_boundaries_supported(&contract, Some("sink_process")).unwrap();
}

#[test]
fn run_process_readiness_rejects_inproc_dataflow_across_process_groups() {
    let contract = unchecked_contract_from_source(
        r#"
[package]
name = "split_rust_demo"
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
process = "source_process"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "sink_process"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let error =
        ensure_run_process_boundaries_supported(&contract, Some("sink_process")).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("backend `inproc`"));
    assert!(message.contains("source_process"));
    assert!(message.contains("sink_process"));
    assert!(message.contains("run --process"));
    ensure_run_process_boundaries_supported(&contract, None).unwrap();
}

#[test]
fn backend_runtime_readiness_allows_cpp_iox2_contracts() {
    let contract = contract_from_source(
        r#"
[package]
name = "cpp_iox2_demo"
rsdl_version = "0.1"

[component.worker]
language = "cpp"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    ensure_backend_runtime_supported(&contract, "build").unwrap();
    ensure_backend_runtime_supported(&contract, "run").unwrap();
}

#[test]
fn backend_runtime_readiness_allows_rust_iox2_contracts() {
    let contract = contract_from_source(
        r#"
[package]
name = "rust_iox2_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    ensure_backend_runtime_supported(&contract, "build").unwrap();
}
