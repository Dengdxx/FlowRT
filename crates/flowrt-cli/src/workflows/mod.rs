use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Duration;

use anyhow::{Context, Result};
use flowrt_codegen::{ArtifactBundle, emit_artifacts};
use flowrt_ir::{
    ContractIr, GraphMode, LanguageKind, TargetPlatform, TemporaryBoundaryMapping,
    TemporaryIslandOverlay, apply_temporary_island_overlay, hash_source, normalize_loaded_document,
    project_contract_to_profile,
};
use flowrt_validate::validate_contract;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::build_model::{
    BuildMode, CacheLayout, DepsCacheKey, RuntimeFeatureSet, default_cache_root,
};
use crate::toolchain::{
    RuntimeDependencyPolicy, ToolchainFieldSources, ToolchainProfile, ToolchainProfileOverrides,
    generate_toolchain_init_toml, resolve_toolchain_profile,
    resolve_toolchain_profile_with_field_sources,
};
use crate::{AppInitLanguage, DepsBackend, build_model, project_manifest};

mod args;
mod bundle_deploy;
mod doctor_toolchain;
mod external;
mod init;
mod prepare_build;
mod run_launch;
mod shared;

pub(crate) use args::*;
pub(crate) use bundle_deploy::*;
pub(crate) use doctor_toolchain::*;
pub(crate) use external::*;
pub(crate) use init::*;
pub(crate) use prepare_build::*;
pub(crate) use run_launch::*;
pub(crate) use shared::*;
