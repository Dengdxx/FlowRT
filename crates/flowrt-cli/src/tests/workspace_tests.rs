use super::*;

#[test]
fn load_prepared_contract_reports_build_required() {
    let root = temp_test_dir("missing-prepared-contract");
    let out_dir = root.join("flowrt");
    let rsdl = root.join("rsdl/robot.rsdl");

    let build_hint = build_command_hint(&rsdl, None, false);
    let error = load_prepared_contract(&out_dir, &build_hint).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("generated contract"));
    assert!(message.contains("flowrt build"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn prepared_profile_must_match_explicit_run_profile() {
    let contract = contract_from_source(
        r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );

    let build_hint = build_command_hint(
        Path::new("examples/profile_switch_demo/rsdl/robot.rsdl"),
        Some("iox2"),
        false,
    );
    let error = ensure_prepared_profile_matches(&contract, Some("iox2"), &build_hint).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("prepared FlowRT artifacts use profile `default`"));
    assert!(message.contains("flowrt build --profile iox2"));
}

#[test]
fn build_command_hint_includes_launcher_when_launch_needs_profile() {
    let hint = build_command_hint(
        Path::new("examples/profile_switch_demo/rsdl/robot.rsdl"),
        Some("iox2"),
        true,
    );

    assert_eq!(
        hint,
        "flowrt build --launcher --profile iox2 examples/profile_switch_demo/rsdl/robot.rsdl"
    );
}

#[test]
fn launch_workspace_requires_prebuilt_supervisor() {
    let contract = contract_from_source(
        r#"
[package]
name = "launcher_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
    );
    let root = temp_test_dir("missing-launcher");

    let error = launch_workspace(&contract, &root.join("flowrt"), Some(1)).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("FlowRT supervisor"));
    assert!(message.contains("flowrt build --launcher"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn workspace_lock_rejects_concurrent_access_to_same_out_dir() {
    let root = temp_test_dir("workspace-lock");
    let out_dir = root.join("flowrt");

    let first = WorkspaceLock::acquire(&out_dir).expect("first lock should be acquired");
    let error =
        WorkspaceLock::acquire(&out_dir).expect_err("second lock for same out dir should fail");

    assert!(error.to_string().contains("already in use"));
    drop(first);
    WorkspaceLock::acquire(&out_dir).expect("lock should be released on drop");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn workspace_lock_reclaims_lock_owned_by_dead_pid() {
    let root = temp_test_dir("workspace-lock-stale");
    let out_dir = root.join("flowrt");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join(".flowrt.lock"), "pid=99999999\nold=metadata\n").unwrap();

    let lock =
        WorkspaceLock::acquire(&out_dir).expect("unlocked stale lock file should be reclaimed");

    let contents = std::fs::read_to_string(out_dir.join(".flowrt.lock")).unwrap();
    assert_eq!(contents, format!("pid={}\n", std::process::id()));
    drop(lock);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_manifest_patch_is_skipped_when_flowrt_dependency_is_absent() {
    let root = temp_test_dir("cargo-patch-skip");
    let build_dir = root.join("flowrt").join("build");
    std::fs::create_dir_all(&build_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        r#"[package]
name = "supervisor-only"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .unwrap();

    let patched_manifest =
        cargo_manifest_with_runtime_patch(&root.join("flowrt"), Some(Path::new("/tmp/unused")))
            .expect("manifest without flowrt dependency should still be accepted");
    let content = std::fs::read_to_string(&patched_manifest).unwrap();

    assert!(!content.contains("[patch.crates-io]"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_manifest_patch_uses_available_rust_runtime_dir() {
    let root = temp_test_dir("cargo-patch-runtime");
    let build_dir = root.join("flowrt").join("build");
    let runtime_dir = root.join("installed").join("runtime").join("rust");
    std::fs::create_dir_all(&build_dir).unwrap();
    std::fs::create_dir_all(&runtime_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        r#"[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[dependencies]
flowrt = { version = "0.1" }
"#,
    )
    .unwrap();

    let patched_manifest =
        cargo_manifest_with_runtime_patch(&root.join("flowrt"), Some(&runtime_dir))
            .expect("manifest with flowrt dependency should be patched to available runtime");
    let content = std::fs::read_to_string(&patched_manifest).unwrap();

    assert!(content.contains("[patch.crates-io]"));
    assert!(content.contains(&format!(
        "flowrt = {{ path = {} }}",
        toml_basic_string(&runtime_dir)
    )));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_manifest_patch_is_skipped_when_no_runtime_dir_is_available() {
    let root = temp_test_dir("cargo-patch-no-runtime");
    let build_dir = root.join("flowrt").join("build");
    std::fs::create_dir_all(&build_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        r#"[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[dependencies]
flowrt = { version = "0.1" }
"#,
    )
    .unwrap();

    let patched_manifest = cargo_manifest_with_runtime_patch(&root.join("flowrt"), None)
        .expect("manifest should remain usable for registry-resolved flowrt");
    let content = std::fs::read_to_string(&patched_manifest).unwrap();

    assert!(!content.contains("[patch.crates-io]"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_build_invocation_uses_manifest_dir_and_offline_config() {
    let root = temp_test_dir("cargo-build-offline");
    let build_dir = root.join("flowrt").join("build");
    std::fs::create_dir_all(build_dir.join(".cargo")).unwrap();
    std::fs::write(
        build_dir.join(".cargo").join("config.toml"),
        "[net]\noffline = true\n",
    )
    .unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        "[package]\nname = \"robot\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();

    let invocation = cargo_build_invocation(&manifest, "robot-flowrt-app")
        .expect("cargo invocation should be derived from manifest");

    assert_eq!(invocation.current_dir, build_dir);
    assert!(invocation.args.iter().any(|arg| arg == "--offline"));
    let manifest_arg = invocation
        .args
        .windows(2)
        .find_map(|args| (args[0] == "--manifest-path").then_some(args[1].as_str()))
        .expect("cargo invocation should pass --manifest-path");
    assert!(Path::new(manifest_arg).is_absolute());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_build_invocation_resolves_relative_manifest_before_changing_dir() {
    let repo_dir = std::env::current_dir().unwrap();
    let root = repo_dir.join("target").join("tmp").join(format!(
        "flowrt-cargo-build-relative-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let build_dir = root.join("flowrt").join("build");
    std::fs::create_dir_all(&build_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        "[package]\nname = \"robot\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let relative_manifest = manifest.strip_prefix(&repo_dir).unwrap();

    let invocation = cargo_build_invocation(relative_manifest, "robot-flowrt-app")
        .expect("relative manifest should be resolved before cargo changes directory");

    assert_eq!(invocation.current_dir, build_dir);
    let manifest_arg = invocation
        .args
        .windows(2)
        .find_map(|args| (args[0] == "--manifest-path").then_some(args[1].as_str()))
        .expect("cargo invocation should pass --manifest-path");
    assert_eq!(Path::new(manifest_arg), manifest.as_path());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_configure_args_do_not_inject_runtime_dir_by_default() {
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");

    let args = cmake_configure_args(source_dir, build_dir, None);

    assert_eq!(
        args,
        vec![
            "-S".to_string(),
            "/tmp/flowrt/build".to_string(),
            "-B".to_string(),
            "/tmp/flowrt/build/cmake".to_string()
        ]
    );
}

#[test]
fn cmake_configure_args_can_pass_explicit_runtime_dir() {
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let runtime_dir = Path::new("/opt/flowrt/runtime/cpp");

    let args = cmake_configure_args(source_dir, build_dir, Some(runtime_dir));

    assert!(args.contains(&"-DFLOWRT_CPP_RUNTIME_DIR=/opt/flowrt/runtime/cpp".to_string()));
    assert!(args.contains(&"-DCMAKE_PREFIX_PATH=/opt/flowrt/runtime/cpp".to_string()));
}

#[test]
fn installed_runtime_candidates_include_private_prefix_layout() {
    let current_exe = Path::new("/opt/flowrt/0.1.0/bin/flowrt");

    let candidates = installed_runtime_candidates(current_exe, "runtime/cpp");

    assert!(
        candidates
            .iter()
            .any(|path| path == Path::new("/opt/flowrt/0.1.0"))
    );
}

#[test]
fn prepare_workspace_projects_selected_profile_before_validation() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "main"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 1

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
    let rsdl_dir = temp_test_dir("prepare-profile");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    let out_dir = rsdl_dir.join("flowrt");

    assert!(load_contract_from_rsdl(&rsdl_path).is_err());
    let prepared = prepare_workspace(&rsdl_path, &out_dir, Some("iox2"))
        .expect("selected profile should prepare");
    let prepared_ir =
        ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
            .unwrap();

    assert_eq!(prepared_ir.profiles.len(), 1);
    assert_eq!(prepared_ir.profiles[0].name, "iox2");
    assert_eq!(prepared_ir.deployments.len(), 1);
    assert_eq!(prepared_ir.deployments[0].profile.name, "iox2");

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}

#[test]
fn prepare_workspace_projects_default_profile_when_selection_is_omitted() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "main"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 1

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let rsdl_dir = temp_test_dir("prepare-default-profile");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    let out_dir = rsdl_dir.join("flowrt");

    assert!(load_contract_from_rsdl(&rsdl_path).is_err());
    let prepared =
        prepare_workspace(&rsdl_path, &out_dir, None).expect("default profile should prepare");
    let prepared_ir =
        ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
            .unwrap();

    assert_eq!(prepared_ir.profiles.len(), 1);
    assert_eq!(prepared_ir.profiles[0].name, "default");
    assert_eq!(prepared_ir.deployments.len(), 1);
    assert_eq!(prepared_ir.deployments[0].profile.name, "default");

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}

#[test]
fn prepare_workspace_writes_projected_channel_policy_to_managed_artifacts() {
    let source = r#"
[package]
name = "profile_policy_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["defaulted:Sample", "explicit:Sample"]

[component.consumer]
language = "rust"
input = ["defaulted:Sample", "explicit:Sample"]

[instance.producer]
component = "producer"
process = "main"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 1
output = ["defaulted", "explicit"]

[instance.consumer]
component = "consumer"
process = "main"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["defaulted", "explicit"]

[[bind.dataflow]]
from = "producer.defaulted"
to = "consumer.defaulted"
channel = "fifo"
depth = 2

[[bind.dataflow]]
from = "producer.explicit"
to = "consumer.explicit"
channel = "latest"
overflow = "drop_newest"
stale_policy = "hold_last"
max_age_ms = 7

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[profile.safety]
backend = "inproc"
default_overflow = "error"
default_stale_policy = "drop"
max_age_ms = 25

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let rsdl_dir = temp_test_dir("prepare-profile-policy");
    let rsdl_path = rsdl_dir.join("robot.rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::write(&rsdl_path, source).unwrap();
    let out_dir = rsdl_dir.join("flowrt");

    let prepared = prepare_workspace(&rsdl_path, &out_dir, Some("safety"))
        .expect("selected profile policy should prepare");
    let prepared_ir =
        ContractIr::from_json_str(&std::fs::read_to_string(&prepared.contract_path).unwrap())
            .unwrap();
    let defaulted_ir = prepared_ir.graphs[0]
        .binds
        .iter()
        .find(|bind| bind.to.port == "defaulted")
        .unwrap();
    let explicit_ir = prepared_ir.graphs[0]
        .binds
        .iter()
        .find(|bind| bind.to.port == "explicit")
        .unwrap();

    assert_eq!(defaulted_ir.overflow, flowrt_ir::OverflowPolicy::Error);
    assert_eq!(defaulted_ir.stale, flowrt_ir::StalePolicy::Drop);
    assert_eq!(defaulted_ir.max_age_ms, Some(25));
    assert_eq!(explicit_ir.overflow, flowrt_ir::OverflowPolicy::DropNewest);
    assert_eq!(explicit_ir.stale, flowrt_ir::StalePolicy::HoldLast);
    assert_eq!(explicit_ir.max_age_ms, Some(7));

    let launch: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("launch/launch.json")).unwrap())
            .unwrap();
    let channels = launch["graphs"][0]["channels"].as_array().unwrap();
    let defaulted_launch = channels
        .iter()
        .find(|channel| channel["to"] == "consumer.defaulted")
        .unwrap();
    let explicit_launch = channels
        .iter()
        .find(|channel| channel["to"] == "consumer.explicit")
        .unwrap();

    assert_eq!(defaulted_launch["overflow"], "error");
    assert_eq!(defaulted_launch["stale_policy"], "drop");
    assert_eq!(defaulted_launch["max_age_ms"], 25);
    assert_eq!(explicit_launch["overflow"], "drop_newest");
    assert_eq!(explicit_launch["stale_policy"], "hold_last");
    assert_eq!(explicit_launch["max_age_ms"], 7);

    let _ = std::fs::remove_dir_all(&rsdl_dir);
}
