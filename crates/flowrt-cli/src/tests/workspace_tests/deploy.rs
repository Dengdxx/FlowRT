use super::*;

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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "bundle".into(),
        platform: None,
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
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

    let output = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap();

    assert!(output.contains("target=pi"), "unexpected output: {output}");
    assert!(
        output.contains("artifacts=1 platforms=[linux-arm64]"),
        "unexpected output: {output}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_dry_run_reports_managed_activate_and_start_plan() {
    let root = temp_test_dir("deploy-managed-plan");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin/linux-arm64")).unwrap();
    std::fs::write(bundle.join("bin/linux-arm64/flowrt-supervisor"), b"pi").unwrap();
    let pi_hash = file_sha256(&bundle.join("bin/linux-arm64/flowrt-supervisor")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "managed_deploy_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![BundleArtifact {
            kind: "supervisor".into(),
            target: "pi".into(),
            platform: Some("linux-arm64".into()),
            path: "bin/linux-arm64/flowrt-supervisor".into(),
            sha256: pi_hash,
        }],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let output = deploy_bundle_with_options(DeployOptions {
        bundle: &bundle,
        host: "robot@192.0.2.10",
        target: "pi",
        remote_dir: "/opt/flowrt-demo",
        dry_run: true,
        allow_island: false,
        activate: true,
        start: true,
    })
    .unwrap();

    assert!(
        output.contains("managed=install,activate,start"),
        "unexpected output: {output}"
    );
    assert!(output.contains("release="), "unexpected output: {output}");
    assert!(
        output.contains("incoming=/opt/flowrt-demo/incoming/"),
        "unexpected output: {output}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_v2_allows_complete_multi_platform_external_package_closure() {
    let root = temp_test_dir("deploy-v2-external-complete");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin/linux-amd64")).unwrap();
    std::fs::create_dir_all(bundle.join("bin/linux-arm64")).unwrap();
    std::fs::create_dir_all(bundle.join("external/fake_sensor_driver/bin")).unwrap();
    std::fs::write(bundle.join("bin/linux-amd64/flowrt-supervisor"), b"desktop").unwrap();
    std::fs::write(bundle.join("bin/linux-arm64/flowrt-supervisor"), b"pi").unwrap();
    std::fs::write(
        bundle.join("external/fake_sensor_driver/bin/driver"),
        b"driver",
    )
    .unwrap();
    let desktop_hash = file_sha256(&bundle.join("bin/linux-amd64/flowrt-supervisor")).unwrap();
    let pi_hash = file_sha256(&bundle.join("bin/linux-arm64/flowrt-supervisor")).unwrap();
    let driver_hash = file_sha256(&bundle.join("external/fake_sensor_driver/bin/driver")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "multi_target_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "bundle".into(),
        platform: None,
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![BundleExternalProcess {
            process: "sensor_proc".into(),
            package: "fake_sensor_driver".into(),
            executable: "bin/driver".into(),
            path: "external/fake_sensor_driver".into(),
            platform: None,
            supported_platforms: vec!["linux-amd64".into(), "linux-arm64".into()],
        }],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![
            BundleArtifact {
                kind: "supervisor".into(),
                target: "desktop".into(),
                platform: Some("linux-amd64".into()),
                path: "bin/linux-amd64/flowrt-supervisor".into(),
                sha256: desktop_hash,
            },
            BundleArtifact {
                kind: "external_process".into(),
                target: "desktop".into(),
                platform: Some("linux-amd64".into()),
                path: "external/fake_sensor_driver/bin/driver".into(),
                sha256: driver_hash.clone(),
            },
            BundleArtifact {
                kind: "supervisor".into(),
                target: "pi".into(),
                platform: Some("linux-arm64".into()),
                path: "bin/linux-arm64/flowrt-supervisor".into(),
                sha256: pi_hash,
            },
            BundleArtifact {
                kind: "external_process".into(),
                target: "pi".into(),
                platform: Some("linux-arm64".into()),
                path: "external/fake_sensor_driver/bin/driver".into(),
                sha256: driver_hash,
            },
        ],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let desktop = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "desktop",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap();
    let pi = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap();

    assert!(desktop.contains("artifacts=2 platforms=[linux-amd64]"));
    assert!(pi.contains("artifacts=2 platforms=[linux-arm64]"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_v2_rejects_resource_provider_without_external_package_closure() {
    let root = temp_test_dir("deploy-v2-resource-provider-missing-package");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin/linux-arm64")).unwrap();
    std::fs::write(bundle.join("bin/linux-arm64/flowrt-supervisor"), b"pi").unwrap();
    let supervisor_hash = file_sha256(&bundle.join("bin/linux-arm64/flowrt-supervisor")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "resource_provider_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![BundleResourceProvider {
            graph: "main".into(),
            name: "camera_provider".into(),
            scope: "external_package".into(),
            target: None,
            process: None,
            external_package: Some("fake_sensor_driver".into()),
            capabilities: vec!["perception.camera.frames".into()],
        }],
        runtime_dependencies: vec![],
        artifacts: vec![BundleArtifact {
            kind: "supervisor".into(),
            target: "pi".into(),
            platform: Some("linux-arm64".into()),
            path: "bin/linux-arm64/flowrt-supervisor".into(),
            sha256: supervisor_hash,
        }],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("resource provider"),
        "unexpected error: {error}"
    );
    assert!(
        message.contains("fake_sensor_driver"),
        "unexpected error: {error}"
    );
    assert!(
        message.contains("external package"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_v2_rejects_missing_external_package_artifact_for_selected_platform() {
    let root = temp_test_dir("deploy-v2-external-missing-platform");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin/linux-amd64")).unwrap();
    std::fs::create_dir_all(bundle.join("external/fake_sensor_driver/bin")).unwrap();
    std::fs::write(bundle.join("bin/linux-amd64/flowrt-supervisor"), b"desktop").unwrap();
    std::fs::write(
        bundle.join("external/fake_sensor_driver/bin/driver"),
        b"driver",
    )
    .unwrap();
    let supervisor_hash = file_sha256(&bundle.join("bin/linux-amd64/flowrt-supervisor")).unwrap();
    let driver_hash = file_sha256(&bundle.join("external/fake_sensor_driver/bin/driver")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "multi_target_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "bundle".into(),
        platform: None,
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-amd64/flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![BundleExternalProcess {
            process: "sensor_proc".into(),
            package: "fake_sensor_driver".into(),
            executable: "bin/driver".into(),
            path: "external/fake_sensor_driver".into(),
            platform: None,
            supported_platforms: vec!["linux-amd64".into(), "linux-arm64".into()],
        }],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![
            BundleArtifact {
                kind: "supervisor".into(),
                target: "desktop".into(),
                platform: Some("linux-amd64".into()),
                path: "bin/linux-amd64/flowrt-supervisor".into(),
                sha256: supervisor_hash,
            },
            BundleArtifact {
                kind: "external_process".into(),
                target: "pi".into(),
                platform: Some("linux-arm64".into()),
                path: "external/fake_sensor_driver/bin/driver".into(),
                sha256: driver_hash,
            },
        ],
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
        false,
    )
    .unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("external package"),
        "unexpected error: {error}"
    );
    assert!(
        message.contains("fake_sensor_driver"),
        "unexpected error: {error}"
    );
    assert!(message.contains("linux-amd64"), "unexpected error: {error}");
    assert!(
        message.contains("missing artifact"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_v2_rejects_external_package_artifact_platform_mismatch() {
    let root = temp_test_dir("deploy-v2-external-platform-mismatch");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin/linux-arm64")).unwrap();
    std::fs::create_dir_all(bundle.join("external/fake_sensor_driver/bin")).unwrap();
    std::fs::write(bundle.join("bin/linux-arm64/flowrt-supervisor"), b"pi").unwrap();
    std::fs::write(
        bundle.join("external/fake_sensor_driver/bin/driver"),
        b"driver",
    )
    .unwrap();
    let supervisor_hash = file_sha256(&bundle.join("bin/linux-arm64/flowrt-supervisor")).unwrap();
    let driver_hash = file_sha256(&bundle.join("external/fake_sensor_driver/bin/driver")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "multi_target_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "bundle".into(),
        platform: None,
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![BundleExternalProcess {
            process: "sensor_proc".into(),
            package: "fake_sensor_driver".into(),
            executable: "bin/driver".into(),
            path: "external/fake_sensor_driver".into(),
            platform: Some("linux-arm64".into()),
            supported_platforms: vec!["linux-arm64".into()],
        }],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![
            BundleArtifact {
                kind: "supervisor".into(),
                target: "pi".into(),
                platform: Some("linux-arm64".into()),
                path: "bin/linux-arm64/flowrt-supervisor".into(),
                sha256: supervisor_hash,
            },
            BundleArtifact {
                kind: "external_process".into(),
                target: "pi".into(),
                platform: Some("linux-amd64".into()),
                path: "external/fake_sensor_driver/bin/driver".into(),
                sha256: driver_hash,
            },
        ],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("external package"),
        "unexpected error: {error}"
    );
    assert!(
        message.contains("platform mismatch"),
        "unexpected error: {error}"
    );
    assert!(message.contains("linux-arm64"), "unexpected error: {error}");

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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "bundle".into(),
        platform: None,
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
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

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();

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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "bundle".into(),
        platform: None,
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
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

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();

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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
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

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();
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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
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

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();
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
fn deploy_bundle_v2_rejects_runtime_dependency_hash_mismatch_with_doctor_hint() {
    let root = temp_test_dir("deploy-v2-runtime-dep-hash");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("bin/linux-arm64")).unwrap();
    std::fs::create_dir_all(bundle.join("runtime-deps/linux-arm64")).unwrap();
    std::fs::write(bundle.join("bin/linux-arm64/pi-supervisor"), b"pi").unwrap();
    std::fs::write(
        bundle.join("runtime-deps/linux-arm64/flowrt-target-sdk.toml"),
        b"platform = \"linux-arm64\"\ncomplete = true\n",
    )
    .unwrap();
    let pi_hash = file_sha256(&bundle.join("bin/linux-arm64/pi-supervisor")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "runtime_dep_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![BundleRuntimeDependency {
            name: "flowrt-target-sdk".into(),
            target: "pi".into(),
            platform: "linux-arm64".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            policy: "bundle".into(),
            path: "runtime-deps/linux-arm64/flowrt-target-sdk.toml".into(),
            sha256: "0".repeat(64),
        }],
        artifacts: vec![
            BundleArtifact {
                kind: "supervisor".into(),
                target: "pi".into(),
                platform: Some("linux-arm64".into()),
                path: "bin/linux-arm64/pi-supervisor".into(),
                sha256: pi_hash,
            },
            BundleArtifact {
                kind: "runtime_dependency".into(),
                target: "pi".into(),
                platform: Some("linux-arm64".into()),
                path: "runtime-deps/linux-arm64/flowrt-target-sdk.toml".into(),
                sha256: "0".repeat(64),
            },
        ],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("runtime dependency"),
        "unexpected error: {error}"
    );
    assert!(
        message.contains("sha256 mismatch"),
        "unexpected error: {error}"
    );
    assert!(
        message.contains("flowrt doctor <rsdl> --target linux-arm64"),
        "unexpected error: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_v2_rejects_runtime_dependency_without_entry_artifact() {
    let root = temp_test_dir("deploy-v2-runtime-dep-no-entry");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(bundle.join("runtime-deps/linux-arm64")).unwrap();
    std::fs::write(
        bundle.join("runtime-deps/linux-arm64/flowrt-target-sdk.toml"),
        b"platform = \"linux-arm64\"\ncomplete = true\n",
    )
    .unwrap();
    let runtime_dep_hash =
        file_sha256(&bundle.join("runtime-deps/linux-arm64/flowrt-target-sdk.toml")).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "runtime_dep_only_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![BundleRuntimeDependency {
            name: "flowrt-target-sdk".into(),
            target: "pi".into(),
            platform: "linux-arm64".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            policy: "bundle".into(),
            path: "runtime-deps/linux-arm64/flowrt-target-sdk.toml".into(),
            sha256: runtime_dep_hash.clone(),
        }],
        artifacts: vec![BundleArtifact {
            kind: "runtime_dependency".into(),
            target: "pi".into(),
            platform: Some("linux-arm64".into()),
            path: "runtime-deps/linux-arm64/flowrt-target-sdk.toml".into(),
            sha256: runtime_dep_hash,
        }],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("entry supervisor artifact"),
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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
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
        false,
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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let output = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap();

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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
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
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();

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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(
        &bundle,
        "-oProxyCommand=sh",
        "pi",
        "/tmp/flowrt-demo",
        true,
        false,
    )
    .unwrap_err();

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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(&bundle, "", "pi", "/tmp/flowrt-demo", true, false).unwrap_err();

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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![],
    };
    std::fs::write(
        bundle.join("bundle.toml"),
        toml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    let error = deploy_bundle(&bundle, "robot@192.0.2.10", "pi", "  ", true, false).unwrap_err();

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
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/external-demo-flowrt-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
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
        let error =
            deploy_bundle(&bundle, "robot@192.0.2.10", "pi", remote_dir, true, false).unwrap_err();
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
fn remote_deploy_probe_rejects_platform_mismatch_with_doctor_hint() {
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "remote_probe_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![],
        artifacts: vec![],
    };
    let error = validate_remote_deploy_probe_output(
        "flowrt-deploy-probe platform=linux-amd64\n",
        &manifest,
        "pi",
        &["linux-arm64".to_string()],
    )
    .unwrap_err();
    let message = error.to_string();

    assert!(message.contains("remote platform mismatch"));
    assert!(message.contains("linux-arm64"));
    assert!(message.contains("linux-amd64"));
    assert!(message.contains("flowrt doctor <rsdl> --target linux-arm64"));
}

#[test]
fn remote_deploy_probe_rejects_runtime_dependency_hash_mismatch() {
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "remote_probe_demo".into(),
        profile: Some("default".into()),
        artifact_mode: "strict".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/linux-arm64/pi-supervisor".into(),
        executables: vec![],
        external_processes: vec![],
        resource_providers: vec![],
        runtime_dependencies: vec![BundleRuntimeDependency {
            name: "flowrt-target-sdk".into(),
            target: "pi".into(),
            platform: "linux-arm64".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            policy: "bundle".into(),
            path: "runtime-deps/linux-arm64/flowrt-target-sdk.toml".into(),
            sha256: "a".repeat(64),
        }],
        artifacts: vec![],
    };
    let output = format!(
        "flowrt-deploy-probe platform=linux-arm64\nruntime_dependency name=flowrt-target-sdk version={} platform=linux-arm64 sha256={}\n",
        env!("CARGO_PKG_VERSION"),
        "b".repeat(64)
    );

    let error =
        validate_remote_deploy_probe_output(&output, &manifest, "pi", &["linux-arm64".to_string()])
            .unwrap_err();
    let message = error.to_string();

    assert!(message.contains("runtime dependency"));
    assert!(message.contains("sha256 mismatch"));
    assert!(message.contains("flowrt-target-sdk"));
    assert!(message.contains("flowrt doctor <rsdl> --target linux-arm64"));
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
