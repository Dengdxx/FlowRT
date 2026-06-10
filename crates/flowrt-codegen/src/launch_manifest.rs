use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    ComponentIr, ComponentKind, ContractIr, ExternalHealthKind, ExternalProcessIr,
    ExternalWorkingDir, GraphIr, InstanceIr, IoBoundaryHealth, IoBoundaryReadiness,
    IoBoundaryShutdown, IoSideEffect, ProcessIr, ResourceKind, ServicePortIr, TaskIr,
};

use crate::runtime_plan::bridge_runtime_plans;
use crate::{
    CodegenError, Result, component_by_name, iox2_service_name_for_edge, language_name,
    ros2_bridge_key_expr, zenoh_key_expr_for_edge,
};

pub(super) fn emit_launch_manifest(contract: &ContractIr) -> Result<String> {
    let graphs = contract
        .graphs
        .iter()
        .map(|graph| {
            Ok(serde_json::json!({
                "name": graph.name,
                "scheduler": launch_scheduler(contract, graph),
                "processes": launch_processes(contract, graph),
                "channels": launch_channels(contract, graph),
                "services": launch_services(contract, graph)?,
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
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let launch = serde_json::json!({
        "package": contract.package.name,
        "ir_version": contract.ir_version,
        "profiles": contract.profiles.iter().map(|profile| &profile.name).collect::<Vec<_>>(),
        "targets": contract.targets.iter().map(|target| &target.name).collect::<Vec<_>>(),
        "graphs": graphs,
    });
    let mut output = serde_json::to_string_pretty(&launch)?;
    output.push('\n');
    Ok(output)
}

fn launch_services(contract: &ContractIr, graph: &GraphIr) -> Result<Vec<serde_json::Value>> {
    let components = contract
        .components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    let instances = graph
        .instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();

    graph
        .services
        .iter()
        .map(|service| {
            let client = service_port_for_instance(
                &components,
                &instances,
                service.client.instance.name.as_str(),
                service.client.port.as_str(),
                ServicePortRole::Client,
            );
            let server = service_port_for_instance(
                &components,
                &instances,
                service.server.instance.name.as_str(),
                service.server.port.as_str(),
                ServicePortRole::Server,
            );
            if client.request != server.request {
                return Err(CodegenError::InvalidLaunchManifest {
                    message: format!(
                        "service `{}` request type mismatch: client `{}` uses `{}`, server `{}` uses `{}`",
                        service.id.0,
                        service.client.port,
                        client.request.canonical_syntax(),
                        service.server.port,
                        server.request.canonical_syntax()
                    ),
                });
            }
            if client.response != server.response {
                return Err(CodegenError::InvalidLaunchManifest {
                    message: format!(
                        "service `{}` response type mismatch: client `{}` uses `{}`, server `{}` uses `{}`",
                        service.id.0,
                        service.client.port,
                        client.response.canonical_syntax(),
                        service.server.port,
                        server.response.canonical_syntax()
                    ),
                });
            }
            let name = format!("{}.{}", service.client.instance.name, service.client.port);
            Ok(serde_json::json!({
                "name": name,
                "client": format!("{}.{}", service.client.instance.name, service.client.port),
                "client_instance": service.client.instance.name,
                "client_port": service.client.port,
                "server": format!("{}.{}", service.server.instance.name, service.server.port),
                "server_instance": service.server.instance.name,
                "server_port": service.server.port,
                "request": client.request.canonical_syntax(),
                "response": client.response.canonical_syntax(),
                "backend": service.backend.0,
                "timeout_ms": service.policy.timeout_ms,
                "queue_depth": service.policy.queue_depth,
                "overflow": service.policy.overflow,
                "lane": service.policy.lane,
                "max_in_flight": service.policy.max_in_flight,
            }))
        })
        .collect()
}

fn service_port_for_instance<'a>(
    components: &BTreeMap<&str, &'a ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    instance_name: &str,
    port_name: &str,
    role: ServicePortRole,
) -> &'a ServicePortIr {
    let instance = instances
        .get(instance_name)
        .expect("validated service bind must reference an existing instance");
    let component = components
        .get(instance.component.name.as_str())
        .expect("validated service bind must reference an existing component");
    let ports = match role {
        ServicePortRole::Client => &component.service_clients,
        ServicePortRole::Server => &component.service_servers,
    };
    ports
        .iter()
        .find(|port| port.name == port_name)
        .expect("validated service bind must reference an existing service port")
}

#[derive(Debug, Clone, Copy)]
enum ServicePortRole {
    Client,
    Server,
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
    let external_processes = graph
        .external_processes
        .iter()
        .map(|external| (external.process.as_str(), external))
        .collect::<BTreeMap<_, _>>();
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
            let orchestration = process_orchestration(graph, &name);
            let external = external_processes
                .get(name.as_str())
                .map(|external| launch_external_process(external));
            let backend = process_backend(
                graph,
                &instance_names,
                external_processes.get(name.as_str()).copied(),
            );
            serde_json::json!({
                "name": name,
                "backend": backend,
                "target": target,
                "runtimes": runtimes,
                "runtime_kind": process_runtime_kind(&runtimes),
                "external": external,
                "depends_on": orchestration.depends_on,
                "restart": {
                    "policy": orchestration.restart.policy,
                    "max_restarts": orchestration.restart.max_restarts,
                    "initial_delay_ms": orchestration.restart.initial_delay_ms,
                    "max_delay_ms": orchestration.restart.max_delay_ms,
                },
                "failure": orchestration.failure_propagation,
                "readiness": orchestration.readiness,
                "startup_delay_ms": orchestration.startup_delay_ms,
                "env": orchestration.env,
                "resource_placement": {
                    "cpu_affinity": orchestration.cpu_affinity,
                    "nice": orchestration.nice,
                    "rt_policy": orchestration.rt_policy,
                    "rt_priority": orchestration.rt_priority,
                },
                "io_boundaries": launch_io_boundaries(contract, &instances),
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
        "external": null,
        "depends_on": [],
            "restart": {
                "policy": "on_failure",
                "max_restarts": 3,
                "initial_delay_ms": 100,
                "max_delay_ms": 1000,
            },
            "failure": "propagate",
            "readiness": "process_started",
            "startup_delay_ms": 0,
            "env": {},
            "resource_placement": {
                "cpu_affinity": [],
                "nice": null,
                "rt_policy": null,
                "rt_priority": null,
            },
            "instances": [],
            "tasks": [],
        }));
    }

    launch_processes
}

fn launch_io_boundaries(
    contract: &ContractIr,
    instances: &[&InstanceIr],
) -> Vec<serde_json::Value> {
    instances
        .iter()
        .filter_map(|instance| {
            let component = component_by_name(contract, &instance.component.name);
            if component.kind != ComponentKind::IoBoundary {
                return None;
            }
            let policy = component.io_boundary.as_ref()?;
            Some(serde_json::json!({
                "instance": instance.name,
                "component": component.name,
                "side_effects": policy
                    .side_effects
                    .iter()
                    .map(|effect| io_side_effect_name(*effect))
                    .collect::<Vec<_>>(),
                "readiness": io_readiness_name(policy.readiness),
                "health": io_health_name(policy.health),
                "shutdown": io_shutdown_name(policy.shutdown),
                "resources": component.resources.iter().map(|resource| {
                    serde_json::json!({
                        "name": resource.name,
                        "kind": resource_kind_name(resource.kind),
                        "required": resource.required,
                    })
                }).collect::<Vec<_>>(),
            }))
        })
        .collect()
}

fn launch_external_process(external: &ExternalProcessIr) -> serde_json::Value {
    serde_json::json!({
        "package": &external.package,
        "executable": &external.executable,
        "args": &external.args,
        "working_dir": external_working_dir_name(external.working_dir),
        "health": external_health_name(external.health),
        "required_backends": external
            .required_backends
            .iter()
            .map(|backend| backend.0.as_str())
            .collect::<Vec<_>>(),
    })
}

fn external_working_dir_name(kind: ExternalWorkingDir) -> &'static str {
    match kind {
        ExternalWorkingDir::Package => "package",
        ExternalWorkingDir::Workspace => "workspace",
    }
}

fn external_health_name(kind: ExternalHealthKind) -> &'static str {
    match kind {
        ExternalHealthKind::ProcessStarted => "process_started",
        ExternalHealthKind::RuntimeSocket => "runtime_socket",
    }
}

fn resource_kind_name(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Serial => "serial",
        ResourceKind::Shm => "shm",
        ResourceKind::Udp => "udp",
        ResourceKind::File => "file",
        ResourceKind::Device => "device",
        ResourceKind::Sdk => "sdk",
    }
}

fn io_side_effect_name(kind: IoSideEffect) -> &'static str {
    match kind {
        IoSideEffect::Read => "read",
        IoSideEffect::Write => "write",
        IoSideEffect::Network => "network",
        IoSideEffect::Filesystem => "filesystem",
        IoSideEffect::Device => "device",
        IoSideEffect::Compute => "compute",
    }
}

fn io_readiness_name(kind: IoBoundaryReadiness) -> &'static str {
    match kind {
        IoBoundaryReadiness::ComponentStarted => "component_started",
        IoBoundaryReadiness::ResourceReady => "resource_ready",
    }
}

fn io_health_name(kind: IoBoundaryHealth) -> &'static str {
    match kind {
        IoBoundaryHealth::RuntimeReported => "runtime_reported",
        IoBoundaryHealth::ProcessStatus => "process_status",
    }
}

fn io_shutdown_name(kind: IoBoundaryShutdown) -> &'static str {
    match kind {
        IoBoundaryShutdown::Cooperative => "cooperative",
        IoBoundaryShutdown::BestEffort => "best_effort",
    }
}

fn process_orchestration<'a>(graph: &'a GraphIr, process_name: &str) -> &'a ProcessIr {
    graph
        .processes
        .iter()
        .find(|process| process.name == process_name)
        .expect("normalized graph must contain process orchestration for every process")
}

fn process_backend(
    graph: &GraphIr,
    instance_names: &BTreeSet<&str>,
    external: Option<&ExternalProcessIr>,
) -> String {
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
    if graph.services.iter().any(|service| {
        service.backend.0 == "zenoh"
            && (instance_names.contains(service.client.instance.name.as_str())
                || instance_names.contains(service.server.instance.name.as_str()))
    }) {
        return "zenoh".to_string();
    }
    if graph.operations.iter().any(|operation| {
        operation.backend.0 == "zenoh"
            && (instance_names.contains(operation.client.instance.name.as_str())
                || instance_names.contains(operation.server.instance.name.as_str()))
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
    if let Some(external) = external {
        if external
            .required_backends
            .iter()
            .any(|backend| backend.0 == "zenoh")
        {
            return "zenoh".to_string();
        }
        if let Some(backend) = external.required_backends.first() {
            return backend.0.clone();
        }
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
