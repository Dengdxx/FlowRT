use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use flowrt_ir::{
    BackendName, BoundaryDirection, ChannelKind, ComponentIr, ContractIr, EntityId, GraphIr,
    GraphMode, InstanceIr, LanguageKind, OperationConcurrencyPolicy, OperationPortIr,
    OperationPortRef, OperationPreemptPolicy, PortIr, PortRef, PrimitiveType, ProcessReadinessGate,
    Ros2BridgeDirection, ServicePortIr, ServicePortRef, TaskIr, TaskReadiness, TriggerKind,
    TypeExpr,
};

use crate::ValidationError;
use crate::components::{validate_param_value_constraints, validate_param_value_matches_schema};

pub(crate) fn validate_graphs(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let components = ir
        .components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    let targets = ir
        .targets
        .iter()
        .map(|target| (target.id.clone(), target))
        .collect::<BTreeMap<_, _>>();

    for graph in &ir.graphs {
        let instances = graph
            .instances
            .iter()
            .map(|instance| (instance.name.as_str(), instance))
            .collect::<BTreeMap<_, _>>();

        validate_instance_targets(&components, &targets, graph, errors);
        validate_process_targets(graph, errors);
        validate_process_orchestration(graph, errors);
        validate_external_processes(&components, graph, errors);
        validate_boundary_mode(ir, graph, errors);
        validate_boundary_endpoints(&components, &instances, graph, errors);
        validate_tasks(ir, &components, &instances, graph, errors);
        validate_instance_params(&components, &instances, graph, errors);
        validate_binds(&components, &instances, graph, errors);
        validate_service_binds(&components, &instances, graph, errors);
        validate_operation_binds(&components, &instances, graph, errors);
        validate_ros2_bridges(ir, &components, &instances, graph, errors);
        validate_graph_is_acyclic(&instances, graph, errors);
    }
}

fn validate_external_processes(
    components: &BTreeMap<&str, &ComponentIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let external_processes = graph
        .external_processes
        .iter()
        .map(|external| (external.process.as_str(), external))
        .collect::<BTreeMap<_, _>>();
    let declared_processes = graph
        .processes
        .iter()
        .map(|process| process.name.as_str())
        .collect::<BTreeSet<_>>();

    for external in &graph.external_processes {
        if !declared_processes.contains(external.process.as_str()) {
            errors.push(ValidationError::new(format!(
                "external_process `{}` references unknown process",
                external.process
            )));
        }
        validate_external_executable_path(&external.process, &external.executable, errors);
        for backend in &external.required_backends {
            if !flowrt_ir::is_known_backend(&backend.0) {
                errors.push(ValidationError::new(format!(
                    "external_process `{}` requires unknown backend `{}`",
                    external.process, backend.0
                )));
            }
        }
    }

    let mut external_instance_count_by_process = BTreeMap::<&str, usize>::new();
    for instance in &graph.instances {
        let process = instance.process.as_deref().unwrap_or("main");
        let is_external = components
            .get(instance.component.name.as_str())
            .is_some_and(|component| component.language == LanguageKind::External);
        if is_external {
            *external_instance_count_by_process
                .entry(process)
                .or_default() += 1;
            if !external_processes.contains_key(process) {
                errors.push(ValidationError::new(format!(
                    "external instance `{}` uses process `{process}` without external_process metadata",
                    instance.name
                )));
            }
        } else if external_processes.contains_key(process) {
            errors.push(ValidationError::new(format!(
                "native instance `{}` cannot run inside external process `{process}`",
                instance.name
            )));
        }
    }

    for external in &graph.external_processes {
        if external_instance_count_by_process
            .get(external.process.as_str())
            .copied()
            .unwrap_or(0)
            == 0
        {
            errors.push(ValidationError::new(format!(
                "external_process `{}` has no external instance",
                external.process
            )));
        }
    }
}

fn validate_external_executable_path(
    process: &str,
    executable: &str,
    errors: &mut Vec<ValidationError>,
) {
    let path = Path::new(executable);
    if executable.trim().is_empty() {
        errors.push(ValidationError::new(format!(
            "external_process `{process}` executable must not be empty"
        )));
        return;
    }
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        errors.push(ValidationError::new(format!(
            "external_process `{process}` executable must be a package-relative path without `.` or `..` components"
        )));
    }
}

fn validate_process_orchestration(graph: &GraphIr, errors: &mut Vec<ValidationError>) {
    let declared = graph
        .processes
        .iter()
        .map(|process| process.name.as_str())
        .collect::<BTreeSet<_>>();
    let used = graph
        .instances
        .iter()
        .map(|instance| instance.process.as_deref().unwrap_or("main"))
        .collect::<BTreeSet<_>>();

    for process in &used {
        if !declared.contains(process) {
            errors.push(ValidationError::new(format!(
                "process `{process}` is used by an instance but missing process orchestration"
            )));
        }
    }
    for process in &declared {
        if !used.contains(process) {
            errors.push(ValidationError::new(format!(
                "process `{process}` has orchestration policy but no instance"
            )));
        }
    }

    for process in &graph.processes {
        let mut seen = BTreeSet::new();
        for dependency in &process.depends_on {
            if dependency == &process.name {
                errors.push(ValidationError::new(format!(
                    "process `{}` must not depend on itself",
                    process.name
                )));
            }
            if !declared.contains(dependency.as_str()) {
                errors.push(ValidationError::new(format!(
                    "process `{}` depends on unknown process `{dependency}`",
                    process.name
                )));
            }
            if !seen.insert(dependency.as_str()) {
                errors.push(ValidationError::new(format!(
                    "process `{}` depends on `{dependency}` more than once",
                    process.name
                )));
            }
        }
    }

    if has_process_dependency_cycle(graph) {
        errors.push(ValidationError::new(
            "process dependency graph contains a cycle",
        ));
    }

    validate_process_resource_hints(graph, errors);
}

fn has_process_dependency_cycle(graph: &GraphIr) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum VisitState {
        Visiting,
        Done,
    }

    fn visit<'a>(
        name: &'a str,
        dependencies: &BTreeMap<&'a str, Vec<&'a str>>,
        states: &mut BTreeMap<&'a str, VisitState>,
    ) -> bool {
        match states.get(name).copied() {
            Some(VisitState::Visiting) => return true,
            Some(VisitState::Done) => return false,
            None => {}
        }
        states.insert(name, VisitState::Visiting);
        if let Some(next) = dependencies.get(name) {
            for dependency in next {
                if visit(dependency, dependencies, states) {
                    return true;
                }
            }
        }
        states.insert(name, VisitState::Done);
        false
    }

    let dependencies = graph
        .processes
        .iter()
        .map(|process| {
            (
                process.name.as_str(),
                process
                    .depends_on
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut states = BTreeMap::new();
    dependencies
        .keys()
        .any(|process| visit(process, &dependencies, &mut states))
}

fn validate_process_resource_hints(graph: &GraphIr, errors: &mut Vec<ValidationError>) {
    for process in &graph.processes {
        // readiness gate 必须是已知变体（防御性校验，normalization 已先行拒绝）
        match process.readiness {
            ProcessReadinessGate::ProcessStarted
            | ProcessReadinessGate::RuntimeReady
            | ProcessReadinessGate::ServiceReady => {}
        }

        // nice 范围：-20 到 19
        if let Some(nice) = process.nice {
            if !(-20..=19).contains(&nice) {
                errors.push(ValidationError::new(format!(
                    "process `{}` has invalid nice value {nice}; must be -20..=19",
                    process.name
                )));
            }
        }

        // rt_priority 范围：1-99
        if let Some(rt_priority) = process.rt_priority {
            if rt_priority == 0 || rt_priority > 99 {
                errors.push(ValidationError::new(format!(
                    "process `{}` has invalid rt_priority {rt_priority}; must be 1..=99",
                    process.name
                )));
            }
        }

        // rt_priority 必须搭配 rt_policy
        if process.rt_priority.is_some() && process.rt_policy.is_none() {
            errors.push(ValidationError::new(format!(
                "process `{}` sets rt_priority without rt_policy",
                process.name
            )));
        }

        // cpu_affinity 不允许重复
        let mut seen_cpus = BTreeSet::new();
        for cpu in &process.cpu_affinity {
            if !seen_cpus.insert(cpu) {
                errors.push(ValidationError::new(format!(
                    "process `{}` has duplicate cpu_affinity entry {cpu}",
                    process.name
                )));
            }
        }

        // startup_delay_ms 非负（u64 已保证，无需额外检查）
    }
}

fn validate_service_binds(
    components: &BTreeMap<&str, &ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut clients = BTreeSet::new();
    for service in &graph.services {
        let client_key = format!("{}.{}", service.client.instance.name, service.client.port);
        let server_key = format!("{}.{}", service.server.instance.name, service.server.port);
        if !clients.insert(client_key.clone()) {
            errors.push(ValidationError::new(format!(
                "service client `{client_key}` is bound more than once"
            )));
        }

        // 校验 service backend 合法性：只允许 inproc 和 zenoh。
        if !flowrt_ir::is_known_service_backend(&service.backend.0) {
            errors.push(ValidationError::new(format!(
                "service bind `{client_key} -> {server_key}` uses unsupported backend `{}`; only `inproc` and `zenoh` are allowed",
                service.backend.0
            )));
        }

        // 校验 service policy 字段为正数。
        if service.policy.timeout_ms == 0 {
            errors.push(ValidationError::new(format!(
                "service bind `{client_key} -> {server_key}` has zero timeout_ms"
            )));
        }
        if service.policy.queue_depth == 0 {
            errors.push(ValidationError::new(format!(
                "service bind `{client_key} -> {server_key}` has zero queue_depth"
            )));
        }
        if service.policy.max_in_flight == 0 {
            errors.push(ValidationError::new(format!(
                "service bind `{client_key} -> {server_key}` has zero max_in_flight"
            )));
        }

        if service.backend.0 == "inproc"
            && service_spans_boundaries(instances, &service.client, &service.server)
        {
            errors.push(ValidationError::new(format!(
                "service bind `{client_key} -> {server_key}` uses `inproc` but spans process or target boundaries"
            )));
        }

        let client = match resolve_service_port(
            components,
            instances,
            &service.client,
            ServiceDirection::Client,
        ) {
            Ok(port) => port,
            Err(message) => {
                errors.push(ValidationError::new(message));
                continue;
            }
        };
        let server = match resolve_service_port(
            components,
            instances,
            &service.server,
            ServiceDirection::Server,
        ) {
            Ok(port) => port,
            Err(message) => {
                errors.push(ValidationError::new(message));
                continue;
            }
        };

        if client.request != server.request {
            errors.push(ValidationError::new(format!(
                "service bind `{client_key} -> {server_key}` has mismatched request type: client uses `{}`, server uses `{}`",
                client.request.canonical_syntax(),
                server.request.canonical_syntax()
            )));
        }
        if client.response != server.response {
            errors.push(ValidationError::new(format!(
                "service bind `{client_key} -> {server_key}` has mismatched response type: client uses `{}`, server uses `{}`",
                client.response.canonical_syntax(),
                server.response.canonical_syntax()
            )));
        }
    }
}

fn validate_operation_binds(
    components: &BTreeMap<&str, &ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut clients = BTreeSet::new();
    for operation in &graph.operations {
        let client_key = format!(
            "{}.{}",
            operation.client.instance.name, operation.client.port
        );
        let server_key = format!(
            "{}.{}",
            operation.server.instance.name, operation.server.port
        );
        if !clients.insert(client_key.clone()) {
            errors.push(ValidationError::new(format!(
                "operation client `{client_key}` is bound more than once"
            )));
        }

        if !flowrt_ir::is_known_operation_backend(&operation.backend.0) {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` uses unsupported backend `{}`; only `inproc` and `zenoh` are allowed",
                operation.backend.0
            )));
        }

        if operation.policy.timeout_ms == 0 {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` has zero timeout_ms"
            )));
        }
        if operation.policy.queue_depth == 0 {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` has zero queue_depth"
            )));
        }
        if operation.policy.max_in_flight == 0 {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` has zero max_in_flight"
            )));
        }
        if operation.policy.concurrency == OperationConcurrencyPolicy::Queue {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` uses unsupported concurrency policy `queue`; generated Operation runtime currently supports only `reject`"
            )));
        }
        if operation.policy.preempt == OperationPreemptPolicy::CancelRunning {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` uses unsupported preempt policy `cancel_running`; generated Operation runtime currently supports only `reject`"
            )));
        }
        if operation.policy.max_in_flight != 1 {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` uses unsupported max_in_flight `{}`; generated Operation runtime currently supports only 1",
                operation.policy.max_in_flight
            )));
        }

        if operation.backend.0 == "inproc"
            && operation_spans_boundaries(instances, &operation.client, &operation.server)
        {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` uses `inproc` but spans process or target boundaries"
            )));
        }

        let client = match resolve_operation_port(
            components,
            instances,
            &operation.client,
            OperationDirection::Client,
        ) {
            Ok(port) => port,
            Err(message) => {
                errors.push(ValidationError::new(message));
                continue;
            }
        };
        let server = match resolve_operation_port(
            components,
            instances,
            &operation.server,
            OperationDirection::Server,
        ) {
            Ok(port) => port,
            Err(message) => {
                errors.push(ValidationError::new(message));
                continue;
            }
        };

        if client.goal != server.goal {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` has mismatched goal type: client uses `{}`, server uses `{}`",
                client.goal.canonical_syntax(),
                server.goal.canonical_syntax()
            )));
        }
        if client.feedback != server.feedback {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` has mismatched feedback type: client uses `{}`, server uses `{}`",
                client.feedback.canonical_syntax(),
                server.feedback.canonical_syntax()
            )));
        }
        if client.result != server.result {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` has mismatched result type: client uses `{}`, server uses `{}`",
                client.result.canonical_syntax(),
                server.result.canonical_syntax()
            )));
        }
    }
}

fn service_spans_boundaries(
    instances: &BTreeMap<&str, &InstanceIr>,
    client: &ServicePortRef,
    server: &ServicePortRef,
) -> bool {
    let client_instance = instances.get(client.instance.name.as_str());
    let server_instance = instances.get(server.instance.name.as_str());
    let client_process = client_instance
        .and_then(|i| i.process.as_deref())
        .unwrap_or("main");
    let server_process = server_instance
        .and_then(|i| i.process.as_deref())
        .unwrap_or("main");
    let client_target = client_instance
        .and_then(|i| i.target.as_ref())
        .map(|target| target.name.as_str());
    let server_target = server_instance
        .and_then(|i| i.target.as_ref())
        .map(|target| target.name.as_str());
    client_process != server_process
        || (client_target.is_some() && server_target.is_some() && client_target != server_target)
}

fn operation_spans_boundaries(
    instances: &BTreeMap<&str, &InstanceIr>,
    client: &OperationPortRef,
    server: &OperationPortRef,
) -> bool {
    let client_instance = instances.get(client.instance.name.as_str());
    let server_instance = instances.get(server.instance.name.as_str());
    let client_process = client_instance
        .and_then(|i| i.process.as_deref())
        .unwrap_or("main");
    let server_process = server_instance
        .and_then(|i| i.process.as_deref())
        .unwrap_or("main");
    let client_target = client_instance
        .and_then(|i| i.target.as_ref())
        .map(|target| target.name.as_str());
    let server_target = server_instance
        .and_then(|i| i.target.as_ref())
        .map(|target| target.name.as_str());
    client_process != server_process
        || (client_target.is_some() && server_target.is_some() && client_target != server_target)
}

fn validate_instance_targets(
    components: &BTreeMap<&str, &ComponentIr>,
    targets: &BTreeMap<EntityId, &flowrt_ir::TargetIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    for instance in &graph.instances {
        let Some(component) = components.get(instance.component.name.as_str()) else {
            errors.push(ValidationError::new(format!(
                "instance `{}` references unknown component `{}`",
                instance.name, instance.component.name
            )));
            continue;
        };

        let Some(target) = &instance.target else {
            continue;
        };
        let Some(target) = targets.get(&target.id) else {
            errors.push(ValidationError::new(format!(
                "instance `{}` references unknown target `{}`",
                instance.name, target.name
            )));
            continue;
        };
        if !target.runtime.contains(&component.language) {
            errors.push(ValidationError::new(format!(
                "target `{}` does not support {:?} runtime required by instance `{}`",
                target.name, component.language, instance.name
            )));
        }
    }
}

fn validate_process_targets(graph: &GraphIr, errors: &mut Vec<ValidationError>) {
    let mut process_targets = BTreeMap::<String, BTreeSet<String>>::new();

    for instance in &graph.instances {
        let process = instance.process.as_deref().unwrap_or("main");
        let target = instance
            .target
            .as_ref()
            .map(|target| target.name.as_str())
            .unwrap_or("default");
        process_targets
            .entry(process.to_string())
            .or_default()
            .insert(target.to_string());
    }

    for (process, targets) in process_targets {
        if targets.len() > 1 {
            errors.push(ValidationError::new(format!(
                "process `{}` spans multiple targets: {}",
                process,
                targets.into_iter().collect::<Vec<_>>().join(", ")
            )));
        }
    }
}

fn validate_boundary_mode(ir: &ContractIr, graph: &GraphIr, errors: &mut Vec<ValidationError>) {
    if graph.boundary_endpoints.is_empty() {
        return;
    }

    for profile in &ir.profiles {
        if profile.mode == GraphMode::Strict {
            errors.push(ValidationError::new(format!(
                "strict profile `{}` cannot be used with boundary endpoints",
                profile.name
            )));
        }
    }
}

fn validate_boundary_endpoints(
    components: &BTreeMap<&str, &ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut names_by_direction = BTreeSet::new();
    for endpoint in &graph.boundary_endpoints {
        let direction = match endpoint.direction {
            BoundaryDirection::Input => PortDirection::Input,
            BoundaryDirection::Output => PortDirection::Output,
        };
        if !names_by_direction.insert((endpoint.direction, endpoint.name.as_str())) {
            errors.push(ValidationError::new(format!(
                "boundary {:?} `{}` is declared more than once",
                endpoint.direction, endpoint.name
            )));
        }
        match resolve_port(components, instances, &endpoint.port, direction) {
            Ok(port) if port.ty != endpoint.ty => {
                errors.push(ValidationError::new(format!(
                    "boundary endpoint `{}` type `{}` does not match port `{}.{}` type `{}`",
                    endpoint.name,
                    endpoint.ty.canonical_syntax(),
                    endpoint.port.instance.name,
                    endpoint.port.port,
                    port.ty.canonical_syntax()
                )));
            }
            Ok(_) => {}
            Err(message) => errors.push(ValidationError::new(message)),
        }
    }
}

fn validate_tasks(
    ir: &ContractIr,
    components: &BTreeMap<&str, &ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut task_names_by_instance = BTreeMap::<String, BTreeSet<String>>::new();
    let incoming_binds = graph
        .binds
        .iter()
        .map(|bind| (bind.to.instance.id.clone(), bind.to.port.as_str()))
        .chain(
            graph
                .ros2_bridges
                .iter()
                .filter(|bridge| bridge.direction == Ros2BridgeDirection::Ros2ToFlowrt)
                .filter(|bridge| bridge.boundary_endpoint.is_none())
                .map(|bridge| {
                    (
                        bridge.flowrt.instance.id.clone(),
                        bridge.flowrt.port.as_str(),
                    )
                }),
        )
        .collect::<BTreeSet<_>>();
    let boundary_inputs = graph
        .boundary_endpoints
        .iter()
        .filter(|endpoint| endpoint.direction == BoundaryDirection::Input)
        .map(|endpoint| {
            (
                endpoint.port.instance.id.clone(),
                endpoint.port.port.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    validate_boundary_input_overlap(&incoming_binds, &boundary_inputs, graph, errors);
    let island_enabled = ir
        .profiles
        .iter()
        .any(|profile| profile.mode == GraphMode::Island);

    for task in &graph.tasks {
        if !task_names_by_instance
            .entry(task.instance.name.clone())
            .or_default()
            .insert(task.name.clone())
        {
            errors.push(ValidationError::new(format!(
                "instance `{}` has duplicate task name `{}`",
                task.instance.name, task.name
            )));
        }

        let Some(instance) = instances.get(task.instance.name.as_str()) else {
            errors.push(ValidationError::new(format!(
                "task references unknown instance `{}`",
                task.instance.name
            )));
            continue;
        };
        let Some(component) = components.get(instance.component.name.as_str()) else {
            continue;
        };

        if task.trigger == TriggerKind::Periodic && task.period_ms.is_none() {
            errors.push(ValidationError::new(format!(
                "periodic task on instance `{}` must set period_ms",
                instance.name
            )));
        }
        if task.trigger == TriggerKind::Periodic && task.period_ms == Some(0) {
            errors.push(ValidationError::new(format!(
                "periodic task on instance `{}` must set period_ms greater than zero",
                instance.name
            )));
        }
        if task.trigger != TriggerKind::Periodic && task.period_ms.is_some() {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` must not set period_ms unless trigger is periodic",
                instance.name
            )));
        }
        if task.trigger == TriggerKind::OnMessage && task.inputs.is_empty() {
            errors.push(ValidationError::new(format!(
                "on_message task on instance `{}` must list at least one input",
                instance.name
            )));
        }
        if task.trigger != TriggerKind::OnMessage && task.readiness != TaskReadiness::AnyReady {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` must not set readiness unless trigger is on_message",
                instance.name
            )));
        }

        validate_task_ports(task, component, errors);
        validate_task_input_binds(
            task,
            component,
            &incoming_binds,
            &boundary_inputs,
            island_enabled,
            errors,
        );
    }
}

fn validate_boundary_input_overlap(
    incoming_binds: &BTreeSet<(EntityId, &str)>,
    boundary_inputs: &BTreeSet<(EntityId, &str)>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    for (instance_id, port) in incoming_binds.intersection(boundary_inputs) {
        let instance_name = graph
            .instances
            .iter()
            .find(|instance| instance.id == *instance_id)
            .map(|instance| instance.name.as_str())
            .unwrap_or("<unknown>");
        errors.push(ValidationError::new(format!(
            "input port `{instance_name}.{port}` is satisfied by both a dataflow bind and boundary input"
        )));
    }
}

fn validate_task_ports(task: &TaskIr, component: &ComponentIr, errors: &mut Vec<ValidationError>) {
    let input_ports = component
        .inputs
        .iter()
        .map(|port| port.name.as_str())
        .collect::<BTreeSet<_>>();
    let output_ports = component
        .outputs
        .iter()
        .map(|port| port.name.as_str())
        .collect::<BTreeSet<_>>();

    let mut listed_inputs = BTreeSet::new();
    for input in &task.inputs {
        if !listed_inputs.insert(input.as_str()) {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` lists input port `{}` more than once",
                task.instance.name, input
            )));
        }
        if !input_ports.contains(input.as_str()) {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` references undeclared input port `{}`",
                task.instance.name, input
            )));
        }
    }
    let mut listed_outputs = BTreeSet::new();
    for output in &task.outputs {
        if !listed_outputs.insert(output.as_str()) {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` lists output port `{}` more than once",
                task.instance.name, output
            )));
        }
        if !output_ports.contains(output.as_str()) {
            errors.push(ValidationError::new(format!(
                "task on instance `{}` references undeclared output port `{}`",
                task.instance.name, output
            )));
        }
    }
}

fn validate_task_input_binds(
    task: &TaskIr,
    component: &ComponentIr,
    incoming_binds: &BTreeSet<(EntityId, &str)>,
    boundary_inputs: &BTreeSet<(EntityId, &str)>,
    island_enabled: bool,
    errors: &mut Vec<ValidationError>,
) {
    let input_ports = component
        .inputs
        .iter()
        .map(|port| port.name.as_str())
        .collect::<BTreeSet<_>>();

    for input in &task.inputs {
        if !input_ports.contains(input.as_str()) {
            continue;
        }
        let key = (task.instance.id.clone(), input.as_str());
        let has_incoming_bind = incoming_binds.contains(&key);
        let has_boundary_input = island_enabled && boundary_inputs.contains(&key);
        if !has_incoming_bind && !has_boundary_input {
            errors.push(ValidationError::new(format!(
                "task input `{}.{}` has no incoming bind",
                task.instance.name, input
            )));
        }
    }
}

fn validate_instance_params(
    components: &BTreeMap<&str, &ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    for instance in &graph.instances {
        let Some(component) = components.get(instance.component.name.as_str()) else {
            continue;
        };
        if !instances.contains_key(instance.name.as_str()) {
            continue;
        }

        let mut seen = BTreeSet::new();
        let instance_params = instance
            .params
            .iter()
            .map(|param| (param.name.as_str(), &param.value))
            .collect::<BTreeMap<_, _>>();

        for param in &instance.params {
            if !seen.insert(param.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "instance `{}` has duplicate param `{}`",
                    instance.name, param.name
                )));
            }
        }

        let component_params = component
            .params
            .iter()
            .map(|param| (param.name.as_str(), param))
            .collect::<BTreeMap<_, _>>();

        for param in &component.params {
            let Some(value) = instance_params.get(param.name.as_str()) else {
                errors.push(ValidationError::new(format!(
                    "instance `{}` is missing param `{}`",
                    instance.name, param.name
                )));
                continue;
            };
            let context = format!("instance `{}` param `{}`", instance.name, param.name);
            validate_param_value_matches_schema(&context, param, "", value, errors);
            validate_param_value_constraints(&context, param, "", value, errors);
        }

        for param in &instance.params {
            if !component_params.contains_key(param.name.as_str()) {
                errors.push(ValidationError::new(format!(
                    "instance `{}` has unknown param `{}`",
                    instance.name, param.name
                )));
            }
        }
    }
}

fn validate_binds(
    components: &BTreeMap<&str, &ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut incoming = BTreeSet::new();
    for bind in &graph.binds {
        validate_channel_depth(bind.channel, bind.depth, &bind.to, errors);

        let from_port = resolve_port(components, instances, &bind.from, PortDirection::Output);
        let to_port = resolve_port(components, instances, &bind.to, PortDirection::Input);

        match (&from_port, &to_port) {
            (Ok(from_port), Ok(to_port)) if from_port.ty != to_port.ty => {
                errors.push(ValidationError::new(format!(
                    "bind `{}.{}` -> `{}.{}` has mismatched types",
                    bind.from.instance.name, bind.from.port, bind.to.instance.name, bind.to.port
                )));
            }
            (Err(message), _) => errors.push(ValidationError::new(message.clone())),
            (_, Err(message)) => errors.push(ValidationError::new(message.clone())),
            _ => {}
        }

        let key = (bind.to.instance.id.clone(), bind.to.port.clone());
        if !incoming.insert(key) {
            errors.push(ValidationError::new(format!(
                "input port `{}.{}` has multiple incoming binds",
                bind.to.instance.name, bind.to.port
            )));
        }
    }
}

fn validate_ros2_bridges(
    ir: &ContractIr,
    components: &BTreeMap<&str, &ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    if !graph.ros2_bridges.is_empty()
        && graph
            .instances
            .iter()
            .any(|instance| instance.process.as_deref() == Some("ros2_bridge"))
    {
        errors.push(ValidationError::new(
            "process name `ros2_bridge` is reserved when `bridge.ros2` is declared",
        ));
    }

    let targets = ir
        .targets
        .iter()
        .map(|target| (target.name.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let boundary_endpoints = graph
        .boundary_endpoints
        .iter()
        .map(|endpoint| (endpoint.name.as_str(), endpoint))
        .collect::<BTreeMap<_, _>>();
    let types = ir
        .types
        .iter()
        .map(|ty| (ty.qualified_name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    let mut ros2_boundary_inputs = BTreeSet::new();

    for bridge in &graph.ros2_bridges {
        if bridge.backend != BackendName("zenoh".to_string()) {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` must use backend `zenoh`; found `{}`",
                bridge.name, bridge.backend.0
            )));
        }
        if let Some(boundary_ref) = &bridge.boundary_endpoint {
            validate_ros2_bridge_boundary_endpoint(
                bridge,
                boundary_ref,
                &boundary_endpoints,
                &mut ros2_boundary_inputs,
                errors,
            );
        }
        let endpoint_direction = match bridge.direction {
            Ros2BridgeDirection::FlowrtToRos2 => PortDirection::Output,
            Ros2BridgeDirection::Ros2ToFlowrt => PortDirection::Input,
        };
        let flowrt_port =
            match resolve_port(components, instances, &bridge.flowrt, endpoint_direction) {
                Ok(port) => port,
                Err(message) => {
                    errors.push(ValidationError::new(message));
                    continue;
                }
            };
        let TypeExpr::Named { name: type_name } = &flowrt_port.ty else {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` FlowRT endpoint `{}.{}` must use a named message type",
                bridge.name, bridge.flowrt.instance.name, bridge.flowrt.port
            )));
            continue;
        };
        let Some(message_type) = types.get(type_name.as_str()) else {
            continue;
        };
        match bridge.ros2_type.as_str() {
            "std_msgs/msg/String" => {
                validate_ros2_string_bridge(bridge, message_type, errors);
            }
            "geometry_msgs/msg/Pose" => {
                validate_ros2_pose_bridge(bridge, message_type, &types, errors);
            }
            _ => {
                errors.push(ValidationError::new(format!(
                    "ROS2 bridge `{}` uses unsupported ROS2 type `{}`",
                    bridge.name, bridge.ros2_type
                )));
            }
        }

        let Some(instance) = instances.get(bridge.flowrt.instance.name.as_str()) else {
            continue;
        };
        let Some(target_ref) = &instance.target else {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` source instance `{}` must declare a target that supports backend `zenoh`",
                bridge.name, instance.name
            )));
            continue;
        };
        let Some(target) = targets.get(target_ref.name.as_str()) else {
            continue;
        };
        if !target.backends.iter().any(|backend| backend.0 == "zenoh") {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` requires target `{}` to support backend `zenoh`",
                bridge.name, target.name
            )));
        }
    }
}

fn validate_ros2_bridge_boundary_endpoint(
    bridge: &flowrt_ir::Ros2BridgeIr,
    boundary_ref: &flowrt_ir::EntityRef,
    boundary_endpoints: &BTreeMap<&str, &flowrt_ir::BoundaryEndpointIr>,
    ros2_boundary_inputs: &mut BTreeSet<String>,
    errors: &mut Vec<ValidationError>,
) {
    let Some(boundary) = boundary_endpoints.get(boundary_ref.name.as_str()).copied() else {
        errors.push(ValidationError::new(format!(
            "ROS2 bridge `{}` references unknown boundary endpoint `{}`",
            bridge.name, boundary_ref.name
        )));
        return;
    };
    if boundary.id != boundary_ref.id {
        errors.push(ValidationError::new(format!(
            "ROS2 bridge `{}` boundary endpoint reference `{}` points to ID `{}`, expected ID `{}`",
            bridge.name, boundary_ref.name, boundary_ref.id.0, boundary.id.0
        )));
    }
    let expected_direction = match bridge.direction {
        Ros2BridgeDirection::FlowrtToRos2 => BoundaryDirection::Output,
        Ros2BridgeDirection::Ros2ToFlowrt => BoundaryDirection::Input,
    };
    if boundary.direction != expected_direction {
        errors.push(ValidationError::new(format!(
            "ROS2 bridge `{}` direction `{}` is incompatible with boundary endpoint `{}` direction `{}`",
            bridge.name,
            ros2_bridge_direction_name(bridge.direction),
            boundary.name,
            boundary_direction_name(boundary.direction)
        )));
    }
    if boundary.port.instance.id != bridge.flowrt.instance.id
        || boundary.port.instance.name != bridge.flowrt.instance.name
        || boundary.port.port != bridge.flowrt.port
    {
        errors.push(ValidationError::new(format!(
            "ROS2 bridge `{}` boundary endpoint `{}` must resolve to FlowRT port `{}.{}`; found `{}.{}`",
            bridge.name,
            boundary.name,
            boundary.port.instance.name,
            boundary.port.port,
            bridge.flowrt.instance.name,
            bridge.flowrt.port
        )));
    }
    if bridge.direction == Ros2BridgeDirection::Ros2ToFlowrt
        && !ros2_boundary_inputs.insert(boundary.name.clone())
    {
        errors.push(ValidationError::new(format!(
            "boundary input `{}` has multiple ROS2 bridge sources",
            boundary.name
        )));
    }
}

fn boundary_direction_name(direction: BoundaryDirection) -> &'static str {
    match direction {
        BoundaryDirection::Input => "input",
        BoundaryDirection::Output => "output",
    }
}

fn ros2_bridge_direction_name(direction: Ros2BridgeDirection) -> &'static str {
    match direction {
        Ros2BridgeDirection::FlowrtToRos2 => "flowrt_to_ros2",
        Ros2BridgeDirection::Ros2ToFlowrt => "ros2_to_flowrt",
    }
}

fn validate_ros2_string_bridge(
    bridge: &flowrt_ir::Ros2BridgeIr,
    message_type: &flowrt_ir::TypeIr,
    errors: &mut Vec<ValidationError>,
) {
    let Some(field) = message_type
        .fields
        .iter()
        .find(|field| field.name == bridge.field)
    else {
        errors.push(ValidationError::new(format!(
            "ROS2 bridge `{}` maps field `{}`, but type `{}` has no such field",
            bridge.name, bridge.field, message_type.name
        )));
        return;
    };
    if !matches!(field.ty, TypeExpr::VarString { .. }) {
        errors.push(ValidationError::new(format!(
            "ROS2 bridge `{}` maps field `{}` of type `{}`, but `std_msgs/msg/String.data` requires `string`",
            bridge.name,
            bridge.field,
            field.ty.canonical_syntax()
        )));
    }
}

fn validate_ros2_pose_bridge(
    bridge: &flowrt_ir::Ros2BridgeIr,
    message_type: &flowrt_ir::TypeIr,
    types: &BTreeMap<&str, &flowrt_ir::TypeIr>,
    errors: &mut Vec<ValidationError>,
) {
    let mut missing = Vec::new();
    if let Some(position) = require_named_field(message_type, "position", types, &mut missing) {
        for field in ["x", "y", "z"] {
            require_primitive_field(position, field, PrimitiveType::F64, &mut missing);
        }
    }
    if let Some(orientation) = require_named_field(message_type, "orientation", types, &mut missing)
    {
        for field in ["x", "y", "z", "w"] {
            require_primitive_field(orientation, field, PrimitiveType::F64, &mut missing);
        }
    }
    if !missing.is_empty() {
        errors.push(ValidationError::new(format!(
            "ROS2 bridge `{}` maps type `{}` to geometry_msgs/msg/Pose, but required fields are missing or mismatched: {}",
            bridge.name,
            message_type.name,
            missing.join(", ")
        )));
    }
}

fn require_named_field<'a>(
    ty: &'a flowrt_ir::TypeIr,
    field: &str,
    types: &BTreeMap<&str, &'a flowrt_ir::TypeIr>,
    missing: &mut Vec<String>,
) -> Option<&'a flowrt_ir::TypeIr> {
    let Some(field_ir) = ty.fields.iter().find(|candidate| candidate.name == field) else {
        missing.push(format!("{}.{}", ty.name, field));
        return None;
    };
    let TypeExpr::Named { name } = &field_ir.ty else {
        missing.push(format!(
            "{}.{} expected named type, found {}",
            ty.name,
            field,
            field_ir.ty.canonical_syntax()
        ));
        return None;
    };
    match types.get(name.as_str()).copied() {
        Some(named) => Some(named),
        None => {
            missing.push(format!(
                "{}.{} references unknown type {}",
                ty.name, field, name
            ));
            None
        }
    }
}

fn require_primitive_field(
    ty: &flowrt_ir::TypeIr,
    field: &str,
    expected: PrimitiveType,
    missing: &mut Vec<String>,
) {
    let Some(field_ir) = ty.fields.iter().find(|candidate| candidate.name == field) else {
        missing.push(format!("{}.{}", ty.name, field));
        return;
    };
    if !matches!(field_ir.ty, TypeExpr::Primitive { name } if name == expected) {
        missing.push(format!(
            "{}.{} expected {}, found {}",
            ty.name,
            field,
            primitive_type_name(expected),
            field_ir.ty.canonical_syntax()
        ));
    }
}

fn primitive_type_name(ty: PrimitiveType) -> &'static str {
    match ty {
        PrimitiveType::Bool => "bool",
        PrimitiveType::U8 => "u8",
        PrimitiveType::U16 => "u16",
        PrimitiveType::U32 => "u32",
        PrimitiveType::U64 => "u64",
        PrimitiveType::U128 => "u128",
        PrimitiveType::I8 => "i8",
        PrimitiveType::I16 => "i16",
        PrimitiveType::I32 => "i32",
        PrimitiveType::I64 => "i64",
        PrimitiveType::I128 => "i128",
        PrimitiveType::F32 => "f32",
        PrimitiveType::F64 => "f64",
    }
}

fn validate_graph_is_acyclic(
    instances: &BTreeMap<&str, &InstanceIr>,
    graph: &GraphIr,
    errors: &mut Vec<ValidationError>,
) {
    let mut indegree = instances
        .keys()
        .map(|name| ((*name).to_string(), 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut edges = BTreeMap::<String, BTreeSet<String>>::new();
    let mut self_loops = BTreeSet::<String>::new();

    for bind in &graph.binds {
        let source = bind.from.instance.name.as_str();
        let target = bind.to.instance.name.as_str();
        if !instances.contains_key(source) || !instances.contains_key(target) {
            continue;
        }

        if source == target {
            self_loops.insert(source.to_string());
            continue;
        }

        let inserted = edges
            .entry(source.to_string())
            .or_default()
            .insert(target.to_string());
        if inserted {
            *indegree
                .get_mut(target)
                .expect("known instance must have an indegree entry") += 1;
        }
    }

    for instance in self_loops {
        errors.push(ValidationError::new(format!(
            "graph `{}` has a dataflow self-loop on instance `{}`",
            graph.name, instance
        )));
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(name, degree)| (*degree == 0).then_some(name.clone()))
        .collect::<BTreeSet<_>>();
    let mut visited = 0usize;

    while let Some(name) = ready.iter().next().cloned() {
        ready.remove(&name);
        visited += 1;

        if let Some(next) = edges.get(&name) {
            for target in next {
                let degree = indegree
                    .get_mut(target)
                    .expect("known instance must have an indegree entry");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(target.clone());
                }
            }
        }
    }

    if visited != instances.len() {
        let cycle_instances = indegree
            .into_iter()
            .filter_map(|(name, degree)| (degree > 0).then_some(format!("`{name}`")))
            .collect::<Vec<_>>()
            .join(", ");
        errors.push(ValidationError::new(format!(
            "graph `{}` has a dataflow cycle involving {}",
            graph.name, cycle_instances
        )));
    }
}

fn validate_channel_depth(
    channel: ChannelKind,
    depth: Option<u32>,
    to: &PortRef,
    errors: &mut Vec<ValidationError>,
) {
    match (channel, depth) {
        (ChannelKind::Latest, Some(depth)) if depth != 1 => {
            errors.push(ValidationError::new(format!(
                "latest channel to `{}.{}` must omit depth or set depth = 1",
                to.instance.name, to.port
            )));
        }
        (ChannelKind::Fifo, Some(0)) => {
            errors.push(ValidationError::new(format!(
                "channel to `{}.{}` has zero depth",
                to.instance.name, to.port
            )));
        }
        (ChannelKind::Fifo, None) => {
            errors.push(ValidationError::new(format!(
                "fifo channel to `{}.{}` must set depth",
                to.instance.name, to.port
            )));
        }
        _ => {}
    }
}

fn resolve_port<'a>(
    components: &'a BTreeMap<&str, &'a ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    endpoint: &PortRef,
    direction: PortDirection,
) -> std::result::Result<&'a PortIr, String> {
    let instance = instances
        .get(endpoint.instance.name.as_str())
        .ok_or_else(|| format!("unknown instance `{}`", endpoint.instance.name))?;
    let component = components
        .get(instance.component.name.as_str())
        .ok_or_else(|| format!("unknown component `{}`", instance.component.name))?;
    let ports = match direction {
        PortDirection::Input => &component.inputs,
        PortDirection::Output => &component.outputs,
    };
    ports
        .iter()
        .find(|port| port.name == endpoint.port)
        .ok_or_else(|| {
            format!(
                "instance `{}` component `{}` has no {:?} port `{}`",
                instance.name, component.name, direction, endpoint.port
            )
        })
}

fn resolve_service_port<'a>(
    components: &'a BTreeMap<&str, &'a ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    endpoint: &ServicePortRef,
    direction: ServiceDirection,
) -> std::result::Result<&'a ServicePortIr, String> {
    let instance = instances
        .get(endpoint.instance.name.as_str())
        .ok_or_else(|| format!("unknown instance `{}`", endpoint.instance.name))?;
    let component = components
        .get(instance.component.name.as_str())
        .ok_or_else(|| format!("unknown component `{}`", instance.component.name))?;
    let ports = match direction {
        ServiceDirection::Client => &component.service_clients,
        ServiceDirection::Server => &component.service_servers,
    };
    ports
        .iter()
        .find(|port| port.name == endpoint.port)
        .ok_or_else(|| {
            format!(
                "instance `{}` component `{}` has no {:?} service `{}`",
                instance.name, component.name, direction, endpoint.port
            )
        })
}

fn resolve_operation_port<'a>(
    components: &'a BTreeMap<&str, &'a ComponentIr>,
    instances: &BTreeMap<&str, &InstanceIr>,
    endpoint: &OperationPortRef,
    direction: OperationDirection,
) -> std::result::Result<&'a OperationPortIr, String> {
    let instance = instances
        .get(endpoint.instance.name.as_str())
        .ok_or_else(|| format!("unknown instance `{}`", endpoint.instance.name))?;
    let component = components
        .get(instance.component.name.as_str())
        .ok_or_else(|| format!("unknown component `{}`", instance.component.name))?;
    let ports = match direction {
        OperationDirection::Client => &component.operation_clients,
        OperationDirection::Server => &component.operation_servers,
    };
    ports
        .iter()
        .find(|port| port.name == endpoint.port)
        .ok_or_else(|| {
            format!(
                "instance `{}` component `{}` has no {:?} operation `{}`",
                instance.name, component.name, direction, endpoint.port
            )
        })
}

#[derive(Debug, Clone, Copy)]
enum PortDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy)]
enum ServiceDirection {
    Client,
    Server,
}

#[derive(Debug, Clone, Copy)]
enum OperationDirection {
    Client,
    Server,
}
