use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{ContractIr, GraphIr, InstanceIr, TaskIr};

use crate::runtime_plan::bridge_runtime_plans;
use crate::{
    Result, component_by_name, iox2_service_name_for_edge, language_name, ros2_bridge_key_expr,
    zenoh_key_expr_for_edge,
};

pub(super) fn emit_launch_manifest(contract: &ContractIr) -> Result<String> {
    let launch = serde_json::json!({
        "package": contract.package.name,
        "ir_version": contract.ir_version,
        "profiles": contract.profiles.iter().map(|profile| &profile.name).collect::<Vec<_>>(),
        "targets": contract.targets.iter().map(|target| &target.name).collect::<Vec<_>>(),
        "graphs": contract.graphs.iter().map(|graph| serde_json::json!({
            "name": graph.name,
            "scheduler": launch_scheduler(contract, graph),
            "processes": launch_processes(contract, graph),
            "channels": launch_channels(contract, graph),
            "ros2_bridges": launch_ros2_bridges(contract, graph),
            "instances": graph.instances.iter().map(|instance| {
                let component = component_by_name(contract, &instance.component.name);
                serde_json::json!({
                    "name": instance.name,
                    "component": instance.component.name,
                    "runtime": language_name(component.language),
                    "process": instance.process,
                    "target": instance.target.as_ref().map(|target| &target.name),
                })
            }).collect::<Vec<_>>(),
            "tasks": graph.tasks.iter().map(launch_task).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    let mut output = serde_json::to_string_pretty(&launch)?;
    output.push('\n');
    Ok(output)
}

fn launch_scheduler(contract: &ContractIr, graph: &GraphIr) -> serde_json::Value {
    serde_json::json!({
        "worker_threads": contract
            .profiles
            .first()
            .map(|profile| profile.scheduler.worker_threads)
            .unwrap_or(1),
        "lanes": scheduler_lanes(graph),
        "tasks": graph.tasks.iter().map(scheduler_task).collect::<Vec<_>>(),
    })
}

fn scheduler_lanes(graph: &GraphIr) -> Vec<serde_json::Value> {
    let mut lanes = BTreeMap::<String, String>::new();
    for task in &graph.tasks {
        lanes.insert(task_lane_name(task), task.instance.name.clone());
    }

    lanes
        .into_iter()
        .map(|(name, instance)| {
            serde_json::json!({
                "name": name,
                "kind": "serial",
                "instance": instance,
            })
        })
        .collect()
}

fn scheduler_task(task: &TaskIr) -> serde_json::Value {
    serde_json::json!({
        "name": task.name,
        "instance": task.instance.name,
        "lane": task_lane_name(task),
        "trigger": task.trigger,
        "readiness": task.readiness,
        "period_ms": task.period_ms,
        "deadline_ms": task.deadline_ms,
        "priority": task.priority,
    })
}

fn task_lane_name(task: &TaskIr) -> String {
    task.lane
        .clone()
        .unwrap_or_else(|| format!("{}_serial", task.instance.name))
}

fn launch_ros2_bridges(contract: &ContractIr, graph: &GraphIr) -> Vec<serde_json::Value> {
    bridge_runtime_plans(contract, graph)
        .iter()
        .map(|bridge| {
            serde_json::json!({
                "name": bridge.name,
                "flowrt": format!("{}.{}", bridge.source_instance, bridge.source_port),
                "ros2_topic": bridge.ros2_topic,
                "ros2_type": bridge.ros2_type,
                "field": bridge.field,
                "backend": "zenoh",
                "key_expr": ros2_bridge_key_expr(contract, graph, bridge),
            })
        })
        .collect()
}

fn launch_channels(contract: &ContractIr, graph: &GraphIr) -> Vec<serde_json::Value> {
    graph
        .binds
        .iter()
        .enumerate()
        .map(|(index, bind)| {
            let backend = bind.backend.0.as_str();
            let service = (backend == "iox2")
                .then(|| iox2_service_name_for_edge(contract, graph, index, bind));
            let key_expr =
                (backend == "zenoh").then(|| zenoh_key_expr_for_edge(contract, graph, index, bind));
            serde_json::json!({
                "from": format!("{}.{}", bind.from.instance.name, bind.from.port),
                "to": format!("{}.{}", bind.to.instance.name, bind.to.port),
                "backend": backend,
                "service": service,
                "key_expr": key_expr,
                "channel": bind.channel,
                "depth": bind.depth,
                "overflow": bind.overflow,
                "stale_policy": bind.stale,
                "max_age_ms": bind.max_age_ms,
            })
        })
        .collect()
}

fn launch_processes(contract: &ContractIr, graph: &GraphIr) -> Vec<serde_json::Value> {
    let mut processes = BTreeMap::<String, Vec<&InstanceIr>>::new();
    for instance in &graph.instances {
        processes
            .entry(
                instance
                    .process
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
            )
            .or_default()
            .push(instance);
    }

    let mut launch_processes = processes
        .into_iter()
        .map(|(name, instances)| {
            let instance_names = instances
                .iter()
                .map(|instance| instance.name.as_str())
                .collect::<BTreeSet<_>>();
            let runtimes = process_runtimes(contract, &instances);
            let target = common_process_target(&instances);
            serde_json::json!({
                "name": name,
                "backend": process_backend(graph, &instance_names),
                "target": target,
                "runtimes": runtimes,
                "runtime_kind": process_runtime_kind(&runtimes),
                "instances": instances.iter().map(|instance| &instance.name).collect::<Vec<_>>(),
                "tasks": graph.tasks.iter().filter(|task| instance_names.contains(task.instance.name.as_str())).map(launch_task).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    if !graph.ros2_bridges.is_empty() {
        launch_processes.push(serde_json::json!({
            "name": "ros2_bridge",
            "backend": "zenoh",
            "target": null,
            "runtimes": ["ros2_bridge"],
            "runtime_kind": "ros2_bridge",
            "instances": [],
            "tasks": [],
        }));
    }

    launch_processes
}

fn process_backend(graph: &GraphIr, instance_names: &BTreeSet<&str>) -> String {
    if graph.ros2_bridges.iter().any(|bridge| {
        bridge.backend.0 == "zenoh" && instance_names.contains(bridge.flowrt.instance.name.as_str())
    }) {
        return "zenoh".to_string();
    }
    if graph.binds.iter().any(|bind| {
        bind.backend.0 == "zenoh"
            && (instance_names.contains(bind.from.instance.name.as_str())
                || instance_names.contains(bind.to.instance.name.as_str()))
    }) {
        return "zenoh".to_string();
    }
    if graph.binds.iter().any(|bind| {
        bind.backend.0 == "iox2"
            && (instance_names.contains(bind.from.instance.name.as_str())
                || instance_names.contains(bind.to.instance.name.as_str()))
    }) {
        return "iox2".to_string();
    }
    "inproc".to_string()
}

fn launch_task(task: &TaskIr) -> serde_json::Value {
    serde_json::json!({
        "name": task.name,
        "instance": task.instance.name,
        "trigger": task.trigger,
        "readiness": task.readiness,
        "period_ms": task.period_ms,
        "deadline_ms": task.deadline_ms,
        "lane": task_lane_name(task),
        "priority": task.priority,
        "inputs": task.inputs,
        "outputs": task.outputs,
    })
}

fn process_runtimes(contract: &ContractIr, instances: &[&InstanceIr]) -> Vec<&'static str> {
    instances
        .iter()
        .map(|instance| component_by_name(contract, &instance.component.name))
        .map(|component| language_name(component.language))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn process_runtime_kind(runtimes: &[&'static str]) -> &'static str {
    if runtimes.len() == 1 {
        runtimes[0]
    } else {
        "mixed"
    }
}

fn common_process_target(instances: &[&InstanceIr]) -> Option<String> {
    let mut targets = instances
        .iter()
        .filter_map(|instance| instance.target.as_ref().map(|target| target.name.clone()))
        .collect::<BTreeSet<_>>();

    if targets.len() == 1 {
        targets.pop_first()
    } else {
        None
    }
}
