use super::*;

use crate::toolchain::{
    RuntimeDependencyPolicy, ToolchainConfigSources, ToolchainProfileOverrides,
    generate_toolchain_init_toml, resolve_toolchain_profile_with_field_sources,
    resolve_toolchain_profile_with_sources,
};

fn write_toolchains(path: &Path, source: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, source).unwrap();
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
fn toolchain_build_profile_uses_native_when_platform_is_absent() {
    let root = temp_test_dir("toolchain-build-native");
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

    assert!(profile.is_none());
    assert!(cargo_target_args(None).is_empty());
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
