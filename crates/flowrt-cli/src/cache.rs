//! `flowrt cache` 命令的实现模块。
//!
//! 对外只暴露 status/clean 两个命令级 Interface；删除相关 Implementation 必须始终
//! 经过 allowed roots、symlink 和 live process 检查，避免把用户 SDK overlay、日志、
//! MCAP 或运行中 socket 当成可重建 cache。

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use crate::build_model::{BuildMode, CacheFragmentMeta, CacheLayout, default_cache_root};
use crate::toolchain::{ToolchainProfileOverrides, ToolchainsFile, resolve_toolchain_profile};
use crate::{DepsReadyMarker, repo_root_dir};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheCleanOptions {
    pub(crate) target: Option<String>,
    pub(crate) build_mode: Option<BuildMode>,
    pub(crate) dry_run: bool,
    pub(crate) flowrt_deps: bool,
    pub(crate) project_build: bool,
    pub(crate) incremental: bool,
    pub(crate) stale_temp: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheEntryPolicy {
    DefaultClean,
    Conditional,
    DisplayOnly,
    NeverAutoClean,
}

impl CacheEntryPolicy {
    fn heading(self) -> &'static str {
        match self {
            Self::DefaultClean => "默认可清",
            Self::Conditional => "条件可清",
            Self::DisplayOnly => "仅展示",
            Self::NeverAutoClean => "永不自动清",
        }
    }
}

#[derive(Debug, Clone)]
struct CacheStatusEntry {
    policy: CacheEntryPolicy,
    label: String,
    path: Option<PathBuf>,
    size_bytes: Option<u64>,
    detail: String,
}

#[derive(Debug, Clone)]
struct CacheCleanEntry {
    label: String,
    path: PathBuf,
    prune_root: PathBuf,
}

#[derive(Debug, Clone)]
struct CacheFilter {
    target_platform: Option<String>,
    target_triple: Option<String>,
    build_mode: Option<BuildMode>,
}

#[derive(Debug, Clone)]
struct ReadyMarkerScan {
    meta: CacheFragmentMeta,
    layout: CacheLayout,
    state: ReadyMarkerState,
    features: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadyMarkerState {
    Ready,
    Stale,
    Invalid,
}

impl ReadyMarkerState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Stale => "stale",
            Self::Invalid => "invalid",
        }
    }
}

#[derive(Debug, Clone)]
struct ProjectCacheContext {
    root: PathBuf,
    out_dir: PathBuf,
    build_dir: PathBuf,
    bin_dir: PathBuf,
    cmake_dir: PathBuf,
    toolchains_path: PathBuf,
    sdk_overlays: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct TempCandidate {
    label: String,
    path: PathBuf,
    policy: CacheEntryPolicy,
    detail: String,
}

pub(crate) fn cache_status_summary_for_cwd(cwd: &Path) -> Result<String> {
    let cache_root = default_cache_root();
    let project = discover_project_cache_context(cwd)?;
    let mut entries = Vec::new();

    if let Some(root) = &cache_root {
        entries.push(CacheStatusEntry {
            policy: CacheEntryPolicy::DefaultClean,
            label: "FlowRT deps cache root".to_string(),
            path: Some(root.clone()),
            size_bytes: Some(path_size_or_zero(root)?),
            detail: "共享 Cargo/CMake 依赖 cache".to_string(),
        });
        for (label, path) in [
            ("cargo-target cache", root.join("cargo-target")),
            ("deps-workspaces", root.join("deps-workspaces")),
            ("ready markers", root.join("ready")),
        ] {
            entries.push(CacheStatusEntry {
                policy: CacheEntryPolicy::DefaultClean,
                label: label.to_string(),
                path: Some(path.clone()),
                size_bytes: Some(path_size_or_zero(&path)?),
                detail: String::new(),
            });
        }
        for ready in scan_ready_markers(root)? {
            entries.push(CacheStatusEntry {
                policy: CacheEntryPolicy::DefaultClean,
                label: "deps ready marker".to_string(),
                path: Some(ready.layout.ready_file.clone()),
                size_bytes: Some(path_size_or_zero(&ready.layout.ready_file)?),
                detail: format!(
                    "target={} build_mode={} backend={} state={}",
                    ready.meta.target_triple,
                    ready.meta.build_mode,
                    ready.features.join(","),
                    ready.state.as_str()
                ),
            });
            for incremental in incremental_dirs_for_layout(
                &ready.layout,
                ready.meta.build_mode,
                &ready.meta.target_triple,
            ) {
                entries.push(CacheStatusEntry {
                    policy: CacheEntryPolicy::DefaultClean,
                    label: "incremental cache".to_string(),
                    path: Some(incremental.clone()),
                    size_bytes: Some(path_size_or_zero(&incremental)?),
                    detail: format!(
                        "target={} build_mode={}",
                        ready.meta.target_triple, ready.meta.build_mode
                    ),
                });
            }
        }
    }

    if let Some(project) = &project {
        entries.push(CacheStatusEntry {
            policy: CacheEntryPolicy::Conditional,
            label: "project flowrt/build".to_string(),
            path: Some(project.build_dir.clone()),
            size_bytes: Some(path_size_or_zero(&project.build_dir)?),
            detail: "传 `flowrt cache clean --project-build` 后才会删除".to_string(),
        });
        entries.push(CacheStatusEntry {
            policy: CacheEntryPolicy::Conditional,
            label: "flowrt/build/bin".to_string(),
            path: Some(project.bin_dir.clone()),
            size_bytes: Some(path_size_or_zero(&project.bin_dir)?),
            detail: "用户最终二进制目录".to_string(),
        });
        entries.push(CacheStatusEntry {
            policy: CacheEntryPolicy::DefaultClean,
            label: "flowrt/build/cmake".to_string(),
            path: Some(project.cmake_dir.clone()),
            size_bytes: Some(path_size_or_zero(&project.cmake_dir)?),
            detail: "可重建的 CMake build dir".to_string(),
        });
        if project.toolchains_path.exists() {
            entries.push(CacheStatusEntry {
                policy: CacheEntryPolicy::NeverAutoClean,
                label: "workspace toolchain config".to_string(),
                path: Some(project.toolchains_path.clone()),
                size_bytes: Some(path_size_or_zero(&project.toolchains_path)?),
                detail: ".flowrt/toolchains.toml 不属于 cache".to_string(),
            });
        }
        for overlay in &project.sdk_overlays {
            entries.push(CacheStatusEntry {
                policy: CacheEntryPolicy::NeverAutoClean,
                label: "sdk_overlay".to_string(),
                path: Some(overlay.clone()),
                size_bytes: Some(path_size_or_zero(overlay)?),
                detail: "SDK overlay 只展示占用，不属于 FlowRT cache".to_string(),
            });
        }
        for hint in user_data_hints(project)? {
            entries.push(hint);
        }
    }

    for candidate in scan_temp_candidates()? {
        entries.push(CacheStatusEntry {
            policy: candidate.policy,
            label: candidate.label,
            path: Some(candidate.path),
            size_bytes: None,
            detail: candidate.detail,
        });
    }

    if let Ok(repo_root) = repo_root_dir()
        && cwd.starts_with(&repo_root)
    {
        let target_dir = repo_root.join("target");
        entries.push(CacheStatusEntry {
            policy: CacheEntryPolicy::DisplayOnly,
            label: "FlowRT repo target".to_string(),
            path: Some(target_dir.clone()),
            size_bytes: Some(path_size_or_zero(&target_dir)?),
            detail: "仓库开发产物，仅展示".to_string(),
        });
        if repo_root.parent().and_then(Path::file_name) == Some(OsStr::new(".worktrees")) {
            entries.push(CacheStatusEntry {
                policy: CacheEntryPolicy::Conditional,
                label: "git worktree".to_string(),
                path: Some(repo_root),
                size_bytes: None,
                detail: "需显式用 git worktree remove 手动处理".to_string(),
            });
        }
    }

    let mut output = String::new();
    match &cache_root {
        Some(root) => output.push_str(&format!("FlowRT cache root: {}\n", root.display())),
        None => output.push_str("FlowRT cache root: unresolved (set FLOWRT_CACHE_DIR or HOME)\n"),
    }
    for policy in [
        CacheEntryPolicy::DefaultClean,
        CacheEntryPolicy::Conditional,
        CacheEntryPolicy::DisplayOnly,
        CacheEntryPolicy::NeverAutoClean,
    ] {
        output.push('\n');
        output.push_str(&format!("[{}]\n", policy.heading()));
        let matching = entries
            .iter()
            .filter(|entry| entry.policy == policy)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            output.push_str("- (none)\n");
            continue;
        }
        for entry in matching {
            output.push_str("- ");
            output.push_str(&entry.label);
            if let Some(path) = &entry.path {
                output.push_str(": ");
                output.push_str(&path.display().to_string());
            }
            if let Some(size_bytes) = entry.size_bytes {
                output.push_str(&format!(" ({})", format_bytes(size_bytes)));
            }
            if !entry.detail.is_empty() {
                output.push_str("; ");
                output.push_str(&entry.detail);
            }
            output.push('\n');
        }
    }
    Ok(output.trim_end().to_string())
}

pub(crate) fn cache_clean_for_cwd(cwd: &Path, options: CacheCleanOptions) -> Result<String> {
    if !options.flowrt_deps && !options.project_build && !options.incremental && !options.stale_temp
    {
        anyhow::bail!(
            "cache clean requires at least one scope flag: --flowrt-deps, --project-build, --incremental, --stale-temp"
        );
    }

    let filter = resolve_cache_filter(cwd, options.target.as_deref(), options.build_mode)?;
    let project = discover_project_cache_context(cwd)?;
    let cache_root = default_cache_root();
    let mut entries = Vec::new();

    if options.flowrt_deps
        && let Some(root) = &cache_root
    {
        if filter.target_triple.is_none() && filter.build_mode.is_none() {
            for path in [
                root.join("cargo-target"),
                root.join("deps-workspaces"),
                root.join("ready"),
                root.join("locks"),
            ] {
                if path.exists() {
                    entries.push(CacheCleanEntry {
                        label: "FlowRT deps cache".to_string(),
                        path,
                        prune_root: root.clone(),
                    });
                }
            }
        } else {
            for ready in scan_ready_markers(root)? {
                if !filter_matches_ready(&filter, &ready) {
                    continue;
                }
                for (label, path, prune_root) in [
                    (
                        "FlowRT deps target dir",
                        ready.layout.target_dir.clone(),
                        root.clone(),
                    ),
                    (
                        "FlowRT deps workspace",
                        ready.layout.deps_workspace_dir.clone(),
                        root.clone(),
                    ),
                    (
                        "FlowRT deps ready marker",
                        ready.layout.ready_file.clone(),
                        root.join("ready"),
                    ),
                    (
                        "FlowRT deps lock file",
                        ready.layout.lock_file.clone(),
                        root.join("locks"),
                    ),
                ] {
                    if path.exists() {
                        entries.push(CacheCleanEntry {
                            label: label.to_string(),
                            path,
                            prune_root,
                        });
                    }
                }
            }
        }
    }

    if options.incremental
        && let Some(root) = &cache_root
    {
        for ready in scan_ready_markers(root)? {
            if !filter_matches_ready(&filter, &ready) {
                continue;
            }
            for path in incremental_dirs_for_layout(
                &ready.layout,
                ready.meta.build_mode,
                &ready.meta.target_triple,
            ) {
                if path.exists() {
                    entries.push(CacheCleanEntry {
                        label: "Cargo incremental cache".to_string(),
                        path,
                        prune_root: root.clone(),
                    });
                }
            }
        }
    }

    if options.project_build
        && let Some(project) = &project
    {
        entries.extend(project_build_clean_entries(project, &filter));
    }

    if options.stale_temp {
        for candidate in scan_temp_candidates()? {
            if candidate.policy != CacheEntryPolicy::DefaultClean {
                continue;
            }
            let prune_root = if candidate.path.starts_with(flowrt::runtime_socket_dir()) {
                flowrt::runtime_socket_dir()
            } else {
                std::env::temp_dir()
            };
            entries.push(CacheCleanEntry {
                label: candidate.label,
                path: candidate.path,
                prune_root,
            });
        }
    }

    let entries = normalize_clean_entries(entries);
    if entries.is_empty() {
        return Ok("cache clean: no matching paths".to_string());
    }

    let mut output = String::new();
    if options.dry_run {
        output.push_str("cache clean dry-run:\n");
        for entry in &entries {
            output.push_str(&format!(
                "- would delete {}: {}\n",
                entry.label,
                entry.path.display()
            ));
        }
        return Ok(output.trim_end().to_string());
    }

    output.push_str("cache clean:\n");
    for entry in &entries {
        delete_path_within_roots(
            &entry.path,
            &[entry.prune_root.as_path()],
            &entry.prune_root,
        )?;
        output.push_str(&format!(
            "- deleted {}: {}\n",
            entry.label,
            entry.path.display()
        ));
    }
    Ok(output.trim_end().to_string())
}

fn resolve_cache_filter(
    cwd: &Path,
    target: Option<&str>,
    build_mode: Option<BuildMode>,
) -> Result<CacheFilter> {
    let target_platform = target.map(str::to_string);
    let target_triple = if let Some(platform) = target {
        let workspace_root = discover_project_cache_context(cwd)?
            .map(|project| project.root)
            .unwrap_or_else(|| cwd.to_path_buf());
        Some(
            resolve_toolchain_profile(
                platform,
                &workspace_root,
                &ToolchainProfileOverrides::default(),
            )?
            .rust_target,
        )
    } else {
        None
    };
    Ok(CacheFilter {
        target_platform,
        target_triple,
        build_mode,
    })
}

fn discover_project_cache_context(cwd: &Path) -> Result<Option<ProjectCacheContext>> {
    let Some(root) = find_project_root(cwd) else {
        return Ok(None);
    };
    let out_dir = root.join("flowrt");
    let build_dir = out_dir.join("build");
    let toolchains_path = root.join(".flowrt").join("toolchains.toml");
    Ok(Some(ProjectCacheContext {
        root,
        bin_dir: build_dir.join("bin"),
        cmake_dir: build_dir.join("cmake"),
        build_dir,
        out_dir,
        sdk_overlays: load_workspace_sdk_overlays(&toolchains_path)?,
        toolchains_path,
    }))
}

fn find_project_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors().find_map(|ancestor| {
        (ancestor.join("rsdl").is_dir()
            || ancestor.join("flowrt").is_dir()
            || ancestor.join(".flowrt").join("toolchains.toml").exists())
        .then_some(ancestor.to_path_buf())
    })
}

fn load_workspace_sdk_overlays(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read `{}`", path.display()))?;
    let config: ToolchainsFile = toml::from_str(&content)
        .with_context(|| format!("failed to parse `{}`", path.display()))?;
    let mut overlays = Vec::new();
    for profile in config.toolchain.values() {
        for overlay in &profile.sdk_overlays {
            if !overlays.iter().any(|existing| existing == overlay) {
                overlays.push(overlay.clone());
            }
        }
    }
    Ok(overlays)
}

fn user_data_hints(project: &ProjectCacheContext) -> Result<Vec<CacheStatusEntry>> {
    let mut entries = Vec::new();
    let mut hinted_mcap = false;
    if let Ok(children) = fs::read_dir(&project.root) {
        for child in children {
            let path = child?.path();
            if path.extension().is_some_and(|ext| ext == "mcap") {
                hinted_mcap = true;
                entries.push(CacheStatusEntry {
                    policy: CacheEntryPolicy::DisplayOnly,
                    label: "user data (.mcap)".to_string(),
                    size_bytes: Some(path_size_or_zero(&path)?),
                    path: Some(path),
                    detail: "录制产物只展示，不计入 FlowRT cache".to_string(),
                });
            }
        }
    }
    for name in ["log", "logs"] {
        for path in [project.root.join(name), project.out_dir.join(name)] {
            if path.exists() {
                entries.push(CacheStatusEntry {
                    policy: CacheEntryPolicy::DisplayOnly,
                    label: "user data (logs)".to_string(),
                    size_bytes: Some(path_size_or_zero(&path)?),
                    path: Some(path),
                    detail: "日志目录只展示，不计入 FlowRT cache".to_string(),
                });
            }
        }
    }
    if !hinted_mcap {
        entries.push(CacheStatusEntry {
            policy: CacheEntryPolicy::DisplayOnly,
            label: "user data hint".to_string(),
            path: None,
            size_bytes: None,
            detail: "*.mcap 和日志目录只展示，不会被 cache clean 自动删除".to_string(),
        });
    }
    Ok(entries)
}

fn scan_ready_markers(cache_root: &Path) -> Result<Vec<ReadyMarkerScan>> {
    let ready_root = cache_root.join("ready");
    let mut files = Vec::new();
    collect_named_files(&ready_root, "ready.json", &mut files)?;
    let mut entries = Vec::new();
    for ready_file in files {
        let Some(parent) = ready_file.parent() else {
            continue;
        };
        let Ok(fragment) = parent.strip_prefix(&ready_root) else {
            continue;
        };
        let Some(meta) = CacheFragmentMeta::parse(fragment) else {
            continue;
        };
        let Some(layout) = CacheLayout::from_fragment(cache_root.to_path_buf(), fragment) else {
            continue;
        };
        let parsed_marker = fs::read_to_string(&ready_file)
            .ok()
            .and_then(|content| serde_json::from_str::<DepsReadyMarker>(&content).ok());
        let state = parsed_marker
            .as_ref()
            .map(|marker| {
                if marker.schema_version == 1 && marker.target_dir == layout.target_dir {
                    ReadyMarkerState::Ready
                } else {
                    ReadyMarkerState::Stale
                }
            })
            .unwrap_or(ReadyMarkerState::Invalid);
        let features = parsed_marker
            .as_ref()
            .map(|marker| {
                if marker.features.is_empty() {
                    vec!["inproc".to_string()]
                } else {
                    marker.features.clone()
                }
            })
            .unwrap_or_else(|| meta.features.clone());
        entries.push(ReadyMarkerScan {
            meta,
            layout,
            state,
            features,
        });
    }
    entries.sort_by(|left, right| {
        left.meta
            .target_triple
            .cmp(&right.meta.target_triple)
            .then(
                left.meta
                    .build_mode
                    .to_string()
                    .cmp(&right.meta.build_mode.to_string()),
            )
            .then(left.features.join(",").cmp(&right.features.join(",")))
    });
    Ok(entries)
}

fn collect_named_files(root: &Path, file_name: &str, output: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(root)
        .with_context(|| format!("failed to stat `{}`", root.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        if root.file_name() == Some(OsStr::new(file_name)) {
            output.push(root.to_path_buf());
        }
        return Ok(());
    }
    for child in
        fs::read_dir(root).with_context(|| format!("failed to read `{}`", root.display()))?
    {
        let path = child?.path();
        collect_named_files(&path, file_name, output)?;
    }
    Ok(())
}

fn incremental_dirs_for_layout(
    layout: &CacheLayout,
    build_mode: BuildMode,
    target_triple: &str,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for path in [
        layout
            .target_dir
            .join(build_mode.cargo_profile_dir())
            .join("incremental"),
        layout
            .target_dir
            .join(target_triple)
            .join(build_mode.cargo_profile_dir())
            .join("incremental"),
    ] {
        if path.exists() && !dirs.iter().any(|existing| existing == &path) {
            dirs.push(path);
        }
    }
    dirs
}

fn filter_matches_ready(filter: &CacheFilter, ready: &ReadyMarkerScan) -> bool {
    if let Some(target_triple) = &filter.target_triple
        && &ready.meta.target_triple != target_triple
    {
        return false;
    }
    if let Some(build_mode) = filter.build_mode
        && ready.meta.build_mode != build_mode
    {
        return false;
    }
    true
}

fn project_build_clean_entries(
    project: &ProjectCacheContext,
    filter: &CacheFilter,
) -> Vec<CacheCleanEntry> {
    if filter.target_platform.is_none() && filter.build_mode.is_none() {
        return project
            .build_dir
            .exists()
            .then_some(CacheCleanEntry {
                label: "project flowrt/build".to_string(),
                path: project.build_dir.clone(),
                prune_root: project.build_dir.clone(),
            })
            .into_iter()
            .collect();
    }
    let mut entries = Vec::new();
    entries.extend(filtered_project_subdirs(
        &project.bin_dir,
        "project binary dir",
        filter,
    ));
    entries.extend(filtered_project_subdirs(
        &project.cmake_dir,
        "project cmake build dir",
        filter,
    ));
    entries
}

fn filtered_project_subdirs(
    root: &Path,
    label: &str,
    filter: &CacheFilter,
) -> Vec<CacheCleanEntry> {
    let mut entries = Vec::new();
    if !root.exists() {
        return entries;
    }
    let modes = if let Some(mode) = filter.build_mode {
        vec![mode]
    } else {
        vec![BuildMode::Release, BuildMode::Debug]
    };
    if let Some(platform) = &filter.target_platform {
        for mode in modes {
            let path = root.join(platform).join(mode.cargo_profile_dir());
            if path.exists() {
                entries.push(CacheCleanEntry {
                    label: label.to_string(),
                    path,
                    prune_root: root.to_path_buf(),
                });
            }
        }
        return entries;
    }
    for mode in modes {
        let native = root.join(mode.cargo_profile_dir());
        if native.exists() {
            entries.push(CacheCleanEntry {
                label: label.to_string(),
                path: native,
                prune_root: root.to_path_buf(),
            });
        }
        if let Ok(children) = fs::read_dir(root) {
            for child in children.flatten() {
                let path = child.path().join(mode.cargo_profile_dir());
                if path.exists() {
                    entries.push(CacheCleanEntry {
                        label: label.to_string(),
                        path,
                        prune_root: root.to_path_buf(),
                    });
                }
            }
        }
    }
    entries
}

fn scan_temp_candidates() -> Result<Vec<TempCandidate>> {
    let mut entries = Vec::new();
    let runtime_dir = flowrt::runtime_socket_dir();
    if runtime_dir.exists() {
        for socket in flowrt::discover_runtime_sockets()
            .with_context(|| format!("failed to scan `{}`", runtime_dir.display()))?
        {
            if let Some(pid) = socket
                .file_stem()
                .and_then(OsStr::to_str)
                .and_then(|raw| raw.parse::<u32>().ok())
            {
                let live = pid_exists(pid);
                entries.push(TempCandidate {
                    label: if live {
                        "live runtime socket".to_string()
                    } else {
                        "stale runtime socket".to_string()
                    },
                    path: socket,
                    policy: if live {
                        CacheEntryPolicy::NeverAutoClean
                    } else {
                        CacheEntryPolicy::DefaultClean
                    },
                    detail: if live {
                        format!("pid={} is still alive", pid)
                    } else {
                        format!("pid={} is gone", pid)
                    },
                });
            }
        }
    }

    let temp_root = std::env::temp_dir();
    if let Ok(children) = fs::read_dir(&temp_root) {
        for child in children.flatten() {
            let path = child.path();
            if path == runtime_dir {
                continue;
            }
            let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            if !name.starts_with("zenoh-") && !name.starts_with("zenoh.") {
                continue;
            }
            let pids = extract_pid_tokens(name);
            let (policy, detail) = if pids.is_empty() {
                (
                    CacheEntryPolicy::DisplayOnly,
                    "无法证明 stale，保留为手动检查项".to_string(),
                )
            } else if pids.iter().any(|pid| pid_exists(*pid)) {
                (
                    CacheEntryPolicy::NeverAutoClean,
                    "仍有关联 pid 存活，跳过自动清理".to_string(),
                )
            } else {
                (
                    CacheEntryPolicy::DefaultClean,
                    format!("关联 pid {:?} 均已退出", pids),
                )
            };
            entries.push(TempCandidate {
                label: "zenoh temp candidate".to_string(),
                path,
                policy,
                detail,
            });
        }
    }
    Ok(entries)
}

fn extract_pid_tokens(value: &str) -> Vec<u32> {
    value
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<u32>().ok())
        .collect()
}

fn pid_exists(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let result = unsafe { libc::kill(pid as i32, 0) };
        if result == 0 {
            return true;
        }
        let code = std::io::Error::last_os_error().raw_os_error();
        code == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn normalize_clean_entries(entries: Vec<CacheCleanEntry>) -> Vec<CacheCleanEntry> {
    let mut entries = entries;
    entries.sort_by(|left, right| {
        left.path
            .components()
            .count()
            .cmp(&right.path.components().count())
            .then(left.path.cmp(&right.path))
    });
    let mut normalized = Vec::new();
    for entry in entries {
        if normalized.iter().any(|existing: &CacheCleanEntry| {
            entry.path == existing.path || entry.path.starts_with(&existing.path)
        }) {
            continue;
        }
        normalized.push(entry);
    }
    normalized
}

fn delete_path_within_roots(path: &Path, allowed_roots: &[&Path], prune_root: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| component == Component::ParentDir)
    {
        anyhow::bail!("refusing to delete unsafe path `{}`", path.display());
    }
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to stat `{}`", path.display()))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!("refusing to delete symlink `{}`", path.display());
    }
    let canonical_path = fs::canonicalize(path)
        .with_context(|| format!("failed to resolve `{}`", path.display()))?;
    let mut allowed = false;
    for root in allowed_roots {
        if !root.exists() {
            continue;
        }
        let canonical_root = fs::canonicalize(root)
            .with_context(|| format!("failed to resolve `{}`", root.display()))?;
        if canonical_path.starts_with(&canonical_root) {
            allowed = true;
            break;
        }
    }
    if !allowed {
        anyhow::bail!(
            "refusing to delete `{}` because it escapes the allowed cache roots",
            path.display()
        );
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to delete `{}`", path.display()))?;
    } else {
        fs::remove_file(path).with_context(|| format!("failed to delete `{}`", path.display()))?;
    }
    prune_empty_parents(path.parent(), prune_root)?;
    Ok(())
}

fn prune_empty_parents(mut current: Option<&Path>, stop_at: &Path) -> Result<()> {
    while let Some(path) = current {
        if path == stop_at || !path.exists() || !path.starts_with(stop_at) {
            break;
        }
        let mut children =
            fs::read_dir(path).with_context(|| format!("failed to read `{}`", path.display()))?;
        if children.next().is_some() {
            break;
        }
        fs::remove_dir(path).with_context(|| format!("failed to prune `{}`", path.display()))?;
        current = path.parent();
    }
    Ok(())
}

fn path_size_or_zero(path: &Path) -> Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to stat `{}`", path.display()))?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        return Ok(metadata.len());
    }
    if !metadata.is_dir() {
        return Ok(0);
    }
    let mut total = 0_u64;
    for child in
        fs::read_dir(path).with_context(|| format!("failed to read `{}`", path.display()))?
    {
        total = total.saturating_add(path_size_or_zero(&child?.path())?);
    }
    Ok(total)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
