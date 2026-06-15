use super::*;

#[test]
fn cmake_target_sdk_root_requires_complete_manifest() {
    let root = temp_test_dir("target-sdk-incomplete");
    let private_prefix = root.join("opt/flowrt/0.8.4");
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
    let private_prefix = root.join("opt/flowrt/0.8.4");
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
fn cmake_build_diagnostics_fail_fast_for_missing_target_sdk() {
    let root = temp_test_dir("cmake-build-diagnostics-target-sdk");
    let private_prefix = root.join("opt/flowrt/0.8.5");
    std::fs::create_dir_all(private_prefix.join("targets/linux-arm64")).unwrap();
    let profile = linux_arm64_toolchain_profile();

    let error = resolve_cpp_target_sdk_for_build(Some(&private_prefix), &profile).unwrap_err();
    let message = error.to_string();

    assert!(message.contains("target=linux-arm64"));
    assert!(message.contains("PKG_CONFIG_LIBDIR="));
    assert!(message.contains("targets/linux-arm64"));
    assert!(message.contains("flowrt doctor <rsdl> --target linux-arm64"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_build_diagnostics_fail_fast_for_missing_pkg_config_module() {
    let root = temp_test_dir("cmake-build-diagnostics-pkg-config");
    let overlay = root.join("vendor/rknn");
    let pkgconfig_dir = root.join("vendor/pkgconfig");
    std::fs::create_dir_all(&overlay).unwrap();
    std::fs::create_dir_all(&pkgconfig_dir).unwrap();
    let sdk_root = root.join("opt/flowrt/0.8.5/targets/linux-arm64");
    write_target_sdk_manifest(&sdk_root, "linux-arm64", true);

    let contract = contract_from_source(
        r#"
[package]
name = "robot"
rsdl_version = "0.1"

[component.camera]
language = "cpp"

[component.camera.build]
pkg_config = ["vendor_capture"]

[instance.camera]
component = "camera"
target = "arm64"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
max_age_ms = 100

[target.arm64]
platform = "linux-arm64"
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );

    let mut profile = linux_arm64_toolchain_profile();
    profile.pkg_config_libdirs = vec![pkgconfig_dir.clone()];
    profile.sdk_overlays = vec![overlay.clone()];
    let target_profile = BuildToolchainProfile {
        profile,
        cargo_target_triple: Some("aarch64-unknown-linux-gnu".to_string()),
        is_cross: true,
    };
    let target_sdk =
        resolve_cpp_target_sdk_root(Some(&root.join("opt/flowrt/0.8.5")), "linux-arm64").unwrap();

    let error = ensure_cmake_build_diagnostics_ready(&contract, &target_profile, Some(&target_sdk))
        .unwrap_err();
    let message = error.to_string();

    assert!(message.contains("target=linux-arm64"));
    assert!(message.contains("PKG_CONFIG_LIBDIR="));
    assert!(message.contains("vendor_capture"));
    assert!(message.contains(&pkgconfig_dir.display().to_string()));
    assert!(message.contains(&overlay.display().to_string()));
    assert!(message.contains("flowrt doctor <rsdl> --target linux-arm64"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cmake_configure_args_use_target_sdk_and_compilers_without_toolchain_file() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);
    let root = temp_test_dir("cmake-target-sdk-compilers");
    let private_prefix = root.join("opt/flowrt/0.8.4");
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
fn cmake_configure_args_pass_toolchain_cpp_options() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let mut profile = linux_arm64_toolchain_profile();
    profile.cpp_compile_args = vec!["-DFLOWRT_BOARD_CAMERA=1".to_string()];
    profile.cpp_link_args = vec![
        "-Wl,--allow-shlib-undefined".to_string(),
        "-Wl,-rpath-link,/opt/vendor/lib".to_string(),
    ];
    profile.cpp_link_libraries = vec![
        "/opt/vendor/lib/libboost_program_options.so".to_string(),
        "m".to_string(),
    ];

    let args = cmake_configure_args(
        source_dir,
        build_dir,
        None,
        &[],
        BuildMode::Release,
        Some(&profile),
        true,
    );

    assert!(args.contains(&"-DFLOWRT_CXX_COMPILE_OPTIONS=-DFLOWRT_BOARD_CAMERA=1".to_string()));
    assert!(args.contains(
        &"-DFLOWRT_EXE_LINK_OPTIONS=-Wl,--allow-shlib-undefined;-Wl,-rpath-link,/opt/vendor/lib"
            .to_string()
    ));
    assert!(args.contains(
        &"-DFLOWRT_EXE_LINK_LIBRARIES=/opt/vendor/lib/libboost_program_options.so;m".to_string()
    ));
}

#[test]
fn native_cmake_configure_args_pass_toolchain_cpp_options() {
    let _lock = REPO_RUNTIME_FALLBACK_ENV_LOCK
        .lock()
        .expect("repo runtime fallback env lock should not be poisoned");
    let _env = EnvOverride::repo_runtime_fallback(None);
    let (_, host_target) = rustc_toolchain_identity().unwrap();
    let Some(platform) = host_flowrt_platform() else {
        return;
    };
    let source_dir = Path::new("/tmp/flowrt/build");
    let build_dir = Path::new("/tmp/flowrt/build/cmake");
    let mut profile = linux_arm64_toolchain_profile();
    profile.platform = platform.to_string();
    profile.rust_target = host_target;
    profile.cpp_compile_args = vec!["-I/opt/eigen/include/eigen3".to_string()];

    let args = cmake_configure_args(
        source_dir,
        build_dir,
        None,
        &[],
        BuildMode::Release,
        Some(&profile),
        false,
    );

    assert!(args.contains(&"-DFLOWRT_CXX_COMPILE_OPTIONS=-I/opt/eigen/include/eigen3".to_string()));
    assert!(
        args.iter()
            .all(|arg| !arg.starts_with("-DCMAKE_SYSTEM_NAME="))
    );
}

#[test]
fn cmake_configure_env_sets_pkg_config_libdir_for_target_sdk() {
    let root = temp_test_dir("cmake-target-sdk-pkgconfig");
    let private_prefix = root.join("opt/flowrt/0.8.4");
    let sdk_root = private_prefix.join("targets/linux-arm64");
    write_target_sdk_manifest(&sdk_root, "linux-arm64", true);
    let sdk = resolve_cpp_target_sdk_root(Some(&private_prefix), "linux-arm64").unwrap();
    let profile = linux_arm64_toolchain_profile();

    let env = cmake_configure_env(Some(&profile), Some(&sdk)).unwrap();
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
fn native_cmake_configure_env_preserves_system_pkg_config_when_no_profile_paths() {
    let (_, host_target) = rustc_toolchain_identity().unwrap();
    let Some(platform) = host_flowrt_platform() else {
        return;
    };
    let mut profile = linux_arm64_toolchain_profile();
    profile.platform = platform.to_string();
    profile.rust_target = host_target;
    profile.pkg_config_libdir = None;
    profile.pkg_config_libdirs.clear();
    profile.sdk_overlays.clear();

    let env = cmake_configure_env(Some(&profile), None).unwrap();

    assert!(
        !env.contains_key("PKG_CONFIG_LIBDIR"),
        "native builds without explicit pkg-config paths must not isolate system pkg-config"
    );
}

#[test]
fn native_cmake_configure_env_extends_pkg_config_path_for_profile_paths() {
    let (_, host_target) = rustc_toolchain_identity().unwrap();
    let Some(platform) = host_flowrt_platform() else {
        return;
    };
    let mut profile = linux_arm64_toolchain_profile();
    profile.platform = platform.to_string();
    profile.rust_target = host_target;
    profile.pkg_config_libdir = None;
    profile.pkg_config_libdirs = vec![PathBuf::from("/opt/native/pkgconfig")];
    profile.sdk_overlays.clear();

    let env = cmake_configure_env(Some(&profile), None).unwrap();

    assert!(!env.contains_key("PKG_CONFIG_LIBDIR"));
    assert_eq!(
        env.get("PKG_CONFIG_PATH").unwrap(),
        &std::ffi::OsString::from("/opt/native/pkgconfig")
    );
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
    let private_prefix = root.join("opt/flowrt/0.8.4");
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
    let private_prefix = root.join("opt/flowrt/0.8.4");
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

    let env = cmake_configure_env(Some(&profile), Some(&sdk)).unwrap();
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
