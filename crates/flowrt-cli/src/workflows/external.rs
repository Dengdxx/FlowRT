use super::*;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalPackageManifest {
    pub(crate) package: ExternalPackageMetadata,
    #[serde(default)]
    pub(crate) executable: Vec<ExternalExecutableMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalPackageMetadata {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) flowrt_version: String,
    pub(crate) license: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalExecutableMetadata {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) platforms: Vec<String>,
    pub(crate) backends: Vec<String>,
    pub(crate) health: String,
}

pub(crate) fn external_check_package_dir(package_dir: &Path) -> Result<String> {
    let manifest = load_external_manifest(package_dir)?;
    validate_external_manifest(package_dir, &manifest)?;
    Ok(format!(
        "external package `{}` version={} executable_count={}",
        manifest.package.name,
        manifest.package.version,
        manifest.executable.len()
    ))
}

pub(crate) fn external_list_packages(path: &Path) -> Result<String> {
    let mut package_dirs = Vec::new();
    if path.join("flowrt-external.toml").is_file() {
        package_dirs.push(path.to_path_buf());
    } else {
        for entry in fs::read_dir(path)
            .with_context(|| format!("failed to read external package path `{}`", path.display()))?
        {
            let entry = entry.with_context(|| {
                format!("failed to read external package path `{}`", path.display())
            })?;
            let child = entry.path();
            if child.join("flowrt-external.toml").is_file() {
                package_dirs.push(child);
            }
        }
    }
    package_dirs.sort();
    if package_dirs.is_empty() {
        anyhow::bail!(
            "no FlowRT external packages found under `{}`",
            path.display()
        );
    }

    let mut lines = Vec::new();
    for package_dir in package_dirs {
        let manifest = load_external_manifest(&package_dir)?;
        validate_external_manifest(&package_dir, &manifest)?;
        let executables = manifest
            .executable
            .iter()
            .map(|executable| {
                let platforms = canonical_external_platforms(&executable.platforms).join(",");
                format!(
                    "{} platforms=[{}] backends=[{}] health={}",
                    executable.name,
                    platforms,
                    executable.backends.join(","),
                    executable.health
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!(
            "package={} version={} path={} executables={}",
            manifest.package.name,
            manifest.package.version,
            package_dir.display(),
            executables
        ));
    }
    Ok(lines.join("\n"))
}

pub(crate) fn load_external_manifest(package_dir: &Path) -> Result<ExternalPackageManifest> {
    let path = package_dir.join("flowrt-external.toml");
    let source = fs::read_to_string(&path)
        .with_context(|| format!("failed to read external manifest `{}`", path.display()))?;
    toml::from_str(&source)
        .with_context(|| format!("failed to parse external manifest `{}`", path.display()))
}

pub(crate) fn validate_external_manifest(
    package_dir: &Path,
    manifest: &ExternalPackageManifest,
) -> Result<()> {
    ensure_non_empty_manifest_field(&manifest.package.name, "package.name")?;
    ensure_non_empty_manifest_field(&manifest.package.version, "package.version")?;
    ensure_non_empty_manifest_field(&manifest.package.flowrt_version, "package.flowrt_version")?;
    ensure_non_empty_manifest_field(&manifest.package.license, "package.license")?;
    if manifest.executable.is_empty() {
        anyhow::bail!(
            "external manifest `{}` must declare at least one [[executable]]",
            package_dir.join("flowrt-external.toml").display()
        );
    }

    let mut names = std::collections::BTreeSet::new();
    for executable in &manifest.executable {
        ensure_non_empty_manifest_field(&executable.name, "executable.name")?;
        if !names.insert(executable.name.as_str()) {
            anyhow::bail!(
                "external package `{}` declares executable `{}` more than once",
                manifest.package.name,
                executable.name
            );
        }
        if executable.path.as_os_str().is_empty() {
            anyhow::bail!(
                "external package `{}` executable `{}` has empty path",
                manifest.package.name,
                executable.name
            );
        }
        let exe_path =
            validate_manifest_executable_path(package_dir, &manifest.package.name, executable)?;
        if !exe_path.is_file() {
            anyhow::bail!(
                "external package `{}` executable `{}` path does not exist: {}",
                manifest.package.name,
                executable.name,
                exe_path.display()
            );
        }
        if executable.platforms.is_empty() {
            anyhow::bail!(
                "external package `{}` executable `{}` must declare at least one platform",
                manifest.package.name,
                executable.name
            );
        }
        for platform in &executable.platforms {
            if TargetPlatform::parse_alias(platform).is_none() {
                anyhow::bail!(
                    "external package `{}` executable `{}` declares unsupported platform `{}`",
                    manifest.package.name,
                    executable.name,
                    platform
                );
            }
        }
        if executable.backends.is_empty() {
            anyhow::bail!(
                "external package `{}` executable `{}` must declare at least one backend",
                manifest.package.name,
                executable.name
            );
        }
        for backend in &executable.backends {
            if !flowrt_ir::is_known_backend(backend) {
                anyhow::bail!(
                    "external package `{}` executable `{}` declares unknown backend `{}`",
                    manifest.package.name,
                    executable.name,
                    backend
                );
            }
        }
        if !matches!(
            executable.health.as_str(),
            "process_started" | "runtime_socket"
        ) {
            anyhow::bail!(
                "external package `{}` executable `{}` declares unsupported health `{}`",
                manifest.package.name,
                executable.name,
                executable.health
            );
        }
    }
    Ok(())
}

pub(crate) fn canonical_external_platforms(platforms: &[String]) -> Vec<String> {
    let mut canonical = platforms
        .iter()
        .filter_map(|platform| TargetPlatform::parse_alias(platform).map(|value| value.as_str()))
        .map(str::to_string)
        .collect::<Vec<_>>();
    canonical.sort();
    canonical.dedup();
    canonical
}

pub(crate) fn validate_manifest_executable_path(
    package_dir: &Path,
    package_name: &str,
    executable: &ExternalExecutableMetadata,
) -> Result<PathBuf> {
    let path = &executable.path;
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        anyhow::bail!(
            "external package `{}` executable `{}` path must be package-relative without `.` or `..` components",
            package_name,
            executable.name
        );
    }
    let exe_path = package_dir.join(path);
    if exe_path.exists() {
        let package_root = package_dir.canonicalize().with_context(|| {
            format!(
                "failed to canonicalize external package root `{}`",
                package_dir.display()
            )
        })?;
        let canonical_exe = exe_path.canonicalize().with_context(|| {
            format!(
                "failed to canonicalize external package `{}` executable `{}` path `{}`",
                package_name,
                executable.name,
                exe_path.display()
            )
        })?;
        if !canonical_exe.starts_with(&package_root) {
            anyhow::bail!(
                "external package `{}` executable `{}` path escapes package root: {}",
                package_name,
                executable.name,
                exe_path.display()
            );
        }
    }
    Ok(exe_path)
}

pub(crate) fn ensure_non_empty_manifest_field(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("external manifest field `{field}` must not be empty");
    }
    Ok(())
}
