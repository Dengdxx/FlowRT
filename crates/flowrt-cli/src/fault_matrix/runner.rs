use std::path::Path;

use anyhow::{Result, bail};

pub(crate) fn run_matrix(_matrix: &Path, _out_dir: &Path) -> Result<serde_json::Value> {
    bail!("`flowrt fault-matrix run` is not implemented yet")
}
