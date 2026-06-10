use super::*;

use crate::toolchain::{
    ToolchainConfigSources, ToolchainProfileOverrides, resolve_toolchain_profile_with_sources,
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
pkg_config_libdir = "/opt/flowrt/0.8.2/targets/linux-arm64/lib/pkgconfig"
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
            "/opt/flowrt/0.8.2/targets/linux-arm64/lib/pkgconfig"
        ))
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
        ..Default::default()
    };
    let profile =
        resolve_toolchain_profile_with_sources("linux-arm64", &sources, &overrides).unwrap();

    assert_eq!(profile.c_compiler, "cli-gcc");
    assert_eq!(profile.cpp_compiler, "user-g++");
    assert_eq!(profile.sysroot, Some(PathBuf::from("/workspace/sysroot")));
    assert_eq!(profile.rust_target, "aarch64-unknown-linux-gnu");
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
