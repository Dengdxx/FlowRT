use flowrt_ir::{LanguageKind, TriggerKind};

use super::*;

const SCHEDULER_PLAN_RSDL: &str = r#"
[package]
name = "scheduler_plan_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[type.PlanRequest]
goal = "u32"

[type.PlanResponse]
accepted = "bool"

[type.PlanGoal]
target = "u32"

[type.PlanFeedback]
progress = "f32"

[type.PlanResult]
accepted = "bool"

[component.source]
language = "rust"
concurrency = "parallel"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[component.plan_service]
language = "rust"
service_server = ["plan:PlanRequest->PlanResponse"]

[component.planner]
language = "rust"
service_client = ["plan:PlanRequest->PlanResponse"]

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

[instance.source]
component = "source"

[[instance.source.task]]
name = "publish"
trigger = "periodic"
period_ms = 10
deadline_ms = 4
priority = 7
lane = "sensor_lane"
concurrency = "parallel"
output = ["sample"]

[instance.sink]
component = "sink"

[[instance.sink.task]]
name = "consume"
trigger = "on_message"
deadline_ms = 6
input = ["sample"]

[instance.plan_svc]
component = "plan_service"

[[instance.plan_svc.task]]
name = "housekeep"
trigger = "periodic"
period_ms = 50

[instance.planner]
component = "planner"

[[instance.planner.task]]
name = "main"
trigger = "periodic"
period_ms = 20

[instance.controller]
component = "controller"

[[instance.controller.task]]
name = "main"
trigger = "periodic"
period_ms = 100

[instance.navigator]
component = "navigator"

[[instance.navigator.task]]
name = "main"
trigger = "periodic"
period_ms = 100

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"

[[bind.service]]
client = "planner.plan"
server = "plan_svc.plan"
backend = "inproc"
timeout_ms = 1000
queue_depth = 16
overflow = "busy"
lane = "service_lane"

[[bind.operation]]
client = "controller.plan"
server = "navigator.plan"
backend = "inproc"
timeout_ms = 5000
queue_depth = 4
max_in_flight = 1
concurrency = "reject"
preempt = "reject"
feedback = "latest"
result_retention_ms = 60000

[profile.default]
backend = "inproc"
worker_threads = 3
"#;

const FAULT_PLAN_RSDL: &str = r#"
[package]
name = "fault_plan_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.worker]
language = "rust"
output = ["sample:Sample"]

[instance.plain]
component = "worker"

[instance.plain.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]

[instance.resilient]
component = "worker"

[instance.resilient.fault]
policy = "restart"
max_restarts = 2
initial_delay_ms = 10
max_delay_ms = 40

[instance.resilient.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;

#[test]
fn recoverable_instances_collects_only_isolate_restart_with_task_ids() {
    let contract = contract_from_source(FAULT_PLAN_RSDL);
    let graph = contract.graphs.first().unwrap();
    let order = topo_order_instances_for_language(&contract, graph, LanguageKind::Rust);

    let recoverable = crate::runtime_plan::recoverable_instances(&contract, graph, &order);

    assert_eq!(recoverable.len(), 1, "{recoverable:?}");
    let plan = &recoverable[0];
    assert_eq!(plan.name, "resilient");
    assert_eq!(plan.policy, flowrt_ir::InstanceFailurePolicy::Restart);
    let restart = plan.restart.expect("restart params");
    assert_eq!(restart.max_restarts, 2);
    assert_eq!(restart.initial_delay_ms, 10);
    assert_eq!(restart.max_delay_ms, 40);
    assert_eq!(plan.task_ids.len(), 1, "resilient has one dataflow task");
}

#[test]
fn scheduler_runtime_plan_collects_dataflow_and_hidden_tasks() {
    let contract = contract_from_source(SCHEDULER_PLAN_RSDL);
    let graph = contract.graphs.first().unwrap();
    let order = topo_order_instances_for_language(&contract, graph, LanguageKind::Rust);

    let plan = crate::runtime_plan::scheduler_runtime_plan(&contract, graph, &order);

    assert_eq!(plan.worker_threads, 3);
    assert_eq!(plan.scheduler_base_period_ms, 10);

    let lane_names = plan
        .lanes
        .iter()
        .map(|lane| lane.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        lane_names,
        vec![
            "controller_serial",
            "navigator_serial",
            "plan_svc_serial",
            "planner_serial",
            "sink_serial",
            "sensor_lane",
            "service_lane",
            "navigator_operation_serial",
        ]
    );

    let dataflow = plan
        .dataflow_tasks
        .iter()
        .map(|task| {
            (
                task.id,
                task.timing_name.as_str(),
                task.task.name.as_str(),
                task.lane.as_str(),
                task.priority,
                task.deadline_ms,
                task.period_ms,
                task.periodic_wake,
                task.trigger,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        dataflow,
        vec![
            (
                1,
                "controller.main",
                "main",
                "controller_serial",
                0,
                None,
                Some(100),
                true,
                TriggerKind::Periodic,
            ),
            (
                2,
                "navigator.main",
                "main",
                "navigator_serial",
                0,
                None,
                Some(100),
                true,
                TriggerKind::Periodic,
            ),
            (
                3,
                "plan_svc.housekeep",
                "housekeep",
                "plan_svc_serial",
                0,
                None,
                Some(50),
                true,
                TriggerKind::Periodic,
            ),
            (
                4,
                "planner.main",
                "main",
                "planner_serial",
                0,
                None,
                Some(20),
                true,
                TriggerKind::Periodic,
            ),
            (
                5,
                "sink.consume",
                "consume",
                "sink_serial",
                0,
                Some(6),
                None,
                false,
                TriggerKind::OnMessage,
            ),
            (
                6,
                "source.publish",
                "publish",
                "sensor_lane",
                7,
                Some(4),
                Some(10),
                true,
                TriggerKind::Periodic,
            ),
        ]
    );

    let hidden = plan
        .hidden_tasks
        .iter()
        .map(|task| (task.id, task.kind, task.name.as_str(), task.lane.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        hidden,
        vec![
            (
                7,
                crate::runtime_plan::SchedulerHiddenTaskKind::Service,
                "__flowrt_service.planner.plan",
                "service_lane",
            ),
            (
                8,
                crate::runtime_plan::SchedulerHiddenTaskKind::Operation,
                "__flowrt_operation.controller.plan",
                "navigator_operation_serial",
            ),
        ]
    );
}
