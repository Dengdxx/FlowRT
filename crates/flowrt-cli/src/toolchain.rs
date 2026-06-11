use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolchainProfile {
    pub(crate) platform: String,
    pub(crate) rust_target: String,
    pub(crate) deb_multiarch: String,
    pub(crate) c_compiler: String,
    pub(crate) cpp_compiler: String,
    pub(crate) sysroot: Option<PathBuf>,
    pub(crate) cmake_toolchain: Option<PathBuf>,
    pub(crate) pkg_config_libdir: Option<PathBuf>,
    pub(crate) pkg_config_libdirs: Vec<PathBuf>,
    pub(crate) cmake_prefix_paths: Vec<PathBuf>,
    pub(crate) sdk_overlays: Vec<PathBuf>,
    pub(crate) cpp_compile_args: Vec<String>,
    pub(crate) cpp_link_args: Vec<String>,
    pub(crate) cpp_link_libraries: Vec<String>,
    pub(crate) runtime_dependency_policy: RuntimeDependencyPolicy,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeDependencyPolicy {
    System,
    #[default]
    Bundle,
    External,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolchainProfileOverrides {
    pub(crate) rust_target: Option<String>,
    pub(crate) c_compiler: Option<String>,
    pub(crate) cpp_compiler: Option<String>,
    pub(crate) sysroot: Option<PathBuf>,
    pub(crate) cmake_toolchain: Option<PathBuf>,
    pub(crate) pkg_config_libdir: Option<PathBuf>,
    #[serde(default)]
    pub(crate) pkg_config_libdirs: Vec<PathBuf>,
    #[serde(default)]
    pub(crate) cmake_prefix_paths: Vec<PathBuf>,
    #[serde(default)]
    pub(crate) sdk_overlays: Vec<PathBuf>,
    #[serde(default)]
    pub(crate) cpp_compile_args: Vec<String>,
    #[serde(default)]
    pub(crate) cpp_link_args: Vec<String>,
    #[serde(default)]
    pub(crate) cpp_link_libraries: Vec<String>,
    pub(crate) runtime_dependency_policy: Option<RuntimeDependencyPolicy>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ToolchainConfigSources {
    pub(crate) system: Option<PathBuf>,
    pub(crate) user: Option<PathBuf>,
    pub(crate) workspace: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
struct PlatformDefaults {
    platform: &'static str,
    rust_target: &'static str,
    deb_multiarch: &'static str,
    c_compiler: &'static str,
    cpp_compiler: &'static str,
}

const PLATFORM_DEFAULTS: &[PlatformDefaults] = &[
    PlatformDefaults {
        platform: "linux-amd64",
        rust_target: "x86_64-unknown-linux-gnu",
        deb_multiarch: "x86_64-linux-gnu",
        c_compiler: "gcc",
        cpp_compiler: "g++",
    },
    PlatformDefaults {
        platform: "linux-arm64",
        rust_target: "aarch64-unknown-linux-gnu",
        deb_multiarch: "aarch64-linux-gnu",
        c_compiler: "aarch64-linux-gnu-gcc",
        cpp_compiler: "aarch64-linux-gnu-g++",
    },
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolchainsFile {
    #[serde(default)]
    pub(crate) toolchain: BTreeMap<String, ToolchainProfileOverrides>,
}

// T03/T04 会把该查询入口接入 deps/build；T01 只提供可测试配置层。
#[allow(dead_code)]
pub(crate) fn resolve_toolchain_profile(
    platform: &str,
    workspace_root: &Path,
    overrides: &ToolchainProfileOverrides,
) -> Result<ToolchainProfile> {
    let sources = default_toolchain_sources(workspace_root);
    resolve_toolchain_profile_with_sources(platform, &sources, overrides)
}

pub(crate) fn resolve_toolchain_profile_with_sources(
    platform: &str,
    sources: &ToolchainConfigSources,
    overrides: &ToolchainProfileOverrides,
) -> Result<ToolchainProfile> {
    let mut profile = default_profile(platform)?;

    for source in [
        ("system", sources.system.as_deref()),
        ("user", sources.user.as_deref()),
        ("workspace", sources.workspace.as_deref()),
    ] {
        if let Some(config) = load_toolchain_config(source.1)? {
            if let Some(config_overrides) = config.toolchain.get(platform) {
                apply_overrides(&mut profile, config_overrides, source.0)?;
            }
        }
    }

    apply_overrides(&mut profile, overrides, "CLI override")?;
    validate_profile(&profile, "resolved profile")?;
    Ok(profile)
}

fn default_toolchain_sources(workspace_root: &Path) -> ToolchainConfigSources {
    ToolchainConfigSources {
        system: Some(PathBuf::from("/etc/flowrt/toolchains.toml")),
        user: user_toolchains_config_path(),
        workspace: Some(workspace_root.join(".flowrt").join("toolchains.toml")),
    }
}

fn user_toolchains_config_path() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|config_root| config_root.join("flowrt").join("toolchains.toml"))
}

fn default_profile(platform: &str) -> Result<ToolchainProfile> {
    let defaults = platform_defaults(platform)?;
    Ok(ToolchainProfile {
        platform: defaults.platform.to_string(),
        rust_target: defaults.rust_target.to_string(),
        deb_multiarch: defaults.deb_multiarch.to_string(),
        c_compiler: defaults.c_compiler.to_string(),
        cpp_compiler: defaults.cpp_compiler.to_string(),
        sysroot: None,
        cmake_toolchain: None,
        pkg_config_libdir: None,
        pkg_config_libdirs: Vec::new(),
        cmake_prefix_paths: Vec::new(),
        sdk_overlays: Vec::new(),
        cpp_compile_args: Vec::new(),
        cpp_link_args: Vec::new(),
        cpp_link_libraries: Vec::new(),
        runtime_dependency_policy: RuntimeDependencyPolicy::Bundle,
    })
}

fn platform_defaults(platform: &str) -> Result<&'static PlatformDefaults> {
    PLATFORM_DEFAULTS
        .iter()
        .find(|defaults| defaults.platform == platform)
        .ok_or_else(|| anyhow::anyhow!("unsupported toolchain platform `{platform}`"))
}

fn load_toolchain_config(path: Option<&Path>) -> Result<Option<ToolchainsFile>> {
    let Some(path) = path else {
        return Ok(None);
    };
    if !path.exists() {
        return Ok(None);
    }

    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read toolchain config `{}`", path.display()))?;
    let config: ToolchainsFile = toml::from_str(&source)
        .with_context(|| format!("failed to parse toolchain config `{}`", path.display()))?;
    validate_config_platforms(&config, path)?;
    for (platform, overrides) in &config.toolchain {
        validate_overrides(
            overrides,
            &format!("toolchain.{platform} in `{}`", path.display()),
        )?;
    }
    Ok(Some(config))
}

fn validate_config_platforms(config: &ToolchainsFile, path: &Path) -> Result<()> {
    for platform in config.toolchain.keys() {
        platform_defaults(platform)
            .with_context(|| format!("invalid toolchain profile in `{}`", path.display()))?;
    }
    Ok(())
}

fn apply_overrides(
    profile: &mut ToolchainProfile,
    overrides: &ToolchainProfileOverrides,
    source: &str,
) -> Result<()> {
    validate_overrides(overrides, source)?;

    if let Some(value) = &overrides.rust_target {
        profile.rust_target = value.clone();
    }
    if let Some(value) = &overrides.c_compiler {
        profile.c_compiler = value.clone();
    }
    if let Some(value) = &overrides.cpp_compiler {
        profile.cpp_compiler = value.clone();
    }
    if let Some(value) = &overrides.sysroot {
        profile.sysroot = Some(value.clone());
    }
    if let Some(value) = &overrides.cmake_toolchain {
        profile.cmake_toolchain = Some(value.clone());
    }
    if let Some(value) = &overrides.pkg_config_libdir {
        profile.pkg_config_libdir = Some(value.clone());
    }
    append_unique_paths(
        &mut profile.pkg_config_libdirs,
        &overrides.pkg_config_libdirs,
    );
    append_unique_paths(
        &mut profile.cmake_prefix_paths,
        &overrides.cmake_prefix_paths,
    );
    append_unique_paths(&mut profile.sdk_overlays, &overrides.sdk_overlays);
    append_unique_strings(&mut profile.cpp_compile_args, &overrides.cpp_compile_args);
    append_unique_strings(&mut profile.cpp_link_args, &overrides.cpp_link_args);
    append_unique_strings(
        &mut profile.cpp_link_libraries,
        &overrides.cpp_link_libraries,
    );
    if let Some(value) = overrides.runtime_dependency_policy {
        profile.runtime_dependency_policy = value;
    }
    Ok(())
}

fn validate_overrides(overrides: &ToolchainProfileOverrides, source: &str) -> Result<()> {
    ensure_optional_non_empty_string(&overrides.rust_target, "rust_target", source)?;
    ensure_optional_non_empty_string(&overrides.c_compiler, "c_compiler", source)?;
    ensure_optional_non_empty_string(&overrides.cpp_compiler, "cpp_compiler", source)?;
    ensure_optional_non_empty_path(&overrides.sysroot, "sysroot", source)?;
    ensure_optional_non_empty_path(&overrides.cmake_toolchain, "cmake_toolchain", source)?;
    ensure_optional_non_empty_path(&overrides.pkg_config_libdir, "pkg_config_libdir", source)?;
    ensure_non_empty_paths(&overrides.pkg_config_libdirs, "pkg_config_libdirs", source)?;
    ensure_non_empty_paths(&overrides.cmake_prefix_paths, "cmake_prefix_paths", source)?;
    ensure_non_empty_paths(&overrides.sdk_overlays, "sdk_overlays", source)?;
    ensure_non_empty_strings(&overrides.cpp_compile_args, "cpp_compile_args", source)?;
    ensure_non_empty_strings(&overrides.cpp_link_args, "cpp_link_args", source)?;
    ensure_non_empty_strings(&overrides.cpp_link_libraries, "cpp_link_libraries", source)?;
    ensure_cmake_list_safe_strings(&overrides.cpp_compile_args, "cpp_compile_args", source)?;
    ensure_cmake_list_safe_strings(&overrides.cpp_link_args, "cpp_link_args", source)?;
    ensure_cmake_list_safe_strings(&overrides.cpp_link_libraries, "cpp_link_libraries", source)?;
    Ok(())
}

fn validate_profile(profile: &ToolchainProfile, source: &str) -> Result<()> {
    ensure_non_empty_string(&profile.platform, "platform", source)?;
    ensure_non_empty_string(&profile.rust_target, "rust_target", source)?;
    ensure_non_empty_string(&profile.deb_multiarch, "deb_multiarch", source)?;
    ensure_non_empty_string(&profile.c_compiler, "c_compiler", source)?;
    ensure_non_empty_string(&profile.cpp_compiler, "cpp_compiler", source)?;
    ensure_optional_non_empty_path(&profile.sysroot, "sysroot", source)?;
    ensure_optional_non_empty_path(&profile.cmake_toolchain, "cmake_toolchain", source)?;
    ensure_optional_non_empty_path(&profile.pkg_config_libdir, "pkg_config_libdir", source)?;
    ensure_non_empty_paths(&profile.pkg_config_libdirs, "pkg_config_libdirs", source)?;
    ensure_non_empty_paths(&profile.cmake_prefix_paths, "cmake_prefix_paths", source)?;
    ensure_non_empty_paths(&profile.sdk_overlays, "sdk_overlays", source)?;
    ensure_non_empty_strings(&profile.cpp_compile_args, "cpp_compile_args", source)?;
    ensure_non_empty_strings(&profile.cpp_link_args, "cpp_link_args", source)?;
    ensure_non_empty_strings(&profile.cpp_link_libraries, "cpp_link_libraries", source)?;
    ensure_cmake_list_safe_strings(&profile.cpp_compile_args, "cpp_compile_args", source)?;
    ensure_cmake_list_safe_strings(&profile.cpp_link_args, "cpp_link_args", source)?;
    ensure_cmake_list_safe_strings(&profile.cpp_link_libraries, "cpp_link_libraries", source)?;
    Ok(())
}

fn ensure_optional_non_empty_string(
    value: &Option<String>,
    field: &str,
    source: &str,
) -> Result<()> {
    if let Some(value) = value {
        ensure_non_empty_string(value, field, source)?;
    }
    Ok(())
}

fn ensure_non_empty_string(value: &str, field: &str, source: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("toolchain {source} field `{field}` must not be empty");
    }
    Ok(())
}

fn ensure_optional_non_empty_path(
    value: &Option<PathBuf>,
    field: &str,
    source: &str,
) -> Result<()> {
    if let Some(value) = value
        && value.as_os_str().is_empty()
    {
        bail!("toolchain {source} field `{field}` must not be empty");
    }
    Ok(())
}

fn ensure_non_empty_paths(values: &[PathBuf], field: &str, source: &str) -> Result<()> {
    for value in values {
        if value.as_os_str().is_empty() {
            bail!("toolchain {source} field `{field}` must not contain empty paths");
        }
    }
    Ok(())
}

fn ensure_non_empty_strings(values: &[String], field: &str, source: &str) -> Result<()> {
    for value in values {
        ensure_non_empty_string(value, field, source)?;
    }
    Ok(())
}

fn ensure_cmake_list_safe_strings(values: &[String], field: &str, source: &str) -> Result<()> {
    for value in values {
        if value.contains(';') {
            bail!(
                "toolchain {source} field `{field}` must not contain `;` because it is passed as a CMake list"
            );
        }
    }
    Ok(())
}

fn append_unique_paths(target: &mut Vec<PathBuf>, values: &[PathBuf]) {
    for value in values {
        if !target.iter().any(|existing| existing == value) {
            target.push(value.clone());
        }
    }
}

fn append_unique_strings(target: &mut Vec<String>, values: &[String]) {
    for value in values {
        if !target.iter().any(|existing| existing == value) {
            target.push(value.clone());
        }
    }
}

/// 每个 profile 字段的来源标注。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolchainFieldSources {
    pub(crate) platform_source: String,
    pub(crate) rust_target_source: String,
    pub(crate) c_compiler_source: String,
    pub(crate) cpp_compiler_source: String,
    pub(crate) sysroot_source: String,
    pub(crate) cmake_toolchain_source: String,
    pub(crate) pkg_config_libdir_source: String,
    pub(crate) runtime_dependency_policy_source: String,
}

/// 为 `toolchain show` 解析 profile 并跟踪每层来源。
pub(crate) fn resolve_toolchain_profile_with_field_sources(
    platform: &str,
    workspace_root: &Path,
) -> Result<(ToolchainProfile, ToolchainFieldSources)> {
    let sources = default_toolchain_sources(workspace_root);
    resolve_toolchain_profile_with_field_sources_from_sources(platform, &sources)
}

/// 使用显式 sources 为 `toolchain show` 解析 profile 并跟踪每层来源。
pub(crate) fn resolve_toolchain_profile_with_field_sources_from_sources(
    platform: &str,
    sources: &ToolchainConfigSources,
) -> Result<(ToolchainProfile, ToolchainFieldSources)> {
    let mut profile = default_profile(platform)?;
    let mut field_sources = ToolchainFieldSources {
        platform_source: "builtin".to_string(),
        rust_target_source: "builtin".to_string(),
        c_compiler_source: "builtin".to_string(),
        cpp_compiler_source: "builtin".to_string(),
        sysroot_source: "(none)".to_string(),
        cmake_toolchain_source: "(none)".to_string(),
        pkg_config_libdir_source: "(none)".to_string(),
        runtime_dependency_policy_source: "builtin".to_string(),
    };

    for (source_name, source_path) in [
        ("system", sources.system.as_deref()),
        ("user", sources.user.as_deref()),
        ("workspace", sources.workspace.as_deref()),
    ] {
        if let Some(config) = load_toolchain_config(source_path)? {
            if let Some(config_overrides) = config.toolchain.get(platform) {
                track_and_apply_overrides(
                    &mut profile,
                    &mut field_sources,
                    config_overrides,
                    source_name,
                )?;
            }
        }
    }

    Ok((profile, field_sources))
}

fn track_and_apply_overrides(
    profile: &mut ToolchainProfile,
    field_sources: &mut ToolchainFieldSources,
    overrides: &ToolchainProfileOverrides,
    source: &str,
) -> Result<()> {
    validate_overrides(overrides, source)?;

    if overrides.rust_target.is_some() {
        field_sources.rust_target_source = source.to_string();
    }
    if overrides.c_compiler.is_some() {
        field_sources.c_compiler_source = source.to_string();
    }
    if overrides.cpp_compiler.is_some() {
        field_sources.cpp_compiler_source = source.to_string();
    }
    if overrides.sysroot.is_some() {
        field_sources.sysroot_source = source.to_string();
    }
    if overrides.cmake_toolchain.is_some() {
        field_sources.cmake_toolchain_source = source.to_string();
    }
    if overrides.pkg_config_libdir.is_some() {
        field_sources.pkg_config_libdir_source = source.to_string();
    }
    if overrides.runtime_dependency_policy.is_some() {
        field_sources.runtime_dependency_policy_source = source.to_string();
    }

    apply_overrides(profile, overrides, source)
}

/// 生成 `toolchain init` 的最小 TOML 内容。
pub(crate) fn generate_toolchain_init_toml(
    platform: &str,
    sdk_overlays: &[PathBuf],
) -> Result<String> {
    let _defaults = platform_defaults(platform)?;
    let mut toml = String::new();
    toml.push_str(&format!("[toolchain.{platform}]\n"));
    if !sdk_overlays.is_empty() {
        let overlay_strs: Vec<String> = sdk_overlays
            .iter()
            .map(|p| {
                let escaped = p
                    .to_string_lossy()
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
                    .replace('\t', "\\t")
                    .replace('\0', "\\0");
                format!("\"{escaped}\"")
            })
            .collect();
        toml.push_str(&format!("sdk_overlays = [{}]\n", overlay_strs.join(", ")));
    }
    Ok(toml)
}
