use super::*;

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
fn cargo_manifest_patch_uses_offline_config_for_repo_runtime() {
    let root = temp_test_dir("cargo-patch-repo-runtime-offline");
    let build_dir = root.join("flowrt").join("build");
    let runtime_dir = repo_root_dir().unwrap().join("runtime").join("rust");
    std::fs::create_dir_all(&build_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        r#"[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[dependencies]
flowrt = { version = "0.24" }
"#,
    )
    .unwrap();

    let patched_manifest =
        cargo_manifest_with_runtime_patch(&root.join("flowrt"), Some(&runtime_dir))
            .expect("repo runtime patch should force generated Cargo offline");
    let content = std::fs::read_to_string(&patched_manifest).unwrap();
    let config = std::fs::read_to_string(build_dir.join(".cargo").join("config.toml")).unwrap();
    let lock = std::fs::read_to_string(build_dir.join("Cargo.lock")).unwrap();
    let repo_lock = std::fs::read_to_string(repo_root_dir().unwrap().join("Cargo.lock")).unwrap();
    let invocation = cargo_build_invocation(
        &patched_manifest,
        "robot-flowrt-app",
        BuildMode::Release,
        &root.join("target-cache"),
        None,
        None,
    )
    .expect("cargo invocation should read generated offline config");

    assert!(content.contains("[patch.crates-io]"));
    assert_eq!(lock, repo_lock);
    assert!(config.contains("[net]"));
    assert!(config.contains("offline = true"));
    assert!(invocation.args.iter().any(|arg| arg == "--offline"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_manifest_patch_backfills_offline_config_for_already_patched_repo_runtime() {
    let root = temp_test_dir("cargo-patch-repo-runtime-backfill-offline");
    let build_dir = root.join("flowrt").join("build");
    let runtime_dir = repo_root_dir().unwrap().join("runtime").join("rust");
    std::fs::create_dir_all(&build_dir).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        format!(
            r#"[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[dependencies]
flowrt = {{ version = "0.24" }}

[patch.crates-io]
flowrt = {{ path = {} }}
"#,
            toml_basic_string(&runtime_dir)
        ),
    )
    .unwrap();

    cargo_manifest_with_runtime_patch(&root.join("flowrt"), Some(&runtime_dir))
        .expect("already patched repo runtime should still force Cargo offline");
    let invocation = cargo_build_invocation(
        &manifest,
        "robot-flowrt-app",
        BuildMode::Release,
        &root.join("target-cache"),
        None,
        None,
    )
    .expect("cargo invocation should read backfilled offline config");

    assert!(build_dir.join(".cargo").join("config.toml").exists());
    assert!(invocation.args.iter().any(|arg| arg == "--offline"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cargo_manifest_patch_replaces_stale_vendor_config_for_repo_runtime() {
    let root = temp_test_dir("cargo-patch-repo-runtime-replaces-stale-vendor");
    let build_dir = root.join("flowrt").join("build");
    let runtime_dir = repo_root_dir().unwrap().join("runtime").join("rust");
    std::fs::create_dir_all(build_dir.join(".cargo")).unwrap();
    let manifest = build_dir.join("Cargo.toml");
    std::fs::write(
        &manifest,
        r#"[package]
name = "robot-flowrt-app"
version = "0.1.0"
edition = "2024"

[dependencies]
flowrt = { version = "0.24" }
"#,
    )
    .unwrap();
    std::fs::write(
        build_dir.join(".cargo").join("config.toml"),
        "[source.crates-io]\nreplace-with = \"flowrt-vendor\"\n\n[source.flowrt-vendor]\ndirectory = \"/tmp/stale-vendor\"\n\n[net]\noffline = true\n",
    )
    .unwrap();

    cargo_manifest_with_runtime_patch(&root.join("flowrt"), Some(&runtime_dir))
        .expect("repo runtime patch should replace stale vendor config");
    let config = std::fs::read_to_string(build_dir.join(".cargo").join("config.toml")).unwrap();

    assert_eq!(config, "[net]\noffline = true\n");

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
fn cargo_internal_names_include_contract_hash_without_changing_public_names() {
    let strict = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
target = "linux"

[profile.default]
backend = "inproc"

[target.linux]
platform = "linux-amd64"
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let mut changed = strict.clone();
    changed.source_hash = "different-source".to_string();
    changed.artifact.test_only = true;
    let strict_names = cargo_internal_names(&strict).unwrap();
    let changed_names = cargo_internal_names(&changed).unwrap();

    assert_eq!(strict_names.app_stable, "robot-demo-flowrt-app");
    assert_eq!(
        strict_names.supervisor_stable,
        "robot-demo-flowrt-supervisor"
    );
    assert_ne!(strict_names.app_internal, changed_names.app_internal);
    assert_ne!(
        strict_names.supervisor_internal,
        changed_names.supervisor_internal
    );

    let manifest = r#"# FlowRT 管理产物。不要手工修改。
[package]
name = "robot-demo-flowrt-app"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "robot-demo-flowrt-app"
path = "../rust/src/main.rs"

[[bin]]
name = "robot-demo-flowrt-supervisor"
path = "../rust/src/supervisor_main.rs"
"#;
    let rewritten = rewrite_cargo_manifest_for_internal_names(manifest, &strict_names).unwrap();
    assert!(rewritten.contains(&format!("name = \"{}\"", strict_names.package_internal)));
    assert!(rewritten.contains(&format!("name = \"{}\"", strict_names.app_internal)));
    assert!(rewritten.contains(&format!("name = \"{}\"", strict_names.supervisor_internal)));
    assert!(!rewritten.contains("\nname = \"robot-demo-flowrt-app\"\n"));
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
