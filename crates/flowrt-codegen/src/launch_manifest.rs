use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{ContractIr, GraphIr, InstanceIr, TaskIr};

use crate::{
    Result, component_by_name, iox2_service_name_for_edge, language_name, selected_backend_name,
    zenoh_key_expr_for_edge,
};

pub(super) fn emit_launch_manifest(contract: &ContractIr) -> Result<String> {
    let selected_backend = selected_backend_name(contract);
    let launch = serde_json::json!({
        "package": contract.package.name,
        "ir_version": contract.ir_version,
        "profiles": contract.profiles.iter().map(|profile| &profile.name).collect::<Vec<_>>(),
        "targets": contract.targets.iter().map(|target| &target.name).collect::<Vec<_>>(),
        "graphs": contract.graphs.iter().map(|graph| serde_json::json!({
            "name": graph.name,
            "processes": launch_processes(contract, graph, &selected_backend),
            "channels": launch_channels(contract, graph, &selected_backend),
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

fn launch_channels(
    contract: &ContractIr,
    graph: &GraphIr,
    backend: &str,
) -> Vec<serde_json::Value> {
    graph
        .binds
        .iter()
        .enumerate()
        .map(|(index, bind)| {
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

fn launch_processes(
    contract: &ContractIr,
    graph: &GraphIr,
    backend: &str,
) -> Vec<serde_json::Value> {
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

    processes
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
                "backend": backend,
                "target": target,
                "runtimes": runtimes,
                "runtime_kind": process_runtime_kind(&runtimes),
                "instances": instances.iter().map(|instance| &instance.name).collect::<Vec<_>>(),
                "tasks": graph.tasks.iter().filter(|task| instance_names.contains(task.instance.name.as_str())).map(launch_task).collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn launch_task(task: &TaskIr) -> serde_json::Value {
    serde_json::json!({
        "instance": task.instance.name,
        "trigger": task.trigger,
        "period_ms": task.period_ms,
        "deadline_ms": task.deadline_ms,
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
