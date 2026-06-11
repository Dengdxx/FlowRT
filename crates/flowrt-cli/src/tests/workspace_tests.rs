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
fn external_check_rejects_absolute_executable_path() {
    let root = temp_test_dir("external-check-absolute-path");
    let package = root.join("fake_sensor_driver");
    std::fs::create_dir_all(&package).unwrap();
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
path = "/bin/sh"
platforms = ["linux-x86_64"]
backends = ["zenoh"]
health = "process_started"
"#,
    )
    .unwrap();

    let error = external_check_package_dir(&package)
        .expect_err("absolute executable path should be rejected");

    assert!(error.to_string().contains("path must be package-relative"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn external_check_rejects_executable_symlink_escape() {
    let root = temp_test_dir("external-check-symlink-escape");
    let package = root.join("fake_sensor_driver");
    let outside = root.join("outside/driver");
    std::fs::create_dir_all(package.join("bin")).unwrap();
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, package.join("bin/driver")).unwrap();
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
platforms = ["linux-amd64", "linux-arm64"]
backends = ["zenoh"]
health = "process_started"
"#,
    )
    .unwrap();

    #[cfg(unix)]
    {
        let error = external_check_package_dir(&package)
            .expect_err("symlink escaping package root should be rejected");
        assert!(error.to_string().contains("escapes package root"));
    }
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
fn external_check_rejects_unknown_platform() {
    let root = temp_test_dir("external-check-platform");
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
platforms = ["linux-riscv64"]
backends = ["zenoh"]
health = "runtime_socket"
"#,
    )
    .unwrap();

    let error = external_check_package_dir(&package).unwrap_err();

    assert!(error.to_string().contains("unsupported platform"));
    assert!(error.to_string().contains("linux-riscv64"));

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
platforms = ["linux-amd64", "linux-arm64"]
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
    std::fs::create_dir_all(out_dir.join("build/bin/linux-arm64/release")).unwrap();
    std::fs::write(
        prepared_contract_path(&out_dir),
        contract.to_canonical_json().unwrap(),
    )
    .unwrap();
    std::fs::write(out_dir.join("selfdesc/selfdesc.json"), "{}\n").unwrap();
    std::fs::write(out_dir.join("launch/launch.json"), "{}\n").unwrap();
    let supervisor = out_dir.join("build/bin/linux-arm64/release/external-demo-flowrt-supervisor");
    std::fs::write(&supervisor, "#!/bin/sh\n").unwrap();
    let supervisor_hash = file_sha256(&supervisor).unwrap();
    let mut info = build_model::BuildInfo::new(
        env!("CARGO_PKG_VERSION"),
        Some("default".into()),
        BuildMode::Release,
        None,
    );
    info.target = Some("pi".into());
    info.platform = Some("linux-arm64".into());
    info.target_identity = Some("linux-arm64:aarch64-unknown-linux-gnu".into());
    info.rust_target_triple = Some("aarch64-unknown-linux-gnu".into());
    info.host_target_triple = Some("x86_64-unknown-linux-gnu".into());
    info.executables.supervisor = Some(PathBuf::from(
        "build/bin/linux-arm64/release/external-demo-flowrt-supervisor",
    ));
    info.artifacts.push(build_model::BuildArtifactInfo {
        kind: "supervisor".into(),
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        path: PathBuf::from("build/bin/linux-arm64/release/external-demo-flowrt-supervisor"),
        sha256: supervisor_hash,
    });
    info.write(&out_dir).unwrap();

    let output = bundle_workspace(&rsdl, &contract, &out_dir, &bundle, None).unwrap();

    assert!(output.contains("created FlowRT bundle"));
    assert!(output.contains("stripped_executables=0"));
    assert!(output.contains("strip_warnings=0"));
    assert!(bundle.join("bundle.toml").is_file());
    assert!(
        bundle
            .join("bin/linux-arm64/external-demo-flowrt-supervisor")
            .is_file()
    );
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
    assert_eq!(
        manifest.entry,
        "bin/linux-arm64/external-demo-flowrt-supervisor"
    );
    assert_eq!(manifest.schema_version, 2);
    assert_eq!(manifest.artifacts.len(), 2);
    let supervisor_artifact = manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == "supervisor")
        .unwrap();
    assert_eq!(supervisor_artifact.target, "pi");
    assert_eq!(supervisor_artifact.platform.as_deref(), Some("linux-arm64"));
    assert_eq!(
        supervisor_artifact.path,
        PathBuf::from("bin/linux-arm64/external-demo-flowrt-supervisor")
    );
    assert_eq!(
        supervisor_artifact.sha256,
        file_sha256(&bundle.join(&supervisor_artifact.path)).unwrap()
    );
    assert!(manifest.artifacts.iter().any(|artifact| {
        artifact.kind == "external_process" && artifact.platform.as_deref() == Some("linux-arm64")
    }));
    assert!(
        manifest
            .artifacts
            .iter()
            .all(|artifact| artifact.sha256.len() == 64)
    );
    assert_eq!(manifest.external_processes.len(), 1);
    assert_eq!(
        manifest.external_processes[0].supported_platforms,
        vec!["linux-amd64".to_string(), "linux-arm64".to_string()]
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn bundle_target_platform_prefers_build_info_over_contract_target() {
    let contract = contract_from_source(
        r#"
[package]
name = "cross_bundle_demo"
rsdl_version = "0.1"

[target.pi]
platform = "linux-amd64"
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let mut info =
        build_model::BuildInfo::new(env!("CARGO_PKG_VERSION"), None, BuildMode::Release, None);
    info.target = Some("pi".into());
    info.platform = Some("linux-arm64".into());

    assert_eq!(bundle_target_name_for_build(&info, &contract), "pi");
    assert_eq!(
        bundle_target_platform_for_build(&info, &contract).unwrap(),
        Some("linux-arm64".into())
    );
}

#[test]
fn bundle_strip_skips_non_elf_executables() {
    let root = temp_test_dir("bundle-strip-non-elf");
    std::fs::create_dir_all(&root).unwrap();
    let script = root.join("app");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();

    let outcome = strip_bundle_executable(&script).unwrap();

    assert_eq!(outcome, BundleStripOutcome::Skipped);
    assert_eq!(std::fs::read_to_string(&script).unwrap(), "#!/bin/sh\n");

    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn copy_dir_recursive_rejects_symlink_entries() {
    let root = temp_test_dir("bundle-reject-symlink");
    let source = root.join("source");
    let dest = root.join("dest");
    let outside = root.join("outside.txt");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("regular.txt"), "inside").unwrap();
    std::fs::write(&outside, "outside").unwrap();
    std::os::unix::fs::symlink(&outside, source.join("leak.txt")).unwrap();

    let error = copy_dir_recursive(&source, &dest).unwrap_err();

    assert!(
        error.to_string().contains("symbolic link"),
        "unexpected error: {error}"
    );
    assert!(!dest.join("leak.txt").exists());

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
        artifacts: vec![],
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
fn deploy_bundle_v2_dry_run_selects_target_artifacts() {
    let root = temp_test_dir("deploy-v2-artifacts");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin/linux-amd64")).unwrap();
    std::fs::create_dir_all(bundle.join("bin/linux-arm64")).unwrap();
    std::fs::write(bundle.join("bin/linux-amd64/flowrt-supervisor"), b"desktop").unwrap();
    std::fs::write(bundle.join("bin/linux-arm64/flowrt-supervisor"), b"pi").unwrap();
    let desktop_hash = file_sha256(&bundle.join("bin/linux-amd64/flowrt-supervisor")).unwrap();
    let pi_hash = file_sha256(&bundle.join("bin/linux-arm64/flowrt-supervisor")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "multi_target_demo".into(),
        profile: Some("default".into()),
        target: "bundle".into(),
        platform: None,
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        artifacts: vec![
            BundleArtifact {
                kind: "supervisor".into(),
                target: "desktop".into(),
                platform: Some("linux-amd64".into()),
                path: "bin/linux-amd64/flowrt-supervisor".into(),
                sha256: desktop_hash,
            },
            BundleArtifact {
                kind: "supervisor".into(),
                target: "pi".into(),
                platform: Some("linux-arm64".into()),
                path: "bin/linux-arm64/flowrt-supervisor".into(),
                sha256: pi_hash,
            },
        ],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let output =
        deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "/tmp/flowrt-demo", true).unwrap();

    assert!(output.contains("target=pi"), "unexpected output: {output}");
    assert!(
        output.contains("artifacts=1 platforms=[linux-arm64]"),
        "unexpected output: {output}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_v2_rejects_unsafe_artifact_path() {
    let root = temp_test_dir("deploy-v2-unsafe-artifact");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "bad_bundle".into(),
        profile: Some("default".into()),
        target: "bundle".into(),
        platform: None,
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        artifacts: vec![BundleArtifact {
            kind: "supervisor".into(),
            target: "pi".into(),
            platform: Some("linux-arm64".into()),
            path: "../escape".into(),
            sha256: "00".into(),
        }],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error =
        deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "/tmp/flowrt-demo", true).unwrap_err();

    assert!(
        error.to_string().contains("unsafe artifact path"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_v2_rejects_missing_artifact_platform() {
    let root = temp_test_dir("deploy-v2-missing-platform");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin")).unwrap();
    std::fs::write(bundle.join("bin/pi-supervisor"), b"pi").unwrap();
    let pi_hash = file_sha256(&bundle.join("bin/pi-supervisor")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "bad_bundle".into(),
        profile: Some("default".into()),
        target: "bundle".into(),
        platform: None,
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        artifacts: vec![BundleArtifact {
            kind: "supervisor".into(),
            target: "pi".into(),
            platform: None,
            path: "bin/pi-supervisor".into(),
            sha256: pi_hash,
        }],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error =
        deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "/tmp/flowrt-demo", true).unwrap_err();

    assert!(
        error.to_string().contains("missing platform metadata"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_v2_rejects_artifact_platform_mismatch_with_rebuild_hint() {
    let root = temp_test_dir("deploy-v2-platform-mismatch");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin/linux-arm64")).unwrap();
    std::fs::write(bundle.join("bin/linux-arm64/pi-supervisor"), b"pi").unwrap();
    let pi_hash = file_sha256(&bundle.join("bin/linux-arm64/pi-supervisor")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "bad_bundle".into(),
        profile: Some("default".into()),
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        artifacts: vec![BundleArtifact {
            kind: "supervisor".into(),
            target: "pi".into(),
            platform: Some("linux-amd64".into()),
            path: "bin/linux-arm64/pi-supervisor".into(),
            sha256: pi_hash,
        }],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error =
        deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "/tmp/flowrt-demo", true).unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("platform mismatch"),
        "unexpected error: {error}"
    );
    assert!(
        message.contains("flowrt build --target linux-arm64 --launcher"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_v2_rejects_hash_mismatch_with_rebuild_hint() {
    let root = temp_test_dir("deploy-v2-hash-mismatch");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin/linux-arm64")).unwrap();
    std::fs::write(bundle.join("bin/linux-arm64/pi-supervisor"), b"pi").unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "bad_bundle".into(),
        profile: Some("default".into()),
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        artifacts: vec![BundleArtifact {
            kind: "supervisor".into(),
            target: "pi".into(),
            platform: Some("linux-arm64".into()),
            path: "bin/linux-arm64/pi-supervisor".into(),
            sha256: "0".repeat(64),
        }],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error =
        deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "/tmp/flowrt-demo", true).unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("sha256 mismatch"),
        "unexpected error: {error}"
    );
    assert!(
        message.contains("flowrt build --target linux-arm64 --launcher"),
        "unexpected error: {error}"
    );

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
        artifacts: vec![],
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
        artifacts: vec![],
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
        artifacts: vec![],
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
        artifacts: vec![],
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
        artifacts: vec![],
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
fn deploy_bundle_rejects_empty_remote_dir_even_in_dry_run() {
    let root = temp_test_dir("deploy-remote-dir-empty");
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
        artifacts: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "  ", true).unwrap_err();

    assert!(
        error.to_string().contains("remote_dir must not be empty"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_rejects_unsafe_remote_dir_even_in_dry_run() {
    let root = temp_test_dir("deploy-remote-dir-unsafe");
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
        artifacts: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    for remote_dir in [
        "relative/path",
        "/tmp/flowrt-demo;touch /tmp/pwned",
        "/tmp/flowrt demo",
        "/tmp/flowrt`id`",
        "/tmp/flowrt$(id)",
        "/tmp/../root",
    ] {
        let error = deploy_bundle(&bundle, "robot@192.0.2.10", "pi", remote_dir, true).unwrap_err();
        assert!(
            error.to_string().contains("deploy remote_dir"),
            "unexpected error for {remote_dir:?}: {error}"
        );
    }

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn remote_flowrt_version_output_accepts_same_minor_patch_mismatch() {
    let warning =
        validate_remote_flowrt_version_check(true, "flowrt 0.7.9\n", "", "0.7.0").unwrap();

    assert!(
        warning
            .as_deref()
            .is_some_and(|message| message.contains("remote patch version 0.7.9")),
        "unexpected warning: {warning:?}"
    );
}

#[test]
fn remote_flowrt_version_output_rejects_incompatible_minor() {
    let error =
        validate_remote_flowrt_version_check(true, "flowrt 0.8.0\n", "", "0.7.0").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("incompatible remote FlowRT version"),
        "unexpected error: {error}"
    );
}

#[test]
fn remote_flowrt_version_output_rejects_missing_version() {
    let error =
        validate_remote_flowrt_version_check(true, "flowrt development build\n", "", "0.7.0")
            .unwrap_err();

    assert!(
        error.to_string().contains("did not contain"),
        "unexpected error: {error}"
    );
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
fn deps_workspace_manifest_declares_own_workspace_root() {
    let root = temp_test_dir("deps-workspace-isolated");
    let deps_workspace = root.join(".flowrt-cache/deps-workspaces/flowrt-test");
    let repo_runtime =
        repo_runtime_dir("runtime/rust", "Cargo.toml").expect("repo runtime should exist");
    let features = RuntimeFeatureSet::all();

    write_deps_workspace(&deps_workspace, &repo_runtime, &features)
        .expect("deps workspace should be written");

    let manifest = std::fs::read_to_string(deps_workspace.join("Cargo.toml")).unwrap();
    assert!(
        manifest.contains("\n[workspace]\n"),
        "deps workspace manifest must stop Cargo from inheriting a parent workspace:\n{manifest}"
    );

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
        None,
        None,
    )
    .expect("cargo invocation should be derived from manifest");

    assert_eq!(invocation.current_dir, build_dir);
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
        None,
        None,
    )
    .expect("relative manifest should be resolved before cargo changes directory");

    assert_eq!(invocation.current_dir, build_dir);
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
fn built_executables_are_copied_to_local_release_bin() {
    let root = temp_test_dir("local-release-bin");
    let out_dir = root.join("flowrt");
    let cmake_dir = out_dir.join("build").join("cmake").join("release");
    std::fs::create_dir_all(&cmake_dir).unwrap();
    let built = cmake_dir.join(format!("robot_cpp_app{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&built, "binary").unwrap();

    let local = copy_executable_to_local_bin(&out_dir, BuildMode::Release, None, &built)
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
fn cross_built_executables_are_copied_to_target_platform_bin() {
    let root = temp_test_dir("target-platform-bin");
    let out_dir = root.join("flowrt");
    let cargo_dir = root
        .join("cargo-target")
        .join("aarch64-unknown-linux-gnu")
        .join("release");
    std::fs::create_dir_all(&cargo_dir).unwrap();
    let built = cargo_dir.join(format!("robot-flowrt-app{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&built, "binary").unwrap();

    let local =
        copy_executable_to_local_bin(&out_dir, BuildMode::Release, Some("linux-arm64"), &built)
            .expect("cross built executable should be copied to target bin");

    assert_eq!(
        local,
        out_dir
            .join("build")
            .join("bin")
            .join("linux-arm64")
            .join("release")
            .join(format!("robot-flowrt-app{}", std::env::consts::EXE_SUFFIX))
    );
    assert_eq!(
        relative_to_out_dir(&out_dir, &local).unwrap(),
        PathBuf::from("build")
            .join("bin")
            .join("linux-arm64")
            .join("release")
            .join(format!("robot-flowrt-app{}", std::env::consts::EXE_SUFFIX))
    );
    assert_eq!(std::fs::read_to_string(local).unwrap(), "binary");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_build_dir_is_separated_by_target_platform() {
    let out_dir = Path::new("/tmp/flowrt");

    assert_eq!(
        cmake_build_dir(out_dir, BuildMode::Release, None),
        PathBuf::from("/tmp/flowrt/build/cmake/release")
    );
    assert_eq!(
        cmake_build_dir(out_dir, BuildMode::Release, Some("linux-arm64")),
        PathBuf::from("/tmp/flowrt/build/cmake/linux-arm64/release")
    );
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

    let args = cmake_configure_args(
        source_dir,
        build_dir,
        None,
        &[],
        BuildMode::Release,
        None,
        false,
    );

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
        None,
        false,
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
fn installed_runtime_vendor_hash_requires_packaged_marker() {
    let root = temp_test_dir("vendor-hash-missing-marker");
    let runtime_dir = root.join("opt/flowrt/0.7.1/share/flowrt/runtime/rust");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::write(
        runtime_dir.join("Cargo.toml"),
        "[package]\nname = \"flowrt\"\n",
    )
    .unwrap();

    let error = flowrt_vendor_hash(Some(&runtime_dir)).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("FlowRT vendor hash marker is missing"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn installed_runtime_vendor_hash_reads_packaged_marker() {
    let root = temp_test_dir("vendor-hash-marker");
    let runtime_dir = root.join("opt/flowrt/0.7.1/share/flowrt/runtime/rust");
    let vendor_dir = root.join("opt/flowrt/0.7.1/share/cargo/vendor");
    std::fs::create_dir_all(&runtime_dir).unwrap();
    std::fs::create_dir_all(&vendor_dir).unwrap();
    std::fs::write(
        runtime_dir.join("Cargo.toml"),
        "[package]\nname = \"flowrt\"\n",
    )
    .unwrap();
    std::fs::write(
        vendor_dir.join(".flowrt-vendor.sha256"),
        "abcdef1234567890  -\n",
    )
    .unwrap();

    let hash = flowrt_vendor_hash(Some(&runtime_dir)).unwrap();

    assert_eq!(hash, "abcdef1234567890");

    let _ = std::fs::remove_dir_all(&root);
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
        None,
        false,
    );

    assert!(args.contains(&"-DFLOWRT_CPP_RUNTIME_DIR=/repo/runtime/cpp".to_string()));
    assert!(args.contains(&"-DCMAKE_PREFIX_PATH=/opt/flowrt/0.1.0".to_string()));
}

fn write_target_sdk_manifest(root: &Path, platform: &str, complete: bool) {
    std::fs::create_dir_all(root.join("include/flowrt")).unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::create_dir_all(root.join("cmake")).unwrap();
    std::fs::create_dir_all(root.join("pkgconfig")).unwrap();
    std::fs::write(root.join("include/flowrt/runtime.hpp"), "").unwrap();
    std::fs::write(
        root.join("flowrt-target-sdk.toml"),
        format!(
            r#"
schema_version = 1
platform = "{platform}"
complete = {complete}
include_dir = "include"
lib_dir = "lib"
cmake_dir = "cmake"
pkgconfig_dir = "pkgconfig"
"#
        ),
    )
    .unwrap();
}

fn linux_arm64_toolchain_profile() -> crate::toolchain::ToolchainProfile {
    crate::toolchain::ToolchainProfile {
        platform: "linux-arm64".to_string(),
        rust_target: "aarch64-unknown-linux-gnu".to_string(),
        deb_multiarch: "aarch64-linux-gnu".to_string(),
        c_compiler: "aarch64-linux-gnu-gcc".to_string(),
        cpp_compiler: "aarch64-linux-gnu-g++".to_string(),
        sysroot: Some(PathBuf::from("/opt/sysroots/linux-arm64")),
        cmake_toolchain: None,
        pkg_config_libdir: Some(PathBuf::from("/opt/toolchains/linux-arm64/pkgconfig")),
        pkg_config_libdirs: Vec::new(),
        cmake_prefix_paths: Vec::new(),
        sdk_overlays: Vec::new(),
        runtime_dependency_policy: crate::toolchain::RuntimeDependencyPolicy::Bundle,
    }
}

#[test]
fn cmake_target_sdk_root_requires_complete_manifest() {
    let root = temp_test_dir("target-sdk-incomplete");
    let private_prefix = root.join("opt/flowrt/0.8.3");
    let sdk_root = private_prefix.join("targets/linux-arm64");
    write_target_sdk_manifest(&sdk_root, "linux-arm64", false);

    let error = resolve_cpp_target_sdk_root(Some(&private_prefix), "linux-arm64").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("FlowRT target SDK for linux-arm64 is incomplete"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_target_sdk_root_reports_missing_manifest() {
    let root = temp_test_dir("target-sdk-missing");
    let private_prefix = root.join("opt/flowrt/0.8.3");
    std::fs::create_dir_all(private_prefix.join("targets/linux-arm64")).unwrap();

    let error = resolve_cpp_target_sdk_root(Some(&private_prefix), "linux-arm64").unwrap_err();

    assert!(
        error
            .to_string()
            .contains("FlowRT target SDK for linux-arm64 is missing"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_configure_args_use_target_sdk_and_compilers_without_toolchain_file() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);
    let root = temp_test_dir("cmake-target-sdk-compilers");
    let private_prefix = root.join("opt/flowrt/0.8.3");
    let sdk_root = private_prefix.join("targets/linux-arm64");
    write_target_sdk_manifest(&sdk_root, "linux-arm64", true);
    let sdk = resolve_cpp_target_sdk_root(Some(&private_prefix), "linux-arm64").unwrap();
    let profile = linux_arm64_toolchain_profile();
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let prefixes = cmake_prefix_paths_for_target_sdk(&sdk, &[], &[PathBuf::from("/opt/ros/jazzy")]);

    let args = cmake_configure_args(
        source_dir,
        build_dir,
        Some(&sdk.root),
        &prefixes,
        BuildMode::Release,
        Some(&profile),
        true,
    );

    let prefix_arg = args
        .iter()
        .find(|arg| arg.starts_with("-DCMAKE_PREFIX_PATH="))
        .expect("target CMake prefix path should be set");
    assert!(
        prefix_arg.starts_with(&format!("-DCMAKE_PREFIX_PATH={}", sdk.root.display())),
        "target SDK root should have prefix priority: {prefix_arg}"
    );
    assert!(args.contains(&format!("-DFLOWRT_CPP_RUNTIME_DIR={}", sdk.root.display())));
    assert!(args.contains(&"-DCMAKE_C_COMPILER=aarch64-linux-gnu-gcc".to_string()));
    assert!(args.contains(&"-DCMAKE_CXX_COMPILER=aarch64-linux-gnu-g++".to_string()));
    assert!(args.contains(&"-DCMAKE_SYSTEM_NAME=Linux".to_string()));
    assert!(args.contains(&"-DCMAKE_SYSTEM_PROCESSOR=aarch64".to_string()));
    assert!(args.contains(&"-DCMAKE_SYSROOT=/opt/sysroots/linux-arm64".to_string()));
    assert!(
        args.iter()
            .all(|arg| !arg.starts_with("-DCMAKE_TOOLCHAIN_FILE="))
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_configure_args_use_profile_toolchain_file_when_present() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let mut profile = linux_arm64_toolchain_profile();
    profile.cmake_toolchain = Some(PathBuf::from("/opt/toolchains/linux-arm64.cmake"));

    let args = cmake_configure_args(
        source_dir,
        build_dir,
        None,
        &[],
        BuildMode::Release,
        Some(&profile),
        true,
    );

    assert!(args.contains(&"-DCMAKE_TOOLCHAIN_FILE=/opt/toolchains/linux-arm64.cmake".to_string()));
    assert!(
        args.iter()
            .all(|arg| !arg.starts_with("-DCMAKE_C_COMPILER="))
    );
    assert!(
        args.iter()
            .all(|arg| !arg.starts_with("-DCMAKE_CXX_COMPILER="))
    );
    assert!(
        args.iter()
            .all(|arg| !arg.starts_with("-DCMAKE_SYSTEM_NAME="))
    );
    assert!(
        args.iter()
            .all(|arg| !arg.starts_with("-DCMAKE_SYSTEM_PROCESSOR="))
    );
}

#[test]
fn cmake_configure_env_sets_pkg_config_libdir_for_target_sdk() {
    let root = temp_test_dir("cmake-target-sdk-pkgconfig");
    let private_prefix = root.join("opt/flowrt/0.8.3");
    let sdk_root = private_prefix.join("targets/linux-arm64");
    write_target_sdk_manifest(&sdk_root, "linux-arm64", true);
    let sdk = resolve_cpp_target_sdk_root(Some(&private_prefix), "linux-arm64").unwrap();
    let profile = linux_arm64_toolchain_profile();

    let env = cmake_configure_env(Some(&profile), Some(&sdk));
    let pkg_config_libdir = env
        .get("PKG_CONFIG_LIBDIR")
        .expect("PKG_CONFIG_LIBDIR should be set")
        .to_string_lossy();

    assert!(
        pkg_config_libdir.contains("/opt/toolchains/linux-arm64/pkgconfig"),
        "profile pkg-config path should be preserved: {pkg_config_libdir}"
    );
    assert!(
        pkg_config_libdir.contains(&sdk.root.join("pkgconfig").to_string_lossy().to_string()),
        "target SDK pkg-config path should be included: {pkg_config_libdir}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_prefix_paths_merge_existing_env_and_runtime_prefix() {
    let runtime_dir = Path::new("/opt/flowrt/0.1.0");
    let existing = vec![PathBuf::from("/opt/ros/jazzy")];

    let prefixes = cmake_prefix_paths_for_runtime(Some(runtime_dir), &[], &existing);

    assert_eq!(
        prefixes,
        vec![
            PathBuf::from("/opt/ros/jazzy"),
            PathBuf::from("/opt/flowrt/0.1.0")
        ]
    );
}

#[test]
fn launch_library_paths_include_private_and_target_sdk_libs() {
    let root = temp_test_dir("launch-library-paths");
    let private_prefix = root.join("opt/flowrt/0.8.3");
    std::fs::create_dir_all(private_prefix.join("lib")).unwrap();
    std::fs::create_dir_all(private_prefix.join("targets/linux-arm64/lib")).unwrap();
    std::fs::create_dir_all(private_prefix.join("include/flowrt")).unwrap();
    std::fs::create_dir_all(private_prefix.join("share")).unwrap();
    std::fs::write(private_prefix.join("include/flowrt/runtime.hpp"), "").unwrap();

    let paths = flowrt_runtime_library_paths(&private_prefix, Some("linux-arm64"));

    assert_eq!(
        paths,
        vec![
            private_prefix.join("lib"),
            private_prefix.join("targets/linux-arm64/lib")
        ]
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_configure_uses_toolchain_sdk_overlays() {
    let root = temp_test_dir("cmake-toolchain-sdk-overlays");
    let private_prefix = root.join("opt/flowrt/0.8.3");
    let sdk_root = private_prefix.join("targets/linux-arm64");
    write_target_sdk_manifest(&sdk_root, "linux-arm64", true);
    let sdk = resolve_cpp_target_sdk_root(Some(&private_prefix), "linux-arm64").unwrap();
    let mut profile = linux_arm64_toolchain_profile();
    profile.cmake_prefix_paths = vec![PathBuf::from("/opt/vendor/cmake-prefix")];
    profile.sdk_overlays = vec![PathBuf::from("/opt/vendor/rknn")];
    profile.pkg_config_libdirs = vec![PathBuf::from("/opt/vendor/pkgconfig")];

    let toolchain_prefixes = toolchain_profile_cmake_prefix_paths(&profile);
    let prefixes = cmake_prefix_paths_for_target_sdk(&sdk, &toolchain_prefixes, &[]);
    assert!(prefixes.contains(&PathBuf::from("/opt/vendor/cmake-prefix")));
    assert!(prefixes.contains(&PathBuf::from("/opt/vendor/rknn")));
    assert!(prefixes.contains(&PathBuf::from("/opt/vendor/rknn/cmake")));

    let env = cmake_configure_env(Some(&profile), Some(&sdk));
    let pkg_config_libdir = env
        .get("PKG_CONFIG_LIBDIR")
        .expect("PKG_CONFIG_LIBDIR should be set")
        .to_string_lossy();
    assert!(pkg_config_libdir.contains("/opt/vendor/pkgconfig"));
    assert!(pkg_config_libdir.contains("/opt/vendor/rknn/pkgconfig"));
    assert!(pkg_config_libdir.contains("/opt/vendor/rknn/lib/pkgconfig"));
    assert!(pkg_config_libdir.contains("/opt/vendor/rknn/lib/aarch64-linux-gnu/pkgconfig"));

    let _ = std::fs::remove_dir_all(&root);
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
    let args = cmake_configure_args(
        source_dir,
        build_dir,
        None,
        &[],
        BuildMode::Release,
        None,
        false,
    );

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
    let args = cmake_configure_args(
        source_dir,
        build_dir,
        None,
        &[],
        BuildMode::Release,
        None,
        false,
    );

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
