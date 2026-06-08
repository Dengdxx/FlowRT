use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    BackendName, ChannelKind, ComponentIr, ContractIr, EntityId, GraphIr, InstanceIr,
    OperationPortIr, OperationPortRef, PortIr, PortRef, ProcessReadinessGate, Ros2BridgeDirection,
    ServicePortIr, ServicePortRef, TaskIr, TaskReadiness, TriggerKind, TypeExpr,
    param_value_compatible, param_value_kind,
};

use crate::ValidationError;

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
        validate_tasks(&components, &instances, graph, errors);
        validate_instance_params(&components, &instances, graph, errors);
        validate_binds(&components, &instances, graph, errors);
        validate_service_binds(&components, &instances, graph, errors);
        validate_operation_binds(&components, &instances, graph, errors);
        validate_ros2_bridges(ir, &components, &instances, graph, errors);
        validate_graph_is_acyclic(&instances, graph, errors);
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

        // 校验显式 inproc 不跨 process。
        if service.backend.0 == "inproc"
            && service.backend_source == flowrt_ir::ServiceBackendSource::Explicit
        {
            let client_process = instances
                .get(service.client.instance.name.as_str())
                .and_then(|i| i.process.as_deref())
                .unwrap_or("main");
            let server_process = instances
                .get(service.server.instance.name.as_str())
                .and_then(|i| i.process.as_deref())
                .unwrap_or("main");
            if client_process != server_process {
                errors.push(ValidationError::new(format!(
                    "service bind `{client_key} -> {server_key}` uses explicit `inproc` but spans processes `{client_process}` and `{server_process}`"
                )));
            }
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

        if operation.backend.0 == "inproc"
            && operation.backend_source == flowrt_ir::OperationBackendSource::Explicit
            && operation_spans_boundaries(instances, &operation.client, &operation.server)
        {
            errors.push(ValidationError::new(format!(
                "operation bind `{client_key} -> {server_key}` uses explicit `inproc` but spans process or target boundaries"
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

fn validate_tasks(
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
        .collect::<BTreeSet<_>>();

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
        validate_task_input_binds(task, component, &incoming_binds, errors);
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
        if !incoming_binds.contains(&(task.instance.id.clone(), input.as_str())) {
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
            .map(|param| (param.name.as_str(), &param.default))
            .collect::<BTreeMap<_, _>>();

        for param in &component.params {
            let Some(value) = instance_params.get(param.name.as_str()) else {
                errors.push(ValidationError::new(format!(
                    "instance `{}` is missing param `{}`",
                    instance.name, param.name
                )));
                continue;
            };
            if !param_value_compatible(&param.default, value) {
                errors.push(ValidationError::new(format!(
                    "instance `{}` param `{}` has incompatible value kind `{}`; expected `{}`",
                    instance.name,
                    param.name,
                    param_value_kind(value),
                    param_value_kind(&param.default)
                )));
            }
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
    let types = ir
        .types
        .iter()
        .map(|ty| (ty.qualified_name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();

    for bridge in &graph.ros2_bridges {
        if bridge.backend != BackendName("zenoh".to_string()) {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` must use backend `zenoh`; found `{}`",
                bridge.name, bridge.backend.0
            )));
        }
        if bridge.direction != Ros2BridgeDirection::FlowrtToRos2 {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` has unsupported direction",
                bridge.name
            )));
        }
        if bridge.ros2_type != "std_msgs/msg/String" {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` uses unsupported ROS2 type `{}`; only `std_msgs/msg/String` is supported",
                bridge.name, bridge.ros2_type
            )));
        }

        let source_port =
            match resolve_port(components, instances, &bridge.flowrt, PortDirection::Output) {
                Ok(port) => port,
                Err(message) => {
                    errors.push(ValidationError::new(message));
                    continue;
                }
            };
        let TypeExpr::Named { name: type_name } = &source_port.ty else {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` source `{}.{}` must use a named message type",
                bridge.name, bridge.flowrt.instance.name, bridge.flowrt.port
            )));
            continue;
        };
        let Some(message_type) = types.get(type_name.as_str()) else {
            continue;
        };
        let Some(field) = message_type
            .fields
            .iter()
            .find(|field| field.name == bridge.field)
        else {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` maps field `{}`, but type `{}` has no such field",
                bridge.name, bridge.field, message_type.name
            )));
            continue;
        };
        if !matches!(field.ty, TypeExpr::VarString { .. }) {
            errors.push(ValidationError::new(format!(
                "ROS2 bridge `{}` maps field `{}` of type `{}`, but `std_msgs/msg/String.data` requires `string`",
                bridge.name,
                bridge.field,
                field.ty.canonical_syntax()
            )));
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
