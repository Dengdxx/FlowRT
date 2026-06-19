use std::path::Path;

use anyhow::{Context, Result};
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
    if let Some(failed) = results.iter().find(|case| !case.passed) {
        anyhow::bail!(
            "fault matrix case `{}` failed: {}",
            failed.name,
            failed.failures.join("; ")
        );
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
    };
    match case.mode {
        FaultMatrixMode::Run => run_workspace_with_env(
            &prepared.selected_contract,
            &out_dir,
            None,
            Some(case.run_ticks),
            Some(BuildMode::Release),
            None,
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
