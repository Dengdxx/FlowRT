use super::*;

pub(crate) fn init_app_project(root: &Path, language: AppInitLanguage) -> Result<String> {
    let package_name = app_init_package_name(root);
    let rsdl_main = Path::new("rsdl/robot.rsdl");
    let files = vec![
        (
            PathBuf::from(project_manifest::MANIFEST_FILE_NAME),
            project_manifest::render_project_manifest(rsdl_main)?,
        ),
        (
            rsdl_main.to_path_buf(),
            app_init_rsdl_template(&package_name, language),
        ),
    ];

    for (relative, _) in &files {
        let target = root.join(relative);
        if target.exists() {
            anyhow::bail!("refusing to overwrite existing file `{}`", target.display());
        }
    }

    for (relative, content) in &files {
        let target = root.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create `{}`", parent.display()))?;
        }
        write_new_file(&target, content)?;
    }

    Ok(format!(
        "initialized FlowRT app: {} language={} main={}\nnext: add contract facts with `flowrt add message` and `flowrt add component --lang {}`; then run `flowrt prepare` or `flowrt explain` before writing app/ code",
        root.display(),
        language.as_str(),
        rsdl_main.display(),
        language.as_str()
    ))
}

pub(crate) fn write_new_file(path: &Path, content: &str) -> Result<()> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            anyhow::bail!("refusing to overwrite existing file `{}`", path.display());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to create `{}`", path.display()));
        }
    };
    file.write_all(content.as_bytes())
        .with_context(|| format!("failed to write `{}`", path.display()))
}

pub(crate) fn app_init_package_name(root: &Path) -> String {
    let raw_name = root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != ".")
        .map(str::to_string)
        .or_else(|| {
            env::current_dir()
                .ok()
                .and_then(|cwd| cwd.file_name().map(|value| value.to_owned()))
                .and_then(|value| value.to_str().map(str::to_string))
        })
        .unwrap_or_else(|| "flowrt_app".to_string());
    sanitize_app_init_identifier(&raw_name)
}

pub(crate) fn sanitize_app_init_identifier(raw: &str) -> String {
    let mut output = String::new();
    let mut previous_was_underscore = false;
    for ch in raw.chars() {
        let normalized = if ch.is_ascii_alphanumeric() { ch } else { '_' };
        if normalized == '_' {
            if !previous_was_underscore && !output.is_empty() {
                output.push('_');
            }
            previous_was_underscore = true;
        } else {
            output.push(normalized.to_ascii_lowercase());
            previous_was_underscore = false;
        }
    }
    while output.ends_with('_') {
        output.pop();
    }
    if output.is_empty() {
        output.push_str("flowrt_app");
    }
    if !output
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
    {
        output.insert_str(0, "app_");
    }
    output
}

pub(crate) fn app_init_rsdl_template(package_name: &str, language: AppInitLanguage) -> String {
    let runtime = language.as_str();
    format!(
        r#"[package]
name = "{package_name}"
version = "0.1.0"
rsdl_version = "0.1"

# Edit this contract directly, or start with:
# flowrt add message Sample value:u32
# flowrt add component Source --lang {runtime} --output sample:Sample

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
max_age_ms = 1000

[target.linux]
platform = "linux-amd64"
runtime = ["{runtime}"]
backends = ["inproc"]
"#
    )
}
