use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use super::model::{
    FailoverExpectation, GraphExpectation, InstanceExpectation, MatrixExpectations,
    RouteExpectation,
};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct FaultMatrixCaseResult {
    pub(crate) name: String,
    pub(crate) passed: bool,
    pub(crate) failures: Vec<String>,
}

pub(crate) fn evaluate_expectations(
    case_name: &str,
    status: &Value,
    expect: &MatrixExpectations,
) -> FaultMatrixCaseResult {
    let mut merged = merge_status(status);
    let mut failures = std::mem::take(&mut merged.failures);
    check_graph(case_name, &merged, expect.graph.as_ref(), &mut failures);
    check_instances(case_name, &merged, &expect.instance, &mut failures);
    check_routes(case_name, &merged, &expect.route, &mut failures);
    check_failovers(case_name, &merged, &expect.failover, &mut failures);
    FaultMatrixCaseResult {
        name: case_name.to_string(),
        passed: failures.is_empty(),
        failures,
    }
}

#[derive(Debug, Default)]
struct MergedStatus<'a> {
    graph_health: Option<String>,
    graph_critical_health: Option<String>,
    instances: BTreeMap<String, &'a Value>,
    routes: BTreeMap<String, &'a Value>,
    failovers: Vec<&'a Value>,
    failures: Vec<String>,
}

fn merge_status(status: &Value) -> MergedStatus<'_> {
    if status.get("mode").and_then(Value::as_str) == Some("launch") {
        merge_launch_status(status)
    } else {
        merge_direct_status(status)
    }
}

fn merge_direct_status(status: &Value) -> MergedStatus<'_> {
    let mut merged = MergedStatus {
        graph_health: status
            .get("graph_health")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        graph_critical_health: status
            .get("graph_critical_health")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        ..MergedStatus::default()
    };
    add_status_entries(status, "direct", &mut merged);
    merged
}

fn merge_launch_status(status: &Value) -> MergedStatus<'_> {
    let mut merged = MergedStatus::default();
    let Some(processes) = status.get("processes").and_then(Value::as_array) else {
        merged
            .failures
            .push("launch status missing `processes` array".to_string());
        return merged;
    };

    let mut graph_health_values = Vec::new();
    let mut graph_critical_health_values = Vec::new();
    for process in processes {
        let process_name = process
            .get("process")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let Some(child_status) = process.get("status") else {
            merged
                .failures
                .push(format!("launch process `{process_name}` missing status"));
            continue;
        };
        if let Some(value) = child_status.get("graph_health").and_then(Value::as_str) {
            graph_health_values.push(value.to_string());
        }
        if let Some(value) = child_status
            .get("graph_critical_health")
            .and_then(Value::as_str)
        {
            graph_critical_health_values.push(value.to_string());
        }
        add_status_entries(child_status, process_name, &mut merged);
    }
    merged.graph_health = worst_health(&graph_health_values);
    merged.graph_critical_health = worst_health(&graph_critical_health_values);
    merged
}

fn add_status_entries<'a>(status: &'a Value, scope: &str, merged: &mut MergedStatus<'a>) {
    for instance in status
        .get("instances")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = instance.get("instance").and_then(Value::as_str) else {
            merged.failures.push(format!(
                "{scope} status has instance without `instance` key"
            ));
            continue;
        };
        if merged
            .instances
            .insert(name.to_string(), instance)
            .is_some()
        {
            merged
                .failures
                .push(format!("duplicate instance status `{name}`"));
        }
    }
    for route in status
        .get("routes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(name) = route.get("name").and_then(Value::as_str) else {
            merged
                .failures
                .push(format!("{scope} status has route without `name` key"));
            continue;
        };
        if let Some(existing) = merged.routes.get_mut(name) {
            if route_score(route) > route_score(existing) {
                *existing = route;
            }
        } else {
            merged.routes.insert(name.to_string(), route);
        }
    }
    if let Some(failovers) = status.get("failovers").and_then(Value::as_array) {
        merged.failovers.extend(failovers);
    }
}

fn check_graph(
    case_name: &str,
    merged: &MergedStatus<'_>,
    expect: Option<&GraphExpectation>,
    failures: &mut Vec<String>,
) {
    let Some(expect) = expect else {
        return;
    };
    if let Some(expected) = &expect.graph_health {
        compare_optional_string(
            failures,
            case_name,
            "graph_health",
            merged.graph_health.as_deref(),
            expected,
        );
    }
    if let Some(expected) = &expect.graph_critical_health {
        compare_optional_string(
            failures,
            case_name,
            "graph_critical_health",
            merged.graph_critical_health.as_deref(),
            expected,
        );
    }
}

fn check_instances(
    case_name: &str,
    merged: &MergedStatus<'_>,
    expected: &[InstanceExpectation],
    failures: &mut Vec<String>,
) {
    for expectation in expected {
        let Some(instance) = merged.instances.get(&expectation.name) else {
            failures.push(format!(
                "{case_name}: missing instance `{}`",
                expectation.name
            ));
            continue;
        };
        if let Some(expected) = &expectation.lifecycle_state {
            compare_optional_string(
                failures,
                case_name,
                &format!("instance `{}` lifecycle_state", expectation.name),
                instance.get("lifecycle_state").and_then(Value::as_str),
                expected,
            );
        }
        if let Some(expected) = expectation.restart_count {
            let actual = instance.get("restart_count").and_then(Value::as_u64);
            if actual != Some(expected) {
                failures.push(format!(
                    "{case_name}: instance `{}` restart_count expected {expected}, got {}",
                    expectation.name,
                    format_optional_u64(actual)
                ));
            }
        }
        if let Some(expected) = &expectation.last_fault_reason_contains {
            let actual = instance.get("last_fault_reason").and_then(Value::as_str);
            if !actual.is_some_and(|reason| reason.contains(expected)) {
                failures.push(format!(
                    "{case_name}: instance `{}` last_fault_reason expected to contain `{expected}`, got {}",
                    expectation.name,
                    actual.unwrap_or("<missing>")
                ));
            }
        }
    }
}

fn check_routes(
    case_name: &str,
    merged: &MergedStatus<'_>,
    expected: &[RouteExpectation],
    failures: &mut Vec<String>,
) {
    for expectation in expected {
        let Some(route) = merged.routes.get(&expectation.name) else {
            failures.push(format!("{case_name}: missing route `{}`", expectation.name));
            continue;
        };
        if let Some(expected) = &expectation.backend_health_state {
            compare_optional_string(
                failures,
                case_name,
                &format!("route `{}` backend_health_state", expectation.name),
                route.get("backend_health_state").and_then(Value::as_str),
                expected,
            );
        }
        if let Some(expected) = expectation.dropped_samples_min {
            let actual = route
                .get("dropped_samples")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            if actual < expected {
                failures.push(format!(
                    "{case_name}: route `{}` dropped_samples expected >= {expected}, got {actual}",
                    expectation.name
                ));
            }
        }
    }
}

fn check_failovers(
    case_name: &str,
    merged: &MergedStatus<'_>,
    expected: &[FailoverExpectation],
    failures: &mut Vec<String>,
) {
    for expectation in expected {
        let matched = merged.failovers.iter().any(|event| {
            event.get("group").and_then(Value::as_str) == Some(expectation.group.as_str())
                && event.get("old_active").and_then(Value::as_str)
                    == Some(expectation.old_active.as_str())
                && event.get("new_active").and_then(Value::as_str)
                    == Some(expectation.new_active.as_str())
                && expectation.reason.as_ref().is_none_or(|expected_reason| {
                    event.get("reason").and_then(Value::as_str) == Some(expected_reason.as_str())
                })
        });
        if !matched {
            failures.push(format!(
                "{case_name}: missing failover group `{}` {} -> {}",
                expectation.group, expectation.old_active, expectation.new_active
            ));
        }
    }
}

fn compare_optional_string(
    failures: &mut Vec<String>,
    case_name: &str,
    field: &str,
    actual: Option<&str>,
    expected: &str,
) {
    if actual != Some(expected) {
        failures.push(format!(
            "{case_name}: {field} expected `{expected}`, got `{}`",
            actual.unwrap_or("<missing>")
        ));
    }
}

fn worst_health(values: &[String]) -> Option<String> {
    values
        .iter()
        .max_by_key(|value| health_rank(value))
        .map(ToOwned::to_owned)
}

fn health_rank(value: &str) -> u8 {
    match value {
        "healthy" | "ready" => 0,
        "warn" | "warning" => 1,
        "stale" => 2,
        "degraded" | "reconnecting" => 3,
        "faulted" | "failed" | "error" => 4,
        _ => 2,
    }
}

fn route_score(route: &Value) -> u64 {
    let dropped = route
        .get("dropped_samples")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let health = route
        .get("backend_health_state")
        .and_then(Value::as_str)
        .map(health_rank)
        .unwrap_or(0) as u64;
    dropped.saturating_mul(16).saturating_add(health)
}

fn format_optional_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "<missing>".to_string())
}
