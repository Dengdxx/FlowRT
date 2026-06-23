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

#[test]
fn fault_matrix_check_rejects_cross_process_without_global_tick() {
    let dir = temp_test_dir("fault-matrix-cross-process");
    let rsdl_dir = dir.join("rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    write_fault_matrix_cross_process_rsdl(&rsdl_dir, "");
    let matrix = dir.join("fault-matrix.toml");
    std::fs::write(
        &matrix,
        r#"
[matrix]
name = "cross"
rsdl = "rsdl/robot.rsdl"
run_ticks = 3
mode = "launch"

[[case]]
name = "drop"

[[case.inject]]
kind = "backend_drop"
instance = "source"
task = "main"
invocations = [1]

[case.expect.graph]
graph_health = "healthy"
"#,
    )
    .unwrap();

    let error = fault_matrix::check::check_matrix(&matrix).unwrap_err();
    assert!(error.to_string().contains("global_tick"));
}

#[test]
fn fault_matrix_check_accepts_global_tick_case() {
    let dir = temp_test_dir("fault-matrix-global-tick");
    let rsdl_dir = dir.join("rsdl");
    std::fs::create_dir_all(&rsdl_dir).unwrap();
    write_fault_matrix_cross_process_rsdl(
        &rsdl_dir,
        r#"
[profile.default.determinism]
mode = "global_tick"
timeout_ms = 1000
on_timeout = "fault_graph"
"#,
    );
    let matrix = dir.join("fault-matrix.toml");
    std::fs::write(
        &matrix,
        r#"
[matrix]
name = "cross"
rsdl = "rsdl/robot.rsdl"
run_ticks = 3
mode = "launch"

[[case]]
name = "drop"

[[case.inject]]
kind = "backend_drop"
instance = "source"
task = "main"
invocations = [1]

[case.expect.graph]
graph_health = "healthy"
"#,
    )
    .unwrap();

    let report = fault_matrix::check::check_matrix(&matrix).unwrap();
    assert_eq!(report.matrix, "cross");
    assert_eq!(report.cases.len(), 1);
    assert_eq!(report.cases[0].name, "drop");
    assert_eq!(report.cases[0].mode, "launch");
    assert_eq!(report.cases[0].run_ticks, 3);
    assert_eq!(report.cases[0].expectations, 1);
    assert_eq!(report.cases[0].status, "ok");
}

#[test]
fn fault_matrix_replay_source_writes_fixed_boundary_default_payload() {
    let dir = temp_test_dir("fault-matrix-replay-fixed-payload");
    std::fs::create_dir_all(&dir).unwrap();
    let contract = contract_from_source(
        r#"
[package]
name = "fault_matrix_replay_payload"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
input = ["sample:Sample"]
output = ["echo:Sample"]

[component.sink]
language = "rust"
input = ["echo:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "on_message"
input = ["sample"]
output = ["echo"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["echo"]

[[bind.dataflow]]
from = "source.echo"
to = "sink.echo"
channel = "latest"

[[boundary.input]]
name = "feed"
port = "source.sample"
type = "Sample"

[profile.default]
mode = "island"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );

    let replay = fault_matrix::runner::write_case_replay_source(&dir, &contract).unwrap();
    let timeline = flowrt_record::read_replay_timeline_from_path(&replay).unwrap();

    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0].target, "feed");
    assert_eq!(timeline[0].payload, vec![0, 0, 0, 0]);
}

#[test]
fn fault_matrix_replay_source_uses_canonical_frame_wire_size_for_padded_fixed_type() {
    let dir = temp_test_dir("fault-matrix-replay-padded-fixed-payload");
    std::fs::create_dir_all(&dir).unwrap();
    let contract = contract_from_source(
        r#"
[package]
name = "fault_matrix_replay_padded"
rsdl_version = "0.1"

[type.Sample]
flag = "u8"
value = "u32"

[component.source]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "on_message"
input = ["sample"]

[[boundary.input]]
name = "feed"
port = "source.sample"
type = "Sample"

[profile.default]
mode = "island"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );

    let replay = fault_matrix::runner::write_case_replay_source(&dir, &contract).unwrap();
    let timeline = flowrt_record::read_replay_timeline_from_path(&replay).unwrap();

    assert_eq!(timeline[0].payload, vec![0, 0, 0, 0, 0]);
}

#[test]
fn fault_matrix_replay_source_writes_empty_variable_frame_payload() {
    let dir = temp_test_dir("fault-matrix-replay-variable-payload");
    std::fs::create_dir_all(&dir).unwrap();
    let contract = contract_from_source(
        r#"
[package]
name = "fault_matrix_replay_variable"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes<max=8>"
label = "string<max=12>"

[component.source]
language = "rust"
input = ["packet:Packet"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "on_message"
input = ["packet"]

[[boundary.input]]
name = "feed"
port = "source.packet"
type = "Packet"

[profile.default]
mode = "island"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );

    let replay = fault_matrix::runner::write_case_replay_source(&dir, &contract).unwrap();
    let timeline = flowrt_record::read_replay_timeline_from_path(&replay).unwrap();

    assert_eq!(timeline[0].payload, vec![0; 16]);
}

#[test]
fn fault_matrix_expectation_evaluator_accepts_route_and_failover() {
    let status = serde_json::json!({
        "mode": "launch",
        "processes": [{
            "process": "controller",
            "runtime": "rust",
            "status": {
                "tick_count": 6,
                "graph_health": "healthy",
                "graph_critical_health": "healthy",
                "instances": [{
                    "instance": "controller_a",
                    "lifecycle_state": "running",
                    "restart_count": 1,
                    "last_fault_reason": "restart path",
                    "last_fault_tick": 2,
                    "last_transition_tick": 3
                }],
                "routes": [{
                    "name": "controller_a.cmd_to_actuator.cmd",
                    "backend_health_state": "degraded",
                    "dropped_samples": 1
                }],
                "failovers": [{
                    "event": "failover",
                    "group": "controller_ha",
                    "old_active": "controller_a",
                    "new_active": "controller_b",
                    "tick_id": 2,
                    "reason": "critical_fault"
                }]
            }
        }]
    });
    let expect = fault_matrix::model::MatrixExpectations {
        graph: Some(fault_matrix::model::GraphExpectation {
            graph_health: Some("healthy".to_string()),
            graph_critical_health: Some("healthy".to_string()),
        }),
        instance: vec![fault_matrix::model::InstanceExpectation {
            name: "controller_a".to_string(),
            lifecycle_state: Some("running".to_string()),
            restart_count: Some(1),
            last_fault_reason_contains: Some("restart".to_string()),
        }],
        route: vec![fault_matrix::model::RouteExpectation {
            name: "controller_a.cmd_to_actuator.cmd".to_string(),
            backend_health_state: Some("degraded".to_string()),
            dropped_samples_min: Some(1),
        }],
        failover: vec![fault_matrix::model::FailoverExpectation {
            group: "controller_ha".to_string(),
            old_active: "controller_a".to_string(),
            new_active: "controller_b".to_string(),
            reason: Some("critical_fault".to_string()),
        }],
    };

    let result =
        fault_matrix::expect::evaluate_expectations("backend_drop_failover", &status, &expect);

    assert!(result.passed, "{:?}", result.failures);
}

#[test]
fn fault_matrix_run_report_preserves_failure_details_before_exit() {
    let report = fault_matrix::runner::FaultMatrixRunReport {
        matrix: "demo".to_string(),
        cases: vec![fault_matrix::expect::FaultMatrixCaseResult {
            name: "bad_case".to_string(),
            passed: false,
            failures: vec!["graph health expected healthy, got faulted".to_string()],
        }],
    };

    let value = serde_json::to_value(&report).unwrap();

    assert_eq!(value["matrix"], "demo");
    assert_eq!(value["cases"][0]["name"], "bad_case");
    assert_eq!(value["cases"][0]["passed"], false);
    assert_eq!(
        value["cases"][0]["failures"][0],
        "graph health expected healthy, got faulted"
    );

    let error = report.ensure_passed().unwrap_err();
    assert!(error.to_string().contains(
        "fault matrix case `bad_case` failed: graph health expected healthy, got faulted"
    ));
}

fn write_fault_matrix_cross_process_rsdl(rsdl_dir: &Path, determinism: &str) {
    std::fs::write(
        rsdl_dir.join("robot.rsdl"),
        format!(
            r#"
[package]
name = "fault_matrix_cross_process"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
input = ["sample:Sample"]
output = ["echo:Sample"]

[component.sink]
language = "rust"
input = ["echo:Sample"]

[instance.source]
component = "source"
process = "source_proc"

[instance.source.task]
trigger = "on_message"
input = ["sample"]
output = ["echo"]

[instance.sink]
component = "sink"
process = "sink_proc"

[instance.sink.task]
trigger = "on_message"
input = ["echo"]

[[process]]
name = "source_proc"

[[process]]
name = "sink_proc"

[[bind.dataflow]]
from = "source.echo"
to = "sink.echo"
channel = "latest"

[profile.default]
mode = "island"
backend = "zenoh"
{determinism}

[[boundary.input]]
name = "feed"
port = "source.sample"
type = "Sample"

[target.linux]
runtime = ["rust"]
backends = ["zenoh"]
"#
        ),
    )
    .unwrap();
}
