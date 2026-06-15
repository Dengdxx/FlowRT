use std::path::Path;

use anyhow::Result;

use super::registry::ReleaseGateRegistry;

pub fn check_registry_for_version(
    registry: &ReleaseGateRegistry,
    repo_root: &Path,
    version: &str,
) -> Result<()> {
    registry.checked_focused_smoke(repo_root, version)?;
    Ok(())
}
