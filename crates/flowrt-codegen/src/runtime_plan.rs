use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    BackendName, BoundaryDirection, ChannelBackendSource, ChannelKind, ContractIr, GraphIr,
    InstanceIr, OperationConcurrencyPolicy, OperationFeedbackPolicy, OperationPreemptPolicy,
    OverflowPolicy as IrOverflowPolicy, ParamIr, Ros2BridgeDirection, Ros2BridgeIr,
    ServiceOverflowPolicy, StalePolicy as IrStalePolicy, TaskConcurrency, TaskIr, TaskReadiness,
    TriggerKind, TypeExpr,
};

use crate::{
    component_by_name, fixed_message_abi_expectations, instance_by_name, port_by_name,
    rust_wire_size, snake_identifier, type_contains_variable_data,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskEmissionPhase {
    Scheduler,
    Startup,
    Shutdown,
}

impl TaskEmissionPhase {
    pub(crate) fn includes(self, trigger: TriggerKind) -> bool {
        match self {
            TaskEmissionPhase::Scheduler => {
                matches!(trigger, TriggerKind::Periodic | TriggerKind::OnMessage)
            }
            TaskEmissionPhase::Startup => trigger == TriggerKind::Startup,
            TaskEmissionPhase::Shutdown => trigger == TriggerKind::Shutdown,
        }
    }
}

pub(crate) fn on_message_trigger_guard<F>(task: &TaskIr, input_name: F) -> Option<String>
where
    F: Fn(&str) -> String,
{
    if task.trigger != TriggerKind::OnMessage || task.inputs.is_empty() {
        return None;
    }

    Some(
        task.inputs
            .iter()
            .map(|input| format!("{}.present()", input_name(input)))
            .collect::<Vec<_>>()
            .join(match task.readiness {
                TaskReadiness::AnyReady => " || ",
                TaskReadiness::AllReady => " && ",
            }),
    )
}

pub(crate) fn indent_generated_block(block: &str, nested: bool) -> String {
    if !nested {
        return block.to_string();
    }

    indent_generated_block_levels(block, 1)
}

pub(crate) fn indent_generated_block_levels(block: &str, levels: usize) -> String {
    if block.is_empty() {
        return String::new();
    }

    if levels == 0 {
        return block.to_string();
    }

    let prefix = "    ".repeat(levels);
    block
        .lines()
        .map(|line| {
            if line.is_empty() {
                String::new()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

pub(crate) fn step_indent(nested: bool) -> &'static str {
    if nested { "        " } else { "    " }
}

pub(crate) fn nested_step_indent(nested: bool) -> &'static str {
    if nested { "            " } else { "        " }
}

pub(crate) fn rust_step_indent(nested: bool) -> &'static str {
    if nested { "            " } else { "        " }
}

pub(crate) fn rust_nested_step_indent(nested: bool) -> &'static str {
    if nested {
        "                "
    } else {
        "            "
    }
}

pub(crate) fn resolved_task_lane_name(task: &TaskIr) -> String {
    if task.concurrency == TaskConcurrency::Exclusive {
        return format!("{}_serial", task.instance.name);
    }
    task.lane
        .clone()
        .unwrap_or_else(|| format!("{}_serial", task.instance.name))
}

#[derive(Debug, Clone)]
pub(crate) struct BindRuntimePlan {
    pub(crate) index: usize,
    pub(crate) field_name: String,
    pub(crate) probe_field_name: String,
    pub(crate) channel: ChannelKind,
    pub(crate) backend: BackendName,
    pub(crate) backend_source: ChannelBackendSource,
    pub(crate) overflow: IrOverflowPolicy,
    pub(crate) stale: IrStalePolicy,
    pub(crate) max_age_ms: Option<u64>,
    pub(crate) depth: Option<u32>,
    pub(crate) source_type: TypeExpr,
    pub(crate) source_uses_variable_frame: bool,
    pub(crate) source_instance: String,
    pub(crate) source_port: String,
    pub(crate) target_instance: String,
    pub(crate) target_port: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ProcessRuntimePlan<'a> {
    pub(crate) name: String,
    pub(crate) method_suffix: String,
    pub(crate) instances: Vec<&'a InstanceIr>,
}

#[derive(Debug, Clone)]
pub(crate) struct BridgeRuntimePlan {
    pub(crate) index: usize,
    pub(crate) name: String,
    pub(crate) field_name: String,
    pub(crate) source_type: TypeExpr,
    pub(crate) source_instance: String,
    pub(crate) source_port: String,
    pub(crate) boundary_endpoint: Option<String>,
    pub(crate) ros2_topic: String,
    pub(crate) ros2_type: String,
    pub(crate) direction: Ros2BridgeDirection,
    pub(crate) field: String,
}

/// island boundary endpoint 的 codegen 计划。
#[derive(Debug, Clone)]
pub(crate) struct BoundaryRuntimePlan {
    pub(crate) index: usize,
    pub(crate) endpoint_name: String,
    pub(crate) field_name: String,
    pub(crate) direction: BoundaryDirection,
    pub(crate) ty: TypeExpr,
    pub(crate) instance: String,
    pub(crate) port: String,
}

pub(crate) fn process_runtime_plans<'a>(order: &[&'a InstanceIr]) -> Vec<ProcessRuntimePlan<'a>> {
    let mut by_process = BTreeMap::<String, Vec<&'a InstanceIr>>::new();
    for &instance in order {
        by_process
            .entry(
                instance
                    .process
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
            )
            .or_default()
            .push(instance);
    }

    let mut used_suffixes = BTreeSet::new();
    by_process
        .into_iter()
        .enumerate()
        .map(|(index, (name, instances))| {
            let base = snake_identifier(&name);
            let mut suffix = base.clone();
            if !used_suffixes.insert(suffix.clone()) {
                suffix = format!("{}_{}", base, index);
                while !used_suffixes.insert(suffix.clone()) {
                    suffix.push('_');
                }
            }
            ProcessRuntimePlan {
                name,
                method_suffix: suffix,
                instances,
            }
        })
        .collect()
}

pub(crate) fn bind_runtime_plans(contract: &ContractIr, graph: &GraphIr) -> Vec<BindRuntimePlan> {
    graph
        .binds
        .iter()
        .enumerate()
        .map(|(index, bind)| {
            let source_instance = instance_by_name(graph, &bind.from.instance.name);
            let source_component = component_by_name(contract, &source_instance.component.name);
            let source_port = port_by_name(&source_component.outputs, &bind.from.port);
            BindRuntimePlan {
                index,
                field_name: format!("bind_{index}"),
                probe_field_name: format!("introspection_probe_bind_{index}"),
                channel: bind.channel,
                backend: bind.backend.clone(),
                backend_source: bind.backend_source,
                overflow: bind.overflow,
                stale: bind.stale,
                max_age_ms: bind.max_age_ms,
                depth: bind.depth,
                source_type: source_port.ty.clone(),
                source_uses_variable_frame: type_contains_variable_data(contract, &source_port.ty),
                source_instance: source_instance.name.clone(),
                source_port: bind.from.port.clone(),
                target_instance: bind.to.instance.name.clone(),
                target_port: bind.to.port.clone(),
            }
        })
        .collect()
}

pub(crate) fn bridge_runtime_plans(
    contract: &ContractIr,
    graph: &GraphIr,
) -> Vec<BridgeRuntimePlan> {
    graph
        .ros2_bridges
        .iter()
        .enumerate()
        .map(|(index, bridge)| bridge_runtime_plan(contract, graph, index, bridge))
        .collect()
}

pub(crate) fn boundary_runtime_plans(graph: &GraphIr) -> Vec<BoundaryRuntimePlan> {
    graph
        .boundary_endpoints
        .iter()
        .enumerate()
        .map(|(index, endpoint)| {
            let prefix = match endpoint.direction {
                BoundaryDirection::Input => "boundary_input",
                BoundaryDirection::Output => "boundary_output",
            };
            BoundaryRuntimePlan {
                index,
                endpoint_name: endpoint.name.clone(),
                field_name: format!("{prefix}_{}", snake_identifier(&endpoint.name)),
                direction: endpoint.direction,
                ty: endpoint.ty.clone(),
                instance: endpoint.port.instance.name.clone(),
                port: endpoint.port.port.clone(),
            }
        })
        .collect()
}

fn bridge_runtime_plan(
    contract: &ContractIr,
    graph: &GraphIr,
    index: usize,
    bridge: &Ros2BridgeIr,
) -> BridgeRuntimePlan {
    let flowrt_instance = instance_by_name(graph, &bridge.flowrt.instance.name);
    let flowrt_component = component_by_name(contract, &flowrt_instance.component.name);
    let flowrt_port = match bridge.direction {
        Ros2BridgeDirection::FlowrtToRos2 => {
            port_by_name(&flowrt_component.outputs, &bridge.flowrt.port)
        }
        Ros2BridgeDirection::Ros2ToFlowrt => {
            port_by_name(&flowrt_component.inputs, &bridge.flowrt.port)
        }
    };
    BridgeRuntimePlan {
        index,
        name: bridge.name.clone(),
        field_name: bridge.name.clone(),
        source_type: flowrt_port.ty.clone(),
        source_instance: flowrt_instance.name.clone(),
        source_port: bridge.flowrt.port.clone(),
        boundary_endpoint: bridge
            .boundary_endpoint
            .as_ref()
            .map(|endpoint| endpoint.name.clone()),
        ros2_topic: bridge.ros2_topic.clone(),
        ros2_type: bridge.ros2_type.clone(),
        direction: bridge.direction,
        field: bridge.field.clone(),
    }
}

pub(crate) fn incoming_bind_index_map(
    plans: &[BindRuntimePlan],
) -> BTreeMap<(String, String), usize> {
    plans
        .iter()
        .map(|plan| {
            (
                (plan.target_instance.clone(), plan.target_port.clone()),
                plan.index,
            )
        })
        .collect()
}

pub(crate) fn outgoing_bind_indices_map(
    plans: &[BindRuntimePlan],
) -> BTreeMap<(String, String), Vec<usize>> {
    let mut map = BTreeMap::new();
    for plan in plans {
        map.entry((plan.source_instance.clone(), plan.source_port.clone()))
            .or_insert_with(Vec::new)
            .push(plan.index);
    }
    map
}

pub(crate) fn outgoing_bridge_indices_map(
    plans: &[BridgeRuntimePlan],
) -> BTreeMap<(String, String), Vec<usize>> {
    let mut map = BTreeMap::new();
    for plan in plans
        .iter()
        .filter(|plan| plan.direction == Ros2BridgeDirection::FlowrtToRos2)
    {
        map.entry((plan.source_instance.clone(), plan.source_port.clone()))
            .or_insert_with(Vec::new)
            .push(plan.index);
    }
    map
}

pub(crate) fn incoming_bridge_index_map(
    plans: &[BridgeRuntimePlan],
) -> BTreeMap<(String, String), usize> {
    plans
        .iter()
        .filter(|plan| plan.direction == Ros2BridgeDirection::Ros2ToFlowrt)
        .filter(|plan| plan.boundary_endpoint.is_none())
        .map(|plan| {
            (
                (plan.source_instance.clone(), plan.source_port.clone()),
                plan.index,
            )
        })
        .collect()
}

pub(crate) fn incoming_boundary_index_map(
    plans: &[BoundaryRuntimePlan],
) -> BTreeMap<(String, String), usize> {
    plans
        .iter()
        .filter(|plan| plan.direction == BoundaryDirection::Input)
        .map(|plan| ((plan.instance.clone(), plan.port.clone()), plan.index))
        .collect()
}

pub(crate) fn outgoing_boundary_indices_map(
    plans: &[BoundaryRuntimePlan],
) -> BTreeMap<(String, String), Vec<usize>> {
    let mut map = BTreeMap::new();
    for plan in plans
        .iter()
        .filter(|plan| plan.direction == BoundaryDirection::Output)
    {
        map.entry((plan.instance.clone(), plan.port.clone()))
            .or_insert_with(Vec::new)
            .push(plan.index);
    }
    map
}

pub(crate) fn active_binds_for_instances<'a>(
    binds: &'a [BindRuntimePlan],
    order: &[&InstanceIr],
) -> Vec<&'a BindRuntimePlan> {
    let active_instances = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();
    binds
        .iter()
        .filter(|bind| {
            active_instances.contains(bind.source_instance.as_str())
                || active_instances.contains(bind.target_instance.as_str())
        })
        .collect()
}

pub(crate) fn active_boundaries_for_instances<'a>(
    boundaries: &'a [BoundaryRuntimePlan],
    order: &[&InstanceIr],
) -> Vec<&'a BoundaryRuntimePlan> {
    let active_instances = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();
    boundaries
        .iter()
        .filter(|boundary| active_instances.contains(boundary.instance.as_str()))
        .collect()
}

pub(crate) fn bind_backend(bind: &BindRuntimePlan) -> &str {
    bind.backend.0.as_str()
}

pub(crate) fn contract_uses_backend(contract: &ContractIr, backend: &str) -> bool {
    contract
        .profiles
        .iter()
        .any(|profile| profile.backend.0 == backend)
        || contract
            .graphs
            .iter()
            .flat_map(|graph| &graph.binds)
            .any(|bind| bind.backend.0 == backend)
        || contract
            .graphs
            .iter()
            .flat_map(|graph| &graph.services)
            .any(|service| service.backend.0 == backend)
        || contract
            .graphs
            .iter()
            .flat_map(|graph| &graph.ros2_bridges)
            .any(|bridge| bridge.backend.0 == backend)
        || contract
            .graphs
            .iter()
            .flat_map(|graph| &graph.operations)
            .any(|operation| operation.backend.0 == backend)
}

pub(crate) fn contract_backend_features(contract: &ContractIr) -> Vec<&'static str> {
    let mut features = Vec::new();
    if contract_uses_backend(contract, "iox2") {
        features.push("iox2");
    }
    if contract_uses_backend(contract, "zenoh")
        || contract_has_params_for_language(contract, flowrt_ir::LanguageKind::Rust)
    {
        features.push("zenoh");
    }
    features
}

pub(crate) fn contract_has_params_for_language(
    contract: &ContractIr,
    language: flowrt_ir::LanguageKind,
) -> bool {
    contract
        .components
        .iter()
        .any(|component| component.language == language && !component.params.is_empty())
}

pub(crate) fn contract_has_runtime_params_for_language(
    contract: &ContractIr,
    language: flowrt_ir::LanguageKind,
) -> bool {
    contract.components.iter().any(|component| {
        component.language == language
            && component
                .params
                .iter()
                .any(|param| param.update == flowrt_ir::ParamUpdatePolicy::OnTick)
    })
}

pub(crate) fn runtime_channel_name(bind: &BindRuntimePlan) -> String {
    format!(
        "{}.{}_to_{}.{}",
        bind.source_instance, bind.source_port, bind.target_instance, bind.target_port
    )
}

pub(crate) fn runtime_channel_message_type(bind: &BindRuntimePlan) -> String {
    bind.source_type.canonical_syntax()
}

pub(crate) fn runtime_channel_probe_capacity(
    contract: &ContractIr,
    bind: &BindRuntimePlan,
) -> Option<usize> {
    match &bind.source_type {
        TypeExpr::Named { name } => fixed_message_abi_size(contract, name).or_else(|| {
            (!bind.source_uses_variable_frame).then(|| rust_wire_size(contract, &bind.source_type))
        }),
        other => Some(rust_wire_size(contract, other)),
    }
}

fn fixed_message_abi_size(contract: &ContractIr, type_name: &str) -> Option<usize> {
    fixed_message_abi_expectations(contract)
        .ok()?
        .into_iter()
        .find(|expectation| expectation.type_name == type_name)
        .map(|expectation| expectation.size_bytes)
}

pub(crate) fn runtime_param_name(instance: &InstanceIr, param: &ParamIr) -> String {
    format!("{}.{}", instance.name, param.name)
}

/// service bind 的 codegen 计划。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ServiceRuntimePlan {
    /// service edge 索引。
    pub(crate) index: usize,
    /// service canonical name（`{client_instance}.{client_port}`）。
    pub(crate) service_name: String,
    /// client 端实例名。
    pub(crate) client_instance: String,
    /// client 端 component 名。
    pub(crate) client_component: String,
    /// client 端端口名。
    pub(crate) client_port: String,
    /// server 端实例名。
    pub(crate) server_instance: String,
    /// server 端 component 名。
    pub(crate) server_component: String,
    /// server 端端口名。
    pub(crate) server_port: String,
    /// request 类型。
    pub(crate) request_type: TypeExpr,
    /// response 类型。
    pub(crate) response_type: TypeExpr,
    /// backend 名称。
    pub(crate) backend: BackendName,
    /// 超时毫秒。
    pub(crate) timeout_ms: u64,
    /// 队列深度。
    pub(crate) queue_depth: u32,
    /// overflow 策略。
    pub(crate) overflow: ServiceOverflowPolicy,
    /// server lane 名称。
    pub(crate) lane: Option<String>,
    /// 最大 in-flight 请求数。
    pub(crate) max_in_flight: u32,
}

/// 为 graph 中所有 service edge 生成 codegen 计划。
pub(crate) fn service_runtime_plans(
    contract: &ContractIr,
    graph: &GraphIr,
) -> Vec<ServiceRuntimePlan> {
    graph
        .services
        .iter()
        .enumerate()
        .map(|(index, service)| {
            let client_instance = &service.client.instance.name;
            let client_port = &service.client.port;
            let server_instance = &service.server.instance.name;
            let server_port = &service.server.port;
            let client_instance_ir = instance_by_name(graph, client_instance);
            let server_instance_ir = instance_by_name(graph, server_instance);

            // 从 server component 查找 request/response 类型
            let server_component =
                crate::component_by_name(contract, &server_instance_ir.component.name);
            let server_port_ir = server_component
                .service_servers
                .iter()
                .find(|p| p.name == *server_port)
                .expect("validated service bind must reference existing server port");

            ServiceRuntimePlan {
                index,
                service_name: format!("{client_instance}.{client_port}"),
                client_instance: client_instance.clone(),
                client_component: client_instance_ir.component.name.clone(),
                client_port: client_port.clone(),
                server_instance: server_instance.clone(),
                server_component: server_instance_ir.component.name.clone(),
                server_port: server_port.clone(),
                request_type: server_port_ir.request.clone(),
                response_type: server_port_ir.response.clone(),
                backend: service.backend.clone(),
                timeout_ms: service.policy.timeout_ms,
                queue_depth: service.policy.queue_depth,
                overflow: service.policy.overflow,
                lane: service.policy.lane.clone(),
                max_in_flight: service.policy.max_in_flight,
            }
        })
        .collect()
}

/// 获取 service server 的 lane 名称。
pub(crate) fn service_server_lane(plan: &ServiceRuntimePlan) -> String {
    plan.lane
        .clone()
        .unwrap_or_else(|| format!("{}_serial", plan.server_instance))
}

/// 获取 service 的 overflow 策略名称。
#[allow(dead_code)]
pub(crate) fn ir_service_overflow_name(policy: ServiceOverflowPolicy) -> &'static str {
    match policy {
        ServiceOverflowPolicy::Busy => "Busy",
        ServiceOverflowPolicy::Error => "Error",
    }
}

/// 查找指定实例作为 client 的所有 service plans。
#[allow(dead_code)]
pub(crate) fn client_service_plans<'a>(
    plans: &'a [ServiceRuntimePlan],
    instance_name: &str,
) -> Vec<&'a ServiceRuntimePlan> {
    plans
        .iter()
        .filter(|plan| plan.client_instance == instance_name)
        .collect()
}

/// 查找指定实例作为 server 的所有 service plans。
#[allow(dead_code)]
pub(crate) fn server_service_plans<'a>(
    plans: &'a [ServiceRuntimePlan],
    instance_name: &str,
) -> Vec<&'a ServiceRuntimePlan> {
    plans
        .iter()
        .filter(|plan| plan.server_instance == instance_name)
        .collect()
}

/// operation bind 的 codegen 计划。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct OperationRuntimePlan {
    /// operation edge 索引。
    pub(crate) index: usize,
    /// operation canonical name（`{client_instance}.{client_port}`）。
    pub(crate) operation_name: String,
    /// client 端实例名。
    pub(crate) client_instance: String,
    /// client 端 component 名。
    pub(crate) client_component: String,
    /// client 端端口名。
    pub(crate) client_port: String,
    /// server 端实例名。
    pub(crate) server_instance: String,
    /// server 端 component 名。
    pub(crate) server_component: String,
    /// server 端端口名。
    pub(crate) server_port: String,
    /// goal 类型。
    pub(crate) goal_type: TypeExpr,
    /// feedback 类型。
    pub(crate) feedback_type: TypeExpr,
    /// result 类型。
    pub(crate) result_type: TypeExpr,
    /// backend 名称。
    pub(crate) backend: BackendName,
    /// 超时毫秒。
    pub(crate) timeout_ms: u64,
    /// 并发策略。
    pub(crate) concurrency: OperationConcurrencyPolicy,
    /// 抢占策略。
    pub(crate) preempt: OperationPreemptPolicy,
    /// 等待队列深度。
    pub(crate) queue_depth: u32,
    /// 最大 in-flight invocation 数。
    pub(crate) max_in_flight: u32,
    /// feedback 保留策略。
    pub(crate) feedback: OperationFeedbackPolicy,
    /// result 保留毫秒。
    pub(crate) result_retention_ms: u64,
}

/// 为 graph 中所有 operation edge 生成 codegen 计划。
pub(crate) fn operation_runtime_plans(
    contract: &ContractIr,
    graph: &GraphIr,
) -> Vec<OperationRuntimePlan> {
    graph
        .operations
        .iter()
        .enumerate()
        .map(|(index, operation)| {
            let client_instance = &operation.client.instance.name;
            let client_port = &operation.client.port;
            let server_instance = &operation.server.instance.name;
            let server_port = &operation.server.port;
            let client_instance_ir = instance_by_name(graph, client_instance);
            let server_instance_ir = instance_by_name(graph, server_instance);
            let server_component = component_by_name(contract, &server_instance_ir.component.name);
            let server_port_ir = server_component
                .operation_servers
                .iter()
                .find(|port| port.name == *server_port)
                .expect("validated operation bind must reference existing server port");

            OperationRuntimePlan {
                index,
                operation_name: format!("{client_instance}.{client_port}"),
                client_instance: client_instance.clone(),
                client_component: client_instance_ir.component.name.clone(),
                client_port: client_port.clone(),
                server_instance: server_instance.clone(),
                server_component: server_instance_ir.component.name.clone(),
                server_port: server_port.clone(),
                goal_type: server_port_ir.goal.clone(),
                feedback_type: server_port_ir.feedback.clone(),
                result_type: server_port_ir.result.clone(),
                backend: operation.backend.clone(),
                timeout_ms: operation.policy.timeout_ms,
                concurrency: operation.policy.concurrency,
                preempt: operation.policy.preempt,
                queue_depth: operation.policy.queue_depth,
                max_in_flight: operation.policy.max_in_flight,
                feedback: operation.policy.feedback,
                result_retention_ms: operation.policy.result_retention_ms,
            }
        })
        .collect()
}

/// 获取 operation server 的 lane 名称。
pub(crate) fn operation_server_lane(plan: &OperationRuntimePlan) -> String {
    format!("{}_operation_serial", plan.server_instance)
}

/// 查找指定实例作为 client 的所有 operation plans。
pub(crate) fn client_operation_plans<'a>(
    plans: &'a [OperationRuntimePlan],
    instance_name: &str,
) -> Vec<&'a OperationRuntimePlan> {
    plans
        .iter()
        .filter(|plan| plan.client_instance == instance_name)
        .collect()
}

/// 查找指定实例作为 server 的所有 operation plans。
#[allow(dead_code)]
pub(crate) fn server_operation_plans<'a>(
    plans: &'a [OperationRuntimePlan],
    instance_name: &str,
) -> Vec<&'a OperationRuntimePlan> {
    plans
        .iter()
        .filter(|plan| plan.server_instance == instance_name)
        .collect()
}
