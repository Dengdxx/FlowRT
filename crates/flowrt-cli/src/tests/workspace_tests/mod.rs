use super::*;
use crate::build_model::RuntimeFeature;

static REPO_RUNTIME_FALLBACK_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static FLOWRT_CACHE_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static XDG_RUNTIME_DIR_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn patch_mismatched_flowrt_version() -> String {
    let parts = env!("CARGO_PKG_VERSION")
        .split('.')
        .map(|part| part.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    format!("{}.{}.{}", parts[0], parts[1], parts[2] + 1)
}

fn minor_mismatched_flowrt_version() -> String {
    let parts = env!("CARGO_PKG_VERSION")
        .split('.')
        .map(|part| part.parse::<u64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(parts.len(), 3);
    format!("{}.{}.0", parts[0], parts[1] + 1)
}

struct EnvOverride {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl EnvOverride {
    fn set(key: &'static str, value: Option<&std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: callers must guard process-wide environment mutation with a test mutex.
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self { key, previous }
    }

    fn repo_runtime_fallback(value: Option<&str>) -> Self {
        Self::set(
            "FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK",
            value.map(std::ffi::OsStr::new),
        )
    }
}

impl Drop for EnvOverride {
    fn drop(&mut self) {
        // SAFETY: guarded by REPO_RUNTIME_FALLBACK_ENV_LOCK in tests below.
        unsafe {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }
}

fn write_target_sdk_manifest(root: &Path, platform: &str, complete: bool) {
    std::fs::create_dir_all(root.join("include/flowrt")).unwrap();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::create_dir_all(root.join("cmake")).unwrap();
    std::fs::create_dir_all(root.join("pkgconfig")).unwrap();
    std::fs::write(root.join("include/flowrt/runtime.hpp"), "").unwrap();
    std::fs::write(
        root.join("flowrt-target-sdk.toml"),
        format!(
            r#"
schema_version = 1
platform = "{platform}"
complete = {complete}
include_dir = "include"
lib_dir = "lib"
cmake_dir = "cmake"
pkgconfig_dir = "pkgconfig"
"#
        ),
    )
    .unwrap();
}

fn linux_arm64_toolchain_profile() -> crate::toolchain::ToolchainProfile {
    crate::toolchain::ToolchainProfile {
        platform: "linux-arm64".to_string(),
        rust_target: "aarch64-unknown-linux-gnu".to_string(),
        deb_multiarch: "aarch64-linux-gnu".to_string(),
        c_compiler: "aarch64-linux-gnu-gcc".to_string(),
        cpp_compiler: "aarch64-linux-gnu-g++".to_string(),
        sysroot: Some(PathBuf::from("/opt/sysroots/linux-arm64")),
        cmake_toolchain: None,
        pkg_config_libdir: Some(PathBuf::from("/opt/toolchains/linux-arm64/pkgconfig")),
        pkg_config_libdirs: Vec::new(),
        cmake_prefix_paths: Vec::new(),
        sdk_overlays: Vec::new(),
        cpp_compile_args: Vec::new(),
        cpp_link_args: Vec::new(),
        cpp_link_libraries: Vec::new(),
        runtime_dependency_policy: crate::toolchain::RuntimeDependencyPolicy::Bundle,
    }
}

mod build_run;
mod bundle;
mod cache;
mod cargo_build;
mod cmake;
mod deploy;
mod external;
mod prepare;
