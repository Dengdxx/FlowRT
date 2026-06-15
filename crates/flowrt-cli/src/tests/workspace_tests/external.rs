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
    let runtime_dep = out_dir.join("build/runtime-deps/linux-arm64/flowrt-target-sdk.toml");
    std::fs::create_dir_all(runtime_dep.parent().unwrap()).unwrap();
    std::fs::write(
        &runtime_dep,
        "platform = \"linux-arm64\"\ncomplete = true\n",
    )
    .unwrap();
    info.runtime_dependencies
        .push(build_model::BuildRuntimeDependencyInfo {
            name: "flowrt-target-sdk".into(),
            target: "pi".into(),
            platform: "linux-arm64".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            policy: "bundle".into(),
            path: PathBuf::from("build/runtime-deps/linux-arm64/flowrt-target-sdk.toml"),
            sha256: file_sha256(&runtime_dep).unwrap(),
        });
    info.write(&out_dir).unwrap();

    let output = bundle_workspace(&rsdl, &contract, &out_dir, &bundle, None, false).unwrap();

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
    assert_eq!(manifest.artifacts.len(), 3);
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
    assert!(manifest.artifacts.iter().any(|artifact| {
        artifact.kind == "runtime_dependency"
            && artifact.platform.as_deref() == Some("linux-arm64")
            && artifact.path.as_path()
                == Path::new("runtime-deps/linux-arm64/flowrt-target-sdk.toml")
    }));
    assert_eq!(manifest.runtime_dependencies.len(), 1);
    assert_eq!(manifest.runtime_dependencies[0].name, "flowrt-target-sdk");
    assert_eq!(manifest.runtime_dependencies[0].platform, "linux-arm64");
    assert_eq!(manifest.runtime_dependencies[0].policy, "bundle");
    assert_eq!(
        manifest.runtime_dependencies[0].path,
        PathBuf::from("runtime-deps/linux-arm64/flowrt-target-sdk.toml")
    );
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
