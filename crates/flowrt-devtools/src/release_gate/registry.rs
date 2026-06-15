use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

pub const REGISTRY_RELATIVE_PATH: &str = "scripts/release-gates/registry.toml";

#[derive(Debug)]
pub struct ReleaseGateRegistry {
    focused_smoke: Vec<FocusedSmokeGate>,
}

impl ReleaseGateRegistry {
    pub fn load_from_path(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("无法读取 release gate registry: {}", path.display()))?;
        Self::from_toml_str(&source)
    }

    pub fn from_toml_str(source: &str) -> Result<Self> {
        let document: RegistryDocument =
            toml::from_str(source).context("无法解析 release gate registry TOML")?;
        Ok(Self {
            focused_smoke: document.focused_smoke,
        })
    }

    pub fn checked_focused_smoke(
        &self,
        repo_root: &Path,
        version: &str,
    ) -> Result<FocusedSmokeGate> {
        self.check_registry(repo_root)?;
        let version = normalize_requested_version(version)?;
        self.focused_smoke
            .iter()
            .find(|gate| gate.version == version)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("版本 v{version} 未登记 release gate focused smoke"))
    }

    pub fn check_registry(&self, repo_root: &Path) -> Result<()> {
        if self.focused_smoke.is_empty() {
            bail!("release gate registry 缺少 focused_smoke 条目");
        }

        let mut seen_versions = HashSet::new();
        for gate in &self.focused_smoke {
            gate.check_version()?;
            gate.check_script_path()?;
            if !seen_versions.insert(gate.version.as_str()) {
                bail!("release gate registry 存在重复版本: {}", gate.version);
            }

            if !gate.planned {
                let script_path = repo_root.join(&gate.script);
                if !script_path.is_file() {
                    bail!(
                        "release gate registry 引用脚本不存在: {}",
                        gate.script.display()
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocusedSmokeGate {
    version: String,
    script: PathBuf,
    #[serde(default)]
    planned: bool,
}

impl FocusedSmokeGate {
    pub fn script(&self) -> &Path {
        &self.script
    }

    fn check_version(&self) -> Result<()> {
        if is_version_core(&self.version) {
            Ok(())
        } else {
            bail!(
                "release gate registry 版本格式无效: {}，必须使用 X.Y.Z",
                self.version
            );
        }
    }

    fn check_script_path(&self) -> Result<()> {
        if self.script.as_os_str().is_empty() {
            bail!(
                "release gate registry v{} 的 focused smoke script 不能为空",
                self.version
            );
        }
        if self.script.is_absolute() {
            bail!(
                "release gate registry v{} 的 focused smoke script 必须是仓库相对路径",
                self.version
            );
        }
        if self
            .script
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            bail!(
                "release gate registry v{} 的 focused smoke script 不能包含 '..'",
                self.version
            );
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryDocument {
    #[serde(default)]
    focused_smoke: Vec<FocusedSmokeGate>,
}

fn normalize_requested_version(version: &str) -> Result<String> {
    let normalized = version.strip_prefix('v').unwrap_or(version);
    if is_version_core(normalized) {
        Ok(normalized.to_string())
    } else {
        bail!("VERSION 必须是 X.Y.Z 或 vX.Y.Z，实际为: {version}");
    }
}

fn is_version_core(version: &str) -> bool {
    let mut parts = version.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }

    [major, minor, patch]
        .iter()
        .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}
