use super::*;

#[derive(Debug)]
pub(crate) struct WorkspaceLock {
    pub(crate) path: PathBuf,
    pub(crate) file: File,
}

impl WorkspaceLock {
    pub(crate) fn acquire(out_dir: &Path) -> Result<Self> {
        fs::create_dir_all(out_dir)
            .with_context(|| format!("failed to create `{}`", out_dir.display()))?;
        let path = out_dir.join(".flowrt.lock");
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .with_context(|| format!("failed to open lock `{}`", path.display()))?;
        if !try_lock_file(&file)? {
            anyhow::bail!(
                "FlowRT output directory `{}` is already in use by another flowrt command; retry after it finishes, or remove `{}` if no FlowRT command is running",
                out_dir.display(),
                path.display()
            )
        }
        file.set_len(0)
            .with_context(|| format!("failed to truncate lock `{}`", path.display()))?;
        writeln!(file, "pid={}", std::process::id())
            .with_context(|| format!("failed to write `{}`", path.display()))?;
        Ok(Self { path, file })
    }
}

impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = unlock_file(&self.file);
    }
}

pub(crate) fn try_lock_file(file: &File) -> Result<bool> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result == 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => Ok(false),
        _ => Err(error).context("failed to lock FlowRT output directory"),
    }
}

pub(crate) fn unlock_file(file: &File) -> Result<()> {
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()).context("failed to unlock FlowRT output directory")
    }
}

pub(crate) fn normalize_contract_from_rsdl(path: &Path) -> Result<ContractIr> {
    let loaded = flowrt_rsdl::load_file(path)
        .with_context(|| format!("failed to load RSDL source `{}`", path.display()))?;
    let source_bundle = loaded.source_bundle_text();
    normalize_loaded_document(&loaded, hash_source(&source_bundle))
        .with_context(|| format!("failed to normalize `{}`", path.display()))
}

pub(crate) fn load_contract_from_rsdl(path: &Path) -> Result<ContractIr> {
    let contract = normalize_contract_from_rsdl(path)?;
    validate_contract(&contract).context("contract validation failed")?;
    Ok(contract)
}

pub(crate) fn load_contract_from_json(path: &Path) -> Result<ContractIr> {
    let source = fs::read_to_string(path)
        .with_context(|| format!("failed to read Contract IR `{}`", path.display()))?;
    let contract = ContractIr::from_json_str(&source)
        .with_context(|| format!("failed to parse Contract IR `{}`", path.display()))?;
    validate_contract(&contract).context("contract validation failed")?;
    Ok(contract)
}

pub(crate) fn prepared_contract_path(out_dir: &Path) -> PathBuf {
    out_dir.join("contract").join("contract.ir.json")
}

pub(crate) fn load_prepared_contract(out_dir: &Path, build_hint: &str) -> Result<ContractIr> {
    let path = prepared_contract_path(out_dir);
    if !path.exists() {
        anyhow::bail!(
            "FlowRT generated contract `{}` not found; run `{build_hint}` first",
            path.display(),
        );
    }
    load_contract_from_json(&path)
}

pub(crate) fn ensure_prepared_profile_matches(
    contract: &ContractIr,
    requested_profile: Option<&str>,
    build_hint: &str,
) -> Result<()> {
    let Some(requested_profile) = requested_profile else {
        return Ok(());
    };
    let prepared_profile = selected_prepared_profile_name(contract);
    if prepared_profile == Some(requested_profile) {
        return Ok(());
    }
    let prepared = prepared_profile.unwrap_or("<none>");
    anyhow::bail!(
        "prepared FlowRT artifacts use profile `{prepared}`, but command requested profile `{requested_profile}`; run `{build_hint}` first"
    );
}

pub(crate) fn selected_prepared_profile_name(contract: &ContractIr) -> Option<&str> {
    contract
        .profiles
        .first()
        .map(|profile| profile.name.as_str())
}

pub(crate) fn build_command_hint(rsdl: &Path, profile: Option<&str>, launcher: bool) -> String {
    let mut command = "flowrt build".to_string();
    if launcher {
        command.push_str(" --launcher");
    }
    if let Some(profile) = profile {
        command.push_str(" --profile ");
        command.push_str(profile);
    }
    command.push(' ');
    command.push_str(&rsdl.display().to_string());
    command
}

pub(crate) fn write_contract(contract: &ContractIr, out_dir: &Path) -> Result<PathBuf> {
    let contract_dir = out_dir.join("contract");
    fs::create_dir_all(&contract_dir)
        .with_context(|| format!("failed to create `{}`", contract_dir.display()))?;
    let output = contract_dir.join("contract.ir.json");
    fs::write(&output, contract.to_canonical_json()?)
        .with_context(|| format!("failed to write `{}`", output.display()))?;
    Ok(output)
}

#[derive(Debug)]
pub(crate) struct PreparedWorkspace {
    pub(crate) contract_path: PathBuf,
    pub(crate) artifact_count: usize,
    pub(crate) selected_contract: ContractIr,
}

#[cfg(test)]
pub(crate) fn prepare_workspace(
    rsdl: &Path,
    out_dir: &Path,
    profile: Option<&str>,
) -> Result<PreparedWorkspace> {
    prepare_workspace_with_options(
        rsdl,
        out_dir,
        profile,
        &TemporaryIslandCliOptions::default(),
        None,
    )
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TemporaryIslandCliOptions {
    pub(crate) enabled: bool,
    pub(crate) boundary_inputs: Vec<String>,
    pub(crate) boundary_outputs: Vec<String>,
}

impl TemporaryIslandCliOptions {
    pub(crate) fn new(
        enabled: bool,
        boundary_inputs: Vec<String>,
        boundary_outputs: Vec<String>,
    ) -> Self {
        Self {
            enabled,
            boundary_inputs,
            boundary_outputs,
        }
    }
}

pub(crate) fn prepare_workspace_with_options(
    rsdl: &Path,
    out_dir: &Path,
    profile: Option<&str>,
    temporary_island: &TemporaryIslandCliOptions,
    inject: Option<&Path>,
) -> Result<PreparedWorkspace> {
    let contract = normalize_contract_from_rsdl(rsdl)?;
    let mut selected_contract = project_contract_to_profile(&contract, profile)
        .with_context(|| format!("failed to select profile for `{}`", rsdl.display()))?;
    if temporary_island.enabled {
        let overlay = parse_temporary_island_overlay(temporary_island, rsdl)?;
        selected_contract = apply_temporary_island_overlay(&selected_contract, &overlay)
            .context("failed to apply temporary island overlay")?;
    } else if !temporary_island.boundary_inputs.is_empty()
        || !temporary_island.boundary_outputs.is_empty()
    {
        anyhow::bail!("`--boundary-input` and `--boundary-output` require `--temporary-island`");
    }
    if let Some(scenario_path) = inject {
        let scenario = parse_fault_injection_scenario(scenario_path)?;
        selected_contract = apply_fault_injection_overlay(&selected_contract, &scenario)
            .context("failed to apply fault injection overlay")?;
    }
    validate_contract(&selected_contract).context("contract validation failed")?;
    let contract_path = write_contract(&selected_contract, out_dir)?;
    let artifacts = emit_artifacts(&selected_contract).context("failed to prepare artifacts")?;
    let artifact_count = write_artifacts(&artifacts, out_dir)?;
    Ok(PreparedWorkspace {
        contract_path,
        artifact_count,
        selected_contract,
    })
}

/// 故障注入场景文件（TOML 子集）：`[[inject]]` 表数组，按名引用契约 instance/task。
#[derive(Debug, Deserialize)]
struct FaultScenarioFile {
    #[serde(default)]
    inject: Vec<FaultScenarioEntry>,
}

#[derive(Debug, Deserialize)]
struct FaultScenarioEntry {
    instance: String,
    task: String,
    #[serde(default)]
    invocations: Vec<u64>,
    #[serde(default)]
    from_invocation: Option<u64>,
    #[serde(default)]
    reason: String,
}

/// 解析 `--inject` 故障注入场景文件为归一化层可消费的 `FaultInjectionScenario`。
pub(crate) fn parse_fault_injection_scenario(path: &Path) -> Result<FaultInjectionScenario> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read fault scenario `{}`", path.display()))?;
    let parsed: FaultScenarioFile = toml::from_str(&text)
        .with_context(|| format!("failed to parse fault scenario `{}`", path.display()))?;
    if parsed.inject.is_empty() {
        anyhow::bail!(
            "fault scenario `{}` must declare at least one `[[inject]]` entry",
            path.display()
        );
    }
    let points = parsed
        .inject
        .into_iter()
        .map(|entry| FaultInjectionScenarioPoint {
            instance: entry.instance,
            task: entry.task,
            invocations: entry.invocations,
            from_invocation: entry.from_invocation,
            reason: entry.reason,
        })
        .collect();
    Ok(FaultInjectionScenario {
        points,
        generated_by: flowrt_ir::TemporaryOverlayGenerationIr {
            command: "flowrt prepare".to_string(),
            source: path.display().to_string(),
        },
    })
}

pub(crate) fn parse_temporary_island_overlay(
    options: &TemporaryIslandCliOptions,
    rsdl: &Path,
) -> Result<TemporaryIslandOverlay> {
    Ok(TemporaryIslandOverlay {
        boundary_inputs: options
            .boundary_inputs
            .iter()
            .map(|mapping| parse_temporary_boundary_mapping(mapping, "--boundary-input"))
            .collect::<Result<Vec<_>>>()?,
        boundary_outputs: options
            .boundary_outputs
            .iter()
            .map(|mapping| parse_temporary_boundary_mapping(mapping, "--boundary-output"))
            .collect::<Result<Vec<_>>>()?,
        generated_by: flowrt_ir::TemporaryOverlayGenerationIr {
            command: "flowrt prepare".to_string(),
            source: rsdl.display().to_string(),
        },
    })
}

pub(crate) fn parse_temporary_boundary_mapping(
    mapping: &str,
    flag: &str,
) -> Result<TemporaryBoundaryMapping> {
    let Some((name, endpoint)) = mapping.split_once('=') else {
        anyhow::bail!("{flag} expects `name=instance.port`, got `{mapping}`");
    };
    let name = name.trim();
    let endpoint = endpoint.trim();
    if name.is_empty() || endpoint.is_empty() {
        anyhow::bail!("{flag} expects non-empty `name=instance.port`, got `{mapping}`");
    }
    Ok(TemporaryBoundaryMapping {
        name: name.to_string(),
        endpoint: endpoint.to_string(),
    })
}

pub(crate) fn overlay_summary(contract: &ContractIr) -> Option<String> {
    if !contract.artifact.temporary_island {
        return None;
    }
    let mappings = contract
        .graphs
        .iter()
        .flat_map(|graph| {
            graph.boundary_endpoints.iter().map(move |endpoint| {
                format!(
                    "{}:{}={}.{}",
                    boundary_direction_name(endpoint.direction),
                    endpoint.name,
                    endpoint.port.instance.name,
                    endpoint.port.port
                )
            })
        })
        .collect::<Vec<_>>();
    let source = contract
        .artifact
        .temporary_overlay
        .as_ref()
        .map(|overlay| overlay.generated_by.source.as_str())
        .unwrap_or("unknown");
    Some(format!(
        "temporary_island=true test_only=true source={} mappings={}",
        source,
        mappings.join(",")
    ))
}

pub(crate) fn boundary_direction_name(direction: flowrt_ir::BoundaryDirection) -> &'static str {
    match direction {
        flowrt_ir::BoundaryDirection::Input => "input",
        flowrt_ir::BoundaryDirection::Output => "output",
    }
}

pub(crate) fn write_artifacts(bundle: &ArtifactBundle, out_dir: &Path) -> Result<usize> {
    for artifact in &bundle.artifacts {
        ensure_safe_relative_path(&artifact.relative_path)?;
        let output = out_dir.join(&artifact.relative_path);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }
        fs::write(&output, &artifact.content)
            .with_context(|| format!("failed to write `{}`", output.display()))?;
    }
    Ok(bundle.artifacts.len())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuildToolchainProfile {
    pub(crate) profile: ToolchainProfile,
    pub(crate) cargo_target_triple: Option<String>,
    pub(crate) is_cross: bool,
}

pub(crate) fn resolve_build_toolchain_profile(
    contract: &ContractIr,
    explicit_target: Option<&str>,
    workspace_root: &Path,
) -> Result<Option<BuildToolchainProfile>> {
    let platform = explicit_target
        .map(str::to_string)
        .or_else(|| contract_target_platform(contract))
        .or_else(|| host_flowrt_platform().map(str::to_string));
    resolve_optional_toolchain_profile(
        platform.as_deref(),
        explicit_target.is_some(),
        workspace_root,
    )
}

pub(crate) fn resolve_deps_toolchain_profile(
    rsdl: Option<&Path>,
    profile: Option<&str>,
    explicit_target: Option<&str>,
) -> Result<Option<BuildToolchainProfile>> {
    let workspace_root = match rsdl {
        Some(rsdl) => application_root_from_rsdl(rsdl)?,
        None => env::current_dir().context("failed to resolve current working directory")?,
    };
    if let Some(explicit_target) = explicit_target {
        return resolve_optional_toolchain_profile(Some(explicit_target), true, &workspace_root);
    }
    let Some(rsdl) = rsdl else {
        return Ok(None);
    };
    let contract = normalize_contract_from_rsdl(rsdl)?;
    let projected = project_contract_to_profile(&contract, profile)
        .with_context(|| format!("failed to select profile for `{}`", rsdl.display()))?;
    validate_contract(&projected).context("contract validation failed")?;
    resolve_optional_toolchain_profile(
        contract_target_platform(&projected).as_deref(),
        false,
        &workspace_root,
    )
}

pub(crate) fn resolve_optional_toolchain_profile(
    platform: Option<&str>,
    explicit_target: bool,
    workspace_root: &Path,
) -> Result<Option<BuildToolchainProfile>> {
    let Some(platform) = platform else {
        return Ok(None);
    };
    let platform = canonical_toolchain_platform(platform)?;
    let profile = resolve_toolchain_profile(
        &platform,
        workspace_root,
        &ToolchainProfileOverrides::default(),
    )?;
    let (_, host_target) = rustc_toolchain_identity()?;
    let is_cross = profile.rust_target != host_target;
    let cargo_target_triple = if explicit_target || is_cross {
        Some(profile.rust_target.clone())
    } else {
        None
    };
    Ok(Some(BuildToolchainProfile {
        profile,
        cargo_target_triple,
        is_cross,
    }))
}

pub(crate) fn canonical_toolchain_platform(platform: &str) -> Result<String> {
    TargetPlatform::parse_alias(platform)
        .map(|platform| platform.as_str().to_string())
        .with_context(|| format!("unsupported toolchain platform `{platform}`"))
}

pub(crate) fn contract_target_platform(contract: &ContractIr) -> Option<String> {
    bundle_target_platform(contract)
}

pub(crate) fn cargo_target_args(target_triple: Option<&str>) -> Vec<String> {
    target_triple
        .map(|target| vec!["--target".to_string(), target.to_string()])
        .unwrap_or_default()
}

pub(crate) fn cargo_target_linker_env(
    target_triple: Option<&str>,
    linker: Option<&str>,
) -> Option<(String, String)> {
    let target_triple = target_triple?;
    let linker = linker?;
    Some((
        format!(
            "CARGO_TARGET_{}_LINKER",
            target_triple.replace('-', "_").to_ascii_uppercase()
        ),
        linker.to_string(),
    ))
}

pub(crate) fn deps_runtime_features(
    rsdl: Option<&Path>,
    profile: Option<&str>,
    backend: Option<DepsBackend>,
) -> Result<RuntimeFeatureSet> {
    if let Some(backend) = backend {
        return match backend {
            DepsBackend::Inproc => Ok(RuntimeFeatureSet::inproc_only()),
            DepsBackend::Iox2 => RuntimeFeatureSet::from_backend_names(["iox2"]),
            DepsBackend::Zenoh => RuntimeFeatureSet::from_backend_names(["zenoh"]),
            DepsBackend::All => Ok(RuntimeFeatureSet::all()),
        };
    }
    let Some(rsdl) = rsdl else {
        return Ok(RuntimeFeatureSet::all());
    };
    let contract = normalize_contract_from_rsdl(rsdl)?;
    let projected = project_contract_to_profile(&contract, profile)
        .with_context(|| format!("failed to select profile for `{}`", rsdl.display()))?;
    validate_contract(&projected).context("contract validation failed")?;
    RuntimeFeatureSet::from_contract(&projected)
}

pub(crate) fn deps_cache_layout(
    build_mode: BuildMode,
    features: RuntimeFeatureSet,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<CacheLayout> {
    let root = default_cache_root()
        .context("failed to resolve FlowRT cache directory; set FLOWRT_CACHE_DIR or HOME")?;
    let (rustc_identity, host_target_triple) = rustc_toolchain_identity()?;
    let target_triple = target_profile
        .map(|profile| profile.profile.rust_target.clone())
        .unwrap_or(host_target_triple);
    let rust_runtime_dir = rust_runtime_dir_for_generated_build()?;
    let vendor_hash = flowrt_vendor_hash(rust_runtime_dir.as_deref())?;
    let key = DepsCacheKey::new(
        env!("CARGO_PKG_VERSION"),
        rustc_identity,
        target_triple,
        vendor_hash,
        build_mode,
        features,
    );
    Ok(CacheLayout::new(root, &key))
}

pub(crate) fn prepare_deps_cache(
    layout: &CacheLayout,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<()> {
    let _lock = CacheLock::acquire(&layout.lock_file)?;
    if deps_ready(layout, build_mode, features)? {
        return Ok(());
    }
    let rust_runtime_dir = rust_runtime_dir_for_generated_build()?.context(
        "FlowRT Rust runtime directory not found; install FlowRT package, set FLOWRT_RUST_RUNTIME_DIR, or set FLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=1 in repository development mode",
    )?;
    if is_repo_rust_runtime_dir(&rust_runtime_dir)? {
        run_repo_runtime_cargo_build(
            &layout.target_dir,
            build_mode,
            features,
            target_profile.and_then(|profile| profile.cargo_target_triple.as_deref()),
            target_profile.and_then(|profile| {
                profile
                    .cargo_target_triple
                    .as_ref()
                    .map(|_| profile.profile.c_compiler.as_str())
            }),
        )?;
    } else {
        write_deps_workspace(&layout.deps_workspace_dir, &rust_runtime_dir, features)?;
        run_deps_cargo_build(
            &layout.deps_workspace_dir,
            &layout.target_dir,
            build_mode,
            target_profile.and_then(|profile| profile.cargo_target_triple.as_deref()),
            target_profile.and_then(|profile| {
                profile
                    .cargo_target_triple
                    .as_ref()
                    .map(|_| profile.profile.c_compiler.as_str())
            }),
        )?;
    }
    write_deps_ready_marker(layout, build_mode, features)
}

pub(crate) fn ensure_deps_ready(
    layout: &CacheLayout,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<()> {
    if deps_ready(layout, build_mode, features)? {
        return Ok(());
    }
    let target_hint = target_profile
        .map(|profile| {
            format!(
                " for platform `{}` / Rust target `{}`",
                profile.profile.platform, profile.profile.rust_target
            )
        })
        .unwrap_or_else(|| format!(" for native Rust target `{}`", layout.target_triple));
    anyhow::bail!(
        "FlowRT dependency cache is missing{target_hint} for build_mode `{}` and backend features {:?}; run `flowrt deps --backend {} --build-mode {}{}` or `flowrt deps <rsdl> --build-mode {}{}` first",
        build_mode,
        features.canonical_names(),
        features.deps_backend_hint(),
        build_mode,
        target_profile
            .map(|profile| format!(" --target {}", profile.profile.platform))
            .unwrap_or_default(),
        build_mode,
        target_profile
            .map(|profile| format!(" --target {}", profile.profile.platform))
            .unwrap_or_default()
    )
}

pub(crate) fn select_ready_deps_cache_layout(
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<CacheLayout> {
    let exact = deps_cache_layout(build_mode, features.clone(), target_profile)?;
    if deps_ready(&exact, build_mode, features)? {
        return Ok(exact);
    }

    let all_features = RuntimeFeatureSet::all();
    if features != &all_features && features.is_subset_of(&all_features) {
        let all = deps_cache_layout(build_mode, all_features.clone(), target_profile)?;
        if deps_ready(&all, build_mode, &all_features)? {
            return Ok(all);
        }
    }

    ensure_deps_ready(&exact, build_mode, features, target_profile)?;
    unreachable!("ensure_deps_ready must return an error when cache is absent")
}

pub(crate) fn deps_ready(
    layout: &CacheLayout,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
) -> Result<bool> {
    if !layout.ready_file.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(&layout.ready_file)
        .with_context(|| format!("failed to read `{}`", layout.ready_file.display()))?;
    let marker: DepsReadyMarker = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse `{}`", layout.ready_file.display()))?;
    Ok(marker.schema_version == 1
        && marker.flowrt_version == env!("CARGO_PKG_VERSION")
        && marker.build_mode == build_mode
        && marker.features == feature_names_owned(features)
        && marker.target_triple.as_deref() == Some(layout.target_triple.as_str())
        && marker.target_dir == layout.target_dir)
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DepsReadyMarker {
    pub(crate) schema_version: u32,
    pub(crate) flowrt_version: String,
    pub(crate) build_mode: BuildMode,
    pub(crate) features: Vec<String>,
    #[serde(default)]
    pub(crate) target_triple: Option<String>,
    pub(crate) target_dir: PathBuf,
}

pub(crate) fn write_deps_ready_marker(
    layout: &CacheLayout,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
) -> Result<()> {
    let marker = DepsReadyMarker {
        schema_version: 1,
        flowrt_version: env!("CARGO_PKG_VERSION").to_string(),
        build_mode,
        features: feature_names_owned(features),
        target_triple: Some(layout.target_triple.clone()),
        target_dir: layout.target_dir.clone(),
    };
    if let Some(parent) = layout.ready_file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    let mut content = serde_json::to_string_pretty(&marker)?;
    content.push('\n');
    fs::write(&layout.ready_file, content)
        .with_context(|| format!("failed to write `{}`", layout.ready_file.display()))
}

pub(crate) fn feature_names_owned(features: &RuntimeFeatureSet) -> Vec<String> {
    features
        .canonical_names()
        .into_iter()
        .map(str::to_string)
        .collect()
}

pub(crate) fn write_deps_workspace(
    workspace_dir: &Path,
    rust_runtime_dir: &Path,
    features: &RuntimeFeatureSet,
) -> Result<()> {
    fs::create_dir_all(workspace_dir.join("src"))
        .with_context(|| format!("failed to create `{}`", workspace_dir.display()))?;
    let feature_args = features.cargo_feature_args();
    let feature_suffix = if feature_args.is_empty() {
        String::new()
    } else {
        format!(
            ", features = [{}]",
            feature_args
                .iter()
                .map(|feature| format!("\"{feature}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let manifest = format!(
        "[package]\nname = \"flowrt-deps-prewarm\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\nflowrt = {{ path = {}{} }}\nserde = {{ version = \"1\", features = [\"derive\"] }}\nserde_json = \"1\"\n",
        toml_basic_string(rust_runtime_dir),
        feature_suffix
    );
    fs::write(workspace_dir.join("Cargo.toml"), manifest).with_context(|| {
        format!(
            "failed to write `{}`",
            workspace_dir.join("Cargo.toml").display()
        )
    })?;
    fs::write(
        workspace_dir.join("src").join("lib.rs"),
        "pub fn flowrt_deps_prewarm_marker() -> flowrt::Status {\n    flowrt::Status::Ok\n}\n",
    )
    .with_context(|| {
        format!(
            "failed to write `{}`",
            workspace_dir.join("src/lib.rs").display()
        )
    })?;

    if let Some(private_prefix) = flowrt_private_prefix_from_runtime_dir(rust_runtime_dir) {
        let vendor_dir = private_prefix.join("share").join("cargo").join("vendor");
        if vendor_dir.is_dir() {
            let cargo_dir = workspace_dir.join(".cargo");
            fs::create_dir_all(&cargo_dir)
                .with_context(|| format!("failed to create `{}`", cargo_dir.display()))?;
            let config = format!(
                "[source.crates-io]\nreplace-with = \"flowrt-vendor\"\n\n[source.flowrt-vendor]\ndirectory = {}\n\n[net]\noffline = true\n",
                toml_basic_string(&vendor_dir)
            );
            fs::write(cargo_dir.join("config.toml"), config).with_context(|| {
                format!(
                    "failed to write `{}`",
                    cargo_dir.join("config.toml").display()
                )
            })?;
        }
    }
    Ok(())
}

pub(crate) fn run_deps_cargo_build(
    workspace_dir: &Path,
    target_dir: &Path,
    build_mode: BuildMode,
    target_triple: Option<&str>,
    target_linker: Option<&str>,
) -> Result<()> {
    ensure_rust_target_available(target_triple)?;
    fs::create_dir_all(target_dir)
        .with_context(|| format!("failed to create `{}`", target_dir.display()))?;
    let mut command = ProcessCommand::new("cargo");
    command
        .current_dir(workspace_dir)
        .arg("build")
        .arg("--lib")
        .env("CARGO_TARGET_DIR", target_dir);
    for arg in build_mode.cargo_args() {
        command.arg(arg);
    }
    for arg in cargo_target_args(target_triple) {
        command.arg(arg);
    }
    if let Some((key, value)) = cargo_target_linker_env(target_triple, target_linker) {
        command.env(key, value);
    }
    if workspace_dir.join(".cargo").join("config.toml").exists() {
        command.arg("--offline");
    }
    let status = command.status().with_context(|| {
        format!(
            "failed to spawn cargo for dependency prewarm in `{}`",
            workspace_dir.display()
        )
    })?;
    if !status.success() {
        bail_cargo_status("FlowRT dependency prewarm", status, target_triple)?;
    }
    Ok(())
}

pub(crate) fn run_repo_runtime_cargo_build(
    target_dir: &Path,
    build_mode: BuildMode,
    features: &RuntimeFeatureSet,
    target_triple: Option<&str>,
    target_linker: Option<&str>,
) -> Result<()> {
    ensure_rust_target_available(target_triple)?;
    let repo_root = repo_root_dir()?;
    fs::create_dir_all(target_dir)
        .with_context(|| format!("failed to create `{}`", target_dir.display()))?;
    let mut command = ProcessCommand::new("cargo");
    command
        .current_dir(&repo_root)
        .arg("build")
        .arg("-p")
        .arg("flowrt")
        .arg("--lib")
        .arg("--locked")
        .env("CARGO_TARGET_DIR", target_dir);
    for arg in build_mode.cargo_args() {
        command.arg(arg);
    }
    for arg in cargo_target_args(target_triple) {
        command.arg(arg);
    }
    if let Some((key, value)) = cargo_target_linker_env(target_triple, target_linker) {
        command.env(key, value);
    }
    let feature_args = features.cargo_feature_args();
    if !feature_args.is_empty() {
        command.arg("--features").arg(feature_args.join(","));
    }
    let status = command.status().with_context(|| {
        format!(
            "failed to spawn cargo for repository dependency prewarm in `{}`",
            repo_root.display()
        )
    })?;
    if !status.success() {
        bail_cargo_status(
            "FlowRT repository dependency prewarm",
            status,
            target_triple,
        )?;
    }
    Ok(())
}

pub(crate) fn ensure_rust_target_available(target_triple: Option<&str>) -> Result<()> {
    let Some(target_triple) = target_triple else {
        return Ok(());
    };
    let output = match ProcessCommand::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error).context("failed to run `rustup target list --installed`");
        }
    };
    if !output.status.success() {
        return Ok(());
    }
    let installed = String::from_utf8_lossy(&output.stdout);
    if installed.lines().any(|line| line.trim() == target_triple) {
        return Ok(());
    }
    anyhow::bail!(
        "Rust target `{target_triple}` is not installed; run `rustup target add {target_triple}` or configure the Rust toolchain before running FlowRT cross build"
    );
}

pub(crate) fn bail_cargo_status(
    context: &str,
    status: std::process::ExitStatus,
    target_triple: Option<&str>,
) -> Result<()> {
    if let Some(target_triple) = target_triple {
        anyhow::bail!(
            "{context} failed with status {status} for Rust target `{target_triple}`; run `rustup target add {target_triple}` if the target std library is missing, then retry"
        );
    }
    anyhow::bail!("{context} failed with status {status}");
}

pub(crate) fn is_repo_rust_runtime_dir(path: &Path) -> Result<bool> {
    let Some(repo_runtime) = repo_runtime_dir("runtime/rust", "Cargo.toml") else {
        return Ok(false);
    };
    let repo_runtime = fs::canonicalize(repo_runtime)
        .context("failed to canonicalize repository Rust runtime directory")?;
    let candidate = fs::canonicalize(path)
        .with_context(|| format!("failed to canonicalize `{}`", path.display()))?;
    Ok(candidate == repo_runtime)
}

#[derive(Debug)]
pub(crate) struct CacheLock {
    pub(crate) file: File,
}

impl CacheLock {
    pub(crate) fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("failed to open cache lock `{}`", path.display()))?;
        if !try_lock_file(&file)? {
            anyhow::bail!(
                "FlowRT dependency cache `{}` is already in use by another flowrt command",
                path.display()
            );
        }
        Ok(Self { file })
    }
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = unlock_file(&self.file);
    }
}

pub(crate) fn rustc_toolchain_identity() -> Result<(String, String)> {
    let output = ProcessCommand::new("rustc")
        .arg("-Vv")
        .output()
        .context("failed to spawn rustc -Vv")?;
    if !output.status.success() {
        anyhow::bail!("rustc -Vv failed with status {}", output.status);
    }
    let stdout = String::from_utf8(output.stdout).context("rustc -Vv output is not UTF-8")?;
    let identity = stdout
        .lines()
        .find(|line| line.starts_with("rustc "))
        .unwrap_or("rustc unknown")
        .to_string();
    let host = stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap_or(std::env::consts::ARCH)
        .to_string();
    Ok((identity, host))
}

pub(crate) fn flowrt_vendor_hash(rust_runtime_dir: Option<&Path>) -> Result<String> {
    if let Some(runtime_dir) = rust_runtime_dir {
        if let Some(private_prefix) = flowrt_private_prefix_from_runtime_dir(runtime_dir) {
            let hash_file = private_prefix
                .join("share")
                .join("cargo")
                .join("vendor")
                .join(".flowrt-vendor.sha256");
            if hash_file.exists() {
                let content = fs::read_to_string(&hash_file)
                    .with_context(|| format!("failed to read `{}`", hash_file.display()))?;
                if let Some(hash) = content.split_whitespace().next() {
                    return Ok(hash.to_string());
                }
                anyhow::bail!(
                    "FlowRT vendor hash marker `{}` is empty; reinstall the FlowRT package",
                    hash_file.display()
                );
            }
            anyhow::bail!(
                "FlowRT vendor hash marker is missing at `{}`; reinstall the FlowRT package",
                hash_file.display()
            );
        }
    }
    let repo_root = repo_root_dir()?;
    let mut hasher = Sha256::new();
    for relative in ["Cargo.lock", "runtime/rust/Cargo.toml", "scripts/deps.lock"] {
        let path = repo_root.join(relative);
        if path.exists() {
            hasher.update(relative.as_bytes());
            hasher.update(fs::read(&path).with_context(|| {
                format!("failed to read `{}` for FlowRT vendor hash", path.display())
            })?);
        }
    }
    Ok(hex_lower(&hasher.finalize())[..16].to_string())
}

pub(crate) fn hex_lower(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

pub(crate) fn build_workspace(
    contract: &ContractIr,
    out_dir: &Path,
    include_launcher: bool,
    build_mode: BuildMode,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<build_model::BuildInfo> {
    ensure_backend_runtime_supported(contract, "build")?;
    let rust_runtime_dir = rust_runtime_dir_for_generated_build()?;
    let mut build_info = build_info_for_contract(contract, build_mode)?;
    apply_build_target_metadata(&mut build_info, target_profile)?;
    let steps = build_steps(contract, include_launcher);
    preflight_cmake_build_diagnostics(contract, &steps, target_profile)?;
    let bin_target_identity = bin_target_identity(target_profile);
    let cargo_cache = if steps
        .iter()
        .any(|step| matches!(step, BuildStep::CargoApp | BuildStep::CargoSupervisor))
    {
        let features = RuntimeFeatureSet::from_contract(contract)?;
        let layout = select_ready_deps_cache_layout(build_mode, &features, target_profile)?;
        build_info.deps_target_dir = Some(layout.target_dir.clone());
        Some(layout)
    } else {
        None
    };
    let cargo_target_triple =
        target_profile.and_then(|profile| profile.cargo_target_triple.as_deref());
    let cargo_target_linker = target_profile.and_then(|profile| {
        profile
            .cargo_target_triple
            .as_ref()
            .map(|_| profile.profile.c_compiler.as_str())
    });
    for step in &steps {
        match *step {
            BuildStep::CargoApp => {
                let cargo_names = cargo_internal_names(contract)?;
                let manifest = cargo_build_manifest_with_runtime_patch(
                    out_dir,
                    rust_runtime_dir.as_deref(),
                    &cargo_names,
                )?;
                let target_dir = cargo_cache
                    .as_ref()
                    .map(|layout| layout.target_dir.as_path())
                    .context("internal error: Cargo app build missing dependency cache layout")?;
                let built = run_cargo_build_bin(
                    &manifest,
                    &cargo_names.app_internal,
                    build_mode,
                    target_dir,
                    cargo_target_triple,
                    cargo_target_linker,
                )?;
                let local = copy_executable_to_local_bin_as(
                    out_dir,
                    build_mode,
                    bin_target_identity.as_deref(),
                    &built,
                    std::ffi::OsStr::new(&cargo_names.app_stable),
                )?;
                build_info.executables.rust_app = Some(relative_to_out_dir(out_dir, &local)?);
                record_build_artifact(&mut build_info, "rust_app", out_dir, &local)?;
            }
            BuildStep::CargoSupervisor => {
                let cargo_names = cargo_internal_names(contract)?;
                let manifest = cargo_build_manifest_with_runtime_patch(
                    out_dir,
                    rust_runtime_dir.as_deref(),
                    &cargo_names,
                )?;
                let target_dir = cargo_cache
                    .as_ref()
                    .map(|layout| layout.target_dir.as_path())
                    .context(
                        "internal error: Cargo supervisor build missing dependency cache layout",
                    )?;
                let built = run_cargo_build_bin(
                    &manifest,
                    &cargo_names.supervisor_internal,
                    build_mode,
                    target_dir,
                    cargo_target_triple,
                    cargo_target_linker,
                )?;
                let local = copy_executable_to_local_bin_as(
                    out_dir,
                    build_mode,
                    bin_target_identity.as_deref(),
                    &built,
                    std::ffi::OsStr::new(&cargo_names.supervisor_stable),
                )?;
                build_info.executables.supervisor = Some(relative_to_out_dir(out_dir, &local)?);
                record_build_artifact(&mut build_info, "supervisor", out_dir, &local)?;
            }
            BuildStep::CmakeApp => {
                let built =
                    run_cmake_configure_and_build(contract, out_dir, build_mode, target_profile)?;
                if let Some(cpp_app) = built.cpp_app {
                    let local = copy_executable_to_local_bin(
                        out_dir,
                        build_mode,
                        bin_target_identity.as_deref(),
                        &cpp_app,
                    )?;
                    build_info.executables.cpp_app = Some(relative_to_out_dir(out_dir, &local)?);
                    record_build_artifact(&mut build_info, "cpp_app", out_dir, &local)?;
                }
                if let Some(ros2_bridge) = built.ros2_bridge {
                    let local = copy_executable_to_local_bin(
                        out_dir,
                        build_mode,
                        bin_target_identity.as_deref(),
                        &ros2_bridge,
                    )?;
                    build_info.executables.ros2_bridge =
                        Some(relative_to_out_dir(out_dir, &local)?);
                    record_build_artifact(&mut build_info, "ros2_bridge", out_dir, &local)?;
                }
            }
        }
    }
    record_build_runtime_dependencies(&mut build_info, out_dir, target_profile, &steps)?;
    build_info.write(out_dir)?;
    Ok(build_info)
}

pub(crate) fn preflight_cmake_build_diagnostics(
    contract: &ContractIr,
    steps: &[BuildStep],
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<()> {
    if !steps.contains(&BuildStep::CmakeApp) {
        return Ok(());
    }
    let Some(target_profile) = target_profile else {
        return Ok(());
    };
    let target_sdk = if target_profile.cargo_target_triple.is_some() {
        let runtime_dir = cpp_runtime_dir_for_generated_build()?;
        Some(resolve_cpp_target_sdk_for_build(
            runtime_dir.as_deref(),
            &target_profile.profile,
        )?)
    } else {
        None
    };
    ensure_cmake_build_diagnostics_ready(contract, target_profile, target_sdk.as_ref())
}

pub(crate) fn format_build_success_summary(
    contract: &ContractIr,
    build_info: &build_model::BuildInfo,
    target_profile: Option<&BuildToolchainProfile>,
    out_dir: &Path,
) -> String {
    let target = target_profile
        .map(|profile| profile.profile.platform.as_str())
        .or(build_info.platform.as_deref())
        .or(host_flowrt_platform())
        .unwrap_or("native");
    let final_paths = build_success_paths(build_info, out_dir)
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    let final_label = if final_paths.len() > 1 {
        "final_binaries"
    } else {
        "final_binary"
    };
    let mut lines = vec![format!(
        "build summary: target={target} mode={}{} {final_label}={}",
        build_info.build_mode,
        build_info
            .rust_target_triple
            .as_deref()
            .map(|triple| format!(" rust_target={triple}"))
            .unwrap_or_default(),
        if final_paths.is_empty() {
            "<none>".to_string()
        } else {
            final_paths.join(", ")
        }
    )];
    if let Some(target_profile) = target_profile
        && build_uses_cpp_toolchain(contract)
    {
        lines.push(format!(
            "toolchain: c={} cxx={} runtime_deps={}",
            target_profile.profile.c_compiler,
            target_profile.profile.cpp_compiler,
            runtime_dependency_policy_name(target_profile.profile.runtime_dependency_policy),
        ));
        lines.push(format!(
            "sdk_overlays={}",
            format_path_list(&target_profile.profile.sdk_overlays)
        ));
        lines.push(format!(
            "pkg-config={}",
            format_string_list(&build_pkg_config_modules(
                contract,
                &target_profile.profile.platform,
            ))
        ));
    }
    lines.join("\n")
}

pub(crate) fn build_success_paths(
    build_info: &build_model::BuildInfo,
    out_dir: &Path,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for relative in [
        build_info.executables.rust_app.as_ref(),
        build_info.executables.cpp_app.as_ref(),
        build_info.executables.ros2_bridge.as_ref(),
        build_info.executables.supervisor.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        push_unique_path(&mut paths, &out_dir.join(relative));
    }
    paths
}

pub(crate) fn build_uses_cpp_toolchain(contract: &ContractIr) -> bool {
    has_cmake_app_components(contract) || has_ros2_bridge(contract)
}

pub(crate) fn build_pkg_config_modules(
    contract: &ContractIr,
    selected_platform: &str,
) -> Vec<String> {
    selected_cpp_pkg_config_requirements(contract, selected_platform)
        .into_iter()
        .map(|requirement| requirement.module)
        .collect()
}

pub(crate) fn format_string_list(values: &[String]) -> String {
    if values.is_empty() {
        return "<none>".to_string();
    }
    values.join(", ")
}

pub(crate) fn ensure_cmake_build_diagnostics_ready(
    contract: &ContractIr,
    target_profile: &BuildToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
) -> Result<()> {
    let requirements =
        selected_cpp_pkg_config_requirements(contract, &target_profile.profile.platform);
    if requirements.is_empty() {
        return Ok(());
    }
    let doctor_hint = build_doctor_hint(&target_profile.profile.platform);
    let pkg_config_path =
        current_pkg_config_env_value(&target_profile.profile, target_sdk, "PKG_CONFIG_PATH");
    let pkg_config_libdir = current_pkg_config_libdir(&target_profile.profile, target_sdk);
    if !command_available("pkg-config") {
        let missing_modules = requirements
            .iter()
            .map(|requirement| format!("{}:{}", requirement.component, requirement.module))
            .collect::<Vec<_>>();
        anyhow::bail!(
            "build diagnostics: target={} PKG_CONFIG_PATH={} PKG_CONFIG_LIBDIR={} missing_modules={} sdk_overlays={} reason=`pkg-config` not found in PATH; run `{}` before retrying",
            target_profile.profile.platform,
            pkg_config_path,
            pkg_config_libdir,
            missing_modules.join(", "),
            format_path_list(&target_profile.profile.sdk_overlays),
            doctor_hint,
        );
    }

    let missing_modules = requirements
        .iter()
        .filter_map(|requirement| {
            match pkg_config_module_exists(
                requirement.module.as_str(),
                &target_profile.profile,
                target_sdk,
            ) {
                Ok(true) => None,
                Ok(false) => Some(Ok(format!(
                    "{}:{}",
                    requirement.component, requirement.module
                ))),
                Err(error) => Some(Err(error)),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    if missing_modules.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "build diagnostics: target={} PKG_CONFIG_PATH={} PKG_CONFIG_LIBDIR={} missing_modules={} sdk_overlays={} hint=run `{}` before retrying",
        target_profile.profile.platform,
        pkg_config_path,
        pkg_config_libdir,
        missing_modules.join(", "),
        format_path_list(&target_profile.profile.sdk_overlays),
        doctor_hint,
    );
}

pub(crate) fn build_doctor_hint(platform: &str) -> String {
    format!("flowrt doctor <rsdl> --target {platform}")
}

pub(crate) fn current_pkg_config_libdir(
    profile: &ToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
) -> String {
    current_pkg_config_env_value(profile, target_sdk, "PKG_CONFIG_LIBDIR")
}

pub(crate) fn current_pkg_config_env_value(
    profile: &ToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
    key: &'static str,
) -> String {
    match doctor_pkg_config_env(profile, target_sdk) {
        Ok(mut env) => {
            format_pkg_config_env_value(env.remove(key).or_else(|| std::env::var_os(key)))
        }
        Err(error) => format!("<invalid: {error}>"),
    }
}

pub(crate) fn format_pkg_config_env_value(value: Option<OsString>) -> String {
    value
        .map(|value| {
            let text = value.to_string_lossy().into_owned();
            if text.is_empty() {
                "<empty>".to_string()
            } else {
                text
            }
        })
        .unwrap_or_else(|| "<unset>".to_string())
}

pub(crate) fn build_info_for_contract(
    contract: &ContractIr,
    build_mode: BuildMode,
) -> Result<build_model::BuildInfo> {
    let mut info = build_model::BuildInfo::new(
        env!("CARGO_PKG_VERSION"),
        selected_prepared_profile_name(contract).map(str::to_string),
        build_mode,
        None,
    );
    info.target = Some(bundle_target_name(contract));
    info.platform = bundle_target_platform(contract);
    Ok(info)
}

pub(crate) fn apply_build_target_metadata(
    build_info: &mut build_model::BuildInfo,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<()> {
    let (_, host_target_triple) = rustc_toolchain_identity()?;
    build_info.host_target_triple = Some(host_target_triple);
    build_info.target_identity = Some(build_target_identity(target_profile));
    if let Some(target_profile) = target_profile {
        build_info.platform = Some(target_profile.profile.platform.clone());
        build_info.rust_target_triple = Some(target_profile.profile.rust_target.clone());
    }
    Ok(())
}

pub(crate) fn record_build_artifact(
    build_info: &mut build_model::BuildInfo,
    kind: &str,
    out_dir: &Path,
    local: &Path,
) -> Result<()> {
    let relative = relative_to_out_dir(out_dir, local)?;
    build_info.artifacts.push(build_model::BuildArtifactInfo {
        kind: kind.to_string(),
        target: build_info
            .target
            .clone()
            .unwrap_or_else(|| "default".to_string()),
        platform: build_info.platform.clone(),
        path: relative,
        sha256: file_sha256(local)?,
    });
    Ok(())
}

pub(crate) fn record_build_runtime_dependencies(
    build_info: &mut build_model::BuildInfo,
    out_dir: &Path,
    target_profile: Option<&BuildToolchainProfile>,
    steps: &[BuildStep],
) -> Result<()> {
    let Some(target_profile) = target_profile else {
        return Ok(());
    };
    if !target_profile.is_cross
        || target_profile.profile.runtime_dependency_policy != RuntimeDependencyPolicy::Bundle
        || !steps.contains(&BuildStep::CmakeApp)
    {
        return Ok(());
    }

    let runtime_dir = cpp_runtime_dir_for_generated_build()?;
    let sdk = resolve_cpp_target_sdk_for_build(runtime_dir.as_deref(), &target_profile.profile)?;
    let source = sdk.root.join("flowrt-target-sdk.toml");
    let dest = out_dir
        .join("build/runtime-deps")
        .join(&target_profile.profile.platform)
        .join("flowrt-target-sdk.toml");
    copy_required_file(&source, &dest)?;
    build_info
        .runtime_dependencies
        .push(build_model::BuildRuntimeDependencyInfo {
            name: "flowrt-target-sdk".to_string(),
            target: build_info
                .target
                .clone()
                .unwrap_or_else(|| "default".to_string()),
            platform: target_profile.profile.platform.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            policy: runtime_dependency_policy_name(
                target_profile.profile.runtime_dependency_policy,
            )
            .to_string(),
            path: relative_to_out_dir(out_dir, &dest)?,
            sha256: file_sha256(&dest)?,
        });
    Ok(())
}

pub(crate) fn load_build_info(
    out_dir: &Path,
    requested_build_mode: Option<BuildMode>,
    launcher: bool,
) -> Result<build_model::BuildInfo> {
    let info = build_model::BuildInfo::read(out_dir).with_context(|| {
        format!(
            "FlowRT build metadata is missing; run `{}` with FlowRT 0.6.1 or newer",
            if launcher {
                "flowrt build --launcher"
            } else {
                "flowrt build"
            }
        )
    })?;
    if info.flowrt_version != env!("CARGO_PKG_VERSION") {
        anyhow::bail!(
            "prepared FlowRT artifacts were built with FlowRT {}, but this CLI is {}; run `{}` again",
            info.flowrt_version,
            env!("CARGO_PKG_VERSION"),
            if launcher {
                "flowrt build --launcher"
            } else {
                "flowrt build"
            }
        );
    }
    if let Some(requested) = requested_build_mode {
        if info.build_mode != requested {
            anyhow::bail!(
                "prepared FlowRT artifacts use build mode `{}`, but command requested `{}`; run `{}` with `--build-mode {}` first",
                info.build_mode,
                requested,
                if launcher {
                    "flowrt build --launcher"
                } else {
                    "flowrt build"
                },
                requested
            );
        }
    }
    Ok(info)
}

pub(crate) fn executable_from_build_info(
    out_dir: &Path,
    relative: Option<&PathBuf>,
    label: &str,
    build_hint: &str,
) -> Result<PathBuf> {
    let relative =
        relative.with_context(|| format!("{label} was not built; run `{build_hint}` first"))?;
    ensure_safe_relative_path(relative)?;
    Ok(out_dir.join(relative))
}

pub(crate) fn cargo_manifest_with_runtime_patch(
    out_dir: &Path,
    runtime_dir: Option<&Path>,
) -> Result<PathBuf> {
    let generated_manifest = out_dir.join("build").join("Cargo.toml");
    let generated = fs::read_to_string(&generated_manifest)
        .with_context(|| format!("failed to read `{}`", generated_manifest.display()))?;
    if generated.contains("[patch.crates-io]") || !manifest_declares_flowrt_dependency(&generated) {
        return Ok(generated_manifest);
    }
    let Some(runtime_dir) = runtime_dir else {
        return Ok(generated_manifest);
    };
    write_cargo_vendor_config(out_dir, runtime_dir)?;
    let patched = format!(
        "{generated}\n[patch.crates-io]\nflowrt = {{ path = {} }}\n",
        toml_basic_string(runtime_dir)
    );
    fs::write(&generated_manifest, patched)
        .with_context(|| format!("failed to write `{}`", generated_manifest.display()))?;
    Ok(generated_manifest)
}

pub(crate) fn manifest_declares_flowrt_dependency(manifest: &str) -> bool {
    manifest
        .lines()
        .any(|line| line.trim_start().starts_with("flowrt ="))
}

pub(crate) fn write_cargo_vendor_config(out_dir: &Path, runtime_dir: &Path) -> Result<()> {
    let Some(private_prefix) = flowrt_private_prefix_from_runtime_dir(runtime_dir) else {
        return Ok(());
    };
    let vendor_dir = private_prefix.join("share").join("cargo").join("vendor");
    if !vendor_dir.is_dir() {
        return Ok(());
    }
    let cargo_dir = out_dir.join("build").join(".cargo");
    fs::create_dir_all(&cargo_dir)
        .with_context(|| format!("failed to create `{}`", cargo_dir.display()))?;
    let config = format!(
        "[source.crates-io]\nreplace-with = \"flowrt-vendor\"\n\n[source.flowrt-vendor]\ndirectory = {}\n\n[net]\noffline = true\n",
        toml_basic_string(&vendor_dir)
    );
    let config_path = cargo_dir.join("config.toml");
    fs::write(&config_path, config)
        .with_context(|| format!("failed to write `{}`", config_path.display()))?;
    Ok(())
}

pub(crate) fn flowrt_private_prefix_from_runtime_dir(runtime_dir: &Path) -> Option<PathBuf> {
    let share_flowrt = runtime_dir.parent()?.parent()?;
    if share_flowrt.file_name()? != OsStr::new("flowrt") {
        return None;
    }
    let share = share_flowrt.parent()?;
    if share.file_name()? != OsStr::new("share") {
        return None;
    }
    Some(share.parent()?.to_path_buf())
}

#[derive(Debug, Default)]
pub(crate) struct CmakeBuildOutputs {
    pub(crate) cpp_app: Option<PathBuf>,
    pub(crate) ros2_bridge: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CppTargetSdk {
    pub(crate) root: PathBuf,
    pub(crate) cmake_dir: Option<PathBuf>,
    pub(crate) pkgconfig_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CppTargetSdkManifest {
    pub(crate) platform: String,
    pub(crate) complete: bool,
    pub(crate) cmake_dir: Option<PathBuf>,
    pub(crate) pkgconfig_dir: Option<PathBuf>,
    pub(crate) include_dir: Option<PathBuf>,
    pub(crate) lib_dir: Option<PathBuf>,
}

pub(crate) fn run_cmake_configure_and_build(
    contract: &ContractIr,
    out_dir: &Path,
    build_mode: BuildMode,
    target_profile: Option<&BuildToolchainProfile>,
) -> Result<CmakeBuildOutputs> {
    let toolchain_profile = target_profile.map(|profile| &profile.profile);
    let cmake_cross_compiling = target_profile
        .map(|profile| profile.is_cross)
        .unwrap_or(false);
    let source_dir = out_dir.join("build");
    let target_identity = bin_target_identity(target_profile);
    let build_dir = cmake_build_dir(out_dir, build_mode, target_identity.as_deref());
    let runtime_dir = cpp_runtime_dir_for_generated_build()?;
    let existing_prefix_paths = cmake_prefix_path_from_env();
    let toolchain_prefix_paths = toolchain_profile
        .map(toolchain_profile_cmake_prefix_paths)
        .unwrap_or_default();
    let target_sdk = toolchain_profile
        .filter(|_| cmake_cross_compiling)
        .map(|profile| resolve_cpp_target_sdk_for_build(runtime_dir.as_deref(), profile))
        .transpose()?;
    let cmake_runtime_dir = target_sdk
        .as_ref()
        .map(|sdk| sdk.root.as_path())
        .or(runtime_dir.as_deref());
    let cmake_prefix_paths = if let Some(sdk) = &target_sdk {
        cmake_prefix_paths_for_target_sdk(sdk, &toolchain_prefix_paths, &existing_prefix_paths)
    } else {
        cmake_prefix_paths_for_runtime(
            runtime_dir.as_deref(),
            &toolchain_prefix_paths,
            &existing_prefix_paths,
        )
    };
    run_cmake_configure(&CmakeConfigureSpec {
        source_dir: &source_dir,
        build_dir: &build_dir,
        runtime_dir: cmake_runtime_dir,
        cmake_prefix_paths: &cmake_prefix_paths,
        build_mode,
        toolchain_profile,
        cmake_cross_compiling,
        target_sdk: target_sdk.as_ref(),
    })
    .map_err(|error| {
        format_cmake_build_error(
            "cmake configure",
            &error,
            contract,
            build_mode,
            target_profile,
            target_sdk.as_ref(),
        )
    })?;
    run_cmake_build(&build_dir).map_err(|error| {
        format_cmake_build_error(
            "cmake build",
            &error,
            contract,
            build_mode,
            target_profile,
            target_sdk.as_ref(),
        )
    })?;
    let cpp_app = build_dir.join(cpp_app_executable_name(contract));
    let ros2_bridge = build_dir.join(ros2_bridge_executable_name(contract));
    Ok(CmakeBuildOutputs {
        cpp_app: has_cmake_app_components(contract)
            .then_some(cpp_app)
            .and_then(existing_executable),
        ros2_bridge: has_ros2_bridge(contract)
            .then_some(ros2_bridge)
            .and_then(existing_executable),
    })
}

pub(crate) fn has_cmake_app_components(contract: &ContractIr) -> bool {
    has_component_language(contract, LanguageKind::C)
        || has_component_language(contract, LanguageKind::Cpp)
}

pub(crate) fn cmake_build_dir(
    out_dir: &Path,
    build_mode: BuildMode,
    target_identity: Option<&str>,
) -> PathBuf {
    let mut build_dir = out_dir.join("build").join("cmake");
    if let Some(target_identity) = target_identity {
        build_dir = build_dir.join(target_identity);
    }
    build_dir.join(build_mode.cargo_profile_dir())
}

pub(crate) fn existing_executable(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

pub(crate) struct CmakeConfigureSpec<'a> {
    pub(crate) source_dir: &'a Path,
    pub(crate) build_dir: &'a Path,
    pub(crate) runtime_dir: Option<&'a Path>,
    pub(crate) cmake_prefix_paths: &'a [PathBuf],
    pub(crate) build_mode: BuildMode,
    pub(crate) toolchain_profile: Option<&'a ToolchainProfile>,
    pub(crate) cmake_cross_compiling: bool,
    pub(crate) target_sdk: Option<&'a CppTargetSdk>,
}

pub(crate) fn run_cmake_configure(spec: &CmakeConfigureSpec<'_>) -> Result<()> {
    let args = cmake_configure_args(
        spec.source_dir,
        spec.build_dir,
        spec.runtime_dir,
        spec.cmake_prefix_paths,
        spec.build_mode,
        spec.toolchain_profile,
        spec.cmake_cross_compiling,
    );
    let configure_env = cmake_configure_env(spec.toolchain_profile, spec.target_sdk)?;
    let mut command = ProcessCommand::new("cmake");
    command.args(args);
    for (key, value) in configure_env {
        command.env(key, value);
    }
    let status = command
        .status()
        .context("failed to spawn cmake configure")?;
    if !status.success() {
        anyhow::bail!("cmake configure failed with status {status}");
    }
    Ok(())
}

pub(crate) fn cmake_configure_args(
    source_dir: &Path,
    build_dir: &Path,
    runtime_dir: Option<&Path>,
    cmake_prefix_paths: &[PathBuf],
    build_mode: BuildMode,
    toolchain_profile: Option<&ToolchainProfile>,
    cmake_cross_compiling: bool,
) -> Vec<String> {
    let mut args = vec![
        "-S".to_string(),
        source_dir.to_string_lossy().into_owned(),
        "-B".to_string(),
        build_dir.to_string_lossy().into_owned(),
        format!("-DCMAKE_BUILD_TYPE={}", build_mode.cmake_build_type()),
    ];
    if let Some(runtime_dir) = runtime_dir {
        args.push(format!(
            "-DFLOWRT_CPP_RUNTIME_DIR={}",
            runtime_dir.to_string_lossy()
        ));
    }
    if !cmake_prefix_paths.is_empty() {
        args.push(format!(
            "-DCMAKE_PREFIX_PATH={}",
            join_cmake_prefix_paths(cmake_prefix_paths)
        ));
    }
    if repo_runtime_fallback_allowed() {
        args.push("-DFLOWRT_ALLOW_REPO_RUNTIME_FALLBACK=ON".to_string());
    }
    if let Some(profile) = toolchain_profile {
        if let Some(cmake_toolchain) = &profile.cmake_toolchain {
            args.push(format!(
                "-DCMAKE_TOOLCHAIN_FILE={}",
                cmake_toolchain.to_string_lossy()
            ));
        } else {
            args.push(format!("-DCMAKE_C_COMPILER={}", profile.c_compiler));
            args.push(format!("-DCMAKE_CXX_COMPILER={}", profile.cpp_compiler));
            if cmake_cross_compiling {
                args.push("-DCMAKE_SYSTEM_NAME=Linux".to_string());
                if let Some(processor) = cmake_system_processor_for_platform(&profile.platform) {
                    args.push(format!("-DCMAKE_SYSTEM_PROCESSOR={processor}"));
                }
            }
            if let Some(sysroot) = &profile.sysroot {
                args.push(format!("-DCMAKE_SYSROOT={}", sysroot.to_string_lossy()));
            }
        }
        if !profile.cpp_compile_args.is_empty() {
            args.push(format!(
                "-DFLOWRT_CXX_COMPILE_OPTIONS={}",
                join_cmake_list_values(&profile.cpp_compile_args)
            ));
        }
        if !profile.cpp_link_args.is_empty() {
            args.push(format!(
                "-DFLOWRT_EXE_LINK_OPTIONS={}",
                join_cmake_list_values(&profile.cpp_link_args)
            ));
        }
        if !profile.cpp_link_libraries.is_empty() {
            args.push(format!(
                "-DFLOWRT_EXE_LINK_LIBRARIES={}",
                join_cmake_list_values(&profile.cpp_link_libraries)
            ));
        }
    }
    args
}

pub(crate) fn join_cmake_list_values(values: &[String]) -> String {
    values.join(";")
}

pub(crate) fn cmake_system_processor_for_platform(platform: &str) -> Option<&'static str> {
    match platform {
        "linux-amd64" => Some("x86_64"),
        "linux-arm64" => Some("aarch64"),
        _ => None,
    }
}

pub(crate) fn cmake_configure_env(
    toolchain_profile: Option<&ToolchainProfile>,
    target_sdk: Option<&CppTargetSdk>,
) -> Result<BTreeMap<&'static str, OsString>> {
    let mut values = BTreeMap::new();
    let pkg_config_paths = pkg_config_search_paths(toolchain_profile, target_sdk);
    if !pkg_config_paths.is_empty() {
        let joined = env::join_paths(&pkg_config_paths).with_context(|| {
            format!(
                "failed to join PKG_CONFIG_LIBDIR paths: {}",
                format_path_list(&pkg_config_paths)
            )
        })?;
        if toolchain_profile.is_some_and(native_pkg_config_should_extend_system)
            && target_sdk.is_none()
        {
            values.insert(
                "PKG_CONFIG_PATH",
                prepend_env_paths("PKG_CONFIG_PATH", &pkg_config_paths)?,
            );
        } else {
            values.insert("PKG_CONFIG_LIBDIR", joined);
        }
    }
    Ok(values)
}

pub(crate) fn pkg_config_search_paths(
    toolchain_profile: Option<&ToolchainProfile>,
    target_sdk: Option<&CppTargetSdk>,
) -> Vec<PathBuf> {
    let mut pkg_config_paths = Vec::new();
    if let Some(profile) = toolchain_profile {
        if let Some(pkg_config_libdir) = &profile.pkg_config_libdir {
            push_unique_path(&mut pkg_config_paths, pkg_config_libdir);
        }
        for pkg_config_libdir in &profile.pkg_config_libdirs {
            push_unique_path(&mut pkg_config_paths, pkg_config_libdir);
        }
        for overlay_pkgconfig in toolchain_profile_overlay_pkgconfig_paths(profile) {
            push_unique_path(&mut pkg_config_paths, &overlay_pkgconfig);
        }
    }
    if let Some(sdk) = target_sdk
        && let Some(pkgconfig_dir) = &sdk.pkgconfig_dir
        && pkgconfig_dir.is_dir()
    {
        push_unique_path(&mut pkg_config_paths, pkgconfig_dir);
    }
    pkg_config_paths
}

pub(crate) fn cmake_prefix_path_from_env() -> Vec<PathBuf> {
    let Some(raw) = env::var_os("CMAKE_PREFIX_PATH") else {
        return Vec::new();
    };
    env::split_paths(&raw).collect()
}

pub(crate) fn cmake_prefix_paths_for_runtime(
    runtime_dir: Option<&Path>,
    toolchain_prefixes: &[PathBuf],
    existing: &[PathBuf],
) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    for prefix in toolchain_prefixes {
        push_unique_path(&mut prefixes, prefix);
    }
    for prefix in existing {
        push_unique_path(&mut prefixes, prefix);
    }
    if let Some(runtime_dir) = runtime_dir {
        push_unique_path(&mut prefixes, runtime_dir);
        if let Some(private_prefix) = flowrt_private_prefix_from_cpp_runtime_dir(runtime_dir) {
            push_unique_path(&mut prefixes, &private_prefix);
        }
    }
    prefixes
}

pub(crate) fn cmake_prefix_paths_for_target_sdk(
    sdk: &CppTargetSdk,
    toolchain_prefixes: &[PathBuf],
    existing: &[PathBuf],
) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    push_unique_path(&mut prefixes, &sdk.root);
    if let Some(cmake_dir) = &sdk.cmake_dir {
        push_unique_path(&mut prefixes, cmake_dir);
    }
    for prefix in toolchain_prefixes {
        push_unique_path(&mut prefixes, prefix);
    }
    for prefix in existing {
        push_unique_path(&mut prefixes, prefix);
    }
    prefixes
}

pub(crate) fn toolchain_profile_cmake_prefix_paths(profile: &ToolchainProfile) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();
    for prefix in &profile.cmake_prefix_paths {
        push_unique_path(&mut prefixes, prefix);
    }
    for overlay in &profile.sdk_overlays {
        push_unique_path(&mut prefixes, overlay);
        let cmake_dir = overlay.join("cmake");
        push_unique_path(&mut prefixes, &cmake_dir);
    }
    prefixes
}

pub(crate) fn toolchain_profile_overlay_pkgconfig_paths(
    profile: &ToolchainProfile,
) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for overlay in &profile.sdk_overlays {
        for candidate in [
            overlay.join("pkgconfig"),
            overlay.join("lib/pkgconfig"),
            overlay
                .join("lib")
                .join(&profile.deb_multiarch)
                .join("pkgconfig"),
        ] {
            push_unique_path(&mut paths, &candidate);
        }
    }
    paths
}

pub(crate) fn resolve_cpp_target_sdk_for_build(
    runtime_dir: Option<&Path>,
    profile: &ToolchainProfile,
) -> Result<CppTargetSdk> {
    let target_sdk_candidates = runtime_dir
        .map(|runtime_dir| cpp_target_sdk_root_candidates(runtime_dir, &profile.platform))
        .unwrap_or_default();
    resolve_cpp_target_sdk_root(runtime_dir, &profile.platform).map_err(|error| {
        anyhow::anyhow!(
            "build diagnostics: target={} PKG_CONFIG_PATH={} PKG_CONFIG_LIBDIR={} target_sdk_candidates={} sdk_overlays={} hint=run `{}` before retrying: {}",
            profile.platform,
            current_pkg_config_env_value(profile, None, "PKG_CONFIG_PATH"),
            current_pkg_config_libdir(profile, None),
            format_path_list(&target_sdk_candidates),
            format_path_list(&profile.sdk_overlays),
            build_doctor_hint(&profile.platform),
            error,
        )
    })
}

pub(crate) fn format_cmake_build_error(
    step: &str,
    error: &anyhow::Error,
    contract: &ContractIr,
    build_mode: BuildMode,
    target_profile: Option<&BuildToolchainProfile>,
    target_sdk: Option<&CppTargetSdk>,
) -> anyhow::Error {
    let target = target_profile
        .map(|profile| profile.profile.platform.as_str())
        .map(str::to_string)
        .or_else(|| contract_target_platform(contract))
        .or_else(|| host_flowrt_platform().map(str::to_string))
        .unwrap_or_else(|| "native".to_string());
    let doctor_hint = build_doctor_hint(&target);
    let pkg_config_modules = target_profile
        .map(|profile| build_pkg_config_modules(contract, &profile.profile.platform))
        .unwrap_or_default();
    let toolchain_line = target_profile
        .map(|profile| {
            format!(
                " c={} cxx={} runtime_deps={}",
                profile.profile.c_compiler,
                profile.profile.cpp_compiler,
                runtime_dependency_policy_name(profile.profile.runtime_dependency_policy),
            )
        })
        .unwrap_or_default();
    let pkg_config_libdir = target_profile
        .map(|profile| current_pkg_config_libdir(&profile.profile, target_sdk))
        .unwrap_or_else(|| "<unset>".to_string());
    let pkg_config_path = target_profile
        .map(|profile| {
            current_pkg_config_env_value(&profile.profile, target_sdk, "PKG_CONFIG_PATH")
        })
        .unwrap_or_else(|| "<unset>".to_string());
    let sdk_overlays = target_profile
        .map(|profile| format_path_list(&profile.profile.sdk_overlays))
        .unwrap_or_else(|| "<none>".to_string());
    anyhow::anyhow!(
        "{step} failed for target={target} mode={build_mode}{toolchain_line} PKG_CONFIG_PATH={pkg_config_path} PKG_CONFIG_LIBDIR={pkg_config_libdir} sdk_overlays={sdk_overlays} pkg-config={} hint=run `{doctor_hint}` before retrying: {error}",
        format_string_list(&pkg_config_modules),
    )
}

pub(crate) fn resolve_cpp_target_sdk_root(
    runtime_dir: Option<&Path>,
    platform: &str,
) -> Result<CppTargetSdk> {
    let runtime_dir = runtime_dir.with_context(|| {
        format!(
            "FlowRT target SDK for {platform} is missing; install a package that embeds this target SDK or configure FLOWRT_CPP_RUNTIME_DIR / toolchain profile to a complete SDK"
        )
    })?;
    let candidates = cpp_target_sdk_root_candidates(runtime_dir, platform);
    for candidate in &candidates {
        let manifest = candidate.join("flowrt-target-sdk.toml");
        if manifest.exists() {
            return read_cpp_target_sdk_manifest(candidate, platform);
        }
    }
    let searched = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!(
        "FlowRT target SDK for {platform} is missing at {searched}; install a package that embeds this target SDK or configure FLOWRT_CPP_RUNTIME_DIR / toolchain profile to a complete SDK"
    );
}

pub(crate) fn cpp_target_sdk_root_candidates(runtime_dir: &Path, platform: &str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if runtime_dir.file_name() == Some(OsStr::new(platform))
        || runtime_dir.join("flowrt-target-sdk.toml").exists()
    {
        push_unique_path(&mut candidates, runtime_dir);
    }
    push_unique_path(&mut candidates, &runtime_dir.join("targets").join(platform));
    if let Some(private_prefix) = flowrt_private_prefix_from_runtime_dir(runtime_dir) {
        push_unique_path(
            &mut candidates,
            &private_prefix.join("targets").join(platform),
        );
    }
    if let Some(private_prefix) = flowrt_private_prefix_from_cpp_runtime_dir(runtime_dir) {
        push_unique_path(
            &mut candidates,
            &private_prefix.join("targets").join(platform),
        );
    }
    candidates
}

pub(crate) fn read_cpp_target_sdk_manifest(root: &Path, platform: &str) -> Result<CppTargetSdk> {
    let manifest_path = root.join("flowrt-target-sdk.toml");
    let source = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read `{}`", manifest_path.display()))?;
    let manifest: CppTargetSdkManifest = toml::from_str(&source)
        .with_context(|| format!("failed to parse `{}`", manifest_path.display()))?;
    if manifest.platform != platform {
        anyhow::bail!(
            "FlowRT target SDK manifest platform `{}` does not match requested `{platform}` at {}",
            manifest.platform,
            manifest_path.display()
        );
    }
    if !manifest.complete {
        anyhow::bail!(
            "FlowRT target SDK for {platform} is incomplete at {}; install a package that embeds this target SDK or configure FLOWRT_CPP_RUNTIME_DIR / toolchain profile to a complete SDK",
            root.display()
        );
    }
    let _include_dir = manifest
        .include_dir
        .as_ref()
        .map(|path| target_sdk_manifest_path(root, path));
    let _lib_dir = manifest
        .lib_dir
        .as_ref()
        .map(|path| target_sdk_manifest_path(root, path));
    Ok(CppTargetSdk {
        root: root.to_path_buf(),
        cmake_dir: manifest
            .cmake_dir
            .as_ref()
            .map(|path| target_sdk_manifest_path(root, path)),
        pkgconfig_dir: manifest
            .pkgconfig_dir
            .as_ref()
            .map(|path| target_sdk_manifest_path(root, path)),
    })
}

pub(crate) fn target_sdk_manifest_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

pub(crate) fn flowrt_private_prefix_from_cpp_runtime_dir(runtime_dir: &Path) -> Option<PathBuf> {
    if runtime_dir.join("include/flowrt/runtime.hpp").exists()
        && runtime_dir.join("lib").is_dir()
        && runtime_dir.join("share").is_dir()
    {
        return Some(runtime_dir.to_path_buf());
    }
    None
}

pub(crate) fn push_unique_path(paths: &mut Vec<PathBuf>, path: &Path) {
    if !paths.iter().any(|existing| existing == path) {
        paths.push(path.to_path_buf());
    }
}

pub(crate) fn join_cmake_prefix_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join(";")
}

pub(crate) fn run_cmake_build(build_dir: &Path) -> Result<()> {
    let status = ProcessCommand::new("cmake")
        .arg("--build")
        .arg(build_dir)
        .status()
        .context("failed to spawn cmake build")?;
    if !status.success() {
        anyhow::bail!("cmake build failed with status {status}");
    }
    Ok(())
}

pub(crate) fn cpp_app_executable_name(contract: &ContractIr) -> String {
    format!(
        "{}_cpp_app{}",
        sanitize_package_name(&contract.package.name).replace('-', "_"),
        std::env::consts::EXE_SUFFIX
    )
}

pub(crate) fn ros2_bridge_executable_name(contract: &ContractIr) -> String {
    format!(
        "{}_ros2_bridge{}",
        sanitize_package_name(&contract.package.name).replace('-', "_"),
        std::env::consts::EXE_SUFFIX
    )
}

pub(crate) fn build_target_identity(target_profile: Option<&BuildToolchainProfile>) -> String {
    bin_target_identity(target_profile).unwrap_or_else(|| "native".to_string())
}

pub(crate) fn bin_target_identity(
    target_profile: Option<&BuildToolchainProfile>,
) -> Option<String> {
    target_profile
        .filter(|profile| profile.cargo_target_triple.is_some())
        .map(|profile| profile.profile.platform.clone())
}

pub(crate) fn copy_executable_to_local_bin(
    out_dir: &Path,
    build_mode: BuildMode,
    target_identity: Option<&str>,
    built: &Path,
) -> Result<PathBuf> {
    let file_name = built
        .file_name()
        .context("built executable path has no file name")?
        .to_owned();
    copy_executable_to_local_bin_as(
        out_dir,
        build_mode,
        target_identity,
        built,
        file_name.as_os_str(),
    )
}

pub(crate) fn copy_executable_to_local_bin_as(
    out_dir: &Path,
    build_mode: BuildMode,
    target_identity: Option<&str>,
    built: &Path,
    file_name: &std::ffi::OsStr,
) -> Result<PathBuf> {
    let mut destination = out_dir.join("build").join("bin");
    if let Some(target_identity) = target_identity {
        destination = destination.join(target_identity);
    }
    let destination = destination
        .join(build_mode.cargo_profile_dir())
        .join(file_name);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }
    fs::copy(built, &destination).with_context(|| {
        format!(
            "failed to copy built executable `{}` to `{}`",
            built.display(),
            destination.display()
        )
    })?;
    Ok(destination)
}

pub(crate) fn relative_to_out_dir(out_dir: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(out_dir)
        .map(Path::to_path_buf)
        .with_context(|| {
            format!(
                "built executable `{}` is not under FlowRT output directory `{}`",
                path.display(),
                out_dir.display()
            )
        })
}

pub(crate) fn run_cargo_build_bin(
    manifest: &Path,
    bin_name: &str,
    build_mode: BuildMode,
    target_dir: &Path,
    target_triple: Option<&str>,
    target_linker: Option<&str>,
) -> Result<PathBuf> {
    let invocation = cargo_build_invocation(
        manifest,
        bin_name,
        build_mode,
        target_dir,
        target_triple,
        target_linker,
    )?;
    ensure_rust_target_available(invocation.target_triple.as_deref())?;
    let mut command = ProcessCommand::new("cargo");
    command
        .current_dir(&invocation.current_dir)
        .env("CARGO_TARGET_DIR", &invocation.target_dir)
        .envs(invocation.env.iter().map(|(key, value)| (key, value)))
        .args(&invocation.args);
    let status = command.status().context("failed to spawn cargo")?;
    if !status.success() {
        bail_cargo_status(
            "cargo invocation",
            status,
            invocation.target_triple.as_deref(),
        )?;
    }
    Ok(invocation.executable_path())
}

#[derive(Debug, Clone)]
pub(crate) struct CargoInternalNames {
    pub(crate) app_stable: String,
    pub(crate) app_internal: String,
    pub(crate) supervisor_stable: String,
    pub(crate) supervisor_internal: String,
    pub(crate) package_internal: String,
}

pub(crate) fn cargo_internal_names(contract: &ContractIr) -> Result<CargoInternalNames> {
    let app_stable = app_bin_name(contract);
    let supervisor_stable = supervisor_bin_name(contract);
    let suffix = contract_source_hash_suffix(contract)?;
    Ok(CargoInternalNames {
        app_internal: format!("{app_stable}-{suffix}"),
        supervisor_internal: format!("{supervisor_stable}-{suffix}"),
        package_internal: format!("{app_stable}-{suffix}"),
        app_stable,
        supervisor_stable,
    })
}

pub(crate) fn contract_source_hash_suffix(contract: &ContractIr) -> Result<String> {
    let canonical = contract
        .to_canonical_json()
        .context("failed to serialize Contract IR for Cargo artifact namespace")?;
    Ok(hash_source(&canonical).chars().take(16).collect::<String>())
}

pub(crate) fn replace_cargo_manifest_name(
    manifest: &str,
    key: &str,
    from: &str,
    to: &str,
) -> Result<String> {
    let from_line = format!("{key} = \"{from}\"");
    let to_line = format!("{key} = \"{to}\"");
    if !manifest.contains(&from_line) {
        anyhow::bail!("generated Cargo manifest is missing `{from_line}`");
    }
    Ok(manifest.replacen(&from_line, &to_line, 1))
}

pub(crate) fn rewrite_cargo_manifest_for_internal_names(
    manifest: &str,
    names: &CargoInternalNames,
) -> Result<String> {
    let package_line = format!("name = \"{}\"", names.package_internal);
    let mut rewritten = if manifest.contains(&package_line) {
        manifest.to_string()
    } else {
        replace_cargo_manifest_name(manifest, "name", &names.app_stable, &names.package_internal)?
    };
    if rewritten.contains(&format!("name = \"{}\"", names.app_stable)) {
        rewritten = replace_cargo_manifest_name(
            &rewritten,
            "name",
            &names.app_stable,
            &names.app_internal,
        )?;
    }
    if rewritten.contains(&format!("name = \"{}\"", names.supervisor_stable)) {
        rewritten = replace_cargo_manifest_name(
            &rewritten,
            "name",
            &names.supervisor_stable,
            &names.supervisor_internal,
        )?;
    }
    Ok(rewritten)
}

pub(crate) fn cargo_build_manifest_with_runtime_patch(
    out_dir: &Path,
    runtime_dir: Option<&Path>,
    names: &CargoInternalNames,
) -> Result<PathBuf> {
    let generated_manifest = out_dir.join("build").join("Cargo.toml");
    let generated = fs::read_to_string(&generated_manifest)
        .with_context(|| format!("failed to read `{}`", generated_manifest.display()))?;
    let rewritten = rewrite_cargo_manifest_for_internal_names(&generated, names)?;
    if generated != rewritten {
        fs::write(&generated_manifest, rewritten)
            .with_context(|| format!("failed to write `{}`", generated_manifest.display()))?;
    }
    cargo_manifest_with_runtime_patch(out_dir, runtime_dir)
}

pub(crate) struct CargoBuildInvocation {
    pub(crate) current_dir: PathBuf,
    pub(crate) args: Vec<String>,
    pub(crate) target_dir: PathBuf,
    pub(crate) target_triple: Option<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) bin_name: String,
    pub(crate) build_mode: BuildMode,
}

impl CargoBuildInvocation {
    pub(crate) fn profile_dir(&self) -> PathBuf {
        let target_dir = if let Some(target_triple) = &self.target_triple {
            self.target_dir.join(target_triple)
        } else {
            self.target_dir.clone()
        };
        target_dir.join(self.build_mode.cargo_profile_dir())
    }

    pub(crate) fn executable_path(&self) -> PathBuf {
        self.profile_dir()
            .join(format!("{}{}", self.bin_name, std::env::consts::EXE_SUFFIX))
    }
}

pub(crate) fn cargo_build_invocation(
    manifest: &Path,
    bin_name: &str,
    build_mode: BuildMode,
    target_dir: &Path,
    target_triple: Option<&str>,
    target_linker: Option<&str>,
) -> Result<CargoBuildInvocation> {
    let manifest = fs::canonicalize(manifest)
        .with_context(|| format!("failed to resolve `{}`", manifest.display()))?;
    let manifest_dir = manifest
        .parent()
        .with_context(|| format!("manifest path has no parent: `{}`", manifest.display()))?
        .to_path_buf();
    let mut args = vec![
        "build".to_string(),
        "--manifest-path".to_string(),
        manifest.to_string_lossy().into_owned(),
        "--bin".to_string(),
        bin_name.to_string(),
    ];
    args.extend(build_mode.cargo_args().iter().map(|arg| (*arg).to_string()));
    args.extend(cargo_target_args(target_triple));
    if manifest_dir.join(".cargo").join("config.toml").exists() {
        args.push("--offline".to_string());
    }
    let env = cargo_target_linker_env(target_triple, target_linker)
        .into_iter()
        .collect();
    Ok(CargoBuildInvocation {
        current_dir: manifest_dir,
        args,
        target_dir: target_dir.to_path_buf(),
        target_triple: target_triple.map(str::to_string),
        env,
        bin_name: bin_name.to_string(),
        build_mode,
    })
}
