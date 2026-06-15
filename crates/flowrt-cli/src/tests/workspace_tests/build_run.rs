use super::*;

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

    let error = launch_workspace(&contract, &root.join("flowrt"), Some(1), None).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("build metadata is missing"));
    assert!(message.contains("flowrt build --launcher"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn build_info_rejects_version_mismatch() {
    let root = temp_test_dir("build-info-version");
    let out_dir = root.join("flowrt");
    let info = build_model::BuildInfo::new("0.0.1", None, BuildMode::Release, None);
    info.write(&out_dir).unwrap();

    let error = load_build_info(&out_dir, None, false).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("built with FlowRT 0.0.1"));
    assert!(message.contains(env!("CARGO_PKG_VERSION")));
    assert!(message.contains("flowrt build"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn build_info_rejects_requested_mode_mismatch() {
    let root = temp_test_dir("build-info-mode");
    let out_dir = root.join("flowrt");
    let info =
        build_model::BuildInfo::new(env!("CARGO_PKG_VERSION"), None, BuildMode::Release, None);
    info.write(&out_dir).unwrap();

    let error = load_build_info(&out_dir, Some(BuildMode::Debug), true).unwrap_err();

    let message = error.to_string();
    assert!(message.contains("artifacts use build mode `release`"));
    assert!(message.contains("requested `debug`"));
    assert!(message.contains("flowrt build --launcher"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn build_info_records_cross_target_identity_metadata() {
    let profile = BuildToolchainProfile {
        profile: linux_arm64_toolchain_profile(),
        cargo_target_triple: Some("aarch64-unknown-linux-gnu".to_string()),
        is_cross: true,
    };
    let mut info =
        build_model::BuildInfo::new(env!("CARGO_PKG_VERSION"), None, BuildMode::Release, None);

    apply_build_target_metadata(&mut info, Some(&profile)).unwrap();

    let (_, host_target) = rustc_toolchain_identity().unwrap();
    assert_eq!(info.platform.as_deref(), Some("linux-arm64"));
    assert_eq!(info.target_identity.as_deref(), Some("linux-arm64"));
    assert_eq!(
        info.rust_target_triple.as_deref(),
        Some("aarch64-unknown-linux-gnu")
    );
    assert_eq!(
        info.host_target_triple.as_deref(),
        Some(host_target.as_str())
    );
}

#[test]
fn deps_runtime_features_project_profile_before_validation() {
    let source = r#"
[package]
name = "profile_demo"
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
process = "main"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 1
output = ["sample"]

[instance.sink]
component = "sink"
process = "main"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
    let root = temp_test_dir("deps-profile");
    let rsdl = root.join("robot.rsdl");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&rsdl, source).unwrap();

    assert!(load_contract_from_rsdl(&rsdl).is_err());
    let features = deps_runtime_features(Some(&rsdl), Some("iox2"), None)
        .expect("deps feature inference should validate selected profile only");

    assert_eq!(features.canonical_names(), vec!["iox2"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn build_can_reuse_all_backend_dependency_cache_for_feature_subset() {
    let _lock = FLOWRT_CACHE_DIR_ENV_LOCK
        .lock()
        .expect("cache env lock should not be poisoned");
    let root = temp_test_dir("deps-cache-all-fallback");
    let cache = root.join("cache");
    let _env = EnvOverride::set("FLOWRT_CACHE_DIR", Some(cache.as_os_str()));

    let all_features = RuntimeFeatureSet::all();
    let all_layout = deps_cache_layout(BuildMode::Release, all_features.clone(), None).unwrap();
    write_deps_ready_marker(&all_layout, BuildMode::Release, &all_features).unwrap();

    let inproc = RuntimeFeatureSet::inproc_only();
    let selected = select_ready_deps_cache_layout(BuildMode::Release, &inproc, None).unwrap();

    assert_eq!(selected.target_dir, all_layout.target_dir);
    let _ = std::fs::remove_dir_all(&root);
}
