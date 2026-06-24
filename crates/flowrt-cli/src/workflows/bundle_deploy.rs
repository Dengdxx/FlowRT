use super::*;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BundleManifest {
    pub(crate) schema_version: u32,
    pub(crate) flowrt_version: String,
    pub(crate) package: String,
    pub(crate) profile: Option<String>,
    #[serde(default = "default_bundle_artifact_mode")]
    pub(crate) artifact_mode: String,
    #[serde(default)]
    pub(crate) temporary_overlay: bool,
    #[serde(default)]
    pub(crate) test_only: bool,
    pub(crate) target: String,
    pub(crate) platform: Option<String>,
    pub(crate) build_mode: BuildMode,
    pub(crate) created_unix_ms: u64,
    pub(crate) entry: String,
    #[serde(default)]
    pub(crate) executables: Vec<BundleExecutable>,
    #[serde(default)]
    pub(crate) external_processes: Vec<BundleExternalProcess>,
    #[serde(default)]
    pub(crate) resource_providers: Vec<BundleResourceProvider>,
    #[serde(default)]
    pub(crate) runtime_dependencies: Vec<BundleRuntimeDependency>,
    #[serde(default)]
    pub(crate) artifacts: Vec<BundleArtifact>,
}

pub(crate) fn default_bundle_artifact_mode() -> String {
    "strict".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BundleExecutable {
    pub(crate) kind: String,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BundleExternalProcess {
    pub(crate) process: String,
    pub(crate) package: String,
    pub(crate) executable: String,
    pub(crate) path: PathBuf,
    #[serde(default)]
    pub(crate) platform: Option<String>,
    #[serde(default)]
    pub(crate) supported_platforms: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BundleResourceProvider {
    pub(crate) graph: String,
    pub(crate) name: String,
    pub(crate) scope: String,
    #[serde(default)]
    pub(crate) target: Option<String>,
    #[serde(default)]
    pub(crate) process: Option<String>,
    #[serde(default)]
    pub(crate) external_package: Option<String>,
    #[serde(default)]
    pub(crate) capabilities: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BundleRuntimeDependency {
    pub(crate) name: String,
    pub(crate) target: String,
    pub(crate) platform: String,
    pub(crate) version: String,
    pub(crate) policy: String,
    pub(crate) path: PathBuf,
    pub(crate) sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BundleArtifact {
    pub(crate) kind: String,
    pub(crate) target: String,
    pub(crate) platform: Option<String>,
    pub(crate) path: PathBuf,
    pub(crate) sha256: String,
}

pub(crate) struct LoadedBundleManifest {
    pub(crate) manifest: BundleManifest,
    pub(crate) version_warning: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct DeployArtifactSelection {
    pub(crate) count: usize,
    pub(crate) platforms: Vec<String>,
}

pub(crate) struct DeployOptions<'a> {
    pub(crate) bundle: &'a Path,
    pub(crate) host: &'a str,
    pub(crate) target: &'a str,
    pub(crate) remote_dir: &'a str,
    pub(crate) dry_run: bool,
    pub(crate) allow_island: bool,
    pub(crate) activate: bool,
    pub(crate) start: bool,
}

pub(crate) struct BundleExecutablePlan {
    pub(crate) kind: String,
    pub(crate) source: PathBuf,
    pub(crate) target: String,
    pub(crate) platform: Option<String>,
    pub(crate) dest: PathBuf,
    pub(crate) source_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FlowrtReleaseVersion {
    pub(crate) major: u64,
    pub(crate) minor: u64,
    pub(crate) patch: u64,
}

pub(crate) fn contract_artifact_mode_name(contract: &ContractIr) -> &'static str {
    if contract.artifact.mode == GraphMode::Island
        || contract
            .profiles
            .iter()
            .any(|profile| profile.mode == GraphMode::Island)
    {
        "island"
    } else {
        "strict"
    }
}

pub(crate) fn ensure_artifact_allowed(
    mode: &str,
    temporary_overlay: bool,
    test_only: bool,
    allow_island: bool,
    action: &str,
) -> Result<()> {
    if temporary_overlay || test_only {
        if allow_island {
            return Ok(());
        }
        let reason = if temporary_overlay {
            "temporary overlay"
        } else {
            "test-only"
        };
        anyhow::bail!(
            "refusing to {action} {reason} FlowRT artifact by default; pass `--allow-island` only for development, test, or migration scaffolds"
        );
    }
    match mode {
        "strict" => Ok(()),
        "island" if allow_island => Ok(()),
        "island" => anyhow::bail!(
            "refusing to {action} island FlowRT artifact by default; pass `--allow-island` only for development, test, or migration scaffolds"
        ),
        other => anyhow::bail!("unsupported FlowRT artifact mode `{other}`"),
    }
}

pub(crate) fn bundle_workspace(
    rsdl: &Path,
    contract: &ContractIr,
    out_dir: &Path,
    output: &Path,
    requested_build_mode: Option<BuildMode>,
    allow_island: bool,
) -> Result<String> {
    let artifact_mode = contract_artifact_mode_name(contract);
    ensure_artifact_allowed(
        artifact_mode,
        contract.artifact.temporary_overlay.is_some(),
        contract.artifact.test_only,
        allow_island,
        "bundle",
    )?;
    let build_info = load_build_info(out_dir, requested_build_mode, true)?;
    let supervisor = executable_from_build_info(
        out_dir,
        build_info.executables.supervisor.as_ref(),
        "FlowRT supervisor",
        "flowrt build --launcher",
    )?;
    if !supervisor.exists() {
        anyhow::bail!(
            "FlowRT supervisor `{}` not found; run `flowrt build --launcher` first",
            supervisor.display()
        );
    }
    ensure_bundle_output_dir(output)?;
    let target_name = bundle_target_name_for_build(&build_info, contract);
    let target_platform = bundle_target_platform_for_build(&build_info, contract)?;

    copy_required_file(
        &prepared_contract_path(out_dir),
        &output.join("flowrt/contract/contract.ir.json"),
    )?;
    copy_required_file(
        &out_dir.join("selfdesc/selfdesc.json"),
        &output.join("flowrt/selfdesc/selfdesc.json"),
    )?;
    copy_required_file(
        &out_dir.join("launch/launch.json"),
        &output.join("flowrt/launch/launch.json"),
    )?;
    copy_required_file(
        &build_model::BuildInfo::path(out_dir),
        &output.join("flowrt/build/build-info.json"),
    )?;

    let mut executables = Vec::new();
    let mut artifacts = Vec::new();
    let mut runtime_dependencies = Vec::new();
    let mut strip_stats = BundleStripStats::default();
    for plan in bundle_executable_plans(
        &build_info,
        out_dir,
        &target_name,
        target_platform.as_deref(),
    )? {
        let dest_abs = output.join(&plan.dest);
        copy_required_file(&plan.source, &dest_abs)?;
        if let Some(expected_hash) = &plan.source_sha256 {
            let actual_hash = file_sha256(&plan.source)?;
            if actual_hash != *expected_hash {
                anyhow::bail!(
                    "build-info artifact `{}` sha256 mismatch before bundle: metadata has {}, actual is {}; run `{}` first",
                    plan.source.display(),
                    expected_hash,
                    actual_hash,
                    build_launcher_hint(plan.platform.as_deref())
                );
            }
        }
        strip_stats.record(strip_bundle_executable(&dest_abs)?);
        artifacts.push(BundleArtifact {
            kind: plan.kind.clone(),
            target: plan.target.clone(),
            platform: plan.platform.clone(),
            path: plan.dest.clone(),
            sha256: file_sha256(&dest_abs)?,
        });
        executables.push(BundleExecutable {
            kind: plan.kind,
            path: plan.dest,
        });
    }
    for dependency in &build_info.runtime_dependencies {
        if dependency.version != env!("CARGO_PKG_VERSION") {
            anyhow::bail!(
                "build-info runtime dependency `{}` was recorded for FlowRT {}, but this CLI is {}; run `{}` first",
                dependency.name,
                dependency.version,
                env!("CARGO_PKG_VERSION"),
                build_launcher_hint(Some(&dependency.platform))
            );
        }
        if dependency.policy != "bundle" {
            anyhow::bail!(
                "build-info runtime dependency `{}` uses unsupported bundle policy `{}`; run `{}` first",
                dependency.name,
                dependency.policy,
                build_launcher_hint(Some(&dependency.platform))
            );
        }
        let platform = TargetPlatform::parse_alias(&dependency.platform)
            .with_context(|| {
                format!(
                    "build-info runtime dependency `{}` declares unsupported platform `{}`",
                    dependency.name, dependency.platform
                )
            })?
            .as_str()
            .to_string();
        let source = if dependency.path.is_absolute() {
            dependency.path.clone()
        } else {
            out_dir.join(&dependency.path)
        };
        let actual_hash = file_sha256(&source)?;
        if actual_hash != dependency.sha256 {
            anyhow::bail!(
                "build-info runtime dependency `{}` sha256 mismatch before bundle: metadata has {}, actual is {}; run `{}` first",
                dependency.name,
                dependency.sha256,
                actual_hash,
                build_launcher_hint(Some(&platform))
            );
        }
        let file_name = dependency.path.file_name().with_context(|| {
            format!(
                "build-info runtime dependency `{}` path `{}` has no file name",
                dependency.name,
                dependency.path.display()
            )
        })?;
        let dest = PathBuf::from("runtime-deps")
            .join(&platform)
            .join(file_name);
        copy_required_file(&source, &output.join(&dest))?;
        let bundle_hash = file_sha256(&output.join(&dest))?;
        artifacts.push(BundleArtifact {
            kind: "runtime_dependency".to_string(),
            target: dependency.target.clone(),
            platform: Some(platform.clone()),
            path: dest.clone(),
            sha256: bundle_hash.clone(),
        });
        runtime_dependencies.push(BundleRuntimeDependency {
            name: dependency.name.clone(),
            target: dependency.target.clone(),
            platform,
            version: dependency.version.clone(),
            policy: dependency.policy.clone(),
            path: dest,
            sha256: bundle_hash,
        });
    }

    let project_root = project_root_for_rsdl(rsdl);
    let mut external_processes = Vec::new();
    for graph in &contract.graphs {
        for external in &graph.external_processes {
            let package_root = resolve_external_package_root(&project_root, external)?;
            let manifest = load_external_manifest(&package_root)?;
            validate_external_manifest(&package_root, &manifest)?;
            let executable_metadata = select_external_executable_metadata(&manifest, external)?;
            let supported_platforms = canonical_external_platforms(&executable_metadata.platforms);
            if let Some(platform) = &target_platform {
                if !supported_platforms
                    .iter()
                    .any(|candidate| candidate == platform)
                {
                    anyhow::bail!(
                        "external package `{}` executable `{}` does not support target platform `{}`",
                        external.package,
                        external.executable,
                        platform
                    );
                }
            }
            let dest = PathBuf::from("external").join(&external.package);
            copy_dir_recursive(&package_root, &output.join(&dest))?;
            let artifact_path = dest.join(&external.executable);
            artifacts.push(BundleArtifact {
                kind: "external_process".to_string(),
                target: target_name.clone(),
                platform: target_platform.clone(),
                path: artifact_path.clone(),
                sha256: file_sha256(&output.join(&artifact_path))?,
            });
            external_processes.push(BundleExternalProcess {
                process: external.process.clone(),
                package: external.package.clone(),
                executable: external.executable.clone(),
                path: dest,
                platform: target_platform.clone(),
                supported_platforms,
            });
        }
    }

    let entry = executables
        .iter()
        .find(|executable| executable.kind == "supervisor")
        .map(|executable| executable.path.clone())
        .context("internal error: bundle entry supervisor executable was not copied")?;
    let resource_providers = bundle_resource_provider_closure(contract);
    let manifest = BundleManifest {
        schema_version: 2,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        package: contract.package.name.clone(),
        profile: build_info.rsdl_profile,
        artifact_mode: artifact_mode.to_string(),
        temporary_overlay: contract.artifact.temporary_overlay.is_some(),
        test_only: contract.artifact.test_only,
        target: target_name,
        platform: target_platform,
        build_mode: build_info.build_mode,
        created_unix_ms: current_unix_ms(),
        entry: entry.to_string_lossy().into_owned(),
        executables,
        external_processes,
        resource_providers,
        runtime_dependencies,
        artifacts,
    };
    let mut manifest_toml = toml::to_string_pretty(&manifest)?;
    manifest_toml.push('\n');
    fs::write(output.join("bundle.toml"), manifest_toml)
        .with_context(|| format!("failed to write `{}`", output.join("bundle.toml").display()))?;

    Ok(format!(
        "created FlowRT bundle: {} entry={} external_packages={} stripped_executables={} strip_warnings={}",
        output.display(),
        manifest.entry,
        manifest.external_processes.len(),
        strip_stats.stripped,
        strip_stats.warnings
    ))
}

pub(crate) fn bundle_executable_plans(
    build_info: &build_model::BuildInfo,
    out_dir: &Path,
    default_target: &str,
    default_platform: Option<&str>,
) -> Result<Vec<BundleExecutablePlan>> {
    let entries = [
        ("supervisor", build_info.executables.supervisor.as_ref()),
        ("rust_app", build_info.executables.rust_app.as_ref()),
        ("cpp_app", build_info.executables.cpp_app.as_ref()),
        ("ros2_bridge", build_info.executables.ros2_bridge.as_ref()),
    ];
    let mut plans = Vec::new();
    let has_artifact_facts = !build_info.artifacts.is_empty();
    for (kind, relative) in entries {
        let Some(relative) = relative else {
            continue;
        };
        ensure_safe_relative_path(relative)?;
        let source = out_dir.join(relative);
        if !source.exists() {
            anyhow::bail!(
                "build-info records {kind} executable `{}`, but it does not exist; run `{}` first",
                source.display(),
                build_launcher_hint(default_platform)
            );
        }
        let artifact = if has_artifact_facts {
            Some(bundle_build_artifact_for_executable(
                build_info, kind, relative,
            )?)
        } else {
            None
        };
        let (target, platform, source_sha256) = if let Some(artifact) = artifact {
            ensure_safe_relative_path(&artifact.path)?;
            if artifact.path != *relative {
                anyhow::bail!(
                    "build-info executable `{}` points to `{}`, but artifact metadata points to `{}`; run `{}` first",
                    kind,
                    relative.display(),
                    artifact.path.display(),
                    build_launcher_hint(artifact.platform.as_deref().or(default_platform))
                );
            }
            validate_build_artifact_target(kind, artifact, default_target, default_platform)?;
            (
                artifact.target.clone(),
                canonical_optional_platform(artifact.platform.as_deref())?,
                Some(artifact.sha256.clone()),
            )
        } else {
            (
                default_target.to_string(),
                canonical_optional_platform(default_platform)?,
                None,
            )
        };
        let dest = bundle_binary_dest(&source, platform.as_deref())?;
        plans.push(BundleExecutablePlan {
            kind: kind.to_string(),
            source,
            target,
            platform,
            dest,
            source_sha256,
        });
    }
    Ok(plans)
}

pub(crate) fn bundle_resource_provider_closure(
    contract: &ContractIr,
) -> Vec<BundleResourceProvider> {
    contract
        .graphs
        .iter()
        .flat_map(|graph| {
            graph
                .resource_providers
                .iter()
                .map(move |provider| BundleResourceProvider {
                    graph: graph.name.clone(),
                    name: provider.name.clone(),
                    scope: resource_provider_scope_name(provider.scope).to_string(),
                    target: provider.target.as_ref().map(|target| target.name.clone()),
                    process: provider.process.clone(),
                    external_package: provider.external_package.clone(),
                    capabilities: provider
                        .capabilities
                        .iter()
                        .map(|capability| capability.0.clone())
                        .collect(),
                })
        })
        .collect()
}

pub(crate) fn resource_provider_scope_name(
    scope: flowrt_ir::ResourceProviderScope,
) -> &'static str {
    match scope {
        flowrt_ir::ResourceProviderScope::Target => "target",
        flowrt_ir::ResourceProviderScope::Process => "process",
        flowrt_ir::ResourceProviderScope::ExternalPackage => "external_package",
    }
}

pub(crate) fn bundle_build_artifact_for_executable<'a>(
    build_info: &'a build_model::BuildInfo,
    kind: &str,
    relative: &Path,
) -> Result<&'a build_model::BuildArtifactInfo> {
    let matches = build_info
        .artifacts
        .iter()
        .filter(|artifact| artifact.kind == kind)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [artifact] => Ok(*artifact),
        [] => anyhow::bail!(
            "build-info records {kind} executable `{}`, but artifact metadata is missing; run `{}` first",
            relative.display(),
            build_launcher_hint(build_info.platform.as_deref())
        ),
        _ => anyhow::bail!(
            "build-info records multiple {kind} artifacts; run `{}` first",
            build_launcher_hint(build_info.platform.as_deref())
        ),
    }
}

pub(crate) fn validate_build_artifact_target(
    kind: &str,
    artifact: &build_model::BuildArtifactInfo,
    expected_target: &str,
    expected_platform: Option<&str>,
) -> Result<()> {
    if artifact.target != expected_target {
        anyhow::bail!(
            "build-info {kind} artifact target `{}` does not match Contract IR target `{expected_target}`; run `{}` first",
            artifact.target,
            build_launcher_hint(artifact.platform.as_deref().or(expected_platform))
        );
    }
    let expected_platform = canonical_optional_platform(expected_platform)?;
    let artifact_platform = canonical_optional_platform(artifact.platform.as_deref())?;
    if artifact_platform != expected_platform {
        anyhow::bail!(
            "build-info {kind} artifact platform {:?} does not match Contract IR platform {:?}; run `{}` first",
            artifact_platform,
            expected_platform,
            build_launcher_hint(
                expected_platform
                    .as_deref()
                    .or(artifact.platform.as_deref())
            )
        );
    }
    Ok(())
}

pub(crate) fn bundle_binary_dest(source: &Path, platform: Option<&str>) -> Result<PathBuf> {
    let file_name = source.file_name().with_context(|| {
        format!(
            "failed to determine executable file name for `{}`",
            source.display()
        )
    })?;
    let mut dest = PathBuf::from("bin");
    if let Some(platform) = platform {
        dest.push(platform);
    }
    dest.push(file_name);
    Ok(dest)
}

pub(crate) fn canonical_optional_platform(platform: Option<&str>) -> Result<Option<String>> {
    platform
        .map(|platform| {
            TargetPlatform::parse_alias(platform)
                .map(|value| value.as_str().to_string())
                .with_context(|| format!("unsupported target platform `{platform}`"))
        })
        .transpose()
}

#[derive(Default)]
pub(crate) struct BundleStripStats {
    pub(crate) stripped: usize,
    pub(crate) warnings: usize,
}

impl BundleStripStats {
    pub(crate) fn record(&mut self, outcome: BundleStripOutcome) {
        match outcome {
            BundleStripOutcome::Stripped => self.stripped += 1,
            BundleStripOutcome::Skipped | BundleStripOutcome::Warning => {
                if outcome == BundleStripOutcome::Warning {
                    self.warnings += 1;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BundleStripOutcome {
    Stripped,
    Skipped,
    Warning,
}

pub(crate) fn strip_bundle_executable(path: &Path) -> Result<BundleStripOutcome> {
    if !is_elf_file(path)? {
        return Ok(BundleStripOutcome::Skipped);
    }
    let strip = env::var_os("FLOWRT_STRIP").unwrap_or_else(|| OsStr::new("strip").to_os_string());
    let output = match ProcessCommand::new(&strip)
        .arg("--strip-unneeded")
        .arg(path)
        .output()
    {
        Ok(output) => output,
        Err(_) => return Ok(BundleStripOutcome::Warning),
    };
    if output.status.success() {
        Ok(BundleStripOutcome::Stripped)
    } else {
        Ok(BundleStripOutcome::Warning)
    }
}

pub(crate) fn is_elf_file(path: &Path) -> Result<bool> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open `{}`", path.display()))?;
    let mut magic = [0u8; 4];
    let read = std::io::Read::read(&mut file, &mut magic)
        .with_context(|| format!("failed to read `{}`", path.display()))?;
    Ok(read == magic.len() && magic == [0x7f, b'E', b'L', b'F'])
}

pub(crate) fn ensure_bundle_output_dir(output: &Path) -> Result<()> {
    if output.exists() {
        if !output.is_dir() {
            anyhow::bail!(
                "bundle output `{}` exists and is not a directory",
                output.display()
            );
        }
        if fs::read_dir(output)
            .with_context(|| format!("failed to read `{}`", output.display()))?
            .next()
            .is_some()
        {
            anyhow::bail!(
                "bundle output directory `{}` is not empty",
                output.display()
            );
        }
    }
    fs::create_dir_all(output)
        .with_context(|| format!("failed to create bundle output `{}`", output.display()))
}

pub(crate) fn project_root_for_rsdl(rsdl: &Path) -> PathBuf {
    let rsdl_dir = rsdl.parent().unwrap_or_else(|| Path::new("."));
    if rsdl_dir.file_name() == Some(OsStr::new("rsdl")) {
        rsdl_dir.parent().unwrap_or(rsdl_dir).to_path_buf()
    } else {
        rsdl_dir.to_path_buf()
    }
}

pub(crate) fn resolve_external_package_root(
    project_root: &Path,
    external: &flowrt_ir::ExternalProcessIr,
) -> Result<PathBuf> {
    let mut roots = Vec::new();
    if let Some(paths) = env::var_os("FLOWRT_EXTERNAL_PATH") {
        for entry in env::split_paths(&paths) {
            push_external_search_entry(&mut roots, entry, &external.package);
        }
    }
    push_unique_external_path(
        &mut roots,
        PathBuf::from("/opt/flowrt/external").join(&external.package),
    );
    push_unique_external_path(
        &mut roots,
        project_root.join("external").join(&external.package),
    );

    let mut searched = Vec::new();
    for root in roots {
        let manifest_path = root.join("flowrt-external.toml");
        let executable_path = root.join(&external.executable);
        searched.push(root.clone());
        if !manifest_path.exists() || !executable_path.exists() {
            continue;
        }
        let manifest = load_external_manifest(&root)?;
        if manifest.package.name == external.package {
            return Ok(root);
        }
    }

    anyhow::bail!(
        "external package `{}` executable `{}` was not found for bundle; searched package roots: {}",
        external.package,
        external.executable,
        searched
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(crate) fn select_external_executable_metadata<'a>(
    manifest: &'a ExternalPackageManifest,
    external: &flowrt_ir::ExternalProcessIr,
) -> Result<&'a ExternalExecutableMetadata> {
    manifest
        .executable
        .iter()
        .find(|executable| executable.path.as_path() == Path::new(&external.executable))
        .or_else(|| {
            manifest
                .executable
                .iter()
                .find(|executable| executable.name == external.executable)
        })
        .with_context(|| {
            format!(
                "external package `{}` manifest does not describe executable `{}`",
                external.package, external.executable
            )
        })
}

pub(crate) fn push_external_search_entry(roots: &mut Vec<PathBuf>, entry: PathBuf, package: &str) {
    push_unique_external_path(roots, entry.clone());
    push_unique_external_path(roots, entry.join(package));
}

pub(crate) fn push_unique_external_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

pub(crate) fn copy_required_file(source: &Path, dest: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("failed to inspect bundle file `{}`", source.display()))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "bundle source `{}` is a symbolic link; symlinks are not allowed",
            source.display()
        );
    }
    if !metadata.is_file() {
        anyhow::bail!("required bundle file `{}` does not exist", source.display());
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    fs::copy(source, dest).with_context(|| {
        format!(
            "failed to copy `{}` to `{}`",
            source.display(),
            dest.display()
        )
    })?;
    Ok(())
}

pub(crate) fn file_sha256(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open `{}`", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)
            .with_context(|| format!("failed to read `{}`", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

pub(crate) fn copy_dir_recursive(source: &Path, dest: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("failed to inspect bundle directory `{}`", source.display()))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "bundle source `{}` is a symbolic link; symlinks are not allowed",
            source.display()
        );
    }
    if !metadata.is_dir() {
        anyhow::bail!(
            "required bundle directory `{}` does not exist",
            source.display()
        );
    }
    fs::create_dir_all(dest).with_context(|| format!("failed to create `{}`", dest.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read directory `{}`", source.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to read `{}` entry", source.display()))?;
        let path = entry.path();
        let target = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect bundle source `{}`", path.display()))?;
        if file_type.is_symlink() {
            anyhow::bail!(
                "bundle source `{}` is a symbolic link; symlinks are not allowed",
                path.display()
            );
        } else if file_type.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else if file_type.is_file() {
            copy_required_file(&path, &target)?;
        }
    }
    Ok(())
}

pub(crate) fn bundle_target_name(contract: &ContractIr) -> String {
    contract
        .deployments
        .first()
        .map(|deployment| deployment.target.name.clone())
        .or_else(|| contract.targets.first().map(|target| target.name.clone()))
        .unwrap_or_else(|| "default".to_string())
}

pub(crate) fn bundle_target_platform(contract: &ContractIr) -> Option<String> {
    let target_name = bundle_target_name(contract);
    contract
        .targets
        .iter()
        .find(|target| target.name == target_name)
        .and_then(|target| {
            target
                .platform
                .map(|platform| platform.as_str().to_string())
        })
}

pub(crate) fn bundle_target_name_for_build(
    build_info: &build_model::BuildInfo,
    contract: &ContractIr,
) -> String {
    build_info
        .target
        .clone()
        .unwrap_or_else(|| bundle_target_name(contract))
}

pub(crate) fn bundle_target_platform_for_build(
    build_info: &build_model::BuildInfo,
    contract: &ContractIr,
) -> Result<Option<String>> {
    if build_info.platform.is_some() {
        return canonical_optional_platform(build_info.platform.as_deref());
    }
    let contract_platform = bundle_target_platform(contract);
    canonical_optional_platform(contract_platform.as_deref())
}

pub(crate) fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn deploy_bundle(
    bundle: &Path,
    host: &str,
    target: &str,
    remote_dir: &str,
    dry_run: bool,
    allow_island: bool,
) -> Result<String> {
    deploy_bundle_with_options(DeployOptions {
        bundle,
        host,
        target,
        remote_dir,
        dry_run,
        allow_island,
        activate: false,
        start: false,
    })
}

pub(crate) fn deploy_bundle_with_options(options: DeployOptions<'_>) -> Result<String> {
    let bundle = options.bundle;
    let host = options.host;
    let target = options.target;
    let remote_dir = options.remote_dir;
    let dry_run = options.dry_run;
    let allow_island = options.allow_island;
    let activate = options.activate || options.start;
    let start = options.start;
    validate_deploy_host(host)?;
    validate_deploy_remote_dir(remote_dir)?;
    let loaded = load_bundle_manifest(bundle)?;
    let manifest = loaded.manifest;
    ensure_artifact_allowed(
        &manifest.artifact_mode,
        manifest.temporary_overlay,
        manifest.test_only,
        allow_island,
        "deploy",
    )?;
    let selected_artifacts = select_deploy_artifacts(bundle, &manifest, target)?;
    let managed_plan = DeployManagedPlan::new(&manifest, target, remote_dir);
    let mut warnings = Vec::new();
    if let Some(version_warning) = loaded.version_warning {
        warnings.push(version_warning);
    }
    let warning = deploy_warning_suffix(&warnings);
    if dry_run {
        let managed_suffix = deploy_managed_suffix(&managed_plan, activate, start);
        return Ok(format!(
            "deploy plan bundle={} host={} target={} remote_dir={} entry={}{}{}{}",
            bundle.display(),
            host,
            target,
            remote_dir,
            manifest.entry,
            deploy_artifact_suffix(&selected_artifacts),
            managed_suffix,
            warning
        ));
    }

    let version_check = ProcessCommand::new("ssh")
        .arg("--")
        .arg(host)
        .arg("flowrt --version")
        .output()
        .with_context(|| format!("failed to spawn ssh for host `{host}`"))?;
    let remote_warning = validate_remote_flowrt_version_check(
        version_check.status.success(),
        &String::from_utf8_lossy(&version_check.stdout),
        &String::from_utf8_lossy(&version_check.stderr),
        &manifest.flowrt_version,
    )?;
    if let Some(remote_warning) = remote_warning {
        warnings.push(remote_warning);
    }
    let warning = deploy_warning_suffix(&warnings);
    validate_remote_deploy_probe(host, &manifest, target, &selected_artifacts.platforms)?;

    let remote = format!("{host}:{}", managed_plan.incoming);
    prepare_remote_incoming(host, remote_dir, &managed_plan.incoming)?;
    let upload = ProcessCommand::new("scp")
        .arg("-r")
        .arg("--")
        .arg(bundle)
        .arg(&remote)
        .status()
        .with_context(|| format!("failed to spawn scp for host `{host}`"))?;
    if !upload.success() {
        anyhow::bail!("bundle upload failed with status {upload}");
    }
    install_remote_managed_release(host, &managed_plan, target, activate)?;
    if start {
        start_remote_managed_release(host, remote_dir)?;
    }

    let managed_suffix = deploy_managed_suffix(&managed_plan, activate, start);
    Ok(format!(
        "deployed FlowRT bundle {} to {}{}{}",
        bundle.display(),
        remote,
        managed_suffix,
        warning
    ))
}

#[derive(Debug, Clone)]
pub(crate) struct DeployManagedPlan {
    pub(crate) release_id: String,
    pub(crate) incoming: String,
}

impl DeployManagedPlan {
    pub(crate) fn new(manifest: &BundleManifest, target: &str, remote_dir: &str) -> Self {
        let release_id = managed_release_id(manifest, target);
        Self {
            incoming: format!("{remote_dir}/incoming/{release_id}"),
            release_id,
        }
    }
}

pub(crate) fn deploy_managed_suffix(
    plan: &DeployManagedPlan,
    activate: bool,
    start: bool,
) -> String {
    let actions = match (activate, start) {
        (false, false) => "install",
        (true, false) => "install,activate",
        (true, true) => "install,activate,start",
        (false, true) => unreachable!("start implies activate"),
    };
    format!(
        " release={} incoming={} managed={actions}",
        plan.release_id, plan.incoming
    )
}

fn prepare_remote_incoming(host: &str, remote_dir: &str, incoming: &str) -> Result<()> {
    let incoming_parent = format!("{remote_dir}/incoming");
    for command in [
        vec!["rm", "-rf", incoming],
        vec!["mkdir", "-p", incoming_parent.as_str()],
    ] {
        let output = ProcessCommand::new("ssh")
            .arg("--")
            .arg(host)
            .args(command)
            .output()
            .with_context(|| format!("failed to prepare remote incoming dir on `{host}`"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.trim();
            anyhow::bail!(
                "remote incoming preparation failed: {}",
                if detail.is_empty() {
                    output.status.to_string()
                } else {
                    detail.to_string()
                }
            );
        }
    }
    Ok(())
}

fn install_remote_managed_release(
    host: &str,
    plan: &DeployManagedPlan,
    target: &str,
    activate: bool,
) -> Result<()> {
    let mut args = vec![
        "flowrt",
        "managed",
        "install",
        plan.incoming.as_str(),
        "--remote-dir",
    ];
    let remote_dir = plan
        .incoming
        .strip_suffix(&format!("/incoming/{}", plan.release_id))
        .unwrap_or("");
    args.push(remote_dir);
    args.push("--target");
    args.push(target);
    if activate {
        args.push("--activate");
    }
    let output = ProcessCommand::new("ssh")
        .arg("--")
        .arg(host)
        .args(args)
        .output()
        .with_context(|| format!("failed to install remote managed release on `{host}`"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        anyhow::bail!(
            "remote managed install failed: {}",
            if detail.is_empty() {
                output.status.to_string()
            } else {
                detail.to_string()
            }
        );
    }
    Ok(())
}

fn start_remote_managed_release(host: &str, remote_dir: &str) -> Result<()> {
    let output = ProcessCommand::new("ssh")
        .arg("--")
        .arg(host)
        .arg("flowrt")
        .arg("managed")
        .arg("start")
        .arg("--remote-dir")
        .arg(remote_dir)
        .output()
        .with_context(|| format!("failed to start remote managed release on `{host}`"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        anyhow::bail!(
            "remote managed start failed: {}",
            if detail.is_empty() {
                output.status.to_string()
            } else {
                detail.to_string()
            }
        );
    }
    Ok(())
}

pub(crate) fn deploy_warning_suffix(warnings: &[String]) -> String {
    if warnings.is_empty() {
        String::new()
    } else {
        format!(" warning={}", warnings.join("; "))
    }
}

pub(crate) fn deploy_artifact_suffix(selection: &DeployArtifactSelection) -> String {
    if selection.count == 0 {
        String::new()
    } else {
        format!(
            " artifacts={} platforms=[{}]",
            selection.count,
            selection.platforms.join(",")
        )
    }
}

pub(crate) fn select_deploy_artifacts(
    bundle: &Path,
    manifest: &BundleManifest,
    target: &str,
) -> Result<DeployArtifactSelection> {
    ensure_safe_relative_path(Path::new(&manifest.entry))?;
    if manifest.schema_version < 2 || manifest.artifacts.is_empty() {
        if manifest.target != target {
            anyhow::bail!(
                "bundle target `{}` does not match requested target `{target}`",
                manifest.target
            );
        }
        return Ok(DeployArtifactSelection {
            count: 0,
            platforms: Vec::new(),
        });
    }

    let mut platforms = Vec::new();
    let mut count = 0usize;
    let mut entry_count = 0usize;
    for artifact in manifest
        .artifacts
        .iter()
        .filter(|artifact| artifact.target == target)
    {
        if artifact.kind != "runtime_dependency" {
            validate_bundle_artifact(bundle, manifest, artifact)?;
        }
        count += 1;
        if is_deploy_entry_artifact_kind(&artifact.kind) {
            let canonical = canonical_bundle_artifact_platform(artifact, "bundle entry")?;
            if !platforms.iter().any(|existing| existing == &canonical) {
                platforms.push(canonical);
            }
            entry_count += 1;
        }
    }
    if count == 0 {
        anyhow::bail!("bundle does not contain deployable artifacts for target `{target}`");
    }
    if entry_count == 0 {
        anyhow::bail!(
            "bundle does not contain entry supervisor artifact for target `{target}`; run `{}` before bundling again",
            build_launcher_hint(manifest.platform.as_deref())
        );
    }
    platforms.sort();
    validate_deploy_resource_provider_closure(manifest)?;
    validate_deploy_external_package_closure(bundle, manifest, target, &platforms)?;
    validate_deploy_runtime_dependency_closure(bundle, manifest, target, &platforms)?;
    Ok(DeployArtifactSelection { count, platforms })
}

pub(crate) fn is_deploy_entry_artifact_kind(kind: &str) -> bool {
    kind == "supervisor"
}

pub(crate) fn validate_deploy_resource_provider_closure(manifest: &BundleManifest) -> Result<()> {
    for provider in &manifest.resource_providers {
        if provider.scope != "external_package" {
            continue;
        }
        let package = provider.external_package.as_deref().with_context(|| {
            format!(
                "resource provider `{}` graph `{}` uses external_package scope but does not declare external_package",
                provider.name, provider.graph
            )
        })?;
        if !manifest
            .external_processes
            .iter()
            .any(|external| external.package == package)
        {
            anyhow::bail!(
                "resource provider `{}` graph `{}` references external package `{package}`, but bundle manifest has no external package closure for it; rebuild the bundle after adding the package artifact",
                provider.name,
                provider.graph
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_deploy_external_package_closure(
    bundle: &Path,
    manifest: &BundleManifest,
    target: &str,
    selected_platforms: &[String],
) -> Result<()> {
    if selected_platforms.is_empty() {
        return Ok(());
    }
    for external in &manifest.external_processes {
        let expected_path = external.path.join(&external.executable);
        ensure_safe_relative_path(&expected_path)?;
        let supported_platforms = canonical_external_platforms(&external.supported_platforms);
        let declared_platform = canonical_optional_platform(external.platform.as_deref())?;
        let expected_platforms = declared_platform
            .map(|platform| vec![platform])
            .unwrap_or_else(|| selected_platforms.to_vec());
        for expected_platform in expected_platforms {
            if !selected_platforms
                .iter()
                .any(|platform| platform == &expected_platform)
            {
                anyhow::bail!(
                    "external package `{}` executable `{}` platform mismatch: target `{target}` selected platforms [{}], package metadata declares `{expected_platform}`; run `{}` before bundling again",
                    external.package,
                    external.executable,
                    selected_platforms.join(","),
                    build_launcher_hint(Some(&expected_platform))
                );
            }
            if !supported_platforms.is_empty()
                && !supported_platforms
                    .iter()
                    .any(|platform| platform == &expected_platform)
            {
                anyhow::bail!(
                    "external package `{}` executable `{}` does not support selected platform `{expected_platform}`; rebuild or install a package artifact for that platform",
                    external.package,
                    external.executable
                );
            }
            let mut mismatched_platform = None;
            let mut found = false;
            for artifact in manifest.artifacts.iter().filter(|artifact| {
                artifact.target == target
                    && artifact.kind == "external_process"
                    && artifact.path == expected_path
            }) {
                let artifact_platform =
                    canonical_bundle_artifact_platform(artifact, "external package")?;
                if artifact_platform == expected_platform {
                    validate_bundle_artifact(bundle, manifest, artifact)?;
                    found = true;
                } else {
                    mismatched_platform = Some(artifact_platform);
                }
            }
            if !found {
                if let Some(actual_platform) = mismatched_platform {
                    anyhow::bail!(
                        "external package `{}` executable `{}` platform mismatch: target `{target}` needs `{expected_platform}`, artifact declares `{actual_platform}`; run `{}` before bundling again",
                        external.package,
                        external.executable,
                        build_launcher_hint(Some(&expected_platform))
                    );
                }
                anyhow::bail!(
                    "external package `{}` executable `{}` missing artifact for target `{target}` platform `{expected_platform}`; run `{}` before bundling again",
                    external.package,
                    external.executable,
                    build_launcher_hint(Some(&expected_platform))
                );
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_deploy_runtime_dependency_closure(
    bundle: &Path,
    manifest: &BundleManifest,
    target: &str,
    selected_platforms: &[String],
) -> Result<()> {
    for dependency in manifest
        .runtime_dependencies
        .iter()
        .filter(|dependency| dependency.target == target)
    {
        validate_bundle_runtime_dependency(bundle, manifest, dependency, selected_platforms)?;
    }
    Ok(())
}

pub(crate) fn validate_bundle_runtime_dependency(
    bundle: &Path,
    manifest: &BundleManifest,
    dependency: &BundleRuntimeDependency,
    selected_platforms: &[String],
) -> Result<()> {
    ensure_safe_relative_path(&dependency.path)?;
    if dependency.policy != "bundle" {
        anyhow::bail!(
            "runtime dependency `{}` uses unsupported deploy policy `{}`; configure a bundle runtime dependency or install matching remote runtime dependencies",
            dependency.name,
            dependency.policy
        );
    }
    let platform = TargetPlatform::parse_alias(&dependency.platform)
        .with_context(|| {
            format!(
                "runtime dependency `{}` declares unsupported platform `{}`",
                dependency.name, dependency.platform
            )
        })?
        .as_str()
        .to_string();
    if !selected_platforms.is_empty()
        && !selected_platforms
            .iter()
            .any(|selected| selected == &platform)
    {
        anyhow::bail!(
            "runtime dependency `{}` platform mismatch: target `{}` selected platforms [{}], dependency declares `{platform}`; run `{}` before bundling again",
            dependency.name,
            dependency.target,
            selected_platforms.join(","),
            build_doctor_hint(&platform)
        );
    }
    if dependency.version != manifest.flowrt_version {
        anyhow::bail!(
            "runtime dependency `{}` version mismatch: bundle FlowRT version is {}, dependency declares {}; install matching FlowRT target SDK or run `{}` before bundling again",
            dependency.name,
            manifest.flowrt_version,
            dependency.version,
            build_doctor_hint(&platform)
        );
    }
    let path = bundle.join(&dependency.path);
    let metadata = fs::symlink_metadata(&path).with_context(|| {
        format!(
            "runtime dependency `{}` artifact `{}` does not exist; run `{}` before bundling again",
            dependency.name,
            path.display(),
            build_doctor_hint(&platform)
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        anyhow::bail!(
            "runtime dependency `{}` artifact `{}` must be a regular file",
            dependency.name,
            dependency.path.display()
        );
    }
    let actual_hash = file_sha256(&path)?;
    if actual_hash != dependency.sha256 {
        anyhow::bail!(
            "runtime dependency `{}` sha256 mismatch: manifest has {}, actual is {}; run `{}` before retrying",
            dependency.name,
            dependency.sha256,
            actual_hash,
            build_doctor_hint(&platform)
        );
    }
    let has_artifact = manifest.artifacts.iter().any(|artifact| {
        artifact.kind == "runtime_dependency"
            && artifact.target == dependency.target
            && artifact.path == dependency.path
            && artifact.platform.as_deref() == Some(platform.as_str())
    });
    if !has_artifact {
        anyhow::bail!(
            "runtime dependency `{}` missing artifact closure for target `{}` platform `{platform}`; run `{}` before bundling again",
            dependency.name,
            dependency.target,
            build_doctor_hint(&platform)
        );
    }
    Ok(())
}

pub(crate) fn validate_bundle_artifact(
    bundle: &Path,
    manifest: &BundleManifest,
    artifact: &BundleArtifact,
) -> Result<()> {
    ensure_safe_relative_path(&artifact.path)?;
    let platform = artifact.platform.as_deref().with_context(|| {
        format!(
            "bundle artifact `{}` is missing platform metadata",
            artifact.path.display()
        )
    })?;
    let canonical_platform = TargetPlatform::parse_alias(platform).with_context(|| {
        format!(
            "bundle artifact `{}` declares unsupported platform `{platform}`",
            artifact.path.display()
        )
    })?;
    let canonical_platform = canonical_platform.as_str().to_string();
    if manifest.target == artifact.target {
        if let Some(manifest_platform) = &manifest.platform {
            let manifest_platform = TargetPlatform::parse_alias(manifest_platform)
                .map(|value| value.as_str().to_string())
                .with_context(|| {
                    format!(
                        "bundle target `{}` declares unsupported platform `{manifest_platform}`",
                        manifest.target
                    )
                })?;
            if manifest_platform != canonical_platform {
                anyhow::bail!(
                    "bundle artifact `{}` platform mismatch: target `{}` expects `{}`, artifact declares `{}`; run `{}` before bundling again",
                    artifact.path.display(),
                    artifact.target,
                    manifest_platform,
                    canonical_platform,
                    build_launcher_hint(Some(&manifest_platform))
                );
            }
        }
    }
    validate_bundle_artifact_path_platform(artifact, &canonical_platform)?;
    let path = bundle.join(&artifact.path);
    let metadata = fs::symlink_metadata(&path).with_context(|| {
        format!(
            "bundle artifact `{}` does not exist; run `{}` before bundling again",
            path.display(),
            build_launcher_hint(Some(&canonical_platform))
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        anyhow::bail!(
            "bundle artifact `{}` must be a regular file",
            artifact.path.display()
        );
    }
    let actual_hash = file_sha256(&path)?;
    if actual_hash != artifact.sha256 {
        anyhow::bail!(
            "bundle artifact `{}` sha256 mismatch: manifest has {}, actual is {}; run `{}` before bundling again",
            artifact.path.display(),
            artifact.sha256,
            actual_hash,
            build_launcher_hint(Some(&canonical_platform))
        );
    }
    Ok(())
}

pub(crate) fn canonical_bundle_artifact_platform(
    artifact: &BundleArtifact,
    context: &str,
) -> Result<String> {
    let platform = artifact.platform.as_deref().with_context(|| {
        format!(
            "{context} artifact `{}` is missing platform metadata",
            artifact.path.display()
        )
    })?;
    TargetPlatform::parse_alias(platform)
        .map(|platform| platform.as_str().to_string())
        .with_context(|| {
            format!(
                "{context} artifact `{}` declares unsupported platform `{platform}`",
                artifact.path.display()
            )
        })
}

pub(crate) fn validate_bundle_artifact_path_platform(
    artifact: &BundleArtifact,
    canonical_platform: &str,
) -> Result<()> {
    let mut components = artifact.path.components();
    if !matches!(components.next(), Some(Component::Normal(value)) if value == "bin") {
        return Ok(());
    }
    let Some(Component::Normal(platform_component)) = components.next() else {
        return Ok(());
    };
    let platform_component = platform_component.to_string_lossy();
    let Some(path_platform) = TargetPlatform::parse_alias(&platform_component) else {
        return Ok(());
    };
    let path_platform = path_platform.as_str();
    if path_platform != canonical_platform {
        anyhow::bail!(
            "bundle artifact `{}` platform mismatch: path uses `{path_platform}`, artifact declares `{canonical_platform}`; run `{}` before bundling again",
            artifact.path.display(),
            build_launcher_hint(Some(path_platform))
        );
    }
    Ok(())
}

pub(crate) fn build_launcher_hint(platform: Option<&str>) -> String {
    match platform {
        Some(platform) => format!("flowrt build --target {platform} --launcher"),
        None => "flowrt build --launcher".to_string(),
    }
}

pub(crate) fn validate_remote_flowrt_version_check(
    success: bool,
    stdout: &str,
    stderr: &str,
    bundle_version: &str,
) -> Result<Option<String>> {
    if !success {
        let stderr = stderr.trim();
        if stderr.is_empty() {
            anyhow::bail!("remote FlowRT version check failed");
        }
        anyhow::bail!("remote FlowRT version check failed: {stderr}");
    }

    let remote_version = parse_flowrt_version_output(stdout)?;
    remote_version_warning(remote_version, bundle_version)
}

pub(crate) fn parse_flowrt_version_output(output: &str) -> Result<&str> {
    output
        .split_whitespace()
        .find(|token| parse_flowrt_release_version(token).is_ok())
        .context("remote `flowrt --version` output did not contain a MAJOR.MINOR.PATCH version")
}

pub(crate) fn remote_version_warning(
    remote_version: &str,
    bundle_version: &str,
) -> Result<Option<String>> {
    if remote_version == bundle_version {
        return Ok(None);
    }
    let remote = parse_flowrt_release_version(remote_version)
        .with_context(|| format!("invalid remote FlowRT version `{remote_version}`"))?;
    let bundle = parse_flowrt_release_version(bundle_version)
        .with_context(|| format!("invalid FlowRT bundle version `{bundle_version}`"))?;
    if remote.major == bundle.major && remote.minor == bundle.minor {
        return Ok(Some(format!(
            "remote patch version {remote_version} differs from bundle {bundle_version}; deploy is allowed within the same major.minor release line"
        )));
    }
    anyhow::bail!(
        "incompatible remote FlowRT version: remote has FlowRT {remote_version}, but bundle was created with FlowRT {bundle_version}"
    );
}

pub(crate) fn deploy_probe(target_platform: &str) -> Result<String> {
    let target_platform = TargetPlatform::parse_alias(target_platform)
        .with_context(|| format!("unsupported target platform `{target_platform}`"))?
        .as_str()
        .to_string();
    let mut lines = vec![format!(
        "flowrt-deploy-probe platform={}",
        host_flowrt_platform().unwrap_or("unknown")
    )];
    if let Ok(runtime_dir) = cpp_runtime_dir_for_generated_build()
        && let Ok(sdk) = resolve_cpp_target_sdk_root(runtime_dir.as_deref(), &target_platform)
    {
        let manifest = sdk.root.join("flowrt-target-sdk.toml");
        if manifest.is_file() {
            lines.push(format!(
                "runtime_dependency name=flowrt-target-sdk version={} platform={} sha256={}",
                env!("CARGO_PKG_VERSION"),
                target_platform,
                file_sha256(&manifest)?
            ));
        }
    }
    Ok(lines.join("\n"))
}

pub(crate) fn validate_remote_deploy_probe(
    host: &str,
    manifest: &BundleManifest,
    target: &str,
    platforms: &[String],
) -> Result<()> {
    for platform in platforms {
        let output = ProcessCommand::new("ssh")
            .arg("--")
            .arg(host)
            .arg("flowrt")
            .arg("deploy-probe")
            .arg("--target-platform")
            .arg(platform)
            .output()
            .with_context(|| format!("failed to spawn ssh deploy probe for host `{host}`"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.trim();
            anyhow::bail!(
                "remote FlowRT deploy probe failed for target platform `{platform}`: {}; install matching FlowRT {} on the remote host, then run `{}` there",
                if detail.is_empty() {
                    output.status.to_string()
                } else {
                    detail.to_string()
                },
                manifest.flowrt_version,
                build_doctor_hint(platform)
            );
        }
        validate_remote_deploy_probe_output(
            &String::from_utf8_lossy(&output.stdout),
            manifest,
            target,
            std::slice::from_ref(platform),
        )?;
    }
    Ok(())
}

#[derive(Debug, Default)]
pub(crate) struct RemoteDeployProbe {
    pub(crate) platform: Option<String>,
    pub(crate) runtime_dependencies: Vec<RemoteRuntimeDependency>,
}

#[derive(Debug)]
pub(crate) struct RemoteRuntimeDependency {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) platform: String,
    pub(crate) sha256: String,
}

pub(crate) fn validate_remote_deploy_probe_output(
    output: &str,
    manifest: &BundleManifest,
    target: &str,
    expected_platforms: &[String],
) -> Result<()> {
    let probe = parse_remote_deploy_probe_output(output)?;
    if probe.platform.is_none() {
        anyhow::bail!(
            "remote deploy probe output did not include platform; install matching FlowRT {} on the remote host and run `{}` there",
            manifest.flowrt_version,
            expected_platforms
                .first()
                .map(|platform| build_doctor_hint(platform))
                .unwrap_or_else(|| "flowrt doctor --target <platform>".to_string())
        );
    }
    if let Some(expected_platform) = expected_platforms.first()
        && let Some(remote_platform) = &probe.platform
        && remote_platform != expected_platform
    {
        anyhow::bail!(
            "remote platform mismatch for target `{target}`: expected `{expected_platform}`, remote reports `{remote_platform}`; install the matching FlowRT package or run `{}` on the remote host",
            build_doctor_hint(expected_platform)
        );
    }
    for dependency in manifest.runtime_dependencies.iter().filter(|dependency| {
        dependency.target == target
            && expected_platforms
                .iter()
                .any(|platform| platform == &dependency.platform)
    }) {
        let Some(remote) = probe.runtime_dependencies.iter().find(|remote| {
            remote.name == dependency.name && remote.platform == dependency.platform
        }) else {
            anyhow::bail!(
                "remote runtime dependency `{}` for platform `{}` is missing; install matching FlowRT target SDK on the remote host and run `{}` there",
                dependency.name,
                dependency.platform,
                build_doctor_hint(&dependency.platform)
            );
        };
        if remote.version != dependency.version {
            anyhow::bail!(
                "remote runtime dependency `{}` version mismatch for platform `{}`: bundle expects {}, remote reports {}; install matching FlowRT target SDK and run `{}` there",
                dependency.name,
                dependency.platform,
                dependency.version,
                remote.version,
                build_doctor_hint(&dependency.platform)
            );
        }
        if remote.sha256 != dependency.sha256 {
            anyhow::bail!(
                "remote runtime dependency `{}` sha256 mismatch for platform `{}`: bundle expects {}, remote reports {}; install matching FlowRT target SDK and run `{}` there",
                dependency.name,
                dependency.platform,
                dependency.sha256,
                remote.sha256,
                build_doctor_hint(&dependency.platform)
            );
        }
    }
    Ok(())
}

pub(crate) fn parse_remote_deploy_probe_output(output: &str) -> Result<RemoteDeployProbe> {
    let mut probe = RemoteDeployProbe::default();
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let values = parse_probe_key_values(line.split_whitespace().skip(1));
        if line.starts_with("flowrt-deploy-probe ") {
            if let Some(platform) = values.get("platform") {
                let platform = TargetPlatform::parse_alias(platform)
                    .map(|platform| platform.as_str().to_string())
                    .unwrap_or_else(|| platform.clone());
                probe.platform = Some(platform);
            }
        } else if line.starts_with("runtime_dependency ") {
            let name = values
                .get("name")
                .context("remote runtime_dependency probe line is missing `name`")?
                .clone();
            let version = values
                .get("version")
                .context("remote runtime_dependency probe line is missing `version`")?
                .clone();
            let platform = values
                .get("platform")
                .context("remote runtime_dependency probe line is missing `platform`")?;
            let platform = TargetPlatform::parse_alias(platform)
                .map(|platform| platform.as_str().to_string())
                .unwrap_or_else(|| platform.clone());
            let sha256 = values
                .get("sha256")
                .context("remote runtime_dependency probe line is missing `sha256`")?
                .clone();
            probe.runtime_dependencies.push(RemoteRuntimeDependency {
                name,
                version,
                platform,
                sha256,
            });
        }
    }
    Ok(probe)
}

pub(crate) fn parse_probe_key_values<'a>(
    tokens: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, String> {
    tokens
        .into_iter()
        .filter_map(|token| {
            token
                .split_once('=')
                .map(|(key, value)| (key.to_string(), value.to_string()))
        })
        .collect()
}

pub(crate) fn validate_deploy_host(host: &str) -> Result<()> {
    if host.is_empty() {
        anyhow::bail!("deploy host must not be empty");
    }
    if host.starts_with('-') {
        anyhow::bail!("deploy host `{host}` is invalid: host must not start with `-`");
    }
    Ok(())
}

pub(crate) fn validate_deploy_remote_dir(remote_dir: &str) -> Result<()> {
    if remote_dir.trim().is_empty() {
        anyhow::bail!("deploy remote_dir must not be empty");
    }
    if !remote_dir.starts_with('/') {
        anyhow::bail!("deploy remote_dir `{remote_dir}` is invalid: path must be absolute");
    }
    if remote_dir
        .split('/')
        .any(|segment| segment == ".." || segment == ".")
    {
        anyhow::bail!(
            "deploy remote_dir `{remote_dir}` is invalid: `.` and `..` path segments are not allowed"
        );
    }
    if !remote_dir.bytes().all(|byte| {
        byte == b'/' || byte == b'.' || byte == b'_' || byte == b'-' || byte.is_ascii_alphanumeric()
    }) {
        anyhow::bail!(
            "deploy remote_dir `{remote_dir}` is invalid: only POSIX-safe characters [A-Za-z0-9._/-] are allowed"
        );
    }
    Ok(())
}

pub(crate) fn load_bundle_manifest(bundle: &Path) -> Result<LoadedBundleManifest> {
    let path = bundle.join("bundle.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("failed to read bundle manifest `{}`", path.display()))?;
    let manifest: BundleManifest = toml::from_str(&source)
        .with_context(|| format!("failed to parse bundle manifest `{}`", path.display()))?;
    if !matches!(manifest.schema_version, 1 | 2) {
        anyhow::bail!(
            "unsupported FlowRT bundle schema version {} in `{}`",
            manifest.schema_version,
            path.display()
        );
    }
    let version_warning =
        bundle_version_warning(&manifest.flowrt_version, env!("CARGO_PKG_VERSION"))?;
    Ok(LoadedBundleManifest {
        manifest,
        version_warning,
    })
}

pub(crate) fn bundle_version_warning(
    bundle_version: &str,
    cli_version: &str,
) -> Result<Option<String>> {
    if bundle_version == cli_version {
        return Ok(None);
    }
    let bundle = parse_flowrt_release_version(bundle_version)
        .with_context(|| format!("invalid FlowRT bundle version `{bundle_version}`"))?;
    let cli = parse_flowrt_release_version(cli_version)
        .with_context(|| format!("invalid FlowRT CLI version `{cli_version}`"))?;
    if bundle.major == cli.major && bundle.minor == cli.minor {
        return Ok(Some(format!(
            "bundle patch version {bundle_version} differs from CLI {cli_version}; deploy is allowed within the same major.minor release line"
        )));
    }
    anyhow::bail!(
        "incompatible FlowRT version: bundle was created with FlowRT {bundle_version}, but this CLI is {cli_version}"
    );
}

pub(crate) fn parse_flowrt_release_version(version: &str) -> Result<FlowrtReleaseVersion> {
    let mut parts = version.split('.');
    let major = parse_release_version_part(parts.next(), "major")?;
    let minor = parse_release_version_part(parts.next(), "minor")?;
    let patch = parse_release_version_part(parts.next(), "patch")?;
    if parts.next().is_some() {
        anyhow::bail!("expected MAJOR.MINOR.PATCH");
    }
    Ok(FlowrtReleaseVersion {
        major,
        minor,
        patch,
    })
}

pub(crate) fn parse_release_version_part(part: Option<&str>, name: &str) -> Result<u64> {
    let part = part.with_context(|| format!("missing {name} version part"))?;
    if part.is_empty() || !part.chars().all(|ch| ch.is_ascii_digit()) {
        anyhow::bail!("{name} version part `{part}` is not a non-negative integer");
    }
    part.parse::<u64>()
        .with_context(|| format!("failed to parse {name} version part `{part}`"))
}
