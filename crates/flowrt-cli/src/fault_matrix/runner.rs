use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use flowrt_conformance::message_frame_expectations;
use flowrt_ir::{BoundaryDirection, ContractIr, PrimitiveType, TypeExpr, TypeIr};
use flowrt_record::{
    FlowrtMcapWriter, PayloadEncoding, RECORD_SCHEMA_VERSION, RecordEntity, RecordEntityKind,
    RecordEnvelope, RecordEventKind,
};
use serde::Serialize;

use crate::build_model::BuildMode;
use crate::workflows::{
    RunExtraEnv, TemporaryIslandCliOptions, application_root_from_rsdl, build_workspace,
    launch_workspace_with_env, prepare_workspace_with_options, resolve_build_toolchain_profile,
    run_workspace_with_env,
};

use super::model::{FaultMatrix, FaultMatrixCase, FaultMatrixMode, parse_matrix_file};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FaultMatrixRunReport {
    pub(crate) matrix: String,
    pub(crate) cases: Vec<super::expect::FaultMatrixCaseResult>,
}

impl FaultMatrixRunReport {
    pub(crate) fn ensure_passed(&self) -> Result<()> {
        if let Some(failed) = self.cases.iter().find(|case| !case.passed) {
            anyhow::bail!(
                "fault matrix case `{}` failed: {}",
                failed.name,
                failed.failures.join("; ")
            );
        }
        Ok(())
    }
}

pub(crate) fn run_matrix(path: &Path, out_root: &Path) -> Result<FaultMatrixRunReport> {
    let matrix = parse_matrix_file(path)?;
    let mut results = Vec::with_capacity(matrix.cases.len());
    for case in &matrix.cases {
        let case_dir = out_root.join(&matrix.name).join(&case.name);
        let status_out = case_dir.join("status.json");
        std::fs::create_dir_all(&case_dir).with_context(|| {
            format!("failed to create matrix case dir `{}`", case_dir.display())
        })?;
        run_case(path, &matrix, case, &case_dir, &status_out)?;
        let status_text = std::fs::read_to_string(&status_out).with_context(|| {
            format!("failed to read status snapshot `{}`", status_out.display())
        })?;
        let status: serde_json::Value = serde_json::from_str(&status_text).with_context(|| {
            format!("failed to parse status snapshot `{}`", status_out.display())
        })?;
        results.push(super::expect::evaluate_expectations(
            &case.name,
            &status,
            &case.expect,
        ));
    }
    Ok(FaultMatrixRunReport {
        matrix: matrix.name,
        cases: results,
    })
}

fn run_case(
    matrix_path: &Path,
    matrix: &FaultMatrix,
    case: &FaultMatrixCase,
    case_dir: &Path,
    status_out: &Path,
) -> Result<()> {
    let matrix_dir = matrix_path.parent().unwrap_or_else(|| Path::new("."));
    let rsdl = matrix_dir.join(&matrix.rsdl);
    let out_dir = case_dir.join("flowrt");
    let overlay = TemporaryIslandCliOptions::new(false, Vec::new(), Vec::new());
    let inject_path = write_case_inject_file(case_dir, case)?;
    let prepared = prepare_workspace_with_options(
        &rsdl,
        &out_dir,
        case.profile.as_deref(),
        &overlay,
        Some(&inject_path),
    )?;
    let workspace_root = application_root_from_rsdl(&rsdl)?;
    sync_matrix_app_sources(&workspace_root, case_dir)?;
    let replay_source = write_case_replay_source(case_dir, &prepared.selected_contract)?;
    let target_profile =
        resolve_build_toolchain_profile(&prepared.selected_contract, None, &workspace_root)?;
    let include_launcher = case.mode == FaultMatrixMode::Launch;
    build_workspace(
        &prepared.selected_contract,
        &out_dir,
        include_launcher,
        BuildMode::Release,
        target_profile.as_ref(),
    )?;
    let extra_env = RunExtraEnv {
        flowrt_status_out: Some(status_out.to_path_buf()),
        flowrt_replay_source: Some(replay_source.clone()),
    };
    match case.mode {
        FaultMatrixMode::Run => run_workspace_with_env(
            &prepared.selected_contract,
            &out_dir,
            None,
            Some(case.run_ticks),
            Some(BuildMode::Release),
            Some(&replay_source),
            &extra_env,
        ),
        FaultMatrixMode::Launch => launch_workspace_with_env(
            &prepared.selected_contract,
            &out_dir,
            Some(case.run_ticks),
            Some(BuildMode::Release),
            &extra_env,
        ),
    }
}

pub(crate) fn write_case_replay_source(
    case_dir: &Path,
    contract: &ContractIr,
) -> Result<std::path::PathBuf> {
    let boundary = contract
        .graphs
        .iter()
        .flat_map(|graph| graph.boundary_endpoints.iter())
        .find(|endpoint| endpoint.direction == BoundaryDirection::Input)
        .context("fault matrix run requires at least one boundary input for simulated replay")?;
    let path = case_dir.join("replay.mcap");
    let mut writer =
        FlowrtMcapWriter::new(fs::File::create(&path).with_context(|| {
            format!("failed to create matrix replay source `{}`", path.display())
        })?)
        .context("failed to create matrix replay MCAP writer")?;
    let channel = writer
        .register_channel("flowrt/fault-matrix/replay", RecordEventKind::ChannelSample)
        .context("failed to register matrix replay MCAP channel")?;
    let event_time_ns = 1_000_000_000_000_u64;
    let payload = default_boundary_replay_payload(contract, &boundary.ty)?;
    writer
        .write_event(
            channel,
            &RecordEnvelope {
                schema_version: RECORD_SCHEMA_VERSION,
                event_kind: RecordEventKind::ChannelSample,
                package: contract.package.name.clone(),
                process: "fault_matrix".to_string(),
                runtime_pid: std::process::id(),
                selfdesc_hash: contract.source_hash.clone(),
                monotonic_ns: event_time_ns,
                sample_time_ns: None,
                wall_unix_ns: 0,
                sequence: 1,
                entity: RecordEntity {
                    kind: RecordEntityKind::Channel,
                    name: boundary.name.clone(),
                    instance: Some(boundary.port.instance.name.clone()),
                    task: None,
                    type_name: Some(boundary.ty.canonical_syntax()),
                },
                payload_encoding: PayloadEncoding::CanonicalFrame,
                payload_schema: boundary.ty.canonical_syntax(),
                payload,
            },
        )
        .context("failed to write matrix replay MCAP event")?;
    writer
        .finish_into_inner()
        .context("failed to finish matrix replay MCAP")?;
    Ok(path)
}

fn default_boundary_replay_payload(contract: &ContractIr, ty: &TypeExpr) -> Result<Vec<u8>> {
    Ok(vec![0; frame_header_size_for_replay(contract, ty)?])
}

fn frame_header_size_for_replay(contract: &ContractIr, ty: &TypeExpr) -> Result<usize> {
    match ty {
        TypeExpr::Primitive { name } => Ok(primitive_wire_size(*name)),
        TypeExpr::Array { element, len } => frame_header_size_for_replay(contract, element)?
            .checked_mul(*len)
            .with_context(|| {
                format!(
                    "boundary replay payload size overflows for `{}`",
                    ty.canonical_syntax()
                )
            }),
        TypeExpr::Named { name } => {
            let message = replay_type_by_name(contract, name)?;
            let frame = message_frame_expectations(contract)
                .with_context(|| {
                    format!(
                        "failed to derive fault matrix replay frame layout for `{}`",
                        message.qualified_name
                    )
                })?
                .into_iter()
                .find(|frame| frame.type_name == message.generated_name)
                .with_context(|| {
                    format!(
                        "fault matrix replay frame layout for `{}` was not derived",
                        message.qualified_name
                    )
                })?;
            Ok(frame.header_size_bytes)
        }
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            Ok(8)
        }
    }
}

fn replay_type_by_name<'a>(contract: &'a ContractIr, name: &str) -> Result<&'a TypeIr> {
    contract
        .types
        .iter()
        .find(|ty| ty.qualified_name == name || ty.generated_name == name || ty.name == name)
        .with_context(|| format!("fault matrix replay boundary references unknown type `{name}`"))
}

fn primitive_wire_size(primitive: PrimitiveType) -> usize {
    match primitive {
        PrimitiveType::Bool | PrimitiveType::U8 | PrimitiveType::I8 => 1,
        PrimitiveType::U16 | PrimitiveType::I16 => 2,
        PrimitiveType::U32 | PrimitiveType::I32 | PrimitiveType::F32 => 4,
        PrimitiveType::U64 | PrimitiveType::I64 | PrimitiveType::F64 => 8,
        PrimitiveType::U128 | PrimitiveType::I128 => 16,
    }
}

fn write_case_inject_file(case_dir: &Path, case: &FaultMatrixCase) -> Result<std::path::PathBuf> {
    #[derive(Serialize)]
    struct InjectFile<'a> {
        inject: Vec<InjectEntry<'a>>,
    }

    #[derive(Serialize)]
    struct InjectEntry<'a> {
        kind: flowrt_ir::FaultInjectionKind,
        instance: &'a str,
        task: &'a str,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        invocations: Vec<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        from_invocation: Option<u64>,
        #[serde(skip_serializing_if = "str::is_empty")]
        reason: &'a str,
    }

    let inject = case
        .inject
        .iter()
        .map(|point| InjectEntry {
            kind: point.kind,
            instance: &point.instance,
            task: &point.task,
            invocations: point.invocations.clone(),
            from_invocation: point.from_invocation,
            reason: &point.reason,
        })
        .collect();
    let file = InjectFile { inject };
    let text = toml::to_string(&file).context("failed to encode matrix injection scenario")?;
    let path = case_dir.join("inject.toml");
    std::fs::write(&path, text)
        .with_context(|| format!("failed to write matrix injection `{}`", path.display()))?;
    Ok(path)
}

fn sync_matrix_app_sources(workspace_root: &Path, case_dir: &Path) -> Result<()> {
    let source = workspace_root.join("app");
    if !source.exists() {
        return Ok(());
    }
    copy_dir_recursive_for_matrix(&source, &case_dir.join("app"))
}

fn copy_dir_recursive_for_matrix(source: &Path, dest: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("failed to inspect matrix app source `{}`", source.display()))?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!(
            "matrix app source `{}` is a symbolic link; symlinks are not allowed",
            source.display()
        );
    }
    if !metadata.is_dir() {
        anyhow::bail!(
            "matrix app source `{}` is not a directory",
            source.display()
        );
    }
    fs::create_dir_all(dest)
        .with_context(|| format!("failed to create matrix app dest `{}`", dest.display()))?;
    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read matrix app source `{}`", source.display()))?
    {
        let entry = entry
            .with_context(|| format!("failed to read matrix app source `{}`", source.display()))?;
        let path = entry.path();
        let target = dest.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect matrix app source `{}`", path.display()))?;
        if file_type.is_symlink() {
            anyhow::bail!(
                "matrix app source `{}` is a symbolic link; symlinks are not allowed",
                path.display()
            );
        } else if file_type.is_dir() {
            copy_dir_recursive_for_matrix(&path, &target)?;
        } else if file_type.is_file() {
            fs::copy(&path, &target).with_context(|| {
                format!(
                    "failed to copy matrix app source `{}` to `{}`",
                    path.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}
