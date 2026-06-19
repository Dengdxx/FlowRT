use std::path::Path;

use anyhow::{Context, Result, anyhow};
use flowrt_ir::{
    FaultInjectionScenario, FaultInjectionScenarioPoint, TemporaryOverlayGenerationIr,
    apply_fault_injection_overlay, project_contract_to_profile,
};
use flowrt_validate::validate_contract;
use serde::Serialize;

use crate::workflows::normalize_contract_from_rsdl;

use super::model::{FaultMatrixCase, parse_matrix_file};

pub(crate) fn check_matrix(path: &Path) -> Result<FaultMatrixCheckReport> {
    let matrix = parse_matrix_file(path)?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let rsdl = base_dir.join(&matrix.rsdl);
    let contract = normalize_contract_from_rsdl(&rsdl)?;
    let mut cases = Vec::with_capacity(matrix.cases.len());

    for case in &matrix.cases {
        let selected = project_contract_to_profile(&contract, case.profile.as_deref())
            .with_context(|| format!("failed to select profile for matrix case `{}`", case.name))?;
        let scenario = scenario_for_case(path, case);
        let injected = apply_fault_injection_overlay(&selected, &scenario).with_context(|| {
            format!("failed to apply injection for matrix case `{}`", case.name)
        })?;
        validate_contract(&injected).map_err(|report| {
            anyhow!(
                "contract validation failed for matrix case `{}`:\n{}",
                case.name,
                report
            )
        })?;
        cases.push(FaultMatrixCaseCheck {
            name: case.name.clone(),
            mode: case.mode.as_str().to_string(),
            run_ticks: case.run_ticks,
            expectations: case.expect.expectation_count(),
            status: "ok".to_string(),
        });
    }

    Ok(FaultMatrixCheckReport {
        matrix: matrix.name,
        cases,
    })
}

fn scenario_for_case(path: &Path, case: &FaultMatrixCase) -> FaultInjectionScenario {
    FaultInjectionScenario {
        points: case
            .inject
            .iter()
            .map(|point| FaultInjectionScenarioPoint {
                kind: point.kind,
                instance: point.instance.clone(),
                task: point.task.clone(),
                invocations: point.invocations.clone(),
                from_invocation: point.from_invocation,
                reason: point.reason.clone(),
            })
            .collect(),
        generated_by: TemporaryOverlayGenerationIr {
            command: "flowrt fault-matrix check".to_string(),
            source: path.display().to_string(),
        },
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FaultMatrixCheckReport {
    pub(crate) matrix: String,
    pub(crate) cases: Vec<FaultMatrixCaseCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FaultMatrixCaseCheck {
    pub(crate) name: String,
    pub(crate) mode: String,
    pub(crate) run_ticks: usize,
    pub(crate) expectations: usize,
    pub(crate) status: String,
}
