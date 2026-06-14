use super::*;

use crate::toolchain::{
    RuntimeDependencyPolicy, ToolchainConfigSources, ToolchainProfileOverrides,
    generate_toolchain_init_toml, resolve_toolchain_profile_with_field_sources,
    resolve_toolchain_profile_with_sources,
};

static PKG_CONFIG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvOverride {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvOverride {
    fn set(key: &'static str, value: Option<&std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: tests guard process-wide environment mutation with a mutex.
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self { key, previous }
    }
}

impl Drop for EnvOverride {
    fn drop(&mut self) {
        // SAFETY: tests guard process-wide environment mutation with a mutex.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn write_toolchains(path: &Path, source: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
}

fn write_pkg_config_module(path: &Path, module: &str, prefix: &Path) {
    let include_dir = prefix.join("include");
    let lib_dir = prefix.join("lib");
    std::fs::create_dir_all(path).unwrap();
    std::fs::create_dir_all(&include_dir).unwrap();
    std::fs::create_dir_all(&lib_dir).unwrap();
    std::fs::write(
        path.join(format!("{module}.pc")),
        format!(
            "prefix={prefix}\n\
exec_prefix=${{prefix}}\n\
libdir=${{prefix}}/lib\n\
includedir=${{prefix}}/include\n\
\n\
Name: {module}\n\
Description: test package\n\
Version: 1.0.0\n\
Libs: -L${{libdir}} -l{module}\n\
Cflags: -I${{includedir}}\n",
            prefix = prefix.display()
        ),
    )
    .unwrap();
}

#[test]
fn toolchain_defaults_cover_linux_arm64() {
    let profile = resolve_toolchain_profile_with_sources(
        "linux-arm64",
        &ToolchainConfigSources::default(),
        &ToolchainProfileOverrides::default(),
    )
    .unwrap();

    assert_eq!(profile.platform, "linux-arm64");
    assert_eq!(profile.rust_target, "aarch64-unknown-linux-gnu");
    assert_eq!(profile.deb_multiarch, "aarch64-linux-gnu");
    assert_eq!(profile.c_compiler, "aarch64-linux-gnu-gcc");
    assert_eq!(profile.cpp_compiler, "aarch64-linux-gnu-g++");
    assert!(profile.sysroot.is_none());
    assert!(profile.cmake_toolchain.is_none());
    assert!(profile.pkg_config_libdir.is_none());
    assert!(profile.pkg_config_libdirs.is_empty());
    assert!(profile.cmake_prefix_paths.is_empty());
    assert!(profile.sdk_overlays.is_empty());
    assert_eq!(
        profile.runtime_dependency_policy,
        RuntimeDependencyPolicy::Bundle
    );
}

#[test]
fn toolchain_build_profile_prefers_explicit_target() {
    let root = temp_test_dir("toolchain-build-explicit-target");
    let contract = contract_from_source(
        r#"
[package]
name = "robot"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[target.dev]
platform = "linux-amd64"
runtime = ["rust"]
backends = ["inproc"]
"#,
    );

    let profile = resolve_build_toolchain_profile(&contract, Some("linux-arm64"), &root).unwrap();
    let profile = profile.expect("explicit target should resolve a toolchain profile");

    assert_eq!(profile.profile.platform, "linux-arm64");
    assert_eq!(profile.profile.rust_target, "aarch64-unknown-linux-gnu");
    assert_eq!(
        cargo_target_args(profile.cargo_target_triple.as_deref()),
        vec!["--target", "aarch64-unknown-linux-gnu"]
    );
}

#[test]
fn toolchain_build_profile_infers_contract_target_platform() {
    let root = temp_test_dir("toolchain-build-contract-target");
    let contract = contract_from_source(
        r#"
[package]
name = "robot"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[target.pi]
platform = "linux-arm64"
runtime = ["rust"]
backends = ["inproc"]
"#,
    );

    let profile = resolve_build_toolchain_profile(&contract, None, &root).unwrap();
    let profile = profile.expect("contract target platform should resolve a profile");

    assert_eq!(profile.profile.platform, "linux-arm64");
    assert_eq!(profile.profile.rust_target, "aarch64-unknown-linux-gnu");
}

#[test]
fn toolchain_build_profile_keeps_inferred_native_target_as_native() {
    let (_, host_target) = rustc_toolchain_identity().unwrap();
    let Some(platform) = host_toolchain_platform_for_test(&host_target) else {
        return;
    };
    let root = temp_test_dir("toolchain-build-native-target-platform");
    let contract = contract_from_source(&format!(
        r#"
[package]
name = "robot"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[target.local]
platform = "{platform}"
runtime = ["rust"]
backends = ["inproc"]
"#
    ));

    let profile = resolve_build_toolchain_profile(&contract, None, &root).unwrap();
    let profile = profile.expect("contract target platform should resolve a profile");

    assert_eq!(profile.profile.platform, platform);
    assert_eq!(profile.profile.rust_target, host_target);
    assert!(profile.cargo_target_triple.is_none());
}

#[test]
fn toolchain_build_profile_uses_host_profile_when_platform_is_absent() {
    let (_, host_target) = rustc_toolchain_identity().unwrap();
    let Some(platform) = host_toolchain_platform_for_test(&host_target) else {
        return;
    };
    let root = temp_test_dir("toolchain-build-native");
    let workspace = root.join(".flowrt/toolchains.toml");
    write_toolchains(
        &workspace,
        &format!(
            r#"
[toolchain.{platform}]
cpp_compile_args = ["-DFLOWRT_NATIVE_VENDOR=1"]
pkg_config_libdirs = ["/opt/native/pkgconfig"]
"#
        ),
    );
    let contract = contract_from_source(
        r#"
[package]
name = "robot"
rsdl_version = "0.1"

[component.worker]
language = "rust"
"#,
    );

    let profile = resolve_build_toolchain_profile(&contract, None, &root).unwrap();
    let profile = profile.expect("host platform should resolve a native toolchain profile");

    assert_eq!(profile.profile.platform, platform);
    assert_eq!(profile.profile.rust_target, host_target);
    assert_eq!(
        profile.profile.cpp_compile_args,
        vec!["-DFLOWRT_NATIVE_VENDOR=1"]
    );
    assert!(profile.cargo_target_triple.is_none());
    assert!(cargo_target_args(profile.cargo_target_triple.as_deref()).is_empty());
}

fn host_toolchain_platform_for_test(host_target: &str) -> Option<&'static str> {
    match host_target {
        "x86_64-unknown-linux-gnu" => Some("linux-amd64"),
        "aarch64-unknown-linux-gnu" => Some("linux-arm64"),
        _ => None,
    }
}

#[test]
fn toolchain_workspace_config_overrides_defaults() {
    let root = temp_test_dir("toolchain-workspace-config");
    let workspace = root.join(".flowrt/toolchains.toml");
    write_toolchains(
        &workspace,
        r#"
[toolchain.linux-arm64]
c_compiler = "custom-aarch64-gcc"
sysroot = "/opt/flowrt/sysroots/linux-arm64"
pkg_config_libdir = "/opt/flowrt/0.8.4/targets/linux-arm64/lib/pkgconfig"
pkg_config_libdirs = ["/opt/vendor/lib/pkgconfig"]
cmake_prefix_paths = ["/opt/vendor/cmake"]
sdk_overlays = ["/opt/vendor/rknn"]
cpp_compile_args = ["-DFLOWRT_BOARD_CAMERA=1"]
cpp_link_args = ["-Wl,--allow-shlib-undefined"]
cpp_link_libraries = ["/opt/vendor/lib/libboost_program_options.so"]
runtime_dependency_policy = "external"
"#,
    );

    let sources = ToolchainConfigSources {
        system: None,
        user: None,
        workspace: Some(workspace),
    };
    let profile =
        resolve_toolchain_profile_with_sources("linux-arm64", &sources, &Default::default())
            .unwrap();

    assert_eq!(profile.rust_target, "aarch64-unknown-linux-gnu");
    assert_eq!(profile.c_compiler, "custom-aarch64-gcc");
    assert_eq!(profile.cpp_compiler, "aarch64-linux-gnu-g++");
    assert_eq!(
        profile.sysroot,
        Some(PathBuf::from("/opt/flowrt/sysroots/linux-arm64"))
    );
    assert_eq!(
        profile.pkg_config_libdir,
        Some(PathBuf::from(
            "/opt/flowrt/0.8.4/targets/linux-arm64/lib/pkgconfig"
        ))
    );
    assert_eq!(
        profile.pkg_config_libdirs,
        vec![PathBuf::from("/opt/vendor/lib/pkgconfig")]
    );
    assert_eq!(
        profile.cmake_prefix_paths,
        vec![PathBuf::from("/opt/vendor/cmake")]
    );
    assert_eq!(
        profile.sdk_overlays,
        vec![PathBuf::from("/opt/vendor/rknn")]
    );
    assert_eq!(profile.cpp_compile_args, vec!["-DFLOWRT_BOARD_CAMERA=1"]);
    assert_eq!(profile.cpp_link_args, vec!["-Wl,--allow-shlib-undefined"]);
    assert_eq!(
        profile.cpp_link_libraries,
        vec!["/opt/vendor/lib/libboost_program_options.so"]
    );
    assert_eq!(
        profile.runtime_dependency_policy,
        RuntimeDependencyPolicy::External
    );
}

#[test]
fn toolchain_rejects_cmake_list_separators_in_cpp_args() {
    let root = temp_test_dir("toolchain-cmake-list-separator");
    let workspace = root.join(".flowrt/toolchains.toml");
    write_toolchains(
        &workspace,
        r#"
[toolchain.linux-arm64]
cpp_compile_args = ["-DFLOWRT_BOARD_CAMERA=1"]
cpp_link_args = ["-Wl,--allow-shlib-undefined;-Wl,-rpath-link,/opt/vendor/lib"]
cpp_link_libraries = ["boost_program_options"]
"#,
    );

    let sources = ToolchainConfigSources {
        system: None,
        user: None,
        workspace: Some(workspace),
    };
    let error =
        resolve_toolchain_profile_with_sources("linux-arm64", &sources, &Default::default())
            .expect_err("semicolon should be rejected in CMake list args");

    assert!(
        error
            .to_string()
            .contains("must not contain `;` because it is passed as a CMake list")
    );
}

#[test]
fn toolchain_priority_is_cli_workspace_user_system_builtin() {
    let root = temp_test_dir("toolchain-priority");
    let system = root.join("system/toolchains.toml");
    let user = root.join("user/toolchains.toml");
    let workspace = root.join(".flowrt/toolchains.toml");

    write_toolchains(
        &system,
        r#"
[toolchain.linux-arm64]
c_compiler = "system-gcc"
cpp_compiler = "system-g++"
sysroot = "/system/sysroot"
"#,
    );
    write_toolchains(
        &user,
        r#"
[toolchain.linux-arm64]
c_compiler = "user-gcc"
cpp_compiler = "user-g++"
sysroot = "/user/sysroot"
"#,
    );
    write_toolchains(
        &workspace,
        r#"
[toolchain.linux-arm64]
c_compiler = "workspace-gcc"
sysroot = "/workspace/sysroot"
"#,
    );

    let sources = ToolchainConfigSources {
        system: Some(system),
        user: Some(user),
        workspace: Some(workspace),
    };
    let overrides = ToolchainProfileOverrides {
        c_compiler: Some("cli-gcc".to_string()),
        sdk_overlays: vec![PathBuf::from("/cli/sdk")],
        ..Default::default()
    };
    let profile =
        resolve_toolchain_profile_with_sources("linux-arm64", &sources, &overrides).unwrap();

    assert_eq!(profile.c_compiler, "cli-gcc");
    assert_eq!(profile.cpp_compiler, "user-g++");
    assert_eq!(profile.sysroot, Some(PathBuf::from("/workspace/sysroot")));
    assert_eq!(profile.rust_target, "aarch64-unknown-linux-gnu");
    assert_eq!(profile.sdk_overlays, vec![PathBuf::from("/cli/sdk")]);
}

#[test]
fn toolchain_path_lists_append_without_duplicates() {
    let root = temp_test_dir("toolchain-list-append");
    let system = root.join("system/toolchains.toml");
    let workspace = root.join(".flowrt/toolchains.toml");

    write_toolchains(
        &system,
        r#"
[toolchain.linux-arm64]
pkg_config_libdirs = ["/sdk/common/pkgconfig"]
cmake_prefix_paths = ["/sdk/common"]
sdk_overlays = ["/sdk/common"]
"#,
    );
    write_toolchains(
        &workspace,
        r#"
[toolchain.linux-arm64]
pkg_config_libdirs = ["/sdk/common/pkgconfig", "/sdk/project/pkgconfig"]
cmake_prefix_paths = ["/sdk/common", "/sdk/project"]
sdk_overlays = ["/sdk/common", "/sdk/project"]
"#,
    );

    let sources = ToolchainConfigSources {
        system: Some(system),
        user: None,
        workspace: Some(workspace),
    };
    let profile =
        resolve_toolchain_profile_with_sources("linux-arm64", &sources, &Default::default())
            .unwrap();

    assert_eq!(
        profile.pkg_config_libdirs,
        vec![
            PathBuf::from("/sdk/common/pkgconfig"),
            PathBuf::from("/sdk/project/pkgconfig")
        ]
    );
    assert_eq!(
        profile.cmake_prefix_paths,
        vec![PathBuf::from("/sdk/common"), PathBuf::from("/sdk/project")]
    );
    assert_eq!(
        profile.sdk_overlays,
        vec![PathBuf::from("/sdk/common"), PathBuf::from("/sdk/project")]
    );
}

#[test]
fn toolchain_rejects_unknown_runtime_dependency_policy() {
    let root = temp_test_dir("toolchain-bad-policy");
    let workspace = root.join(".flowrt/toolchains.toml");
    write_toolchains(
        &workspace,
        r#"
[toolchain.linux-arm64]
runtime_dependency_policy = "vendored"
"#,
    );

    let sources = ToolchainConfigSources {
        system: None,
        user: None,
        workspace: Some(workspace),
    };
    let error =
        resolve_toolchain_profile_with_sources("linux-arm64", &sources, &Default::default())
            .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("failed to parse toolchain config")
    );
}

#[test]
fn toolchain_rejects_unknown_platform() {
    let error = resolve_toolchain_profile_with_sources(
        "linux-riscv64",
        &ToolchainConfigSources::default(),
        &ToolchainProfileOverrides::default(),
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("unsupported toolchain platform `linux-riscv64`")
    );
}

#[test]
fn toolchain_rejects_empty_compiler_field() {
    let root = temp_test_dir("toolchain-empty-compiler");
    let workspace = root.join(".flowrt/toolchains.toml");
    write_toolchains(
        &workspace,
        r#"
[toolchain.linux-arm64]
c_compiler = ""
"#,
    );

    let sources = ToolchainConfigSources {
        system: None,
        user: None,
        workspace: Some(workspace),
    };
    let error =
        resolve_toolchain_profile_with_sources("linux-arm64", &sources, &Default::default())
            .unwrap_err();

    assert!(error.to_string().contains("c_compiler"));
    assert!(error.to_string().contains("must not be empty"));
}

#[test]
fn toolchain_field_sources_track_defaults() {
    let root = temp_test_dir("toolchain-field-sources-defaults");
    let (profile, field_sources) =
        resolve_toolchain_profile_with_field_sources("linux-arm64", &root).unwrap();

    assert_eq!(profile.platform, "linux-arm64");
    assert_eq!(field_sources.platform_source, "builtin");
    assert_eq!(field_sources.rust_target_source, "builtin");
    assert_eq!(field_sources.c_compiler_source, "builtin");
    assert_eq!(field_sources.cpp_compiler_source, "builtin");
    assert_eq!(field_sources.sysroot_source, "(none)");
    assert_eq!(field_sources.cmake_toolchain_source, "(none)");
    assert_eq!(field_sources.pkg_config_libdir_source, "(none)");
    assert_eq!(field_sources.runtime_dependency_policy_source, "builtin");
}

#[test]
fn toolchain_field_sources_track_workspace_overrides() {
    let root = temp_test_dir("toolchain-field-sources-workspace");
    let workspace = root.join(".flowrt/toolchains.toml");
    write_toolchains(
        &workspace,
        r#"
[toolchain.linux-arm64]
c_compiler = "workspace-gcc"
sysroot = "/workspace/sysroot"
runtime_dependency_policy = "external"
"#,
    );

    let (profile, field_sources) =
        resolve_toolchain_profile_with_field_sources("linux-arm64", &root).unwrap();

    assert_eq!(profile.c_compiler, "workspace-gcc");
    assert_eq!(field_sources.c_compiler_source, "workspace");
    assert_eq!(profile.sysroot, Some(PathBuf::from("/workspace/sysroot")));
    assert_eq!(field_sources.sysroot_source, "workspace");
    assert_eq!(
        profile.runtime_dependency_policy,
        RuntimeDependencyPolicy::External
    );
    assert_eq!(field_sources.runtime_dependency_policy_source, "workspace");
    // Fields not overridden stay builtin.
    assert_eq!(field_sources.cpp_compiler_source, "builtin");
    assert_eq!(field_sources.rust_target_source, "builtin");
}

#[test]
fn toolchain_field_sources_track_system_and_user_layers() {
    let root = temp_test_dir("toolchain-field-sources-layers");
    let system = root.join("system/toolchains.toml");
    let user = root.join("user/toolchains.toml");

    write_toolchains(
        &system,
        r#"
[toolchain.linux-arm64]
c_compiler = "system-gcc"
sysroot = "/system/sysroot"
"#,
    );
    write_toolchains(
        &user,
        r#"
[toolchain.linux-arm64]
c_compiler = "user-gcc"
"#,
    );

    let sources = ToolchainConfigSources {
        system: Some(system),
        user: Some(user),
        workspace: None,
    };
    let (profile, field_sources) =
        crate::toolchain::resolve_toolchain_profile_with_field_sources_from_sources(
            "linux-arm64",
            &sources,
        )
        .unwrap();

    // user overrides system for c_compiler
    assert_eq!(profile.c_compiler, "user-gcc");
    assert_eq!(field_sources.c_compiler_source, "user");
    // sysroot only set by system
    assert_eq!(profile.sysroot, Some(PathBuf::from("/system/sysroot")));
    assert_eq!(field_sources.sysroot_source, "system");
}

#[test]
fn toolchain_init_generates_minimal_toml() {
    let toml = generate_toolchain_init_toml("linux-arm64", &[]).unwrap();

    assert!(toml.contains("[toolchain.linux-arm64]"));
    assert!(!toml.contains("sdk_overlays"));
}

#[test]
fn toolchain_init_includes_sdk_overlays() {
    let overlays = vec![
        PathBuf::from("/opt/vendor/rknn"),
        PathBuf::from("/opt/vendor/camera"),
    ];
    let toml = generate_toolchain_init_toml("linux-arm64", &overlays).unwrap();

    assert!(toml.contains("[toolchain.linux-arm64]"));
    assert!(toml.contains("sdk_overlays"));
    assert!(toml.contains("/opt/vendor/rknn"));
    assert!(toml.contains("/opt/vendor/camera"));
}

#[test]
fn toolchain_init_rejects_unknown_platform() {
    let error = generate_toolchain_init_toml("linux-riscv64", &[]).unwrap_err();

    assert!(error.to_string().contains("unsupported toolchain platform"));
}

#[test]
fn toolchain_init_toml_is_valid_and_parseable() {
    let overlays = vec![PathBuf::from("/opt/vendor/rknn")];
    let toml_content = generate_toolchain_init_toml("linux-arm64", &overlays).unwrap();

    // Verify the generated TOML can be parsed back as a valid toolchain config.
    let config: crate::toolchain::ToolchainsFile =
        toml::from_str(&toml_content).expect("generated TOML should be parseable");
    let overrides = config
        .toolchain
        .get("linux-arm64")
        .expect("should have linux-arm64 section");
    assert_eq!(
        overrides.sdk_overlays,
        vec![PathBuf::from("/opt/vendor/rknn")]
    );
}

#[test]
fn toolchain_init_writes_relative_overlay_as_workspace_path() {
    let root = temp_test_dir("toolchain-init-relative-overlay");

    toolchain_init("linux-arm64", &[PathBuf::from("vendor/rknn")], false, &root).unwrap();

    let toml_content = std::fs::read_to_string(root.join(".flowrt/toolchains.toml")).unwrap();
    assert!(toml_content.contains(&root.join("vendor/rknn").display().to_string()));
}

#[test]
fn toolchain_show_output_contains_expected_fields() {
    let root = temp_test_dir("toolchain-show-output");
    let workspace = root.join(".flowrt/toolchains.toml");
    write_toolchains(
        &workspace,
        r#"
[toolchain.linux-arm64]
c_compiler = "custom-gcc"
sysroot = "/opt/sysroot"
sdk_overlays = ["/opt/vendor/rknn"]
runtime_dependency_policy = "external"
"#,
    );

    let (profile, field_sources) =
        resolve_toolchain_profile_with_field_sources("linux-arm64", &root).unwrap();
    let output = crate::format_toolchain_show(&profile, &field_sources);

    assert!(output.contains("platform: linux-arm64 (source: builtin)"));
    assert!(output.contains("rust_target: aarch64-unknown-linux-gnu (source: builtin)"));
    assert!(output.contains("c_compiler: custom-gcc (source: workspace)"));
    assert!(output.contains("cpp_compiler: aarch64-linux-gnu-g++ (source: builtin)"));
    assert!(output.contains("sysroot: /opt/sysroot (source: workspace)"));
    assert!(output.contains("sdk_overlays: /opt/vendor/rknn"));
    assert!(output.contains("runtime_dependency_policy: external (source: workspace)"));
    assert!(output.contains("source priority: builtin < system < user < workspace < CLI override"));
}

#[test]
fn cli_parses_toolchain_show_command() {
    let cli =
        Cli::try_parse_from(["flowrt", "toolchain", "show", "--target", "linux-arm64"]).unwrap();

    let Command::Toolchain {
        command: ToolchainCommand::Show { target },
    } = cli.command
    else {
        panic!("toolchain show should parse into Command::Toolchain")
    };
    assert_eq!(target, "linux-arm64");
}

#[test]
fn cli_parses_toolchain_init_command_with_sdk_overlay() {
    let cli = Cli::try_parse_from([
        "flowrt",
        "toolchain",
        "init",
        "--target",
        "linux-arm64",
        "--sdk-overlay",
        "/opt/vendor/rknn",
        "--force",
    ])
    .unwrap();

    let Command::Toolchain {
        command:
            ToolchainCommand::Init {
                target,
                sdk_overlay,
                force,
            },
    } = cli.command
    else {
        panic!("toolchain init should parse into Command::Toolchain")
    };
    assert_eq!(target, "linux-arm64");
    assert_eq!(sdk_overlay, vec![PathBuf::from("/opt/vendor/rknn")]);
    assert!(force);
}

#[test]
fn doctor_contract_pkg_config_checks_report_found_module() {
    if !command_available("pkg-config") {
        return;
    }

    let _env_lock = PKG_CONFIG_ENV_LOCK.lock().unwrap();
    let _pkg_config_path = EnvOverride::set("PKG_CONFIG_PATH", None);
    let _pkg_config_sysroot_dir = EnvOverride::set("PKG_CONFIG_SYSROOT_DIR", None);

    let root = temp_test_dir("doctor-pkg-config-found");
    let overlay = root.join("overlay");
    let pkgconfig_dir = overlay.join("lib/pkgconfig");
    write_pkg_config_module(&pkgconfig_dir, "vendor_capture", &overlay);

    let contract = contract_from_source(
        r#"
[package]
name = "robot"
rsdl_version = "0.1"

[component.camera]
language = "cpp"

[component.camera.build]
pkg_config = ["vendor_capture"]

[component.host_only]
language = "cpp"

[component.host_only.build]
pkg_config = ["host_only"]

[instance.camera]
component = "camera"
target = "arm64"
process = "arm64_proc"

[instance.host_only]
component = "host_only"
target = "host"
process = "host_proc"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
max_age_ms = 100

[target.arm64]
platform = "linux-arm64"
runtime = ["cpp"]
backends = ["inproc"]

[target.host]
platform = "linux-amd64"
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );

    let mut profile = resolve_toolchain_profile_with_sources(
        "linux-arm64",
        &ToolchainConfigSources::default(),
        &ToolchainProfileOverrides::default(),
    )
    .unwrap();
    profile.pkg_config_libdirs = vec![pkgconfig_dir.clone()];

    let checks = doctor_contract_pkg_config_checks(
        &contract,
        &BuildToolchainProfile {
            profile,
            cargo_target_triple: Some("aarch64-unknown-linux-gnu".to_string()),
            is_cross: true,
        },
        None,
    )
    .unwrap();

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].level, DoctorLevel::Ok);
    assert!(checks[0].detail.contains("component=camera"));
    assert!(checks[0].detail.contains("module=vendor_capture"));
    assert!(checks[0].detail.contains("status=found"));
    assert!(
        checks[0].detail.contains(
            &pkgconfig_dir
                .join("vendor_capture.pc")
                .display()
                .to_string()
        )
    );
    assert!(
        checks[0]
            .detail
            .contains(&overlay.join("include").display().to_string())
    );
    assert!(
        checks[0]
            .detail
            .contains(&overlay.join("lib").display().to_string())
    );
    assert!(!checks[0].detail.contains("host_only"));
}

#[test]
fn doctor_contract_pkg_config_checks_report_missing_module_with_overlay_hint() {
    if !command_available("pkg-config") {
        return;
    }

    let _env_lock = PKG_CONFIG_ENV_LOCK.lock().unwrap();
    let _pkg_config_path = EnvOverride::set("PKG_CONFIG_PATH", None);
    let _pkg_config_sysroot_dir = EnvOverride::set("PKG_CONFIG_SYSROOT_DIR", None);

    let root = temp_test_dir("doctor-pkg-config-missing");
    let overlay = root.join("vendor/rknn");
    let pkgconfig_dir = root.join("vendor/pkgconfig");
    std::fs::create_dir_all(&overlay).unwrap();
    std::fs::create_dir_all(&pkgconfig_dir).unwrap();

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

    let mut profile = resolve_toolchain_profile_with_sources(
        "linux-arm64",
        &ToolchainConfigSources::default(),
        &ToolchainProfileOverrides::default(),
    )
    .unwrap();
    profile.pkg_config_libdirs = vec![pkgconfig_dir.clone()];
    profile.sdk_overlays = vec![overlay.clone()];

    let checks = doctor_contract_pkg_config_checks(
        &contract,
        &BuildToolchainProfile {
            profile,
            cargo_target_triple: Some("aarch64-unknown-linux-gnu".to_string()),
            is_cross: true,
        },
        None,
    )
    .unwrap();

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].level, DoctorLevel::Error);
    assert!(checks[0].detail.contains("component=camera"));
    assert!(checks[0].detail.contains("module=vendor_capture"));
    assert!(checks[0].detail.contains("status=missing"));
    assert!(
        checks[0]
            .detail
            .contains(&pkgconfig_dir.display().to_string())
    );
    assert!(checks[0].detail.contains(&overlay.display().to_string()));
    assert!(
        checks[0]
            .detail
            .contains("flowrt toolchain init --target linux-arm64 --sdk-overlay")
    );
    assert!(checks[0].detail.contains("prepare the external SDK"));
}

#[test]
fn doctor_toolchain_path_checks_include_actionable_cross_sdk_hints() {
    let sdk_overlay = doctor_sdk_overlay_check("linux-arm64", Path::new("/missing/vendor/rknn"));
    assert_eq!(sdk_overlay.level, DoctorLevel::Error);
    assert!(sdk_overlay.detail.contains("SDK overlay"));
    assert!(
        sdk_overlay.detail.contains(
            "flowrt toolchain init --target linux-arm64 --sdk-overlay /missing/vendor/rknn"
        )
    );

    let pkg_config =
        doctor_pkg_config_path_check("linux-arm64", Path::new("/missing/vendor/pkgconfig"));
    assert_eq!(pkg_config.level, DoctorLevel::Warn);
    assert!(pkg_config.detail.contains("pkg-config"));
    assert!(pkg_config.detail.contains("prepare the SDK overlay"));
    assert!(
        pkg_config
            .detail
            .contains("flowrt doctor <rsdl> --target linux-arm64")
    );

    let cmake_toolchain =
        doctor_cmake_toolchain_check("linux-arm64", Path::new("/missing/toolchain.cmake"));
    assert_eq!(cmake_toolchain.level, DoctorLevel::Error);
    assert!(cmake_toolchain.detail.contains("CMake toolchain"));
    assert!(cmake_toolchain.detail.contains(".flowrt/toolchains.toml"));
    assert!(
        cmake_toolchain
            .detail
            .contains("flowrt doctor <rsdl> --target linux-arm64")
    );
}

#[test]
fn doctor_contract_pkg_config_checks_report_ok_when_selected_target_has_no_cpp_pkg_config() {
    let contract = contract_from_source(
        r#"
[package]
name = "robot"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
target = "arm64"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
max_age_ms = 100

[target.arm64]
platform = "linux-arm64"
runtime = ["rust"]
backends = ["inproc"]
"#,
    );

    let profile = resolve_toolchain_profile_with_sources(
        "linux-arm64",
        &ToolchainConfigSources::default(),
        &ToolchainProfileOverrides::default(),
    )
    .unwrap();

    let checks = doctor_contract_pkg_config_checks(
        &contract,
        &BuildToolchainProfile {
            profile,
            cargo_target_triple: Some("aarch64-unknown-linux-gnu".to_string()),
            is_cross: true,
        },
        None,
    )
    .unwrap();

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].level, DoctorLevel::Ok);
    assert!(
        checks[0]
            .detail
            .contains("no C++ component pkg_config dependencies")
    );
}
