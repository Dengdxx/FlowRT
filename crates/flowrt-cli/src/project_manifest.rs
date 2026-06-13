use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub(crate) const MANIFEST_FILE_NAME: &str = "flowrt.toml";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectManifest {
    project: ProjectSection,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProjectSection {
    main: PathBuf,
}

pub(crate) fn resolve_rsdl_arg(explicit: Option<PathBuf>, cwd: &Path) -> Result<PathBuf> {
    if let Some(rsdl) = explicit {
        return Ok(rsdl);
    }
    discover_manifest_rsdl_from(cwd)
        .with_context(|| format!("需要传入 RSDL 路径或在项目根创建 {MANIFEST_FILE_NAME}"))
}

pub(crate) fn resolve_optional_rsdl_arg(
    explicit: Option<PathBuf>,
    cwd: &Path,
) -> Result<Option<PathBuf>> {
    if let Some(rsdl) = explicit {
        return Ok(Some(rsdl));
    }
    discover_manifest_path_from(cwd)
        .map(|path| load_manifest_rsdl(&path))
        .transpose()
}

pub(crate) fn discover_manifest_rsdl_from(start_dir: &Path) -> Result<PathBuf> {
    let manifest_path = discover_manifest_path_from(start_dir).with_context(|| {
        format!(
            "未从 `{}` 或其父目录找到 {MANIFEST_FILE_NAME}",
            start_dir.display()
        )
    })?;
    load_manifest_rsdl(&manifest_path)
}

pub(crate) fn load_manifest_rsdl(manifest_path: &Path) -> Result<PathBuf> {
    let source = fs::read_to_string(manifest_path)
        .with_context(|| format!("读取 `{}` 失败", manifest_path.display()))?;
    let manifest: ProjectManifest = toml::from_str(&source)
        .with_context(|| format!("解析 `{}` 失败", manifest_path.display()))?;
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    resolve_manifest_main(manifest_dir, &manifest.project.main)
}

pub(crate) fn render_project_manifest(main: &Path) -> Result<String> {
    validate_manifest_main(main)?;
    let manifest = ProjectManifest {
        project: ProjectSection {
            main: main.to_path_buf(),
        },
    };
    toml::to_string(&manifest).context("渲染 flowrt.toml 失败")
}

#[allow(dead_code)]
pub(crate) fn write_project_manifest(manifest_path: &Path, main: &Path) -> Result<()> {
    let content = render_project_manifest(main)?;
    fs::write(manifest_path, content)
        .with_context(|| format!("写入 `{}` 失败", manifest_path.display()))
}

fn discover_manifest_path_from(start_dir: &Path) -> Option<PathBuf> {
    let mut current = start_dir.to_path_buf();
    loop {
        let candidate = current.join(MANIFEST_FILE_NAME);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn resolve_manifest_main(manifest_dir: &Path, main: &Path) -> Result<PathBuf> {
    validate_manifest_main(main)?;
    Ok(manifest_dir.join(main))
}

fn validate_manifest_main(main: &Path) -> Result<()> {
    if main.as_os_str().is_empty() {
        anyhow::bail!("flowrt.toml project.main 不能为空");
    }
    if main.is_absolute() {
        anyhow::bail!("flowrt.toml project.main 不能是绝对路径");
    }
    for component in main.components() {
        match component {
            Component::ParentDir => anyhow::bail!("flowrt.toml project.main 不能包含 .."),
            Component::Prefix(_) | Component::RootDir => {
                anyhow::bail!("flowrt.toml project.main 不能是绝对路径")
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    if main.extension() != Some(OsStr::new("rsdl")) {
        anyhow::bail!("flowrt.toml project.main 必须指向 .rsdl 文件");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project_dir(test_name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "flowrt-project-manifest-{test_name}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn project_manifest_resolves_main_rsdl_relative_to_manifest_dir() {
        let root = temp_project_dir("relative-main");
        std::fs::create_dir_all(root.join("rsdl")).unwrap();
        let manifest_path = root.join("flowrt.toml");
        std::fs::write(&manifest_path, "[project]\nmain = \"rsdl/robot.rsdl\"\n").unwrap();

        let rsdl = load_manifest_rsdl(&manifest_path).unwrap();

        assert_eq!(rsdl, root.join("rsdl/robot.rsdl"));
    }

    #[test]
    fn project_manifest_discovery_walks_up_from_child_dir() {
        let root = temp_project_dir("discover-up");
        let child = root.join("app/rust");
        std::fs::create_dir_all(root.join("rsdl")).unwrap();
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(
            root.join(MANIFEST_FILE_NAME),
            "[project]\nmain = \"rsdl/robot.rsdl\"\n",
        )
        .unwrap();

        let discovered = discover_manifest_rsdl_from(&child).unwrap();

        assert_eq!(discovered, root.join("rsdl/robot.rsdl"));
    }

    #[test]
    fn explicit_rsdl_path_wins_over_project_manifest() {
        let root = temp_project_dir("explicit-wins");
        std::fs::create_dir_all(root.join("rsdl")).unwrap();
        std::fs::write(
            root.join(MANIFEST_FILE_NAME),
            "[project]\nmain = \"rsdl/robot.rsdl\"\n",
        )
        .unwrap();

        let rsdl = resolve_rsdl_arg(Some(std::path::PathBuf::from("custom.rsdl")), &root).unwrap();

        assert_eq!(rsdl, std::path::PathBuf::from("custom.rsdl"));
    }

    #[test]
    fn missing_rsdl_and_manifest_reports_project_entry_hint() {
        let root = temp_project_dir("missing");
        std::fs::create_dir_all(&root).unwrap();

        let error = resolve_rsdl_arg(None, &root).unwrap_err().to_string();

        assert!(error.contains("需要传入 RSDL 路径或在项目根创建 flowrt.toml"));
    }

    #[test]
    fn project_manifest_rejects_absolute_main() {
        let root = temp_project_dir("absolute-main");
        std::fs::create_dir_all(&root).unwrap();
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        std::fs::write(&manifest_path, "[project]\nmain = \"/tmp/robot.rsdl\"\n").unwrap();

        let error = load_manifest_rsdl(&manifest_path).unwrap_err().to_string();

        assert!(error.contains("project.main 不能是绝对路径"));
    }

    #[test]
    fn project_manifest_rejects_parent_escape_main() {
        let root = temp_project_dir("parent-main");
        std::fs::create_dir_all(&root).unwrap();
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        std::fs::write(&manifest_path, "[project]\nmain = \"../robot.rsdl\"\n").unwrap();

        let error = load_manifest_rsdl(&manifest_path).unwrap_err().to_string();

        assert!(error.contains("project.main 不能包含 .."));
    }

    #[test]
    fn project_manifest_rejects_non_rsdl_main() {
        let root = temp_project_dir("non-rsdl-main");
        std::fs::create_dir_all(&root).unwrap();
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        std::fs::write(&manifest_path, "[project]\nmain = \"rsdl/robot.toml\"\n").unwrap();

        let error = load_manifest_rsdl(&manifest_path).unwrap_err().to_string();

        assert!(error.contains("project.main 必须指向 .rsdl 文件"));
    }

    #[test]
    fn project_manifest_can_write_minimal_toml() {
        let root = temp_project_dir("render");
        std::fs::create_dir_all(&root).unwrap();
        let manifest_path = root.join(MANIFEST_FILE_NAME);

        write_project_manifest(&manifest_path, std::path::Path::new("rsdl/robot.rsdl")).unwrap();

        let rsdl = load_manifest_rsdl(&manifest_path).unwrap();

        assert_eq!(rsdl, root.join("rsdl/robot.rsdl"));
    }
}
