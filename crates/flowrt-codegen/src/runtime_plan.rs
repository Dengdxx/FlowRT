use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    BackendName, BoundaryDirection, ChannelBackendSource, ChannelKind, ContractIr,
    FaultInjectionPointIr, GraphIr, InstanceFailurePolicy, InstanceIr, InstanceRestartParamsIr,
    OperationConcurrencyPolicy, OperationFeedbackPolicy, OperationPreemptPolicy,
    OverflowPolicy as IrOverflowPolicy, ParamIr, ParamValue, Ros2BridgeDirection, Ros2BridgeIr,
    ServiceOverflowPolicy, StalePolicy as IrStalePolicy, TaskConcurrency, TaskIr, TaskReadiness,
    TriggerKind, TypeExpr,
    derived::{ContractDerivedFacts, GraphDerivedFacts, derive_contract_facts},
};

use crate::{
    component_by_name, fixed_message_abi_expectations, instance_by_name, port_by_name,
    rust_wire_size, scheduler_tasks_for_order, selected_profile_worker_threads, snake_identifier,
    type_contains_variable_data,
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
                matches!(
                    trigger,
                    TriggerKind::Periodic | TriggerKind::OnMessage | TriggerKind::OnSynchronized
                )
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
pub(crate) struct SchedulerRuntimePlan<'a> {
    pub(crate) worker_threads: u32,
    pub(crate) lanes: Vec<SchedulerLanePlan>,
    pub(crate) dataflow_tasks: Vec<SchedulerDataflowTaskPlan<'a>>,
    pub(crate) hidden_tasks: Vec<SchedulerHiddenTaskPlan>,
    pub(crate) scheduler_base_period_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchedulerLanePlan {
    pub(crate) id: usize,
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SchedulerDataflowTaskPlan<'a> {
    pub(crate) id: usize,
    pub(crate) task: &'a TaskIr,
    pub(crate) timing_name: String,
    pub(crate) lane: String,
    pub(crate) lane_id: usize,
    pub(crate) priority: u32,
    pub(crate) deadline_ms: Option<u64>,
    pub(crate) period_ms: Option<u64>,
    pub(crate) periodic_wake: bool,
    pub(crate) trigger: TriggerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchedulerHiddenTaskKind {
    Service,
    Operation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SchedulerHiddenTaskPlan {
    pub(crate) id: usize,
    pub(crate) kind: SchedulerHiddenTaskKind,
    pub(crate) source_index: usize,
    pub(crate) source_name: String,
    pub(crate) name: String,
    pub(crate) lane: String,
    pub(crate) lane_id: usize,
    pub(crate) priority: u32,
}

pub(crate) fn scheduler_runtime_plan<'a>(
    contract: &ContractIr,
    graph: &'a GraphIr,
    order: &[&'a InstanceIr],
) -> SchedulerRuntimePlan<'a> {
    let mut lanes = Vec::new();
    let mut lane_ids = BTreeMap::<String, usize>::new();
    let tasks = scheduler_tasks_for_order(graph, order);
    let dataflow_tasks = tasks
        .iter()
        .enumerate()
        .map(|(index, &task)| {
            let lane = resolved_task_lane_name(task);
            let lane_id = scheduler_lane_id(&mut lanes, &mut lane_ids, lane.clone());
            SchedulerDataflowTaskPlan {
                id: index + 1,
                task,
                timing_name: scheduler_task_timing_name(task),
                lane,
                lane_id,
                priority: task.priority.unwrap_or(0),
                deadline_ms: task.deadline_ms,
                period_ms: task.period_ms,
                periodic_wake: task.trigger == TriggerKind::Periodic,
                trigger: task.trigger,
            }
        })
        .collect::<Vec<_>>();

    let scheduler_base_period_ms = dataflow_tasks
        .iter()
        .filter(|task| task.trigger == TriggerKind::Periodic)
        .filter_map(|task| task.period_ms)
        .min()
        .unwrap_or(1);

    let mut hidden_tasks = Vec::new();
    let mut next_task_id = dataflow_tasks.len();
    for plan in service_runtime_plans(contract, graph) {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let lane = service_server_lane(&plan);
        let lane_id = scheduler_lane_id(&mut lanes, &mut lane_ids, lane.clone());
        next_task_id += 1;
        hidden_tasks.push(SchedulerHiddenTaskPlan {
            id: next_task_id,
            kind: SchedulerHiddenTaskKind::Service,
            source_index: plan.index,
            source_name: plan.service_name.clone(),
            name: format!("__flowrt_service.{}", plan.service_name),
            lane,
            lane_id,
            priority: 0,
        });
    }
    for plan in operation_runtime_plans(contract, graph) {
        if plan.backend.0 == "zenoh" {
            continue;
        }
        let lane = operation_server_lane(&plan);
        let lane_id = scheduler_lane_id(&mut lanes, &mut lane_ids, lane.clone());
        next_task_id += 1;
        hidden_tasks.push(SchedulerHiddenTaskPlan {
            id: next_task_id,
            kind: SchedulerHiddenTaskKind::Operation,
            source_index: plan.index,
            source_name: plan.operation_name.clone(),
            name: format!("__flowrt_operation.{}", plan.operation_name),
            lane,
            lane_id,
            priority: 0,
        });
    }

    SchedulerRuntimePlan {
        worker_threads: selected_profile_worker_threads(contract),
        lanes,
        dataflow_tasks,
        hidden_tasks,
        scheduler_base_period_ms,
    }
}

/// 一个 isolate/restart/degrade instance 的运行时容错计划：策略、重启参数及其全部 dataflow task id。
///
/// fail_fast instance 不收录。task_ids 是 scheduler dataflow task 的稳定 id（与
/// `scheduler_runtime_plan` 一致）：isolate/restart 隔离时 `suspend_task`、重启成功时
/// `resume_task`；degrade 不挂起 task，仅用 task_ids 匹配错误/恢复以翻转 `Degraded`/`Running`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecoverableInstancePlan {
    pub(crate) name: String,
    pub(crate) policy: InstanceFailurePolicy,
    pub(crate) restart: Option<InstanceRestartParamsIr>,
    pub(crate) task_ids: Vec<usize>,
}

/// 收集本进程 order 内 policy ∈ {isolate, restart, degrade} 的 instance 及其 dataflow task id。
///
/// 返回顺序按 instance 名称稳定排序，保证生成 shell 输出确定。fail_fast-only 图返回空 vec，
/// 生成 shell 据此完全不 emit 容错机制（保既有 golden 不漂移）。
pub(crate) fn recoverable_instances(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
) -> Vec<RecoverableInstancePlan> {
    let plan = scheduler_runtime_plan(contract, graph, order);
    let mut by_name: BTreeMap<String, RecoverableInstancePlan> = BTreeMap::new();
    for instance in order {
        if !matches!(
            instance.fault.policy,
            InstanceFailurePolicy::Isolate
                | InstanceFailurePolicy::Restart
                | InstanceFailurePolicy::Degrade
        ) {
            continue;
        }
        by_name.insert(
            instance.name.clone(),
            RecoverableInstancePlan {
                name: instance.name.clone(),
                policy: instance.fault.policy,
                restart: instance.fault.restart,
                task_ids: Vec::new(),
            },
        );
    }
    for task in &plan.dataflow_tasks {
        if let Some(entry) = by_name.get_mut(task.task.instance.name.as_str()) {
            entry.task_ids.push(task.id);
        }
    }
    by_name.into_values().collect()
}

fn scheduler_lane_id(
    lanes: &mut Vec<SchedulerLanePlan>,
    lane_ids: &mut BTreeMap<String, usize>,
    lane: String,
) -> usize {
    if let Some(id) = lane_ids.get(&lane) {
        return *id;
    }
    let id = lane_ids.len() + 1;
    lane_ids.insert(lane.clone(), id);
    lanes.push(SchedulerLanePlan { id, name: lane });
    id
}

fn scheduler_task_timing_name(task: &TaskIr) -> String {
    format!("{}.{}", task.instance.name, task.name)
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
    pub(crate) feedback: bool,
    /// 反馈边初值（源消息字面量 `ParamValue::Table`）。`None` 表示零初值播种。
    pub(crate) init: Option<ParamValue>,
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

/// 若 boundary 消息类型声明了 sample-time 源，返回（stamp 字段名, unit→ns 乘子）。
///
/// 仅命名消息类型可声明 timestamp 源；primitive/array/variable 表达式无源。Rust/C++ 生成 shell
/// 共用此解析，为该 boundary 注册 typed sample-time 提取器，回放时按 sensor 采集时刻步进。
pub(crate) fn boundary_sample_time_source(
    contract: &ContractIr,
    ty: &TypeExpr,
) -> Option<(String, u64)> {
    let TypeExpr::Named { name } = ty else {
        return None;
    };
    let source = crate::type_by_name(contract, name).timestamp.as_ref()?;
    let unit_to_ns = match source.unit {
        flowrt_ir::TimestampUnit::Ns => 1u64,
        flowrt_ir::TimestampUnit::Us => 1_000,
        flowrt_ir::TimestampUnit::Ms => 1_000_000,
    };
    Some((source.field.clone(), unit_to_ns))
}

/// 返回 `on_synchronized` task 引用的 sync 组（其余 trigger 返回 None）。
pub(crate) fn sync_group_for_task<'a>(
    graph: &'a GraphIr,
    task: &TaskIr,
) -> Option<&'a flowrt_ir::SyncGroupIr> {
    let group_ref = task.sync_group.as_ref()?;
    graph
        .sync_groups
        .iter()
        .find(|group| group.id == group_ref.id)
}

/// task 的有效输入端口列表：`on_synchronized` 取自所属 sync 组，其余取自 `task.inputs`。
///
/// codegen 的 wake/snapshot/revision/参数等机制统一面向有效输入，使 on_synchronized
/// 复用 on_message 的输入机器，差异仅在 task body 的 synchronizer gate。
pub(crate) fn effective_task_inputs(graph: &GraphIr, task: &TaskIr) -> Vec<String> {
    if task.trigger == TriggerKind::OnSynchronized {
        return sync_group_for_task(graph, task)
            .map(|group| group.inputs.clone())
            .unwrap_or_default();
    }
    task.inputs.clone()
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
                suffix = format!("{base}_{index}");
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
    let facts = validated_contract_derived_facts(contract);
    let graph_facts = graph_derived_facts(&facts, graph);
    bind_runtime_plans_from_facts(contract, graph, graph_facts)
}

fn bind_runtime_plans_from_facts(
    contract: &ContractIr,
    graph: &GraphIr,
    graph_facts: &GraphDerivedFacts,
) -> Vec<BindRuntimePlan> {
    let route_facts = graph_facts
        .routes
        .iter()
        .map(|route| (route.bind_id.0.as_str(), route))
        .collect::<BTreeMap<_, _>>();
    graph
        .binds
        .iter()
        .enumerate()
        .map(|(index, bind)| {
            let route = route_facts
                .get(bind.id.0.as_str())
                .copied()
                .expect("derived route facts must contain every validated bind");
            let source_instance = instance_by_name(graph, &bind.from.instance.name);
            let source_component = component_by_name(contract, &source_instance.component.name);
            let source_port = port_by_name(&source_component.outputs, &bind.from.port);
            BindRuntimePlan {
                index,
                field_name: format!("bind_{index}"),
                probe_field_name: format!("introspection_probe_bind_{index}"),
                channel: bind.channel,
                backend: route.backend.clone(),
                backend_source: route.backend_source,
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
                feedback: bind.feedback,
                init: bind.init.clone(),
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

/// channel overflow policy 的 runtime 枚举路径。`flowrt::OverflowPolicy::X` 在 Rust 与 C++ 生成
/// 代码中拼写一致，故两语言 shell 共用本映射，避免 enum→string 孪生漂移。
pub(crate) fn runtime_overflow_policy_path(policy: IrOverflowPolicy) -> &'static str {
    match policy {
        IrOverflowPolicy::DropOldest => "flowrt::OverflowPolicy::DropOldest",
        IrOverflowPolicy::DropNewest => "flowrt::OverflowPolicy::DropNewest",
        IrOverflowPolicy::Error => "flowrt::OverflowPolicy::Error",
        IrOverflowPolicy::Block => "flowrt::OverflowPolicy::Block",
    }
}

/// channel stale policy 的 runtime 枚举路径，两语言 shell 共用，理由同上。
pub(crate) fn runtime_stale_policy_path(policy: IrStalePolicy) -> &'static str {
    match policy {
        IrStalePolicy::Warn => "flowrt::StalePolicy::Warn",
        IrStalePolicy::Drop => "flowrt::StalePolicy::Drop",
        IrStalePolicy::HoldLast => "flowrt::StalePolicy::HoldLast",
        IrStalePolicy::Error => "flowrt::StalePolicy::Error",
    }
}

/// task trigger 的诊断名（launch/selfdesc 等语言无关产物），两语言 shell 共用同一拼写。
pub(crate) fn runtime_trigger_name(trigger: TriggerKind) -> &'static str {
    match trigger {
        TriggerKind::Periodic => "periodic",
        TriggerKind::OnMessage => "on_message",
        TriggerKind::Startup => "startup",
        TriggerKind::Shutdown => "shutdown",
        TriggerKind::OnSynchronized => "on_synchronized",
    }
}

/// 查找命中给定 task 的 test-only 故障注入点（按 EntityId 匹配），无注入或不命中返回 None。
///
/// 两语言 codegen 共用：注入门只在 `artifact.fault_injection` 存在且命中本 task 时生成。
pub(crate) fn fault_injection_point_for<'a>(
    contract: &'a ContractIr,
    task: &TaskIr,
) -> Option<&'a FaultInjectionPointIr> {
    contract
        .artifact
        .fault_injection
        .as_ref()?
        .points
        .iter()
        .find(|point| point.task.id == task.id)
}

pub(crate) fn contract_uses_backend(contract: &ContractIr, backend: &str) -> bool {
    let facts = validated_contract_derived_facts(contract);
    contract_uses_backend_with_facts(contract, &facts, backend)
}

fn contract_uses_backend_with_facts(
    contract: &ContractIr,
    facts: &ContractDerivedFacts,
    backend: &str,
) -> bool {
    contract
        .profiles
        .iter()
        .any(|profile| profile.backend.0 == backend)
        || facts
            .deployments
            .iter()
            .any(|deployment| deployment.backend.0 == backend)
        || facts
            .graphs
            .iter()
            .flat_map(|graph| &graph.routes)
            .any(|route| route.backend.0 == backend)
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
    let facts = validated_contract_derived_facts(contract);
    if contract_uses_backend_with_facts(contract, &facts, "iox2") {
        features.push("iox2");
    }
    if contract_uses_backend_with_facts(contract, &facts, "zenoh")
        || contract_has_params_for_language(contract, flowrt_ir::LanguageKind::Rust)
    {
        features.push("zenoh");
    }
    features
}

pub(crate) fn contract_derived_facts(
    contract: &ContractIr,
) -> flowrt_ir::Result<ContractDerivedFacts> {
    derive_contract_facts(contract)
}

pub(crate) fn validated_contract_derived_facts(contract: &ContractIr) -> ContractDerivedFacts {
    contract_derived_facts(contract).expect("validated Contract IR derived facts should recompute")
}

pub(crate) fn graph_derived_facts<'a>(
    facts: &'a ContractDerivedFacts,
    graph: &GraphIr,
) -> &'a GraphDerivedFacts {
    facts
        .graphs
        .iter()
        .find(|graph_facts| graph_facts.graph.id == graph.id)
        .or_else(|| {
            facts
                .graphs
                .iter()
                .find(|graph_facts| graph_facts.graph.name == graph.name)
        })
        .expect("derived facts must contain every graph")
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
