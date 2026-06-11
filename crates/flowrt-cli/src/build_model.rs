use std::collections::BTreeSet;
use std::env;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::ValueEnum;
use flowrt_ir::ContractIr;
use serde::{Deserialize, Serialize};

pub const BUILD_INFO_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BuildMode {
    #[default]
    Release,
    Debug,
}

impl BuildMode {
    pub fn cargo_profile_dir(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Debug => "debug",
        }
    }

    pub fn cargo_args(self) -> &'static [&'static str] {
        match self {
            Self::Release => &["--release"],
            Self::Debug => &[],
        }
    }

    pub fn cmake_build_type(self) -> &'static str {
        match self {
            Self::Release => "Release",
            Self::Debug => "Debug",
        }
    }
}

impl fmt::Display for BuildMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Release => "release",
            Self::Debug => "debug",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFeature {
    Iox2,
    Zenoh,
}

impl RuntimeFeature {
    pub fn name(self) -> &'static str {
        match self {
            Self::Iox2 => "iox2",
            Self::Zenoh => "zenoh",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RuntimeFeatureSet {
    features: BTreeSet<RuntimeFeature>,
}

impl RuntimeFeatureSet {
    pub fn inproc_only() -> Self {
        Self::default()
    }

    pub fn all() -> Self {
        Self::from_features([RuntimeFeature::Iox2, RuntimeFeature::Zenoh])
    }

    pub fn from_features(features: impl IntoIterator<Item = RuntimeFeature>) -> Self {
        Self {
            features: features.into_iter().collect(),
        }
    }

    pub fn from_backend_names(backends: impl IntoIterator<Item = impl AsRef<str>>) -> Result<Self> {
        let mut features = BTreeSet::new();
        for backend in backends {
            match backend.as_ref() {
                "inproc" => {}
                "iox2" => {
                    features.insert(RuntimeFeature::Iox2);
                }
                "zenoh" => {
                    features.insert(RuntimeFeature::Zenoh);
                }
                other => anyhow::bail!("unsupported FlowRT runtime backend `{other}`"),
            }
        }
        Ok(Self { features })
    }

    pub fn from_contract(contract: &ContractIr) -> Result<Self> {
        let mut backends = Vec::new();
        for graph in &contract.graphs {
            backends.extend(graph.binds.iter().map(|bind| bind.backend.0.as_str()));
            backends.extend(
                graph
                    .services
                    .iter()
                    .map(|service| service.backend.0.as_str()),
            );
            backends.extend(
                graph
                    .operations
                    .iter()
                    .map(|operation| operation.backend.0.as_str()),
            );
            backends.extend(
                graph
                    .ros2_bridges
                    .iter()
                    .map(|bridge| bridge.backend.0.as_str()),
            );
        }
        Self::from_backend_names(backends)
    }

    #[cfg(test)]
    pub fn is_inproc_only(&self) -> bool {
        self.features.is_empty()
    }

    pub fn is_subset_of(&self, other: &Self) -> bool {
        self.features.is_subset(&other.features)
    }

    pub fn deps_backend_hint(&self) -> &'static str {
        match (
            self.features.contains(&RuntimeFeature::Iox2),
            self.features.contains(&RuntimeFeature::Zenoh),
        ) {
            (false, false) => "inproc",
            (true, false) => "iox2",
            (false, true) => "zenoh",
            (true, true) => "all",
        }
    }

    pub fn canonical_names(&self) -> Vec<&'static str> {
        self.features.iter().map(|feature| feature.name()).collect()
    }

    pub fn cargo_feature_args(&self) -> Vec<String> {
        self.canonical_names()
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    pub fn path_fragment(&self) -> String {
        let names = self.canonical_names();
        if names.is_empty() {
            "features-inproc".to_string()
        } else {
            format!("features-{}", names.join("-"))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepsCacheKey {
    pub flowrt_version: String,
    pub rustc_identity: String,
    pub target_triple: String,
    pub vendor_hash: String,
    pub build_mode: BuildMode,
    pub features: RuntimeFeatureSet,
}

impl DepsCacheKey {
    pub fn new(
        flowrt_version: impl Into<String>,
        rustc_identity: impl Into<String>,
        target_triple: impl Into<String>,
        vendor_hash: impl Into<String>,
        build_mode: BuildMode,
        features: RuntimeFeatureSet,
    ) -> Self {
        Self {
            flowrt_version: flowrt_version.into(),
            rustc_identity: rustc_identity.into(),
            target_triple: target_triple.into(),
            vendor_hash: vendor_hash.into(),
            build_mode,
            features,
        }
    }

    pub fn path_fragment(&self) -> PathBuf {
        PathBuf::from(format!(
            "flowrt-{}",
            sanitize_cache_part(&self.flowrt_version)
        ))
        .join(format!(
            "rust-{}",
            sanitize_cache_part(&self.rustc_identity)
        ))
        .join(format!(
            "target-{}",
            sanitize_cache_part(&self.target_triple)
        ))
        .join(format!("vendor-{}", sanitize_cache_part(&self.vendor_hash)))
        .join(format!("mode-{}", self.build_mode))
        .join(self.features.path_fragment())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheLayout {
    pub root: PathBuf,
    pub target_triple: String,
    pub target_dir: PathBuf,
    pub deps_workspace_dir: PathBuf,
    pub lock_file: PathBuf,
    pub ready_file: PathBuf,
}

impl CacheLayout {
    pub fn new(root: PathBuf, key: &DepsCacheKey) -> Self {
        let fragment = key.path_fragment();
        Self {
            target_triple: key.target_triple.clone(),
            target_dir: root.join("cargo-target").join(&fragment),
            deps_workspace_dir: root.join("deps-workspaces").join(&fragment),
            lock_file: root
                .join("locks")
                .join(format!("{}.lock", fragment_to_file_name(&fragment))),
            ready_file: root.join("ready").join(&fragment).join("ready.json"),
            root,
        }
    }

    pub fn from_fragment(root: PathBuf, fragment: &Path) -> Option<Self> {
        let meta = CacheFragmentMeta::parse(fragment)?;
        Some(Self {
            target_triple: meta.target_triple,
            target_dir: root.join("cargo-target").join(fragment),
            deps_workspace_dir: root.join("deps-workspaces").join(fragment),
            lock_file: root
                .join("locks")
                .join(format!("{}.lock", fragment_to_file_name(fragment))),
            ready_file: root.join("ready").join(fragment).join("ready.json"),
            root,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheFragmentMeta {
    pub fragment: PathBuf,
    pub target_triple: String,
    pub build_mode: BuildMode,
    pub features: Vec<String>,
}

impl CacheFragmentMeta {
    pub fn parse(fragment: &Path) -> Option<Self> {
        let components = fragment
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if components.len() != 6 {
            return None;
        }
        let target_triple = components.get(2)?.strip_prefix("target-")?.to_string();
        let build_mode = match components.get(4)?.strip_prefix("mode-")? {
            "release" => BuildMode::Release,
            "debug" => BuildMode::Debug,
            _ => return None,
        };
        let feature_component = components.get(5)?.strip_prefix("features-")?;
        let features = if feature_component == "inproc" {
            vec!["inproc".to_string()]
        } else {
            feature_component
                .split('-')
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        };
        Some(Self {
            fragment: fragment.to_path_buf(),
            target_triple,
            build_mode,
            features,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInfo {
    pub schema_version: u32,
    pub flowrt_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust_target_triple: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_target_triple: Option<String>,
    pub rsdl_profile: Option<String>,
    pub build_mode: BuildMode,
    pub deps_target_dir: Option<PathBuf>,
    pub executables: BuildExecutables,
    #[serde(default)]
    pub artifacts: Vec<BuildArtifactInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BuildExecutables {
    pub rust_app: Option<PathBuf>,
    pub supervisor: Option<PathBuf>,
    pub cpp_app: Option<PathBuf>,
    pub ros2_bridge: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildArtifactInfo {
    pub kind: String,
    pub target: String,
    pub platform: Option<String>,
    pub path: PathBuf,
    pub sha256: String,
}

impl BuildInfo {
    pub fn new(
        flowrt_version: impl Into<String>,
        rsdl_profile: Option<String>,
        build_mode: BuildMode,
        deps_target_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            schema_version: BUILD_INFO_SCHEMA_VERSION,
            flowrt_version: flowrt_version.into(),
            target: None,
            platform: None,
            target_identity: None,
            rust_target_triple: None,
            host_target_triple: None,
            rsdl_profile,
            build_mode,
            deps_target_dir,
            executables: BuildExecutables::default(),
            artifacts: Vec::new(),
        }
    }

    pub fn path(out_dir: &Path) -> PathBuf {
        out_dir.join("build").join("build-info.json")
    }

    pub fn read(out_dir: &Path) -> Result<Self> {
        let path = Self::path(out_dir);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read `{}`", path.display()))?;
        let info: Self = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse `{}`", path.display()))?;
        if info.schema_version != BUILD_INFO_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported FlowRT build-info schema version {} in `{}`",
                info.schema_version,
                path.display()
            );
        }
        Ok(info)
    }

    pub fn write(&self, out_dir: &Path) -> Result<()> {
        let path = Self::path(out_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }
        let mut content = serde_json::to_string_pretty(self)?;
        content.push('\n');
        std::fs::write(&path, content)
            .with_context(|| format!("failed to write `{}`", path.display()))
    }
}

pub fn default_cache_root() -> Option<PathBuf> {
    cache_root_from_parts(
        env::var_os("FLOWRT_CACHE_DIR").map(PathBuf::from),
        env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
    )
}

pub fn cache_root_from_parts(
    flowrt_cache_dir: Option<PathBuf>,
    xdg_cache_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Option<PathBuf> {
    flowrt_cache_dir
        .or_else(|| xdg_cache_home.map(|path| path.join("flowrt")))
        .or_else(|| home.map(|path| path.join(".cache").join("flowrt")))
}

fn sanitize_cache_part(value: &str) -> String {
    let mut output = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if !previous_dash {
                output.push(mapped);
            }
            previous_dash = true;
        } else {
            output.push(mapped);
            previous_dash = false;
        }
    }
    output.trim_matches('-').to_string()
}

fn fragment_to_file_name(fragment: &Path) -> String {
    fragment
        .components()
        .map(|component| sanitize_cache_part(&component.as_os_str().to_string_lossy()))
        .collect::<Vec<_>>()
        .join("__")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_mode_defaults_to_release_profile() {
        let mode = BuildMode::default();

        assert_eq!(mode, BuildMode::Release);
        assert_eq!(mode.cargo_profile_dir(), "release");
        assert_eq!(mode.cargo_args(), &["--release"]);
        assert_eq!(mode.cmake_build_type(), "Release");

        let debug = BuildMode::Debug;
        assert_eq!(debug.cargo_profile_dir(), "debug");
        assert_eq!(debug.cargo_args(), &[] as &[&str]);
        assert_eq!(debug.cmake_build_type(), "Debug");
    }

    #[test]
    fn runtime_features_use_canonical_ordering() {
        let iox2_then_zenoh =
            RuntimeFeatureSet::from_features([RuntimeFeature::Iox2, RuntimeFeature::Zenoh]);
        let zenoh_then_iox2 =
            RuntimeFeatureSet::from_features([RuntimeFeature::Zenoh, RuntimeFeature::Iox2]);

        assert_eq!(iox2_then_zenoh, zenoh_then_iox2);
        assert_eq!(iox2_then_zenoh.canonical_names(), vec!["iox2", "zenoh"]);
        assert_eq!(iox2_then_zenoh.cargo_feature_args(), vec!["iox2", "zenoh"]);
        assert_eq!(iox2_then_zenoh.path_fragment(), "features-iox2-zenoh");
    }

    #[test]
    fn runtime_features_model_inproc_and_all() {
        let inproc = RuntimeFeatureSet::inproc_only();

        assert!(inproc.is_inproc_only());
        assert_eq!(inproc.canonical_names(), Vec::<&'static str>::new());
        assert_eq!(inproc.path_fragment(), "features-inproc");

        let all = RuntimeFeatureSet::all();
        assert_eq!(
            all,
            RuntimeFeatureSet::from_features([RuntimeFeature::Iox2, RuntimeFeature::Zenoh])
        );
        assert!(inproc.is_subset_of(&all));
        assert_eq!(inproc.deps_backend_hint(), "inproc");
        assert_eq!(
            RuntimeFeatureSet::from_features([RuntimeFeature::Iox2]).deps_backend_hint(),
            "iox2"
        );
        assert_eq!(
            RuntimeFeatureSet::from_features([RuntimeFeature::Zenoh]).deps_backend_hint(),
            "zenoh"
        );
        assert_eq!(all.deps_backend_hint(), "all");
    }

    #[test]
    fn deps_cache_key_path_fragment_is_stable_and_readable() {
        let key = DepsCacheKey::new(
            "0.6.1",
            "rustc 1.90.0 (abc 2026-01-01)",
            "x86_64-unknown-linux-gnu",
            "vendor cafebabe",
            BuildMode::Release,
            RuntimeFeatureSet::from_features([RuntimeFeature::Zenoh, RuntimeFeature::Iox2]),
        );

        assert_eq!(
            key.path_fragment(),
            PathBuf::from("flowrt-0.6.1")
                .join("rust-rustc-1.90.0-abc-2026-01-01")
                .join("target-x86_64-unknown-linux-gnu")
                .join("vendor-vendor-cafebabe")
                .join("mode-release")
                .join("features-iox2-zenoh")
        );
    }

    #[test]
    fn cache_layout_uses_key_for_target_workspace_lock_and_ready_marker() {
        let key = DepsCacheKey::new(
            "0.6.1",
            "rustc 1.90.0",
            "x86_64-unknown-linux-gnu",
            "abc123",
            BuildMode::Debug,
            RuntimeFeatureSet::inproc_only(),
        );
        let layout = CacheLayout::new(PathBuf::from("/cache/flowrt"), &key);

        assert_eq!(layout.target_triple, "x86_64-unknown-linux-gnu");
        assert!(layout.target_dir.starts_with("/cache/flowrt/cargo-target"));
        assert!(
            layout
                .deps_workspace_dir
                .starts_with("/cache/flowrt/deps-workspaces")
        );
        assert!(layout.ready_file.ends_with("ready.json"));
        assert!(layout.lock_file.ends_with(
            "flowrt-0.6.1__rust-rustc-1.90.0__target-x86_64-unknown-linux-gnu__vendor-abc123__mode-debug__features-inproc.lock"
        ));
    }

    #[test]
    fn cache_fragment_meta_parses_target_mode_and_features() {
        let fragment = PathBuf::from("flowrt-0.8.5")
            .join("rust-rustc-1.90.0")
            .join("target-aarch64-unknown-linux-gnu")
            .join("vendor-abcd")
            .join("mode-release")
            .join("features-iox2-zenoh");

        let parsed = CacheFragmentMeta::parse(&fragment).expect("fragment should parse");

        assert_eq!(parsed.fragment, fragment);
        assert_eq!(parsed.target_triple, "aarch64-unknown-linux-gnu");
        assert_eq!(parsed.build_mode, BuildMode::Release);
        assert_eq!(parsed.features, vec!["iox2", "zenoh"]);
    }

    #[test]
    fn cache_layout_can_be_reconstructed_from_fragment() {
        let fragment = PathBuf::from("flowrt-0.8.5")
            .join("rust-rustc-1.90.0")
            .join("target-x86_64-unknown-linux-gnu")
            .join("vendor-abcd")
            .join("mode-debug")
            .join("features-inproc");

        let layout = CacheLayout::from_fragment(PathBuf::from("/cache/flowrt"), &fragment)
            .expect("layout should be reconstructed");

        assert_eq!(layout.target_triple, "x86_64-unknown-linux-gnu");
        assert_eq!(
            layout.target_dir,
            PathBuf::from("/cache/flowrt/cargo-target").join(&fragment)
        );
        assert_eq!(
            layout.deps_workspace_dir,
            PathBuf::from("/cache/flowrt/deps-workspaces").join(&fragment)
        );
        assert_eq!(
            layout.ready_file,
            PathBuf::from("/cache/flowrt/ready")
                .join(&fragment)
                .join("ready.json")
        );
        assert!(
            layout
                .lock_file
                .starts_with(PathBuf::from("/cache/flowrt/locks")),
            "lock file should stay under cache root: {}",
            layout.lock_file.display()
        );
    }

    #[test]
    fn cache_root_prefers_flowrt_cache_dir() {
        let root = cache_root_from_parts(
            Some(PathBuf::from("/tmp/flowrt-cache")),
            Some(PathBuf::from("/tmp/xdg-cache")),
            Some(PathBuf::from("/home/user")),
        );

        assert_eq!(root, Some(PathBuf::from("/tmp/flowrt-cache")));
    }

    #[test]
    fn cache_root_uses_xdg_cache_home_before_home_default() {
        let root = cache_root_from_parts(
            None,
            Some(PathBuf::from("/tmp/xdg-cache")),
            Some(PathBuf::from("/home/user")),
        );

        assert_eq!(root, Some(PathBuf::from("/tmp/xdg-cache").join("flowrt")));
    }

    #[test]
    fn cache_root_falls_back_to_home_cache() {
        let root = cache_root_from_parts(None, None, Some(PathBuf::from("/home/user")));

        assert_eq!(
            root,
            Some(PathBuf::from("/home/user").join(".cache").join("flowrt"))
        );
    }

    #[test]
    fn build_info_roundtrips_with_relative_executables() {
        let mut info = BuildInfo::new(
            "0.6.1",
            Some("default".to_string()),
            BuildMode::Release,
            Some(PathBuf::from("/cache/flowrt/target")),
        );
        info.target = Some("pi".to_string());
        info.platform = Some("linux-arm64".to_string());
        info.rust_target_triple = Some("aarch64-unknown-linux-gnu".to_string());
        info.host_target_triple = Some("x86_64-unknown-linux-gnu".to_string());
        info.target_identity = Some("linux-arm64".to_string());
        info.executables.rust_app = Some(PathBuf::from(
            "build/bin/linux-arm64/release/robot-flowrt-app",
        ));
        let json = serde_json::to_string(&info).unwrap();
        let decoded: BuildInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, info);
    }
}
