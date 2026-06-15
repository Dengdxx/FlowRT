use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DoctorLevel {
    Ok,
    Warn,
    Error,
}

impl DoctorLevel {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            DoctorLevel::Ok => "ok",
            DoctorLevel::Warn => "warn",
            DoctorLevel::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DoctorCheck {
    pub(crate) label: &'static str,
    pub(crate) level: DoctorLevel,
    pub(crate) detail: String,
}

impl DoctorCheck {
    pub(crate) fn ok(label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            label,
            level: DoctorLevel::Ok,
            detail: detail.into(),
        }
    }

    pub(crate) fn warn(label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            label,
            level: DoctorLevel::Warn,
            detail: detail.into(),
        }
    }

    pub(crate) fn error(label: &'static str, detail: impl Into<String>) -> Self {
        Self {
            label,
            level: DoctorLevel::Error,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DoctorReport {
    pub(crate) header_lines: Vec<String>,
    pub(crate) checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DoctorPkgConfigRequirement {
    pub(crate) component: String,
    pub(crate) module: String,
}

pub(crate) fn run_doctor(rsdl: Option<&Path>, target: Option<&str>) -> Result<()> {
    let report = collect_doctor_report(rsdl, target)?;

    println!("FlowRT doctor");
    for line in &report.header_lines {
        println!("{line}");
    }

    let mut errors = 0usize;
    for check in &report.checks {
        println!(
            "{}: {} - {}",
            check.label,
            check.level.as_str(),
            check.detail
        );
        if check.level == DoctorLevel::Error {
            errors += 1;
        }
    }
    if errors > 0 {
        anyhow::bail!("FlowRT doctor found {errors} error(s)");
    }
    Ok(())
}

pub(crate) fn collect_doctor_report(
    rsdl: Option<&Path>,
    target: Option<&str>,
) -> Result<DoctorReport> {
    let workspace_root = match rsdl {
        Some(rsdl) => application_root_from_rsdl(rsdl)?,
        None => env::current_dir().context("failed to resolve current working directory")?,
    };
    let selected_contract = match rsdl {
        Some(rsdl) => Some(load_selected_contract_from_rsdl(rsdl)?),
        None => None,
    };
    let target_profile =
        resolve_doctor_toolchain_profile(selected_contract.as_ref(), target, &workspace_root)?;
    let mut checks = Vec::new();
    let mut header_lines = Vec::new();

    checks.push(command_check("cargo", "cargo"));
    checks.push(command_check("cmake", "cmake"));
    checks.push(command_check("pkg-config", "pkg-config"));

    if let Some(target_profile) = &target_profile {
        let profile = &target_profile.profile;
        header_lines.push(format!("target platform: {}", profile.platform));
        header_lines.push(format!("rust target: {}", profile.rust_target));
        header_lines.push(format!("deb multiarch: {}", profile.deb_multiarch));
        header_lines.push(format!(
            "runtime dependency policy: {}",
            runtime_dependency_policy_name(profile.runtime_dependency_policy)
        ));
        if let Some(rsdl) = rsdl {
            header_lines.push(format!("rsdl: {}", rsdl.display()));
        }
        if let Some(contract) = &selected_contract
            && let Some(profile_name) = selected_prepared_profile_name(contract)
        {
            header_lines.push(format!("contract profile: {profile_name}"));
        }

        checks.push(rust_target_check(
            target_profile.cargo_target_triple.as_deref(),
        ));
        checks.push(command_check("C compiler", &profile.c_compiler));
        checks.push(command_check("C++ compiler", &profile.cpp_compiler));
        if let Some(sysroot) = &profile.sysroot {
            checks.push(path_check("sysroot", sysroot, true));
        }
        if let Some(cmake_toolchain) = &profile.cmake_toolchain {
            checks.push(doctor_cmake_toolchain_check(
                &profile.platform,
                cmake_toolchain,
            ));
        }
        let (target_sdk, target_sdk_check) = target_sdk_check_with_resolved_sdk(&profile.platform);
        checks.push(target_sdk_check);
        for path in profile
            .pkg_config_libdir
            .iter()
            .chain(profile.pkg_config_libdirs.iter())
        {
            checks.push(doctor_pkg_config_path_check(&profile.platform, path));
        }
        for path in &profile.cmake_prefix_paths {
            checks.push(path_check("cmake prefix", path, false));
        }
        for overlay in &profile.sdk_overlays {
            checks.push(doctor_sdk_overlay_check(&profile.platform, overlay));
        }
        if let Some(contract) = &selected_contract {
            checks.extend(doctor_contract_pkg_config_checks(
                contract,
                target_profile,
                target_sdk.as_ref(),
            )?);
        }
    } else {
        let (_, host_target) = rustc_toolchain_identity()?;
        header_lines.push("target platform: native".to_string());
        header_lines.push(format!("rust target: {host_target}"));
        if let Some(rsdl) = rsdl {
            header_lines.push(format!("rsdl: {}", rsdl.display()));
        }
        checks.push(DoctorCheck::ok(
            "rust target",
            format!("native host target {host_target}"),
        ));
        if let Some(contract) = &selected_contract {
            if contract_has_any_cpp_pkg_config_requirements(contract) {
                checks.push(DoctorCheck::warn(
                    "pkg-config dependencies",
                    "contract has C++ component pkg_config dependencies but no target platform was selected; pass `--target <platform>` or declare target.platform in RSDL".to_string(),
                ));
            } else {
                checks.push(DoctorCheck::ok(
                    "pkg-config dependencies",
                    "selected profile has no C++ component pkg_config dependencies".to_string(),
                ));
            }
        }
    }

    Ok(DoctorReport {
        header_lines,
        checks,
    })
}

pub(crate) fn load_selected_contract_from_rsdl(path: &Path) -> Result<ContractIr> {
    let contract = normalize_contract_from_rsdl(path)?;
    let selected_contract = project_contract_to_profile(&contract, None)
        .with_context(|| format!("failed to select profile for `{}`", path.display()))?;
    validate_contract(&selected_contract).context("contract validation failed")?;
    Ok(selected_contract)
}

pub(crate) fn resolve_doctor_toolchain_profile(
    selected_contract: Option<&ContractIr>,
    explicit_target: Option<&str>,
    workspace_root: &Path,
) -> Result<Option<BuildToolchainProfile>> {
    let platform = explicit_target
        .map(str::to_string)
        .or_else(|| selected_contract.and_then(contract_target_platform));
    resolve_optional_toolchain_profile(
        platform.as_deref(),
        explicit_target.is_some(),
        workspace_root,
    )
}

pub(crate) fn contract_has_any_cpp_pkg_config_requirements(contract: &ContractIr) -> bool {
    contract.components.iter().any(|component| {
        component.language == LanguageKind::Cpp && !component.build.pkg_config.is_empty()
    })
}

pub(crate) fn doctor_contract_pkg_config_checks(
    contract: &ContractIr,
    target_profile: &BuildToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
) -> Result<Vec<DoctorCheck>> {
    let requirements =
        selected_cpp_pkg_config_requirements(contract, &target_profile.profile.platform);
    if requirements.is_empty() {
        return Ok(vec![DoctorCheck::ok(
            "pkg-config dependencies",
            "selected profile has no C++ component pkg_config dependencies".to_string(),
        )]);
    }

    let search_paths = pkg_config_search_paths(Some(&target_profile.profile), target_sdk);
    if !command_available("pkg-config") {
        return Ok(requirements
            .into_iter()
            .map(|requirement| {
                DoctorCheck::error(
                    "pkg-config module",
                    format!(
                        "component={} module={} status=missing reason=`pkg-config` not found in PATH; install pkg-config before checking target dependencies",
                        requirement.component, requirement.module
                    ),
                )
            })
            .collect());
    }

    requirements
        .into_iter()
        .map(|requirement| {
            doctor_pkg_config_module_check(
                &requirement,
                &target_profile.profile,
                target_sdk,
                &search_paths,
            )
        })
        .collect()
}

pub(crate) fn selected_cpp_pkg_config_requirements(
    contract: &ContractIr,
    selected_platform: &str,
) -> Vec<DoctorPkgConfigRequirement> {
    let selected_target_ids = contract
        .targets
        .iter()
        .filter(|target| {
            target.platform.map(|platform| platform.as_str()) == Some(selected_platform)
        })
        .map(|target| target.id.clone())
        .collect::<BTreeSet<_>>();
    let include_targetless_instances = contract.targets.len() == 1
        && contract
            .targets
            .first()
            .and_then(|target| target.platform.map(|platform| platform.as_str()))
            == Some(selected_platform);

    let mut selected_components = BTreeSet::new();
    for graph in &contract.graphs {
        for instance in &graph.instances {
            let matches_target = match &instance.target {
                Some(target) => selected_target_ids.contains(&target.id),
                None => include_targetless_instances,
            };
            if matches_target {
                selected_components.insert(instance.component.name.clone());
            }
        }
    }

    let fallback_to_all_cpp_components = selected_components.is_empty();
    let mut requirements = BTreeSet::new();
    for component in &contract.components {
        let is_selected_component = fallback_to_all_cpp_components
            || selected_components.contains(&component.qualified_name);
        if !is_selected_component
            || component.language != LanguageKind::Cpp
            || component.build.pkg_config.is_empty()
        {
            continue;
        }
        for module in &component.build.pkg_config {
            requirements.insert(DoctorPkgConfigRequirement {
                component: component.qualified_name.clone(),
                module: module.clone(),
            });
        }
    }
    requirements.into_iter().collect()
}

pub(crate) fn doctor_pkg_config_module_check(
    requirement: &DoctorPkgConfigRequirement,
    profile: &ToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
    search_paths: &[PathBuf],
) -> Result<DoctorCheck> {
    let exists = pkg_config_module_exists(requirement.module.as_str(), profile, target_sdk)?;
    if !exists {
        let pkg_config_libdirs = profile
            .pkg_config_libdir
            .iter()
            .chain(profile.pkg_config_libdirs.iter())
            .cloned()
            .collect::<Vec<_>>();
        return Ok(DoctorCheck::error(
            "pkg-config module",
            format!(
                "component={} module={} status=missing pkg_config_libdirs={} search_paths={} sdk_overlays={} hint=prepare the external SDK first; if it lives in an overlay, run `flowrt toolchain init --target {} --sdk-overlay <path>`",
                requirement.component,
                requirement.module,
                format_path_list(&pkg_config_libdirs),
                format_path_list(search_paths),
                format_path_list(&profile.sdk_overlays),
                profile.platform,
            ),
        ));
    }

    let pc_dir = pkg_config_module_variable(
        requirement.module.as_str(),
        "pcfiledir",
        profile,
        target_sdk,
    )?
    .map(PathBuf::from);
    let pc_path =
        find_pkg_config_pc_path(requirement.module.as_str(), pc_dir.as_deref(), search_paths);
    let include_dirs = pkg_config_module_flag_paths(
        requirement.module.as_str(),
        "--cflags-only-I",
        "-I",
        profile,
        target_sdk,
    )?;
    let lib_dirs = pkg_config_module_flag_paths(
        requirement.module.as_str(),
        "--libs-only-L",
        "-L",
        profile,
        target_sdk,
    )?;
    Ok(DoctorCheck::ok(
        "pkg-config module",
        format!(
            "component={} module={} status=found pc={} include_dirs={} lib_dirs={}",
            requirement.component,
            requirement.module,
            pc_path
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<unknown>".to_string()),
            format_path_list(&include_dirs),
            format_path_list(&lib_dirs),
        ),
    ))
}

pub(crate) fn pkg_config_module_exists(
    module: &str,
    profile: &ToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
) -> Result<bool> {
    let status = pkg_config_command(["--exists", module], profile, target_sdk)?.status;
    Ok(status.success())
}

pub(crate) fn pkg_config_module_variable(
    module: &str,
    variable: &str,
    profile: &ToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
) -> Result<Option<String>> {
    let output = pkg_config_command(
        [format!("--variable={variable}"), module.to_string()],
        profile,
        target_sdk,
    )?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8(output.stdout)
        .with_context(|| format!("pkg-config output for module `{module}` is not UTF-8"))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

pub(crate) fn pkg_config_module_flag_paths(
    module: &str,
    flag: &str,
    prefix: &str,
    profile: &ToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
) -> Result<Vec<PathBuf>> {
    let output = pkg_config_command([flag.to_string(), module.to_string()], profile, target_sdk)?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("pkg-config output for module `{module}` is not UTF-8"))?;
    let mut paths = Vec::new();
    for token in stdout.split_whitespace() {
        if let Some(path) = token.strip_prefix(prefix) {
            push_unique_path(&mut paths, Path::new(path));
        }
    }
    Ok(paths)
}

pub(crate) fn pkg_config_command<I, S>(
    args: I,
    profile: &ToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
) -> Result<std::process::Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = ProcessCommand::new("pkg-config");
    command.args(args);
    for (key, value) in doctor_pkg_config_env(profile, target_sdk)? {
        command.env(key, value);
    }
    if profile.sysroot.is_none() {
        command.env_remove("PKG_CONFIG_SYSROOT_DIR");
    }
    command
        .output()
        .with_context(|| format!("failed to run pkg-config for target `{}`", profile.platform))
}

pub(crate) fn doctor_pkg_config_env(
    profile: &ToolchainProfile,
    target_sdk: Option<&CppTargetSdk>,
) -> Result<BTreeMap<&'static str, OsString>> {
    let mut values = BTreeMap::new();
    let search_paths = pkg_config_search_paths(Some(profile), target_sdk);
    if native_pkg_config_should_extend_system(profile) && target_sdk.is_none() {
        if !search_paths.is_empty() {
            values.insert(
                "PKG_CONFIG_PATH",
                prepend_env_paths("PKG_CONFIG_PATH", &search_paths)?,
            );
        }
        return Ok(values);
    }
    let joined = if search_paths.is_empty() {
        OsString::new()
    } else {
        env::join_paths(&search_paths).with_context(|| {
            format!(
                "failed to join PKG_CONFIG_LIBDIR paths for target `{}`: {}",
                profile.platform,
                format_path_list(&search_paths)
            )
        })?
    };
    values.insert("PKG_CONFIG_LIBDIR", joined);
    values.insert("PKG_CONFIG_PATH", OsString::new());
    if let Some(sysroot) = &profile.sysroot {
        values.insert("PKG_CONFIG_SYSROOT_DIR", sysroot.as_os_str().to_os_string());
    }
    Ok(values)
}

pub(crate) fn native_pkg_config_should_extend_system(profile: &ToolchainProfile) -> bool {
    host_flowrt_platform().is_some_and(|platform| profile.platform == platform)
}

pub(crate) fn find_pkg_config_pc_path(
    module: &str,
    pc_dir: Option<&Path>,
    search_paths: &[PathBuf],
) -> Option<PathBuf> {
    let module_file = format!("{module}.pc");
    if let Some(pc_dir) = pc_dir {
        let candidate = pc_dir.join(&module_file);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    search_paths
        .iter()
        .map(|path| path.join(&module_file))
        .find(|candidate| candidate.is_file())
}

pub(crate) fn format_path_list(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return "<none>".to_string();
    }
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn runtime_dependency_policy_name(policy: RuntimeDependencyPolicy) -> &'static str {
    match policy {
        RuntimeDependencyPolicy::System => "system",
        RuntimeDependencyPolicy::Bundle => "bundle",
        RuntimeDependencyPolicy::External => "external",
    }
}

pub(crate) fn toolchain_show(target: &str, workspace_root: &Path) -> Result<String> {
    let platform = canonical_toolchain_platform(target)?;
    let (profile, field_sources) =
        resolve_toolchain_profile_with_field_sources(&platform, workspace_root)?;
    Ok(format_toolchain_show(&profile, &field_sources))
}

pub(crate) fn format_toolchain_show(
    profile: &ToolchainProfile,
    field_sources: &ToolchainFieldSources,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "platform: {} (source: {})",
        profile.platform, field_sources.platform_source
    ));
    lines.push(format!(
        "rust_target: {} (source: {})",
        profile.rust_target, field_sources.rust_target_source
    ));
    lines.push(format!(
        "deb_multiarch: {} (source: builtin)",
        profile.deb_multiarch
    ));
    lines.push(format!(
        "c_compiler: {} (source: {})",
        profile.c_compiler, field_sources.c_compiler_source
    ));
    lines.push(format!(
        "cpp_compiler: {} (source: {})",
        profile.cpp_compiler, field_sources.cpp_compiler_source
    ));
    lines.push(format!(
        "sysroot: {} (source: {})",
        profile
            .sysroot
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".to_string()),
        field_sources.sysroot_source
    ));
    lines.push(format!(
        "cmake_toolchain: {} (source: {})",
        profile
            .cmake_toolchain
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".to_string()),
        field_sources.cmake_toolchain_source
    ));
    lines.push(format!(
        "pkg_config_libdir: {} (source: {})",
        profile
            .pkg_config_libdir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(none)".to_string()),
        field_sources.pkg_config_libdir_source
    ));
    if !profile.pkg_config_libdirs.is_empty() {
        lines.push(format!(
            "pkg_config_libdirs: {}",
            profile
                .pkg_config_libdirs
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !profile.cmake_prefix_paths.is_empty() {
        lines.push(format!(
            "cmake_prefix_paths: {}",
            profile
                .cmake_prefix_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !profile.sdk_overlays.is_empty() {
        lines.push(format!(
            "sdk_overlays: {}",
            profile
                .sdk_overlays
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !profile.cpp_compile_args.is_empty() {
        lines.push(format!(
            "cpp_compile_args: {}",
            profile.cpp_compile_args.join(" ")
        ));
    }
    if !profile.cpp_link_args.is_empty() {
        lines.push(format!(
            "cpp_link_args: {}",
            profile.cpp_link_args.join(" ")
        ));
    }
    if !profile.cpp_link_libraries.is_empty() {
        lines.push(format!(
            "cpp_link_libraries: {}",
            profile.cpp_link_libraries.join(", ")
        ));
    }
    lines.push(format!(
        "runtime_dependency_policy: {} (source: {})",
        runtime_dependency_policy_name(profile.runtime_dependency_policy),
        field_sources.runtime_dependency_policy_source
    ));
    lines.push(String::new());
    lines.push("source priority: builtin < system < user < workspace < CLI override".to_string());
    lines.join("\n")
}

pub(crate) fn toolchain_init(
    target: &str,
    sdk_overlays: &[PathBuf],
    force: bool,
    workspace_root: &Path,
) -> Result<String> {
    let platform = canonical_toolchain_platform(target)?;
    let config_path = workspace_root.join(".flowrt").join("toolchains.toml");

    if config_path.exists() && !force {
        anyhow::bail!(
            "toolchain config `{}` already exists; use `--force` to overwrite",
            config_path.display()
        );
    }

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create `{}`", parent.display()))?;
    }

    let sdk_overlays = sdk_overlays
        .iter()
        .map(|path| {
            if path.is_absolute() {
                path.clone()
            } else {
                workspace_root.join(path)
            }
        })
        .collect::<Vec<_>>();
    let toml_content = generate_toolchain_init_toml(&platform, &sdk_overlays)?;
    fs::write(&config_path, &toml_content)
        .with_context(|| format!("failed to write `{}`", config_path.display()))?;

    Ok(format!(
        "wrote toolchain config to `{}`",
        config_path.display()
    ))
}

pub(crate) fn command_check(label: &'static str, command: &str) -> DoctorCheck {
    if command_available(command) {
        DoctorCheck::ok(label, command.to_string())
    } else {
        DoctorCheck::error(
            label,
            format!("`{command}` not found in PATH; install or configure the toolchain profile"),
        )
    }
}

pub(crate) fn rust_target_check(target_triple: Option<&str>) -> DoctorCheck {
    let Some(target_triple) = target_triple else {
        return DoctorCheck::ok("rust target", "native target");
    };
    let output = match ProcessCommand::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return DoctorCheck::warn(
                "rust target",
                format!("rustup not found; cannot confirm `{target_triple}` is installed"),
            );
        }
        Err(error) => {
            return DoctorCheck::error("rust target", format!("failed to run rustup: {error}"));
        }
    };
    if !output.status.success() {
        return DoctorCheck::warn(
            "rust target",
            format!("rustup target list failed with status {}", output.status),
        );
    }
    let installed = String::from_utf8_lossy(&output.stdout);
    if installed.lines().any(|line| line.trim() == target_triple) {
        DoctorCheck::ok("rust target", target_triple.to_string())
    } else {
        DoctorCheck::error(
            "rust target",
            format!("`{target_triple}` is missing; run `rustup target add {target_triple}`"),
        )
    }
}

pub(crate) fn target_sdk_check_with_resolved_sdk(
    platform: &str,
) -> (Option<CppTargetSdk>, DoctorCheck) {
    let runtime_dir = match cpp_runtime_dir_for_generated_build() {
        Ok(runtime_dir) => runtime_dir,
        Err(error) => {
            return (
                None,
                DoctorCheck::error(
                    "target SDK",
                    format!("failed to resolve FlowRT C++ runtime directory: {error}"),
                ),
            );
        }
    };
    match resolve_cpp_target_sdk_root(runtime_dir.as_deref(), platform) {
        Ok(sdk) => (
            Some(sdk.clone()),
            DoctorCheck::ok("target SDK", sdk.root.display().to_string()),
        ),
        Err(error) => (None, DoctorCheck::error("target SDK", error.to_string())),
    }
}

pub(crate) fn path_check(label: &'static str, path: &Path, required: bool) -> DoctorCheck {
    if path.is_dir() {
        DoctorCheck::ok(label, path.display().to_string())
    } else if required {
        DoctorCheck::error(label, format!("missing directory `{}`", path.display()))
    } else {
        DoctorCheck::warn(label, format!("missing directory `{}`", path.display()))
    }
}

pub(crate) fn doctor_sdk_overlay_check(platform: &str, path: &Path) -> DoctorCheck {
    if path.is_dir() {
        DoctorCheck::ok("sdk overlay", path.display().to_string())
    } else {
        DoctorCheck::error(
            "sdk overlay",
            format!(
                "missing SDK overlay directory `{}`; prepare or mount the private SDK, then run `flowrt toolchain init --target {platform} --sdk-overlay {}` and retry `{}`",
                path.display(),
                path.display(),
                build_doctor_hint(platform)
            ),
        )
    }
}

pub(crate) fn doctor_pkg_config_path_check(platform: &str, path: &Path) -> DoctorCheck {
    if path.is_dir() {
        DoctorCheck::ok("pkg-config path", path.display().to_string())
    } else {
        DoctorCheck::warn(
            "pkg-config path",
            format!(
                "missing pkg-config directory `{}`; prepare the SDK overlay or add the directory to `.flowrt/toolchains.toml`, then retry `{}`",
                path.display(),
                build_doctor_hint(platform)
            ),
        )
    }
}

pub(crate) fn doctor_cmake_toolchain_check(platform: &str, path: &Path) -> DoctorCheck {
    if path.is_file() {
        DoctorCheck::ok("cmake toolchain", path.display().to_string())
    } else {
        DoctorCheck::error(
            "cmake toolchain",
            format!(
                "missing CMake toolchain file `{}`; create it or update `cmake_toolchain` in `.flowrt/toolchains.toml`, then retry `{}`",
                path.display(),
                build_doctor_hint(platform)
            ),
        )
    }
}

pub(crate) fn command_available(command: &str) -> bool {
    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.is_file();
    }
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|dir| dir.join(command).is_file())
}
