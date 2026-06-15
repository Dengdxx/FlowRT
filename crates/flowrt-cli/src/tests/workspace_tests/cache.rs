use super::*;

#[test]
fn cache_status_groups_entries_and_marks_non_cache_paths() {
    let _cache_lock = FLOWRT_CACHE_DIR_ENV_LOCK
        .lock()
        .expect("cache env lock should not be poisoned");
    let root = temp_test_dir("cache-status");
    let cache = root.join("cache");
    let _cache_env = EnvOverride::set("FLOWRT_CACHE_DIR", Some(cache.as_os_str()));
    let project = root.join("demo");
    let out_dir = project.join("flowrt");
    std::fs::create_dir_all(project.join("rsdl")).unwrap();
    std::fs::create_dir_all(project.join(".flowrt")).unwrap();
    std::fs::create_dir_all(out_dir.join("build/bin/linux-arm64/release")).unwrap();
    std::fs::create_dir_all(out_dir.join("build/cmake/linux-arm64/release")).unwrap();
    std::fs::write(
        out_dir.join("build/bin/linux-arm64/release/robot-flowrt-app"),
        "binary",
    )
    .unwrap();
    std::fs::write(
        out_dir.join("build/cmake/linux-arm64/release/CMakeCache.txt"),
        "cache",
    )
    .unwrap();
    std::fs::write(project.join("capture.mcap"), "mcap").unwrap();
    std::fs::write(
        project.join(".flowrt/toolchains.toml"),
        "[toolchain.linux-arm64]\nsdk_overlays = [\"/opt/vendor/rknn\"]\n",
    )
    .unwrap();

    let features = RuntimeFeatureSet::from_features([RuntimeFeature::Zenoh]);
    let layout = CacheLayout::new(
        cache.clone(),
        &DepsCacheKey::new(
            env!("CARGO_PKG_VERSION"),
            "rustc 1.90.0",
            "aarch64-unknown-linux-gnu",
            "vendor-hash",
            BuildMode::Release,
            features.clone(),
        ),
    );
    std::fs::create_dir_all(
        layout
            .target_dir
            .join("aarch64-unknown-linux-gnu")
            .join("release")
            .join("incremental"),
    )
    .unwrap();
    std::fs::create_dir_all(&layout.deps_workspace_dir).unwrap();
    write_deps_ready_marker(&layout, BuildMode::Release, &features).unwrap();

    let output = cache_status_summary_for_cwd(&project).unwrap();

    assert!(output.contains("FlowRT cache root"));
    assert!(output.contains("默认可清"));
    assert!(output.contains("条件可清"));
    assert!(output.contains("仅展示"));
    assert!(output.contains("永不自动清"));
    assert!(output.contains("deps ready marker"));
    assert!(output.contains("aarch64-unknown-linux-gnu"));
    assert!(output.contains("zenoh"));
    assert!(output.contains("flowrt/build"));
    assert!(output.contains("flowrt/build/bin"));
    assert!(output.contains("flowrt/build/cmake"));
    assert!(output.contains("sdk_overlay"));
    assert!(output.contains("/opt/vendor/rknn"));
    assert!(output.contains(".mcap"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cache_clean_dry_run_respects_target_and_build_mode_filters() {
    let _lock = FLOWRT_CACHE_DIR_ENV_LOCK
        .lock()
        .expect("cache env lock should not be poisoned");
    let root = temp_test_dir("cache-clean-filter");
    let cache = root.join("cache");
    let _env = EnvOverride::set("FLOWRT_CACHE_DIR", Some(cache.as_os_str()));
    let project = root.join("demo");
    std::fs::create_dir_all(project.join("rsdl")).unwrap();

    let arm_features = RuntimeFeatureSet::from_features([RuntimeFeature::Zenoh]);
    let arm_layout = CacheLayout::new(
        cache.clone(),
        &DepsCacheKey::new(
            env!("CARGO_PKG_VERSION"),
            "rustc 1.90.0",
            "aarch64-unknown-linux-gnu",
            "vendor-a",
            BuildMode::Release,
            arm_features.clone(),
        ),
    );
    std::fs::create_dir_all(
        arm_layout
            .target_dir
            .join("aarch64-unknown-linux-gnu")
            .join("release")
            .join("incremental"),
    )
    .unwrap();
    std::fs::create_dir_all(&arm_layout.deps_workspace_dir).unwrap();
    write_deps_ready_marker(&arm_layout, BuildMode::Release, &arm_features).unwrap();

    let host_features = RuntimeFeatureSet::from_features([RuntimeFeature::Iox2]);
    let host_layout = CacheLayout::new(
        cache.clone(),
        &DepsCacheKey::new(
            env!("CARGO_PKG_VERSION"),
            "rustc 1.90.0",
            "x86_64-unknown-linux-gnu",
            "vendor-b",
            BuildMode::Debug,
            host_features.clone(),
        ),
    );
    std::fs::create_dir_all(host_layout.target_dir.join("debug").join("incremental")).unwrap();
    std::fs::create_dir_all(&host_layout.deps_workspace_dir).unwrap();
    write_deps_ready_marker(&host_layout, BuildMode::Debug, &host_features).unwrap();

    let output = cache_clean_for_cwd(
        &project,
        CacheCleanOptions {
            target: Some("linux-arm64".to_string()),
            build_mode: Some(BuildMode::Release),
            dry_run: true,
            flowrt_deps: true,
            project_build: false,
            incremental: true,
            stale_temp: false,
        },
    )
    .unwrap();

    assert!(output.contains("dry-run"));
    assert!(output.contains(arm_layout.target_dir.to_string_lossy().as_ref()));
    assert!(output.contains(arm_layout.deps_workspace_dir.to_string_lossy().as_ref()));
    assert!(output.contains(arm_layout.ready_file.to_string_lossy().as_ref()));
    assert!(!output.contains(host_layout.target_dir.to_string_lossy().as_ref()));
    assert!(arm_layout.target_dir.exists());
    assert!(host_layout.target_dir.exists());

    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn cache_clean_rejects_project_build_symlink_escape() {
    let root = temp_test_dir("cache-clean-project-symlink");
    let project = root.join("demo");
    let outside = root.join("outside-build");
    std::fs::create_dir_all(project.join("rsdl")).unwrap();
    std::fs::create_dir_all(project.join("flowrt")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, project.join("flowrt/build")).unwrap();

    let error = cache_clean_for_cwd(
        &project,
        CacheCleanOptions {
            target: None,
            build_mode: None,
            dry_run: false,
            flowrt_deps: false,
            project_build: true,
            incremental: false,
            stale_temp: false,
        },
    )
    .expect_err("symlinked project build dir should be rejected");

    assert!(error.to_string().contains("symlink"));
    assert!(outside.exists());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cache_clean_stale_temp_removes_only_dead_runtime_socket_candidates() {
    let _lock = XDG_RUNTIME_DIR_ENV_LOCK
        .lock()
        .expect("runtime dir env lock should not be poisoned");
    let root = temp_test_dir("cache-clean-stale-temp");
    let runtime_dir = root.join("xdg-runtime");
    let _env = EnvOverride::set("XDG_RUNTIME_DIR", Some(runtime_dir.as_os_str()));
    let socket_dir = flowrt::runtime_socket_dir();
    std::fs::create_dir_all(&socket_dir).unwrap();
    let dead_socket = socket_dir.join("999999.sock");
    let live_socket = socket_dir.join(format!("{}.sock", std::process::id()));
    std::fs::write(&dead_socket, "dead").unwrap();
    std::fs::write(&live_socket, "live").unwrap();

    let output = cache_clean_for_cwd(
        &root,
        CacheCleanOptions {
            target: None,
            build_mode: None,
            dry_run: false,
            flowrt_deps: false,
            project_build: false,
            incremental: false,
            stale_temp: true,
        },
    )
    .unwrap();

    assert!(output.contains(dead_socket.to_string_lossy().as_ref()));
    assert!(!output.contains(live_socket.to_string_lossy().as_ref()));
    assert!(!dead_socket.exists());
    assert!(live_socket.exists());

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
