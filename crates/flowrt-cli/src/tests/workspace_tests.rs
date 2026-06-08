use super::*;

static REPO_RUNTIME_FALLBACK_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static FLOWRT_CACHE_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn patch_mismatched_flowrt_version() -> String {
    let parts = env!("CARGO_PKG_VERSION")
        .split('.')
        .map(|part| part.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    format!("{}.{}.{}", parts[0], parts[1], parts[2] + 1)
}

fn minor_mismatched_flowrt_version() -> String {
    let parts = env!("CARGO_PKG_VERSION")
        .split('.')
        .map(|part| part.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    format!("{}.{}.0", parts[0], parts[1] + 1)
}

struct EnvOverride {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvOverride {
    fn set(key: &'static str, value: Option<&std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: callers must guard process-wide environment mutation with a test mutex.
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self { key, previous }
    }

    fn repo_runtime_fallback(value: Option<&str>) -> Self {
        Self::set(
            "FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK",
            value.map(std::ffi::OsStr::new),
        )
    }
}

impl Drop for EnvOverride {
    fn drop(&mut self) {
        // SAFETY: guarded by REPO_RUNTIME_FALLBACK_ENV_LOCK in tests below.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

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
fn external_check_accepts_valid_package_manifest() {
    let root = temp_test_dir("external-check-valid");
    let package = root.join("fake_sensor_driver");
    std::fs::create_dir_all(package.join("bin")).unwrap();
    std::fs::write(package.join("bin/driver"), "#!/bin/sh\n").unwrap();
    std::fs::write(
        package.join("flowrt-external.toml"),
        r#"
[package]
name = "fake_sensor_driver"
version = "0.1.0"
flowrt_version = "0.7"
license = "MIT"

[[executable]]
name = "driver"
path = "bin/driver"
platforms = ["linux-x86_64", "linux-arm64"]
backends = ["zenoh"]
health = "runtime_socket"
"#,
    )
    .unwrap();

    let output = external_check_package_dir(&package).unwrap();

    assert!(output.contains("external package `fake_sensor_driver`"));
    assert!(output.contains("executable_count=1"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_list_reports_package_executable_summary() {
    let root = temp_test_dir("external-list");
    let package = root.join("fake_sensor_driver");
    std::fs::create_dir_all(package.join("bin")).unwrap();
    std::fs::write(package.join("bin/driver"), "#!/bin/sh\n").unwrap();
    std::fs::write(
        package.join("flowrt-external.toml"),
        r#"
[package]
name = "fake_sensor_driver"
version = "0.1.0"
flowrt_version = "0.7"
license = "MIT"

[[executable]]
name = "driver"
path = "bin/driver"
platforms = ["linux-arm64"]
backends = ["zenoh"]
health = "process_started"
"#,
    )
    .unwrap();

    let output = external_list_packages(&root).unwrap();

    assert!(output.contains("package=fake_sensor_driver"));
    assert!(output.contains("driver platforms=[linux-arm64] backends=[zenoh]"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_check_rejects_unknown_backend() {
    let root = temp_test_dir("external-check-backend");
    let package = root.join("bad_driver");
    std::fs::create_dir_all(package.join("bin")).unwrap();
    std::fs::write(package.join("bin/driver"), "#!/bin/sh\n").unwrap();
    std::fs::write(
        package.join("flowrt-external.toml"),
        r#"
[package]
name = "bad_driver"
version = "0.1.0"
flowrt_version = "0.7"
license = "MIT"

[[executable]]
name = "driver"
path = "bin/driver"
platforms = ["linux-x86_64"]
backends = ["mystery"]
health = "runtime_socket"
"#,
    )
    .unwrap();

    let error = external_check_package_dir(&package).unwrap_err();

    assert!(error.to_string().contains("unknown backend `mystery`"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn bundle_workspace_copies_built_artifacts_and_external_package() {
    let root = temp_test_dir("bundle-workspace");
    let rsdl_dir = root.join("rsdl");
    let external_root = root.join("external/fake_sensor_driver");
    let out_dir = root.join("flowrt");
    let bundle = root.join("dist/external-demo");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    std::fs::create_dir_all(external_root.join("bin")).unwrap();
    std::fs::write(external_root.join("bin/driver"), "#!/bin/sh\n").unwrap();
    std::fs::write(
        external_root.join("flowrt-external.toml"),
        r#"
[package]
name = "fake_sensor_driver"
version = "0.1.0"
flowrt_version = "0.7"
license = "MIT"

[[executable]]
name = "driver"
path = "bin/driver"
platforms = ["linux-x86_64"]
backends = ["zenoh"]
health = "process_started"
"#,
    )
    .unwrap();
    let source = r#"
[package]
name = "external_demo"
rsdl_version = "0.1"

[component.sensor]
language = "external"
kind = "external"
output = ["value:u32"]

[instance.sensor]
component = "sensor"
process = "sensor_proc"
target = "pi"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "bin/driver"
health = "process_started"
required_backends = ["zenoh"]

[profile.default]
backend = "zenoh"

[target.pi]
platform = "linux-arm64"
runtime = ["external"]
backends = ["zenoh"]
"#;
    let rsdl = rsdl_dir.join("robot.rsdl");
    std::fs::write(&rsdl, source).unwrap();
    let contract = contract_from_source(source);
    std::fs::create_dir_all(out_dir.join("contract")).unwrap();
    std::fs::create_dir_all(out_dir.join("selfdesc")).unwrap();
    std::fs::create_dir_all(out_dir.join("launch")).unwrap();
    std::fs::create_dir_all(out_dir.join("build/bin/release")).unwrap();
    std::fs::write(
        prepared_contract_path(&out_dir),
        contract.to_canonical_json().unwrap(),
    )
    .unwrap();
    std::fs::write(out_dir.join("selfdesc/selfdesc.json"), "{}\n").unwrap();
    std::fs::write(out_dir.join("launch/launch.json"), "{}\n").unwrap();
    let supervisor = out_dir.join("build/bin/release/external-demo-flowrt-supervisor");
    std::fs::write(&supervisor, "#!/bin/sh\n").unwrap();
    let mut info = build_model::BuildInfo::new(
        env!("CARGO_PKG_VERSION"),
        Some("default".into()),
        BuildMode::Release,
        None,
    );
    info.executables.supervisor = Some(PathBuf::from(
        "build/bin/release/external-demo-flowrt-supervisor",
    ));
    info.write(&out_dir).unwrap();

    let output = bundle_workspace(&rsdl, &contract, &out_dir, &bundle, None).unwrap();

    assert!(output.contains("created FlowRT bundle"));
    assert!(bundle.join("bundle.toml").is_file());
    assert!(bundle.join("bin/external-demo-flowrt-supervisor").is_file());
    assert!(bundle.join("flowrt/contract/contract.ir.json").is_file());
    assert!(
        bundle
            .join("external/fake_sensor_driver/flowrt-external.toml")
            .is_file()
    );
    assert!(
        bundle
            .join("external/fake_sensor_driver/bin/driver")
            .is_file()
    );
    let manifest: BundleManifest =
        toml::from_str(&std::fs::read_to_string(bundle.join("bundle.toml")).unwrap()).unwrap();
    assert_eq!(manifest.target, "pi");
    assert_eq!(manifest.platform.as_deref(), Some("linux-arm64"));
    assert_eq!(manifest.entry, "bin/external-demo-flowrt-supervisor");
    assert_eq!(manifest.external_processes.len(), 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_dry_run_reports_plan() {
    let root = temp_test_dir("deploy-dry-run");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = BundleManifest {
        schema_version: 1,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "external_demo".into(),
        profile: Some("default".into()),
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let output =
        deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "/tmp/flowrt-demo", true).unwrap();

    assert!(output.contains("deploy plan"));
    assert!(output.contains("robot@192.0.2.10"));
    assert!(output.contains("target=pi"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_rejects_target_mismatch() {
    let root = temp_test_dir("deploy-target-mismatch");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = BundleManifest {
        schema_version: 1,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "external_demo".into(),
        profile: Some("default".into()),
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "desktop",
        "/tmp/flowrt-demo",
        true,
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("does not match requested target")
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_allows_patch_version_mismatch_with_warning() {
    let root = temp_test_dir("deploy-patch-version-mismatch");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = BundleManifest {
        schema_version: 1,
        flowrt_version: patch_mismatched_flowrt_version(),
        package: "external_demo".into(),
        profile: Some("default".into()),
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let output =
        deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "/tmp/flowrt-demo", true).unwrap();

    assert!(output.contains("warning="), "unexpected output: {output}");
    assert!(
        output.contains("patch version"),
        "unexpected output: {output}"
    );
    assert!(
        output.contains("deploy plan"),
        "unexpected output: {output}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_rejects_minor_version_mismatch() {
    let root = temp_test_dir("deploy-minor-version-mismatch");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = BundleManifest {
        schema_version: 1,
        flowrt_version: minor_mismatched_flowrt_version(),
        package: "external_demo".into(),
        profile: Some("default".into()),
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error =
        deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "/tmp/flowrt-demo", true).unwrap_err();

    assert!(
        error.to_string().contains("incompatible FlowRT version"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_rejects_option_like_host_even_in_dry_run() {
    let root = temp_test_dir("deploy-host-option");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = BundleManifest {
        schema_version: 1,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "external_demo".into(),
        profile: Some("default".into()),
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error =
        deploy_bundle(&bundle, "-oProxyCommand=sh", "pi", "/tmp/flowrt-demo", true).unwrap_err();

    assert!(
        error.to_string().contains("must not start with `-`"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_rejects_empty_host_even_in_dry_run() {
    let root = temp_test_dir("deploy-host-empty");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = BundleManifest {
        schema_version: 1,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "external_demo".into(),
        profile: Some("default".into()),
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(&bundle, "", "pi", "/tmp/flowrt-demo", true).unwrap_err();

    assert!(
        error.to_string().contains("must not be empty"),
        "unexpected error: {error}"
    );

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
    let all_layout = deps_cache_layout(BuildMode::Release, all_features.clone()).unwrap();
    write_deps_ready_marker(&all_layout, BuildMode::Release, &all_features).unwrap();

    let inproc = RuntimeFeatureSet::inproc_only();
    let selected = select_ready_deps_cache_layout(BuildMode::Release, &inproc).unwrap();

    assert_eq!(selected.target_dir, all_layout.target_dir);
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
fn repo_runtime_dir_is_detected_for_dependency_prewarm() {
    let root = temp_test_dir("deps-repo-runtime-detection");
    let repo_runtime =
        repo_runtime_dir("runtime/rust", "Cargo.toml").expect("repo runtime should exist");
    let other_runtime = root.join("runtime/rust");
    std::fs::create_dir_all(&other_runtime).unwrap();
    std::fs::write(
        other_runtime.join("Cargo.toml"),
        "[package]\nname = \"flowrt\"\n",
    )
    .unwrap();

    assert!(is_repo_rust_runtime_dir(&repo_runtime).unwrap());
    assert!(!is_repo_rust_runtime_dir(&other_runtime).unwrap());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_manifest_package_name_reads_generated_package() {
    let root = temp_test_dir("cargo-manifest-package-name");
    let manifest = root.join("Cargo.toml");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        &manifest,
        r#"
[package]
name = "robot-flowrt-app"
version = "0.1.0"
"#,
    )
    .unwrap();

    assert_eq!(
        cargo_manifest_package_name(&manifest).unwrap(),
        "robot-flowrt-app"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn stale_generated_binary_outputs_are_removed_by_bin_name() {
    let root = temp_test_dir("stale-generated-bin");
    let manifest = root.join("Cargo.toml");
    let target_dir = root.join("target");
    let release_dir = target_dir.join("release");
    let deps_dir = release_dir.join("deps");
    std::fs::create_dir_all(&deps_dir).unwrap();
    std::fs::write(
        &manifest,
        r#"
[package]
name = "robot-flowrt-app"
version = "0.1.0"

[lib]
name = "flowrt_app"
"#,
    )
    .unwrap();
    std::fs::write(release_dir.join("robot-flowrt-supervisor"), "").unwrap();
    std::fs::write(release_dir.join("robot-flowrt-supervisor.d"), "").unwrap();
    std::fs::write(deps_dir.join("robot_flowrt_supervisor-123"), "").unwrap();
    std::fs::write(deps_dir.join("robot_flowrt_supervisor-123.d"), "").unwrap();
    std::fs::write(deps_dir.join("flowrt_app-123.rmeta"), "").unwrap();
    std::fs::write(deps_dir.join("libflowrt_app-123.rlib"), "").unwrap();
    std::fs::write(deps_dir.join("serde-123"), "").unwrap();
    let invocation = CargoBuildInvocation {
        manifest_path: manifest.clone(),
        current_dir: root.clone(),
        args: Vec::new(),
        target_dir: target_dir.clone(),
        bin_name: "robot-flowrt-supervisor".to_string(),
        build_mode: BuildMode::Release,
    };

    remove_stale_generated_binary_outputs(&invocation).unwrap();

    assert!(!release_dir.join("robot-flowrt-supervisor").exists());
    assert!(!release_dir.join("robot-flowrt-supervisor.d").exists());
    assert!(!deps_dir.join("robot_flowrt_supervisor-123").exists());
    assert!(!deps_dir.join("robot_flowrt_supervisor-123.d").exists());
    assert!(!deps_dir.join("flowrt_app-123.rmeta").exists());
    assert!(!deps_dir.join("libflowrt_app-123.rlib").exists());
    assert!(deps_dir.join("serde-123").exists());

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

    let target_dir = root.join("flowrt-cache").join("target");
    let invocation = cargo_build_invocation(
        &manifest,
        "robot-flowrt-app",
        BuildMode::Release,
        &target_dir,
    )
    .expect("cargo invocation should be derived from manifest");

    assert_eq!(invocation.current_dir, build_dir);
    assert_eq!(invocation.manifest_path, manifest);
    assert_eq!(invocation.target_dir, target_dir);
    assert!(invocation.args.iter().any(|arg| arg == "--release"));
    assert!(invocation.args.iter().any(|arg| arg == "--offline"));
    assert_eq!(
        invocation.executable_path(),
        invocation
            .target_dir
            .join("release")
            .join(format!("robot-flowrt-app{}", std::env::consts::EXE_SUFFIX))
    );
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

    let target_dir = root.join("target-cache");
    let invocation = cargo_build_invocation(
        relative_manifest,
        "robot-flowrt-app",
        BuildMode::Debug,
        &target_dir,
    )
    .expect("relative manifest should be resolved before cargo changes directory");

    assert_eq!(invocation.current_dir, build_dir);
    assert_eq!(invocation.manifest_path, manifest);
    assert_eq!(invocation.target_dir, target_dir);
    assert!(!invocation.args.iter().any(|arg| arg == "--release"));
    let manifest_arg = invocation
        .args
        .windows(2)
        .find_map(|args| (args[0] == "--manifest-path").then_some(args[1].as_str()))
        .expect("cargo invocation should pass --manifest-path");
    assert_eq!(Path::new(manifest_arg), manifest.as_path());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn clean_generated_cargo_package_uses_resolved_manifest_path() {
    let repo_dir = std::env::current_dir().unwrap();
    let root = repo_dir.join("target").join("tmp").join(format!(
        "flowrt-cargo-clean-relative-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let build_dir = root.join("flowrt").join("build");
    std::fs::create_dir_all(&build_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        r#"
[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[workspace]
"#,
    )
    .unwrap();
    std::fs::create_dir_all(build_dir.join("src")).unwrap();
    std::fs::write(build_dir.join("src").join("lib.rs"), "").unwrap();
    let relative_manifest = manifest.strip_prefix(&repo_dir).unwrap();

    let invocation = cargo_build_invocation(
        relative_manifest,
        "robot-flowrt-app",
        BuildMode::Release,
        &root.join("target-cache"),
    )
    .expect("relative manifest should be canonicalized");

    clean_generated_cargo_package(&invocation).unwrap();

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn built_executables_are_copied_to_local_release_bin() {
    let root = temp_test_dir("local-release-bin");
    let out_dir = root.join("flowrt");
    let cmake_dir = out_dir.join("build").join("cmake").join("release");
    std::fs::create_dir_all(&cmake_dir).unwrap();
    let built = cmake_dir.join(format!("robot_cpp_app{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&built, "binary").unwrap();

    let local = copy_executable_to_local_bin(&out_dir, BuildMode::Release, &built)
        .expect("built executable should be copied to local bin");

    assert_eq!(
        local,
        out_dir
            .join("build")
            .join("bin")
            .join("release")
            .join(format!("robot_cpp_app{}", std::env::consts::EXE_SUFFIX))
    );
    assert_eq!(
        relative_to_out_dir(&out_dir, &local).unwrap(),
        PathBuf::from("build")
            .join("bin")
            .join("release")
            .join(format!("robot_cpp_app{}", std::env::consts::EXE_SUFFIX))
    );
    assert_eq!(std::fs::read_to_string(local).unwrap(), "binary");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn existing_executable_only_records_real_files() {
    let root = temp_test_dir("existing-executable");
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("robot_app");
    let missing = root.join("missing_app");
    std::fs::write(&file, "binary").unwrap();

    assert_eq!(existing_executable(file.clone()), Some(file));
    assert_eq!(existing_executable(missing), None);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_configure_args_do_not_inject_runtime_dir_by_default() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");

    let args = cmake_configure_args(source_dir, build_dir, None, &[], BuildMode::Release);

    assert_eq!(
        args,
        vec![
            "-S".to_string(),
            "/tmp/flowrt/build".to_string(),
            "-B".to_string(),
            "/tmp/flowrt/build/cmake".to_string(),
            "-DCMAKE_BUILD_TYPE=Release".to_string()
        ]
    );
}

#[test]
fn cmake_configure_args_can_pass_explicit_runtime_dir() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let runtime_dir = Path::new("/opt/flowrt/runtime/cpp");

    let args = cmake_configure_args(
        source_dir,
        build_dir,
        Some(runtime_dir),
        &[runtime_dir.to_path_buf()],
        BuildMode::Debug,
    );

    assert!(args.contains(&"-DFLOWRT_CPP_RUNTIME_DIR=/opt/flowrt/runtime/cpp".to_string()));
    assert!(args.contains(&"-DCMAKE_PREFIX_PATH=/opt/flowrt/runtime/cpp".to_string()));
    assert!(args.contains(&"-DCMAKE_BUILD_TYPE=Debug".to_string()));
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
fn cmake_configure_args_can_split_runtime_headers_from_dependency_prefix() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let runtime_dir = Path::new("/repo/runtime/cpp");
    let sdk_prefix = Path::new("/opt/flowrt/0.1.0");

    let args = cmake_configure_args(
        source_dir,
        build_dir,
        Some(runtime_dir),
        &[sdk_prefix.to_path_buf()],
        BuildMode::Release,
    );

    assert!(args.contains(&"-DFLOWRT_CPP_RUNTIME_DIR=/repo/runtime/cpp".to_string()));
    assert!(args.contains(&"-DCMAKE_PREFIX_PATH=/opt/flowrt/0.1.0".to_string()));
}

#[test]
fn cmake_prefix_paths_merge_existing_env_and_runtime_prefix() {
    let runtime_dir = Path::new("/opt/flowrt/0.1.0");
    let existing = vec![PathBuf::from("/opt/ros/jazzy")];

    let prefixes = cmake_prefix_paths_for_runtime(Some(runtime_dir), &existing);

    assert_eq!(
        prefixes,
        vec![
            PathBuf::from("/opt/ros/jazzy"),
            PathBuf::from("/opt/flowrt/0.1.0")
        ]
    );
}

#[test]
fn repo_runtime_fallback_is_disabled_by_default() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);

    assert!(
        !repo_runtime_fallback_allowed(),
        "repo runtime fallback must be disabled by default"
    );
}

#[test]
fn repo_runtime_fallback_is_enabled_when_env_is_on() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);
    for value in &["ON", "1", "on", "true", "TRUE"] {
        // SAFETY: guarded by REPO_RUNTIME_FALLBACK_ENV_LOCK.
        unsafe { std::env::set_var("FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK", value) };
        assert!(
            repo_runtime_fallback_allowed(),
            "should be allowed for value={value}"
        );
    }
}

#[test]
fn cmake_configure_args_include_repo_fallback_flag_when_env_is_set() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(Some("ON"));

    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let args = cmake_configure_args(source_dir, build_dir, None, &[], BuildMode::Release);

    assert!(
        args.contains(&"-DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON".to_string()),
        "cmake args should include repo fallback flag when env is set: {args:?}"
    );
}

#[test]
fn cmake_configure_args_do_not_include_repo_fallback_flag_by_default() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);

    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let args = cmake_configure_args(source_dir, build_dir, None, &[], BuildMode::Release);

    assert!(
        !args
            .iter()
            .any(|arg| arg.contains("FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK")),
        "cmake args should not include repo fallback flag by default: {args:?}"
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
