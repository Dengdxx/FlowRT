use super::*;

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

#[test]
fn bundle_workspace_rejects_island_artifact_without_escape_hatch() {
    let contract = contract_from_source(
        r#"
[package]
name = "island_bundle_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.dev]
mode = "island"
backend = "inproc"
"#,
    );
    let root = temp_test_dir("bundle-island-reject");
    let rsdl = root.join("robot.rsdl");
    let out_dir = root.join("flowrt");
    let bundle = root.join("dist/island");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&rsdl, "").unwrap();

    let error = bundle_workspace(&rsdl, &contract, &out_dir, &bundle, None, false).unwrap_err();

    assert!(error.to_string().contains("refusing to bundle island"));
    let allow_error = bundle_workspace(&rsdl, &contract, &out_dir, &bundle, None, true)
        .unwrap_err()
        .to_string();
    assert!(
        allow_error.contains("build metadata"),
        "allow-island should pass the island gate and then fail on missing build metadata: {allow_error}"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn deploy_bundle_rejects_island_artifact_without_escape_hatch() {
    let root = temp_test_dir("deploy-island-reject");
    let bundle = root.join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: "island_demo".into(),
        profile: Some("dev".into()),
        artifact_mode: "island".into(),
        temporary_overlay: false,
        test_only: false,
        target: "pi".into(),
        platform: Some("linux-arm64".into()),
        build_mode: BuildMode::Release,
        created_unix_ms: 0,
        entry: "bin/supervisor".into(),
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
    let output = deploy_bundle(
        &bundle,
        "robot@192.0.2.10",
        "pi",
        "/tmp/flowrt-demo",
        true,
        true,
    )
    .unwrap();

    assert!(error.to_string().contains("refusing to deploy island"));
    assert!(output.contains("deploy plan"));

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

    assert!(output.contains("deploy plan"));
    assert!(output.contains("robot@192.0.2.10"));
    assert!(output.contains("target=pi"));

    let _ = std::fs::remove_dir_all(&root);
}
