use std::collections::BTreeMap;

use flowrt_rsdl::{LoadedDocument, RawDocument, RawModuleDocument};

use crate::{ContractIr, EntityRef, Result};

mod backends;
mod graphs;
mod ids;
mod modules;
mod operations;
mod params;
mod profiles;
mod resolver;
mod services;
mod targets;

pub use ids::hash_source;
pub use params::{param_value_compatible, param_value_kind};
pub use profiles::project_contract_to_profile;

/// 将已解析的 RSDL 文档归一化为 Contract IR。
pub fn normalize_document(document: &RawDocument, source_hash: String) -> Result<ContractIr> {
    normalize_document_with_modules(document, &[], source_hash)
}

/// 将带 workspace/module 边界的 RSDL 文档归一化为 Contract IR。
pub fn normalize_loaded_document(
    loaded: &LoadedDocument,
    source_hash: String,
) -> Result<ContractIr> {
    normalize_document_with_modules(&loaded.document, &loaded.modules, source_hash)
}

fn normalize_document_with_modules(
    document: &RawDocument,
    raw_modules: &[RawModuleDocument],
    source_hash: String,
) -> Result<ContractIr> {
    let package_qualified_name = format!(
        "{}@{}",
        document.package.name,
        document.package.version.as_deref().unwrap_or("0.0.0")
    );
    let package_id = ids::entity_id("package", &package_qualified_name);

    let package = crate::PackageIr {
        name: document.package.name.clone(),
        version: document.package.version.clone(),
        rsdl_version: document.package.rsdl_version.clone(),
        imports: document
            .package
            .imports
            .iter()
            .map(|(kind, patterns)| {
                let mut patterns = patterns.clone();
                patterns.sort();
                crate::ImportIr {
                    kind: kind.clone(),
                    patterns,
                }
            })
            .collect(),
    };

    let mut name_resolver = resolver::NameResolver::new(raw_modules);
    name_resolver.register_document_symbols(document);
    let normalized_modules = modules::normalize_modules(raw_modules);

    let types = modules::normalize_types(document, raw_modules, &name_resolver)?;
    let type_ids = types
        .iter()
        .map(|ty| (ty.qualified_name.clone(), ty.id.clone()))
        .collect::<BTreeMap<_, _>>();

    let components =
        modules::normalize_components(document, raw_modules, &name_resolver, &type_ids)?;
    let component_ids = components
        .iter()
        .map(|component| (component.qualified_name.clone(), component.id.clone()))
        .collect::<BTreeMap<_, _>>();

    let profiles = profiles::normalize_profiles(document)?;
    let targets = targets::normalize_targets(document)?;
    let target_ids = targets
        .iter()
        .map(|target| (target.name.clone(), target.id.clone()))
        .collect::<BTreeMap<_, _>>();

    let graph_id = ids::entity_id("graph", "default");
    let graph_name = "default".to_string();
    let (instances, mut tasks) = graphs::normalize_instances(
        document,
        raw_modules,
        &name_resolver,
        &component_ids,
        &target_ids,
        &graph_name,
    )?;
    tasks.sort_by(|left, right| {
        (&left.instance.name, &left.name).cmp(&(&right.instance.name, &right.name))
    });
    let instance_refs = instances
        .iter()
        .map(|instance| {
            (
                instance.name.clone(),
                EntityRef {
                    id: instance.id.clone(),
                    name: instance.name.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let binds = graphs::normalize_binds(
        document,
        &instance_refs,
        &types,
        &components,
        &instances,
        &profiles,
    )?;
    let processes = graphs::normalize_processes(document, &instances)?;
    let external_processes = graphs::normalize_external_processes(document)?;
    let service_edges = services::normalize_service_binds(
        document,
        &instance_refs,
        &instances,
        &components,
        &graph_name,
    )?;
    let operation_edges = operations::normalize_operation_binds(
        document,
        &instance_refs,
        &instances,
        &components,
        &graph_name,
    )?;
    let ros2_bridges = services::normalize_ros2_bridges(document, &instance_refs, &graph_name)?;
    let graph = crate::GraphIr {
        id: graph_id.clone(),
        name: graph_name.clone(),
        instances,
        processes,
        external_processes,
        tasks,
        binds,
        services: service_edges,
        operations: operation_edges,
        ros2_bridges,
    };

    let deployments =
        graphs::normalize_deployments(&graph, &types, &components, &profiles, &targets);

    Ok(ContractIr {
        ir_version: crate::CONTRACT_IR_VERSION.to_string(),
        schema_version: crate::CONTRACT_SCHEMA_VERSION.to_string(),
        source_hash,
        package_id,
        package,
        modules: normalized_modules,
        types,
        components,
        graphs: vec![graph],
        profiles,
        targets,
        deployments,
    })
}

#[cfg(test)]
mod tests {
    use flowrt_rsdl::{load_file, parse_str};

    use super::*;
    use crate::{
        CapabilityAtom, ChannelBackendSource, ChannelKind, IrError, OperationBackendSource,
        OperationConcurrencyPolicy, OperationFeedbackPolicy, OperationPreemptPolicy,
        OverflowPolicy, ParamType, ParamUpdatePolicy, ParamValue, PolicyValueSource, PrimitiveType,
        ProcessFailurePropagation, ProcessReadinessGate, ProcessRestartPolicyKind, RouteTopology,
        RtPolicy, ServiceBackendSource, ServiceOverflowPolicy, StalePolicy, TaskReadiness,
        TypeExpr, channel_route_capabilities, deployment_capability_decision,
    };

    #[test]
    fn normalizes_minimal_document() {
        let source = r#"
[package]
name = "robot_demo"
version = "0.1.0"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.imu_sim]
language = "rust"
output = ["imu:Imu"]

[instance.imu_sim]
component = "imu_sim"
process = "main"
target = "linux"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[profile.default]
backend = "inproc"
worker_threads = 3
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[target.linux]
platform = "linux-x86_64"
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert_eq!(ir.package.name, "robot_demo");
        assert_eq!(
            ir.types[0].fields[0].ty,
            TypeExpr::Primitive {
                name: PrimitiveType::U64
            }
        );
        assert_eq!(ir.graphs[0].tasks[0].period_ms, Some(5));
        assert_eq!(ir.profiles[0].scheduler.worker_threads, 3);
    }

    #[test]
    fn normalizes_external_component_and_process_contract() {
        let source = r#"
[package]
name = "external_demo"
version = "0.1.0"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.fake_sensor]
language = "external"
kind = "external"
output = ["sample:Sample"]

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"
target = "linux"

[[process]]
name = "sensor_proc"
readiness = "runtime_ready"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "driver"
args = ["--rate", "50"]
working_dir = "package"
health = "runtime_socket"
required_backends = ["zenoh"]

[profile.default]
backend = "zenoh"

[target.linux]
platform = "linux-arm64"
runtime = ["external"]
backends = ["zenoh"]
"#;

        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        let component = &ir.components[0];
        assert_eq!(component.language, crate::LanguageKind::External);
        assert_eq!(component.kind, crate::ComponentKind::External);
        assert_eq!(ir.targets[0].runtime, vec![crate::LanguageKind::External]);
        assert_eq!(ir.graphs[0].external_processes.len(), 1);
        let external = &ir.graphs[0].external_processes[0];
        assert_eq!(external.process, "sensor_proc");
        assert_eq!(external.package, "fake_sensor_driver");
        assert_eq!(external.executable, "driver");
        assert_eq!(external.args, vec!["--rate", "50"]);
        assert_eq!(external.working_dir, crate::ExternalWorkingDir::Package);
        assert_eq!(external.health, crate::ExternalHealthKind::RuntimeSocket);
        assert_eq!(
            external.required_backends,
            vec![crate::BackendName("zenoh".to_string())]
        );
    }

    #[test]
    fn external_dataflow_auto_backend_resolves_to_zenoh() {
        let source = r#"
[package]
name = "external_route_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.fake_sensor]
language = "external"
kind = "external"
output = ["sample:Sample"]

[component.monitor]
language = "rust"
input = ["sample:Sample"]

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"
target = "linux"

[instance.monitor]
component = "monitor"
process = "monitor_proc"
target = "linux"

[instance.monitor.task]
trigger = "on_message"
input = ["sample"]

[[process]]
name = "sensor_proc"

[[process]]
name = "monitor_proc"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "driver"

[[bind.dataflow]]
from = "fake_sensor.sample"
to = "monitor.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust", "external"]
backends = ["inproc", "zenoh"]
"#;

        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert_eq!(ir.graphs[0].binds[0].backend.0, "zenoh");
        assert_eq!(
            ir.graphs[0].binds[0].backend_source,
            ChannelBackendSource::AutoFallback
        );
    }

    #[test]
    fn rejects_explicit_inproc_for_external_dataflow() {
        let source = r#"
[package]
name = "external_route_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.fake_sensor]
language = "external"
kind = "external"
output = ["sample:Sample"]

[component.monitor]
language = "rust"
input = ["sample:Sample"]

[instance.fake_sensor]
component = "fake_sensor"
process = "sensor_proc"

[instance.monitor]
component = "monitor"
process = "monitor_proc"

[[external_process]]
process = "sensor_proc"
package = "fake_sensor_driver"
executable = "driver"

[[bind.dataflow]]
from = "fake_sensor.sample"
to = "monitor.sample"
channel = "latest"
backend = "inproc"
"#;

        let raw = parse_str(source).unwrap();
        let err = normalize_document(&raw, hash_source(source))
            .expect_err("external dataflow must not use inproc");
        assert!(format!("{err}").contains("external dataflow route cannot use `inproc`"));
    }

    #[test]
    fn external_service_auto_backend_resolves_to_zenoh() {
        let source = r#"
[package]
name = "external_service_demo"
rsdl_version = "0.1"

[type.Request]
value = "u32"

[type.Response]
accepted = "bool"

[component.client]
language = "rust"
service_client = ["plan:Request->Response"]

[component.external_planner]
language = "external"
kind = "external"
service_server = ["plan:Request->Response"]

[instance.client]
component = "client"
process = "client_proc"

[instance.external_planner]
component = "external_planner"
process = "planner_proc"

[[external_process]]
process = "planner_proc"
package = "planner_driver"
executable = "planner"

[[bind.service]]
client = "client.plan"
server = "external_planner.plan"

[profile.default]
backend = "inproc"
"#;

        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert_eq!(ir.graphs[0].services[0].backend.0, "zenoh");
        assert_eq!(
            ir.graphs[0].services[0].backend_source,
            ServiceBackendSource::AutoResolved
        );
    }

    #[test]
    fn rejects_explicit_inproc_for_external_operation() {
        let source = r#"
[package]
name = "external_operation_demo"
rsdl_version = "0.1"

[type.Goal]
target = "u32"

[type.Feedback]
progress = "u32"

[type.Result]
ok = "bool"

[component.client]
language = "rust"

[component.client.operation_client.plan]
goal = "Goal"
feedback = "Feedback"
result = "Result"

[component.external_planner]
language = "external"
kind = "external"

[component.external_planner.operation_server.plan]
goal = "Goal"
feedback = "Feedback"
result = "Result"

[instance.client]
component = "client"
process = "client_proc"

[instance.external_planner]
component = "external_planner"
process = "planner_proc"

[[external_process]]
process = "planner_proc"
package = "planner_driver"
executable = "planner"

[[bind.operation]]
client = "client.plan"
server = "external_planner.plan"
backend = "inproc"
"#;

        let raw = parse_str(source).unwrap();
        let err = normalize_document(&raw, hash_source(source))
            .expect_err("external operation must not use inproc");
        assert!(format!("{err}").contains("external operation route cannot use `inproc`"));
    }

    #[test]
    fn normalizes_named_task_array_for_one_instance() {
        let source = r#"
[package]
name = "multi_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 100
output = ["slow"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let tasks = &ir.graphs[0].tasks;

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "fast_loop");
        assert_eq!(tasks[1].name, "slow_loop");
        assert_ne!(tasks[0].id, tasks[1].id);
        assert_eq!(tasks[0].outputs, vec!["fast"]);
        assert_eq!(tasks[1].outputs, vec!["slow"]);
    }

    #[test]
    fn normalizes_scheduler_v2_task_fields() {
        let source = r#"
[package]
name = "scheduler_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
input = ["in:u32"]
output = ["out:u32"]

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "on_message"
readiness = "all_ready"
lane = "worker_serial"
priority = 7
input = ["in"]
output = ["out"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let task = &ir.graphs[0].tasks[0];

        assert_eq!(task.readiness, TaskReadiness::AllReady);
        assert_eq!(task.lane.as_deref(), Some("worker_serial"));
        assert_eq!(task.priority, Some(7));
    }

    #[test]
    fn normalizes_process_orchestration_defaults_and_overrides() {
        let source = r#"
[package]
name = "process_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensor_proc"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control_proc"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[[process]]
name = "sensor_proc"
restart = "on_failure"
max_restarts = 5
initial_delay_ms = 50
max_delay_ms = 500
failure = "propagate"

[[process]]
name = "control_proc"
depends_on = ["sensor_proc"]
restart = "never"
failure = "isolate"

[profile.default]
backend = "iox2"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let processes = &ir.graphs[0].processes;

        assert_eq!(processes.len(), 2);
        assert_eq!(processes[0].name, "control_proc");
        assert_eq!(processes[0].depends_on, vec!["sensor_proc"]);
        assert_eq!(processes[0].restart.policy, ProcessRestartPolicyKind::Never);
        assert_eq!(processes[0].restart.max_restarts, 0);
        assert_eq!(
            processes[0].failure_propagation,
            ProcessFailurePropagation::Isolate
        );
        assert_eq!(processes[1].name, "sensor_proc");
        assert_eq!(
            processes[1].restart.policy,
            ProcessRestartPolicyKind::OnFailure
        );
        assert_eq!(processes[1].restart.max_restarts, 5);
        assert_eq!(processes[1].restart.initial_delay_ms, 50);
        assert_eq!(processes[1].restart.max_delay_ms, 500);
        assert_eq!(
            processes[1].failure_propagation,
            ProcessFailurePropagation::Propagate
        );
    }

    #[test]
    fn normalizes_process_resource_hints() {
        let source = r#"
[package]
name = "resource_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensor_proc"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control_proc"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[[process]]
name = "sensor_proc"
restart = "on_failure"
max_restarts = 5
initial_delay_ms = 50
max_delay_ms = 500
failure = "propagate"
readiness = "runtime_ready"
startup_delay_ms = 200
cpu_affinity = [0, 1]
nice = -5
rt_policy = "fifo"
rt_priority = 50

[[process]]
name = "control_proc"
depends_on = ["sensor_proc"]
restart = "never"
failure = "isolate"
readiness = "service_ready"
env = { APP_MODE = "control" }

[profile.default]
backend = "iox2"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let processes = &ir.graphs[0].processes;

        assert_eq!(processes.len(), 2);
        let control = processes.iter().find(|p| p.name == "control_proc").unwrap();
        let sensor = processes.iter().find(|p| p.name == "sensor_proc").unwrap();

        assert_eq!(control.readiness, ProcessReadinessGate::ServiceReady);
        assert_eq!(control.startup_delay_ms, 0);
        assert_eq!(control.env["APP_MODE"], "control");
        assert!(control.cpu_affinity.is_empty());
        assert_eq!(control.nice, None);
        assert_eq!(control.rt_policy, None);
        assert_eq!(control.rt_priority, None);

        assert_eq!(sensor.readiness, ProcessReadinessGate::RuntimeReady);
        assert_eq!(sensor.startup_delay_ms, 200);
        assert_eq!(sensor.cpu_affinity, vec![0, 1]);
        assert_eq!(sensor.nice, Some(-5));
        assert_eq!(sensor.rt_policy, Some(RtPolicy::Fifo));
        assert_eq!(sensor.rt_priority, Some(50));
    }

    #[test]
    fn normalizes_process_defaults_when_no_resource_hints() {
        let source = r#"
[package]
name = "default_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[profile.default]
backend = "inproc"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let processes = &ir.graphs[0].processes;

        assert_eq!(processes.len(), 1);
        assert_eq!(processes[0].name, "main");
        assert_eq!(processes[0].readiness, ProcessReadinessGate::ProcessStarted);
        assert_eq!(processes[0].startup_delay_ms, 0);
        assert!(processes[0].env.is_empty());
        assert!(processes[0].cpu_affinity.is_empty());
        assert_eq!(processes[0].nice, None);
        assert_eq!(processes[0].rt_policy, None);
        assert_eq!(processes[0].rt_priority, None);
    }

    #[test]
    fn rejects_unknown_process_readiness_gate() {
        let source = r#"
[package]
name = "bad_readiness"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[[process]]
name = "main"
readiness = "custom_health_check"

[profile.default]
backend = "inproc"
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("unknown readiness gate should fail");

        assert!(matches!(
            error,
            IrError::InvalidEnum {
                kind: "process readiness gate",
                value,
                ..
            } if value == "custom_health_check"
        ));
    }

    #[test]
    fn rejects_unknown_rt_policy() {
        let source = r#"
[package]
name = "bad_rt"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[[process]]
name = "main"
rt_policy = "deadline"

[profile.default]
backend = "inproc"
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("unknown rt_policy should fail");

        assert!(matches!(
            error,
            IrError::InvalidEnum {
                kind: "RT scheduling policy",
                value,
                ..
            } if value == "deadline"
        ));
    }

    #[test]
    fn normalizes_service_ports_and_binds() {
        let source = r#"
[package]
name = "service_demo"
rsdl_version = "0.1"

[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[component.client]
language = "rust"
service_client = ["plan:PlanRequest->PlanResponse"]

[component.server]
language = "rust"
service_server = ["plan:PlanRequest->PlanResponse"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let client = ir
            .components
            .iter()
            .find(|component| component.name == "client")
            .unwrap();
        let service = &ir.graphs[0].services[0];

        assert_eq!(client.service_clients[0].name, "plan");
        assert_eq!(
            client.service_clients[0].request.canonical_syntax(),
            "PlanRequest"
        );
        assert_eq!(
            client.service_clients[0].response.canonical_syntax(),
            "PlanResponse"
        );
        assert_eq!(service.client.instance.name, "client");
        assert_eq!(service.client.port, "plan");
        assert_eq!(service.server.instance.name, "server");
        assert_eq!(service.server.port, "plan");
        // 默认 policy
        assert_eq!(service.backend.0, "inproc");
        assert_eq!(service.backend_source, ServiceBackendSource::AutoResolved);
        assert_eq!(service.policy.timeout_ms, 5000);
        assert_eq!(service.policy.queue_depth, 32);
        assert_eq!(service.policy.overflow, ServiceOverflowPolicy::Busy);
        assert_eq!(service.policy.lane, None);
        assert_eq!(service.policy.max_in_flight, 64);
    }

    #[test]
    fn normalizes_operation_ports_and_binds() {
        let source = r#"
[package]
name = "operation_demo"
rsdl_version = "0.1"

[type.PlanGoal]
target = "u32"

[type.PlanFeedback]
progress = "f32"

[type.PlanResult]
accepted = "bool"

[component.controller]
language = "rust"

[component.controller.operation_client.plan]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

[component.navigator]
language = "rust"

[component.navigator.operation_server.plan]
goal = "PlanGoal"
feedback = "PlanFeedback"
result = "PlanResult"

[instance.controller]
component = "controller"

[instance.navigator]
component = "navigator"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let client = ir
            .components
            .iter()
            .find(|component| component.name == "controller")
            .unwrap();
        let operation = &ir.graphs[0].operations[0];

        assert_eq!(client.operation_clients[0].name, "plan");
        assert_eq!(
            client.operation_clients[0].goal.canonical_syntax(),
            "PlanGoal"
        );
        assert_eq!(
            client.operation_clients[0].feedback.canonical_syntax(),
            "PlanFeedback"
        );
        assert_eq!(
            client.operation_clients[0].result.canonical_syntax(),
            "PlanResult"
        );
        assert_eq!(operation.client.instance.name, "controller");
        assert_eq!(operation.client.port, "plan");
        assert_eq!(operation.server.instance.name, "navigator");
        assert_eq!(operation.server.port, "plan");
        assert_eq!(operation.backend.0, "inproc");
        assert_eq!(
            operation.backend_source,
            OperationBackendSource::AutoResolved
        );
        assert_eq!(operation.policy.timeout_ms, 30000);
        assert_eq!(
            operation.policy.concurrency,
            OperationConcurrencyPolicy::Reject
        );
        assert_eq!(operation.policy.preempt, OperationPreemptPolicy::Reject);
        assert_eq!(operation.policy.queue_depth, 8);
        assert_eq!(operation.policy.max_in_flight, 1);
        assert_eq!(operation.policy.feedback, OperationFeedbackPolicy::Latest);
        assert_eq!(operation.policy.result_retention_ms, 60000);
    }

    #[test]
    fn operation_auto_backend_resolves_to_zenoh_for_cross_process() {
        let source = r#"
[package]
name = "operation_cross_process"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.operation_client.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[component.navigator]
language = "rust"

[component.navigator.operation_server.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[instance.controller]
component = "controller"
process = "control_proc"

[instance.navigator]
component = "navigator"
process = "nav_proc"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let operation = &ir.graphs[0].operations[0];

        assert_eq!(operation.backend.0, "zenoh");
        assert_eq!(
            operation.backend_source,
            OperationBackendSource::AutoResolved
        );
    }

    #[test]
    fn operation_bind_with_explicit_policy_fields() {
        let source = r#"
[package]
name = "operation_policy_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.operation_client.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[component.navigator]
language = "rust"

[component.navigator.operation_server.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[instance.controller]
component = "controller"

[instance.navigator]
component = "navigator"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
backend = "zenoh"
timeout_ms = 1000
concurrency = "queue"
preempt = "cancel_running"
queue_depth = 16
max_in_flight = 4
feedback = "fifo"
result_retention_ms = 2000
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let operation = &ir.graphs[0].operations[0];

        assert_eq!(operation.backend.0, "zenoh");
        assert_eq!(operation.backend_source, OperationBackendSource::Explicit);
        assert_eq!(operation.policy.timeout_ms, 1000);
        assert_eq!(
            operation.policy.concurrency,
            OperationConcurrencyPolicy::Queue
        );
        assert_eq!(
            operation.policy.preempt,
            OperationPreemptPolicy::CancelRunning
        );
        assert_eq!(operation.policy.queue_depth, 16);
        assert_eq!(operation.policy.max_in_flight, 4);
        assert_eq!(operation.policy.feedback, OperationFeedbackPolicy::Fifo);
        assert_eq!(operation.policy.result_retention_ms, 2000);
        assert_eq!(operation.policy_source.backend, PolicyValueSource::Explicit);
        assert_eq!(
            operation.policy_source.concurrency,
            PolicyValueSource::Explicit
        );
    }

    #[test]
    fn rejects_operation_bind_with_iox2_backend() {
        let source = r#"
[package]
name = "bad_operation"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.operation_client.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[component.navigator]
language = "rust"

[component.navigator.operation_server.plan]
goal = "u32"
feedback = "u32"
result = "bool"

[instance.controller]
component = "controller"

[instance.navigator]
component = "navigator"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
backend = "iox2"
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("iox2 operation backend should fail");

        assert!(matches!(
            error,
            IrError::InvalidEnum {
                kind: "operation backend",
                value,
                ..
            } if value == "iox2"
        ));
    }

    #[test]
    fn normalizes_workspace_module_qualified_names() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("modules")).unwrap();
        std::fs::create_dir_all(root.join("composition")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "workspace_demo"
rsdl_version = "0.1"

[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules/perception.rsdl"),
            r#"
[module]
name = "perception"

[type.Imu]
timestamp = "u64"

[component.imu_sim]
language = "rust"
output = ["imu:Imu"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules/control.rsdl"),
            r#"
[module]
name = "control"

[type.Odom]
timestamp = "u64"

[component.estimator]
language = "rust"
input = ["imu:perception::Imu"]
output = ["odom:Odom"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("composition/default.rsdl"),
            r#"
[instance.imu_sim]
component = "perception::imu_sim"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.estimator]
component = "control::estimator"

[instance.estimator.task]
trigger = "on_message"
input = ["imu"]
output = ["odom"]

[[bind.dataflow]]
from = "imu_sim.imu"
to = "estimator.imu"
channel = "latest"
"#,
        )
        .unwrap();

        let loaded = load_file(root.join("robot.rsdl")).unwrap();
        let ir =
            normalize_loaded_document(&loaded, hash_source(&loaded.source_bundle_text())).unwrap();

        assert_eq!(ir.modules.len(), 2);
        assert_eq!(ir.modules[0].name, "control");
        assert_eq!(ir.types[0].qualified_name, "control::Odom");
        assert_eq!(ir.types[1].qualified_name, "perception::Imu");
        assert_eq!(ir.components[0].qualified_name, "control::estimator");
        assert_eq!(ir.components[1].qualified_name, "perception::imu_sim");
        assert_eq!(
            ir.graphs[0].instances[0].component.name,
            "control::estimator"
        );
        assert_eq!(
            ir.components[0].inputs[0].ty.canonical_syntax(),
            "perception::Imu"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_ambiguous_short_type_references_across_modules() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("modules")).unwrap();
        std::fs::create_dir_all(root.join("composition")).unwrap();

        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "workspace_demo"
rsdl_version = "0.1"

[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules/a.rsdl"),
            r#"
[module]
name = "perception"

[type.Sample]
value = "u32"
"#,
        )
        .unwrap();
        std::fs::write(
            root.join("modules/b.rsdl"),
            r#"
[module]
name = "control"

[type.Sample]
value = "u64"
"#,
        )
        .unwrap();
        std::fs::write(root.join("composition/default.rsdl"), "").unwrap();

        let loaded = load_file(root.join("robot.rsdl")).unwrap();
        let error = normalize_loaded_document(&loaded, hash_source(&loaded.source_bundle_text()))
            .expect_err("ambiguous short type reference should fail");

        assert!(matches!(
            error,
            IrError::AmbiguousName { kind: "type", name, .. } if name == "Sample"
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn expands_dataflow_binds() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"

[component.producer]
language = "rust"
output = ["imu:Imu"]

[component.consumer]
language = "rust"
input = ["imu:Imu"]

[instance.producer]
component = "producer"

[instance.consumer]
component = "consumer"

[[bind.dataflow]]
from = "producer.imu"
to = "consumer.imu"
channel = "latest"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert_eq!(ir.graphs[0].binds[0].channel, ChannelKind::Latest);
        assert_eq!(ir.graphs[0].binds[0].depth, Some(1));
    }

    #[test]
    fn canonicalizes_bind_order_independent_of_source_order() {
        let source_a = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.alpha]
language = "rust"
input = ["sample:Sample"]

[component.beta]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"

[instance.alpha]
component = "alpha"

[instance.beta]
component = "beta"

[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest"
"#;
        let source_b = source_a.replace(
            r#"[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest""#,
            r#"[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest""#,
        );
        let raw_a = parse_str(source_a).unwrap();
        let raw_b = parse_str(&source_b).unwrap();
        let source_hash = hash_source("same logical source");

        let ir_a = normalize_document(&raw_a, source_hash.clone()).unwrap();
        let ir_b = normalize_document(&raw_b, source_hash).unwrap();

        assert_eq!(ir_a, ir_b);
    }

    #[test]
    fn canonicalizes_target_set_order_independent_of_source_order() {
        let source_a = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["iox2", "inproc"]
"#;
        let source_b = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["inproc", "iox2"]
"#;
        let raw_a = parse_str(source_a).unwrap();
        let raw_b = parse_str(source_b).unwrap();
        let source_hash = hash_source("same logical source");

        let ir_a = normalize_document(&raw_a, source_hash.clone()).unwrap();
        let ir_b = normalize_document(&raw_b, source_hash).unwrap();

        assert_eq!(ir_a, ir_b);
    }

    #[test]
    fn canonicalizes_import_pattern_order_independent_of_source_order() {
        let source_a = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/b.rsdl", "types/a.rsdl"]
components = ["components/b.rsdl", "components/a.rsdl"]
"#;
        let source_b = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/a.rsdl", "types/b.rsdl"]
components = ["components/a.rsdl", "components/b.rsdl"]
"#;
        let raw_a = parse_str(source_a).unwrap();
        let raw_b = parse_str(source_b).unwrap();
        let source_hash = hash_source("same logical source");

        let ir_a = normalize_document(&raw_a, source_hash.clone()).unwrap();
        let ir_b = normalize_document(&raw_b, source_hash).unwrap();

        assert_eq!(ir_a, ir_b);
    }

    #[test]
    fn deadline_tasks_require_deadline_aware_backend_capability() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
deadline_ms = 2

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert!(
            ir.deployments[0]
                .required_capabilities
                .contains(&CapabilityAtom("timing:deadline_aware".to_string()))
        );
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn int128_component_ports_require_route_abi_capability() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["sample:u128"]

[component.consumer]
language = "rust"
input = ["sample:u128"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert!(
            ir.graphs[0].binds[0]
                .capability_requirements
                .contains(&CapabilityAtom("abi:int128".to_string()))
        );
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn declared_int128_message_types_do_not_affect_unused_routes() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.UnusedWide]
value = "u128"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert!(
            !ir.graphs[0].binds[0]
                .capability_requirements
                .contains(&CapabilityAtom("abi:int128".to_string()))
        );
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn iox2_route_records_int128_abi_capability() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["sample:i128"]

[component.consumer]
language = "rust"
input = ["sample:i128"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert!(
            ir.graphs[0].binds[0]
                .capability_requirements
                .contains(&CapabilityAtom("abi:int128".to_string()))
        );
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn normalized_deployment_satisfied_matches_shared_capability_decision() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.WideSample]
value = "i128"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 5

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let deployment = &ir.deployments[0];
        let decision = deployment_capability_decision(
            &deployment.backend,
            &ir.targets[0].backends,
            &deployment.required_capabilities,
        );

        assert!(decision.selected_backend_known);
        assert!(decision.target_supports_selected_backend);
        assert!(decision.missing_required_capabilities.is_empty());
        assert_eq!(deployment.satisfied, decision.satisfied);
        assert!(deployment.satisfied);
    }

    #[test]
    fn inserts_implicit_default_profile_when_source_omits_profiles() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert_eq!(ir.profiles.len(), 1);
        assert_eq!(ir.profiles[0].name, "default");
        assert_eq!(ir.profiles[0].backend.0, "inproc");
        assert_eq!(ir.deployments.len(), 1);
        assert_eq!(ir.deployments[0].profile.name, "default");
        assert_eq!(ir.deployments[0].backend.0, "inproc");
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn rejects_instance_param_overrides_with_incompatible_types() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = 1.0
enabled = true
gains = [1.0, 2.0]

[component.controller.params.limits]
max = 10

[instance.controller]
component = "controller"

[instance.controller.params]
kp = "fast"
enabled = 1
gains = [true]

[instance.controller.params.limits]
max = false
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("incompatible parameter overrides should fail");

        assert!(matches!(
            error,
            IrError::IncompatibleParamOverride {
                instance,
                component,
                ..
            } if instance == "controller" && component == "controller"
        ));
    }

    #[test]
    fn rejects_non_empty_array_override_for_empty_default_array() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
gains = []

[instance.controller]
component = "controller"

[instance.controller.params]
gains = [true]
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("non-empty override for empty array default should fail");

        assert!(matches!(
            error,
            IrError::IncompatibleParamOverride {
                instance,
                component,
                param,
                ..
            } if instance == "controller" && component == "controller" && param == "gains"
        ));
    }

    #[test]
    fn normalizes_parameter_schema_and_legacy_defaults() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }
mode = { type = "string", default = "normal", enum = ["normal", "safe"], update = "on_tick" }
legacy_gain = 2.0

[instance.controller]
component = "controller"

[instance.controller.params]
kp = 2.5
mode = "safe"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let component = &ir.components[0];

        assert_eq!(component.params[0].name, "kp");
        assert_eq!(component.params[0].ty, ParamType::F32);
        assert_eq!(component.params[0].default, ParamValue::Float(1.0));
        assert_eq!(component.params[0].update, ParamUpdatePolicy::OnTick);
        assert_eq!(component.params[0].min, Some(ParamValue::Float(0.0)));
        assert_eq!(component.params[0].max, Some(ParamValue::Float(10.0)));

        assert_eq!(component.params[1].name, "legacy_gain");
        assert_eq!(component.params[1].ty, ParamType::F64);
        assert_eq!(component.params[1].update, ParamUpdatePolicy::Startup);

        assert_eq!(component.params[2].name, "mode");
        assert_eq!(component.params[2].ty, ParamType::String);
        assert_eq!(component.params[2].choices.len(), 2);
        assert_eq!(component.params[2].update, ParamUpdatePolicy::OnTick);

        let instance = &ir.graphs[0].instances[0];
        assert_eq!(instance.params[0].value, ParamValue::Float(2.5));
        assert_eq!(instance.params[1].value, ParamValue::Float(2.0));
        assert_eq!(
            instance.params[2].value,
            ParamValue::String("safe".to_string())
        );
    }

    #[test]
    fn rejects_parameter_override_outside_schema_range() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.params]
kp = 12.0
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("out-of-range parameter override should fail");

        assert!(matches!(
            error,
            IrError::InvalidParamSchema {
                component,
                param,
                ..
            } if component == "controller" && param == "kp"
        ));
    }

    #[test]
    fn rejects_unknown_parameter_update_policy() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = { type = "f32", default = 1.0, update = "immediate" }

[instance.controller]
component = "controller"
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("unknown parameter update policy should fail");

        assert!(matches!(
            error,
            IrError::InvalidEnum {
                context,
                kind: "parameter update policy",
                value
            } if context == "component.controller.params.kp.update" && value == "immediate"
        ));
    }

    #[test]
    fn projects_selected_profile_without_touching_other_profiles() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, Some("iox2")).unwrap();

        assert_eq!(ir.profiles.len(), 2);
        assert_eq!(projected.profiles.len(), 1);
        assert_eq!(projected.profiles[0].name, "iox2");
        assert_eq!(projected.profiles[0].backend.0, "iox2");
        assert_eq!(projected.deployments.len(), 1);
        assert_eq!(projected.deployments[0].profile.name, "iox2");
        assert_eq!(projected.deployments[0].backend.0, "iox2");
    }

    #[test]
    fn projects_selected_profile_channel_policy_defaults() {
        let source = r#"
[package]
name = "profile_policy_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["defaulted:Sample", "explicit:Sample"]

[component.consumer]
language = "rust"
input = ["defaulted:Sample", "explicit:Sample"]

[instance.producer]
component = "producer"
target = "linux"

[instance.consumer]
component = "consumer"
target = "linux"

[[bind.dataflow]]
from = "producer.defaulted"
to = "consumer.defaulted"
channel = "fifo"
depth = 2

[[bind.dataflow]]
from = "producer.explicit"
to = "consumer.explicit"
channel = "latest"
overflow = "drop_newest"
stale_policy = "hold_last"
max_age_ms = 7

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[profile.safety]
backend = "inproc"
default_overflow = "error"
default_stale_policy = "drop"
max_age_ms = 25

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, Some("safety")).unwrap();
        let defaulted = projected.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.to.port == "defaulted")
            .unwrap();
        let explicit = projected.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.to.port == "explicit")
            .unwrap();

        assert_eq!(defaulted.overflow, OverflowPolicy::Error);
        assert_eq!(defaulted.stale, StalePolicy::Drop);
        assert_eq!(defaulted.max_age_ms, Some(25));
        assert_eq!(
            defaulted.capability_requirements,
            channel_route_capabilities(
                &projected.types,
                &TypeExpr::Primitive {
                    name: PrimitiveType::U32
                },
                defaulted.channel,
                defaulted.overflow,
                defaulted.stale,
                RouteTopology::local()
            )
        );

        assert_eq!(explicit.overflow, OverflowPolicy::DropNewest);
        assert_eq!(explicit.stale, StalePolicy::HoldLast);
        assert_eq!(explicit.max_age_ms, Some(7));
    }

    #[test]
    fn projects_auto_route_backend_and_falls_back_for_variable_frames() {
        let source = r#"
[package]
name = "route_backend_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"

[type.Counter]
value = "u32"

[component.producer]
language = "rust"
output = ["packet:Packet", "counter:Counter"]

[component.consumer]
language = "rust"
input = ["packet:Packet", "counter:Counter"]

[instance.producer]
component = "producer"
target = "linux"

[instance.consumer]
component = "consumer"
target = "linux"

[[bind.dataflow]]
from = "producer.packet"
to = "consumer.packet"
channel = "latest"
backend = "auto"

[[bind.dataflow]]
from = "producer.counter"
to = "consumer.counter"
channel = "latest"

[profile.default]
backend = "inproc"

[profile.ipc]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2", "zenoh"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, Some("ipc")).unwrap();
        let packet = projected.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.from.port == "packet")
            .unwrap();
        let counter = projected.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.from.port == "counter")
            .unwrap();

        assert_eq!(packet.backend.0, "zenoh");
        assert_eq!(packet.backend_source, ChannelBackendSource::AutoFallback);
        assert_eq!(counter.backend.0, "iox2");
        assert_eq!(counter.backend_source, ChannelBackendSource::ProfileDefault);
    }

    #[test]
    fn explicit_route_backend_survives_profile_projection() {
        let source = r#"
[package]
name = "explicit_route_backend_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"

[component.producer]
language = "rust"
output = ["packet:Packet"]

[component.consumer]
language = "rust"
input = ["packet:Packet"]

[instance.producer]
component = "producer"
target = "linux"

[instance.consumer]
component = "consumer"
target = "linux"

[[bind.dataflow]]
from = "producer.packet"
to = "consumer.packet"
channel = "latest"
backend = "zenoh"

[profile.default]
backend = "inproc"

[profile.ipc]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2", "zenoh"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, Some("ipc")).unwrap();
        let bind = &projected.graphs[0].binds[0];

        assert_eq!(bind.backend.0, "zenoh");
        assert_eq!(bind.backend_source, ChannelBackendSource::Explicit);
    }

    #[test]
    fn projects_default_profile_when_selection_is_omitted() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, None).unwrap();

        assert_eq!(projected.profiles.len(), 1);
        assert_eq!(projected.profiles[0].name, "default");
        assert_eq!(projected.deployments.len(), 1);
        assert_eq!(projected.deployments[0].profile.name, "default");
        assert_eq!(projected.deployments[0].backend.0, "inproc");
    }

    #[test]
    fn projects_first_profile_when_selection_is_omitted_and_default_is_absent() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.alpha]
backend = "inproc"

[profile.beta]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, None).unwrap();

        assert_eq!(projected.profiles.len(), 1);
        assert_eq!(projected.profiles[0].name, "alpha");
        assert_eq!(projected.deployments.len(), 1);
        assert_eq!(projected.deployments[0].profile.name, "alpha");
        assert_eq!(projected.deployments[0].backend.0, "inproc");
    }

    #[test]
    fn rejects_unknown_profile_selection() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let error = project_contract_to_profile(&ir, Some("iox2"))
            .expect_err("unknown profile selection should fail");

        assert!(matches!(
            error,
            IrError::UnknownProfile { profile } if profile == "iox2"
        ));
    }

    /// 回归测试：覆盖 workspace import + module name resolver + service bind normalization
    /// + profile iox2 variable route auto-zenoh fallback 四个 seam。
    #[test]
    fn split_seams_regression_workspace_service_profile_fallback() {
        let root = unique_temp_dir();
        std::fs::create_dir_all(root.join("modules")).unwrap();
        std::fs::create_dir_all(root.join("composition")).unwrap();

        // workspace root
        std::fs::write(
            root.join("robot.rsdl"),
            r#"
[package]
name = "seam_regression"
rsdl_version = "0.1"

[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]
"#,
        )
        .unwrap();

        // perception module: outputs a variable-frame type (bytes)
        std::fs::write(
            root.join("modules/perception.rsdl"),
            r#"
[module]
name = "perception"

[type.SensorFrame]
payload = "bytes"

[component.sensor]
language = "rust"
output = ["frame:SensorFrame"]

[component.display]
language = "rust"
service_server = ["status:u32->bool"]
"#,
        )
        .unwrap();

        // control module: references perception::SensorFrame via qualified name
        std::fs::write(
            root.join("modules/control.rsdl"),
            r#"
[module]
name = "control"

[component.controller]
language = "rust"
input = ["frame:perception::SensorFrame"]
service_client = ["status:u32->bool"]
"#,
        )
        .unwrap();

        // composition: instances, dataflow bind, service bind
        std::fs::write(
            root.join("composition/default.rsdl"),
            r#"
[instance.sensor]
component = "perception::sensor"
process = "main"
target = "linux"

[instance.sensor.task]
trigger = "periodic"
period_ms = 10
output = ["frame"]

[instance.controller]
component = "control::controller"
process = "main"
target = "linux"

[instance.controller.task]
trigger = "on_message"
input = ["frame"]

[[bind.dataflow]]
from = "sensor.frame"
to = "controller.frame"
channel = "latest"

[[bind.service]]
client = "controller.status"
server = "display.status"

[instance.display]
component = "perception::display"
process = "main"
target = "linux"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2", "zenoh"]
"#,
        )
        .unwrap();

        let loaded = load_file(root.join("robot.rsdl")).unwrap();
        let ir =
            normalize_loaded_document(&loaded, hash_source(&loaded.source_bundle_text())).unwrap();

        // seam 1: workspace import resolved 2 modules
        assert_eq!(ir.modules.len(), 2);
        assert_eq!(ir.modules[0].name, "control");
        assert_eq!(ir.modules[1].name, "perception");

        // seam 2: module name resolver — qualified names work
        assert_eq!(ir.types[0].qualified_name, "perception::SensorFrame");
        assert_eq!(ir.components[0].qualified_name, "control::controller");
        assert_eq!(ir.components[1].qualified_name, "perception::display");
        assert_eq!(ir.components[2].qualified_name, "perception::sensor");
        // controller input references perception::SensorFrame
        assert_eq!(
            ir.components[0].inputs[0].ty.canonical_syntax(),
            "perception::SensorFrame"
        );

        // seam 3: service bind normalization
        assert_eq!(ir.graphs[0].services.len(), 1);
        let service = &ir.graphs[0].services[0];
        assert_eq!(service.client.instance.name, "controller");
        assert_eq!(service.client.port, "status");
        assert_eq!(service.server.instance.name, "display");
        assert_eq!(service.server.port, "status");

        // seam 4: iox2 profile + variable frame (bytes) → auto-zenoh fallback
        let bind = &ir.graphs[0].binds[0];
        assert_eq!(bind.backend.0, "zenoh");
        assert_eq!(bind.backend_source, ChannelBackendSource::AutoFallback);

        // profile projection preserves the fallback
        let projected = project_contract_to_profile(&ir, None).unwrap();
        let projected_bind = &projected.graphs[0].binds[0];
        assert_eq!(projected_bind.backend.0, "zenoh");
        assert_eq!(
            projected_bind.backend_source,
            ChannelBackendSource::AutoFallback
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn service_bind_with_explicit_policy_fields() {
        let source = r#"
[package]
name = "service_policy_demo"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "zenoh"
timeout_ms = 1000
queue_depth = 16
overflow = "error"
lane = "rpc_lane"
max_in_flight = 8
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let service = &ir.graphs[0].services[0];

        assert_eq!(service.backend.0, "zenoh");
        assert_eq!(service.backend_source, ServiceBackendSource::Explicit);
        assert_eq!(service.policy.timeout_ms, 1000);
        assert_eq!(service.policy.queue_depth, 16);
        assert_eq!(service.policy.overflow, ServiceOverflowPolicy::Error);
        assert_eq!(service.policy.lane.as_deref(), Some("rpc_lane"));
        assert_eq!(service.policy.max_in_flight, 8);
        assert_eq!(service.policy_source.backend, PolicyValueSource::Explicit);
        assert_eq!(
            service.policy_source.timeout_ms,
            PolicyValueSource::Explicit
        );
    }

    #[test]
    fn service_auto_backend_resolves_to_zenoh_for_cross_process() {
        let source = r#"
[package]
name = "service_cross_process"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"
process = "proc_a"

[instance.server]
component = "server"
process = "proc_b"

[[bind.service]]
client = "client.plan"
server = "server.plan"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let service = &ir.graphs[0].services[0];

        assert_eq!(service.backend.0, "zenoh");
        assert_eq!(service.backend_source, ServiceBackendSource::AutoResolved);
    }

    #[test]
    fn service_explicit_inproc_same_process() {
        let source = r#"
[package]
name = "service_inproc"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"
process = "main"

[instance.server]
component = "server"
process = "main"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "inproc"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let service = &ir.graphs[0].services[0];

        assert_eq!(service.backend.0, "inproc");
        assert_eq!(service.backend_source, ServiceBackendSource::Explicit);
    }

    #[test]
    fn rejects_service_bind_with_iox2_backend() {
        let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "iox2"
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("iox2 service backend should fail");

        assert!(matches!(
            error,
            IrError::InvalidEnum {
                kind: "service backend",
                value,
                ..
            } if value == "iox2"
        ));
    }

    #[test]
    fn rejects_service_bind_with_unknown_backend() {
        let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "grpc"
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("unknown service backend should fail");

        assert!(matches!(
            error,
            IrError::InvalidEnum {
                kind: "service backend",
                value,
                ..
            } if value == "grpc"
        ));
    }

    #[test]
    fn rejects_explicit_inproc_across_processes() {
        let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"
process = "proc_a"

[instance.server]
component = "server"
process = "proc_b"

[[bind.service]]
client = "client.plan"
server = "server.plan"
backend = "inproc"
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("cross-process inproc should fail");

        assert!(matches!(error, IrError::InvalidValue { .. }));
    }

    #[test]
    fn rejects_service_zero_timeout() {
        let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
timeout_ms = 0
"#;
        let raw = parse_str(source).unwrap();
        let error =
            normalize_document(&raw, hash_source(source)).expect_err("zero timeout should fail");

        assert!(matches!(error, IrError::InvalidValue { .. }));
    }

    #[test]
    fn rejects_service_zero_queue_depth() {
        let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
queue_depth = 0
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("zero queue_depth should fail");

        assert!(matches!(error, IrError::InvalidValue { .. }));
    }

    #[test]
    fn rejects_service_zero_max_in_flight() {
        let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
max_in_flight = 0
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("zero max_in_flight should fail");

        assert!(matches!(error, IrError::InvalidValue { .. }));
    }

    #[test]
    fn rejects_service_unknown_overflow_policy() {
        let source = r#"
[package]
name = "bad_service"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"

[instance.server]
component = "server"

[[bind.service]]
client = "client.plan"
server = "server.plan"
overflow = "drop_oldest"
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("unknown overflow policy should fail");

        assert!(matches!(
            error,
            IrError::InvalidEnum {
                kind: "service overflow policy",
                value,
                ..
            } if value == "drop_oldest"
        ));
    }

    fn unique_temp_dir() -> std::path::PathBuf {
        let suffix = format!(
            "flowrt-ir-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(suffix)
    }
}
