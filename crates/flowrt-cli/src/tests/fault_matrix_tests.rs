use super::*;

#[test]
fn fault_matrix_parser_rejects_unknown_fields() {
    let dir = temp_test_dir("fault-matrix-unknown-field");
    std::fs::create_dir_all(&dir).unwrap();
    let matrix = dir.join("fault-matrix.toml");
    std::fs::write(
        &matrix,
        r#"
[matrix]
name = "bad"
rsdl = "robot.rsdl"
profile = "deterministic"
run_ticks = 3
mode = "run"
extra = "typo"

[[case]]
name = "restart"

[[case.inject]]
kind = "status_error"
instance = "controller"
task = "main"
invocations = [1]

[case.expect.graph]
graph_health = "healthy"
"#,
    )
    .unwrap();

    let error = fault_matrix::model::parse_matrix_file(&matrix).unwrap_err();
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn fault_matrix_parser_normalizes_case_defaults() {
    let dir = temp_test_dir("fault-matrix-defaults");
    std::fs::create_dir_all(&dir).unwrap();
    let matrix = dir.join("fault-matrix.toml");
    std::fs::write(
        &matrix,
        r#"
[matrix]
name = "demo"
rsdl = "rsdl/robot.rsdl"
profile = "deterministic"
run_ticks = 6
mode = "launch"

[[case]]
name = "restart"

[[case.inject]]
kind = "status_error"
instance = "controller"
task = "main"
invocations = [2]
reason = "restart path"

[[case.expect.instance]]
name = "controller"
lifecycle_state = "running"
restart_count = 1

[case.expect.graph]
graph_health = "healthy"
"#,
    )
    .unwrap();

    let matrix = fault_matrix::model::parse_matrix_file(&matrix).unwrap();
    assert_eq!(matrix.name, "demo");
    assert_eq!(matrix.cases[0].profile.as_deref(), Some("deterministic"));
    assert_eq!(matrix.cases[0].run_ticks, 6);
    assert_eq!(
        matrix.cases[0].mode,
        fault_matrix::model::FaultMatrixMode::Launch
    );
    assert_eq!(matrix.cases[0].inject[0].instance, "controller");
}

#[test]
fn fault_matrix_parser_reads_all_expectation_shapes() {
    let dir = temp_test_dir("fault-matrix-full-expect");
    std::fs::create_dir_all(&dir).unwrap();
    let matrix = dir.join("fault-matrix.toml");
    std::fs::write(
        &matrix,
        r#"
[matrix]
name = "full"
rsdl = "rsdl/robot.rsdl"
run_ticks = 4
mode = "run"

[[case]]
name = "backend-drop"
profile = "global_tick"
run_ticks = 8
mode = "launch"

[[case.inject]]
kind = "backend_drop"
instance = "source"
task = "main"
from_invocation = 2
reason = "drop once"

[case.expect.graph]
graph_health = "healthy"
graph_critical_health = "healthy"

[[case.expect.instance]]
name = "source"
lifecycle_state = "running"
restart_count = 0
last_fault_reason_contains = "backend"

[[case.expect.route]]
name = "source.sample->sink.sample"
backend_health_state = "degraded"
dropped_samples_min = 1

[[case.expect.failover]]
group = "control"
old_active = "primary"
new_active = "standby"
reason = "critical"
"#,
    )
    .unwrap();

    let matrix = fault_matrix::model::parse_matrix_file(&matrix).unwrap();
    assert_eq!(matrix.name, "full");
    assert_eq!(matrix.rsdl, PathBuf::from("rsdl/robot.rsdl"));
    let case = &matrix.cases[0];
    assert_eq!(case.name, "backend-drop");
    assert_eq!(case.profile.as_deref(), Some("global_tick"));
    assert_eq!(case.run_ticks, 8);
    assert_eq!(case.mode, fault_matrix::model::FaultMatrixMode::Launch);
    assert_eq!(case.inject[0].kind.as_str(), "backend_drop");
    assert_eq!(case.inject[0].from_invocation, Some(2));
    assert_eq!(case.inject[0].reason, "drop once");

    let graph = case.expect.graph.as_ref().unwrap();
    assert_eq!(graph.graph_health.as_deref(), Some("healthy"));
    assert_eq!(graph.graph_critical_health.as_deref(), Some("healthy"));
    assert_eq!(case.expect.instance[0].name, "source");
    assert_eq!(
        case.expect.instance[0].lifecycle_state.as_deref(),
        Some("running")
    );
    assert_eq!(case.expect.instance[0].restart_count, Some(0));
    assert_eq!(
        case.expect.instance[0]
            .last_fault_reason_contains
            .as_deref(),
        Some("backend")
    );
    assert_eq!(case.expect.route[0].name, "source.sample->sink.sample");
    assert_eq!(
        case.expect.route[0].backend_health_state.as_deref(),
        Some("degraded")
    );
    assert_eq!(case.expect.route[0].dropped_samples_min, Some(1));
    assert_eq!(case.expect.failover[0].group, "control");
    assert_eq!(case.expect.failover[0].old_active, "primary");
    assert_eq!(case.expect.failover[0].new_active, "standby");
    assert_eq!(case.expect.failover[0].reason.as_deref(), Some("critical"));
}
