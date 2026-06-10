use super::*;

use crate::toolchain::{
    RuntimeDependencyPolicy, ToolchainConfigSources, ToolchainProfileOverrides,
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
pkg_config_libdir = "/opt/flowrt/0.8.3/targets/linux-arm64/lib/pkgconfig"
pkg_config_libdirs = ["/opt/vendor/lib/pkgconfig"]
cmake_prefix_paths = ["/opt/vendor/cmake"]
sdk_overlays = ["/opt/vendor/rknn"]
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
            "/opt/flowrt/0.8.3/targets/linux-arm64/lib/pkgconfig"
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
    assert_eq!(
        profile.runtime_dependency_policy,
        RuntimeDependencyPolicy::External
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
