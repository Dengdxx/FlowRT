use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use flowrt_ir::FaultInjectionKind;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FaultMatrixMode {
    Run,
    Launch,
}

impl FaultMatrixMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Launch => "launch",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawFaultMatrixMode {
    Run,
    Launch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFaultMatrixFile {
    matrix: RawFaultMatrixDefaults,
    #[serde(default)]
    case: Vec<RawFaultMatrixCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFaultMatrixDefaults {
    name: String,
    rsdl: PathBuf,
    profile: Option<String>,
    run_ticks: usize,
    mode: RawFaultMatrixMode,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFaultMatrixCase {
    name: String,
    profile: Option<String>,
    run_ticks: Option<usize>,
    mode: Option<RawFaultMatrixMode>,
    #[serde(default)]
    inject: Vec<MatrixInjectPoint>,
    #[serde(default)]
    expect: MatrixExpectations,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MatrixInjectPoint {
    #[serde(default)]
    pub(crate) kind: FaultInjectionKind,
    pub(crate) instance: String,
    pub(crate) task: String,
    #[serde(default)]
    pub(crate) invocations: Vec<u64>,
    #[serde(default)]
    pub(crate) from_invocation: Option<u64>,
    #[serde(default)]
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MatrixExpectations {
    #[serde(default)]
    pub(crate) graph: Option<GraphExpectation>,
    #[serde(default)]
    pub(crate) instance: Vec<InstanceExpectation>,
    #[serde(default)]
    pub(crate) route: Vec<RouteExpectation>,
    #[serde(default)]
    pub(crate) failover: Vec<FailoverExpectation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GraphExpectation {
    pub(crate) graph_health: Option<String>,
    pub(crate) graph_critical_health: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstanceExpectation {
    pub(crate) name: String,
    pub(crate) lifecycle_state: Option<String>,
    pub(crate) restart_count: Option<u64>,
    pub(crate) last_fault_reason_contains: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RouteExpectation {
    pub(crate) name: String,
    pub(crate) backend_health_state: Option<String>,
    pub(crate) dropped_samples_min: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FailoverExpectation {
    pub(crate) group: String,
    pub(crate) old_active: String,
    pub(crate) new_active: String,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FaultMatrix {
    pub(crate) name: String,
    pub(crate) rsdl: PathBuf,
    pub(crate) cases: Vec<FaultMatrixCase>,
}

#[derive(Debug, Clone)]
pub(crate) struct FaultMatrixCase {
    pub(crate) name: String,
    pub(crate) profile: Option<String>,
    pub(crate) run_ticks: usize,
    pub(crate) mode: FaultMatrixMode,
    pub(crate) inject: Vec<MatrixInjectPoint>,
    pub(crate) expect: MatrixExpectations,
}

pub(crate) fn parse_matrix_file(path: &Path) -> Result<FaultMatrix> {
    let text = std::fs::read_to_string(path)
        .map_err(|err| anyhow!("failed to read fault matrix `{}`: {err}", path.display()))?;
    let raw: RawFaultMatrixFile = toml::from_str(&text)
        .map_err(|err| anyhow!("failed to parse fault matrix `{}`: {err}", path.display()))?;
    normalize_matrix(raw)
}

fn normalize_matrix(raw: RawFaultMatrixFile) -> Result<FaultMatrix> {
    let name = require_non_empty("matrix.name", raw.matrix.name)?;
    if raw.matrix.rsdl.as_os_str().is_empty() {
        bail!("fault matrix `{name}` must set matrix.rsdl");
    }
    if raw.matrix.run_ticks == 0 {
        bail!("fault matrix `{name}` must set matrix.run_ticks > 0");
    }
    if raw.case.is_empty() {
        bail!("fault matrix `{name}` must define at least one case");
    }

    let mut seen_cases = BTreeSet::new();
    let mut cases = Vec::with_capacity(raw.case.len());
    for case in raw.case {
        let case_name = require_non_empty("case.name", case.name)?;
        if !seen_cases.insert(case_name.clone()) {
            bail!("fault matrix `{name}` has duplicate case `{case_name}`");
        }
        if case.inject.is_empty() {
            bail!("fault matrix case `{case_name}` must define at least one inject point");
        }
        if case.expect.is_empty() {
            bail!("fault matrix case `{case_name}` must define at least one expectation");
        }
        validate_expectations(&case_name, &case.expect)?;
        let run_ticks = case.run_ticks.unwrap_or(raw.matrix.run_ticks);
        if run_ticks == 0 {
            bail!("fault matrix case `{case_name}` must set run_ticks > 0");
        }
        validate_inject_points(&case_name, &case.inject)?;

        cases.push(FaultMatrixCase {
            name: case_name,
            profile: case.profile.or_else(|| raw.matrix.profile.clone()),
            run_ticks,
            mode: case.mode.unwrap_or(raw.matrix.mode).into(),
            inject: case.inject,
            expect: case.expect,
        });
    }

    Ok(FaultMatrix {
        name,
        rsdl: raw.matrix.rsdl,
        cases,
    })
}

fn validate_inject_points(case_name: &str, inject: &[MatrixInjectPoint]) -> Result<()> {
    for point in inject {
        require_non_empty_ref("inject.instance", &point.instance)?;
        require_non_empty_ref("inject.task", &point.task)?;
        if point.invocations.is_empty() && point.from_invocation.is_none() {
            bail!(
                "fault matrix case `{case_name}` inject point `{}.{}` must set invocations or from_invocation",
                point.instance,
                point.task
            );
        }
        if point.invocations.contains(&0) || point.from_invocation == Some(0) {
            bail!(
                "fault matrix case `{case_name}` inject point `{}.{}` uses 1-based invocation indices",
                point.instance,
                point.task
            );
        }
    }
    Ok(())
}

fn validate_expectations(case_name: &str, expect: &MatrixExpectations) -> Result<()> {
    if let Some(graph) = &expect.graph {
        validate_optional_string(case_name, "expect.graph.graph_health", &graph.graph_health)?;
        validate_optional_string(
            case_name,
            "expect.graph.graph_critical_health",
            &graph.graph_critical_health,
        )?;
        if graph.graph_health.is_none() && graph.graph_critical_health.is_none() {
            bail!("fault matrix case `{case_name}` graph expectation must set at least one field");
        }
    }
    for instance in &expect.instance {
        require_non_empty_ref("expect.instance.name", &instance.name)?;
        validate_optional_string(
            case_name,
            "expect.instance.lifecycle_state",
            &instance.lifecycle_state,
        )?;
        validate_optional_string(
            case_name,
            "expect.instance.last_fault_reason_contains",
            &instance.last_fault_reason_contains,
        )?;
        let _ = instance.restart_count;
    }
    for route in &expect.route {
        require_non_empty_ref("expect.route.name", &route.name)?;
        validate_optional_string(
            case_name,
            "expect.route.backend_health_state",
            &route.backend_health_state,
        )?;
        let _ = route.dropped_samples_min;
    }
    for failover in &expect.failover {
        require_non_empty_ref("expect.failover.group", &failover.group)?;
        require_non_empty_ref("expect.failover.old_active", &failover.old_active)?;
        require_non_empty_ref("expect.failover.new_active", &failover.new_active)?;
        validate_optional_string(case_name, "expect.failover.reason", &failover.reason)?;
    }
    Ok(())
}

fn validate_optional_string(case_name: &str, field: &str, value: &Option<String>) -> Result<()> {
    if value.as_deref().is_some_and(|text| text.trim().is_empty()) {
        bail!("fault matrix case `{case_name}` field `{field}` must not be empty");
    }
    Ok(())
}

fn require_non_empty(field: &str, value: String) -> Result<String> {
    if value.trim().is_empty() {
        bail!("fault matrix field `{field}` must not be empty");
    }
    Ok(value)
}

fn require_non_empty_ref(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("fault matrix field `{field}` must not be empty");
    }
    Ok(())
}

impl MatrixExpectations {
    fn is_empty(&self) -> bool {
        self.graph.is_none()
            && self.instance.is_empty()
            && self.route.is_empty()
            && self.failover.is_empty()
    }

    pub(crate) fn expectation_count(&self) -> usize {
        usize::from(self.graph.is_some())
            + self.instance.len()
            + self.route.len()
            + self.failover.len()
    }
}

impl From<RawFaultMatrixMode> for FaultMatrixMode {
    fn from(value: RawFaultMatrixMode) -> Self {
        match value {
            RawFaultMatrixMode::Run => Self::Run,
            RawFaultMatrixMode::Launch => Self::Launch,
        }
    }
}
