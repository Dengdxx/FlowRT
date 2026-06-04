use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CapabilityAtom, ChannelKind, ComponentIr, GraphIr, OverflowPolicy, PrimitiveType, StalePolicy,
    TriggerKind, TypeExpr, TypeIr,
};

// 枚举声明顺序是所有已知 capability 的全局 canonical 顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Capability {
    AbiFixedSizePlainData,
    AbiInt128,
    LayoutNativeLayout,
    AllocationBounded,
    GraphStaticGraph,
    TriggerPeriodic,
    TriggerOnMessage,
    TriggerStartup,
    TriggerShutdown,
    TimingDeadlineAware,
    ChannelLatest,
    ChannelFifo,
    OverflowDropOldest,
    OverflowDropNewest,
    OverflowError,
    OverflowBlock,
    StaleWarn,
    StaleDrop,
    StaleHoldLast,
    StaleError,
    TopologySingleProcess,
    TopologyMultiProcess,
    TopologySingleHost,
    TransferCopy,
    TransferZeroCopy,
    TransferLoaned,
    ObservabilityHealth,
}

impl Capability {
    fn atom(self) -> CapabilityAtom {
        CapabilityAtom(self.name().to_string())
    }

    fn name(self) -> &'static str {
        match self {
            Capability::AbiFixedSizePlainData => "abi:fixed_size_plain_data",
            Capability::AbiInt128 => "abi:int128",
            Capability::LayoutNativeLayout => "layout:native_layout",
            Capability::AllocationBounded => "allocation:bounded",
            Capability::GraphStaticGraph => "graph:static_graph",
            Capability::TriggerPeriodic => "trigger:periodic",
            Capability::TriggerOnMessage => "trigger:on_message",
            Capability::TriggerStartup => "trigger:startup",
            Capability::TriggerShutdown => "trigger:shutdown",
            Capability::TimingDeadlineAware => "timing:deadline_aware",
            Capability::ChannelLatest => "channel:latest",
            Capability::ChannelFifo => "channel:fifo",
            Capability::OverflowDropOldest => "overflow:drop_oldest",
            Capability::OverflowDropNewest => "overflow:drop_newest",
            Capability::OverflowError => "overflow:error",
            Capability::OverflowBlock => "overflow:block",
            Capability::StaleWarn => "stale:warn",
            Capability::StaleDrop => "stale:drop",
            Capability::StaleHoldLast => "stale:hold_last",
            Capability::StaleError => "stale:error",
            Capability::TopologySingleProcess => "topology:single_process",
            Capability::TopologyMultiProcess => "topology:multi_process",
            Capability::TopologySingleHost => "topology:single_host",
            Capability::TransferCopy => "transfer:copy",
            Capability::TransferZeroCopy => "transfer:zero_copy",
            Capability::TransferLoaned => "transfer:loaned",
            Capability::ObservabilityHealth => "observability:health",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendKind {
    Inproc,
    Iox2,
}

impl BackendKind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "inproc" => Some(Self::Inproc),
            "iox2" => Some(Self::Iox2),
            _ => None,
        }
    }

    fn spec(self) -> BackendSpec {
        match self {
            Self::Inproc => BackendSpec {
                capabilities: &[&COMMON_BACKEND_CAPABILITIES, INPROC_BACKEND_CAPABILITIES],
            },
            Self::Iox2 => BackendSpec {
                capabilities: &[&COMMON_BACKEND_CAPABILITIES, IOX2_BACKEND_CAPABILITIES],
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BackendSpec {
    capabilities: &'static [&'static [Capability]],
}

impl BackendSpec {
    fn capabilities(self) -> Vec<CapabilityAtom> {
        let mut capabilities = CapabilityList::new();
        self.extend_capabilities(&mut capabilities);
        capabilities.into_atoms()
    }

    fn extend_capabilities(self, capabilities: &mut CapabilityList) {
        for group in self.capabilities {
            capabilities.extend(group.iter().copied());
        }
    }
}

struct CapabilityList {
    items: BTreeSet<Capability>,
}

impl CapabilityList {
    fn new() -> Self {
        Self {
            items: BTreeSet::new(),
        }
    }

    fn push(&mut self, capability: Capability) {
        self.items.insert(capability);
    }

    fn extend(&mut self, capabilities: impl IntoIterator<Item = Capability>) {
        self.items.extend(capabilities);
    }

    fn extend_list(&mut self, capabilities: Self) {
        self.items.extend(capabilities.items);
    }

    fn into_atoms(self) -> Vec<CapabilityAtom> {
        self.items
            .into_iter()
            .map(Capability::atom)
            .collect::<Vec<_>>()
    }
}

const BASE_DEPLOYMENT_CAPABILITIES: [Capability; 4] = [
    Capability::AbiFixedSizePlainData,
    Capability::LayoutNativeLayout,
    Capability::AllocationBounded,
    Capability::GraphStaticGraph,
];

const COMMON_BACKEND_CAPABILITIES: [Capability; 19] = [
    Capability::AbiFixedSizePlainData,
    Capability::LayoutNativeLayout,
    Capability::AllocationBounded,
    Capability::GraphStaticGraph,
    Capability::TriggerPeriodic,
    Capability::TriggerOnMessage,
    Capability::TriggerStartup,
    Capability::TriggerShutdown,
    Capability::TimingDeadlineAware,
    Capability::ChannelLatest,
    Capability::ChannelFifo,
    Capability::OverflowDropOldest,
    Capability::OverflowDropNewest,
    Capability::OverflowError,
    Capability::OverflowBlock,
    Capability::StaleWarn,
    Capability::StaleDrop,
    Capability::StaleHoldLast,
    Capability::StaleError,
];

const INPROC_BACKEND_CAPABILITIES: &[Capability] = &[
    Capability::TopologySingleProcess,
    Capability::TransferCopy,
    Capability::ObservabilityHealth,
];

const IOX2_BACKEND_CAPABILITIES: &[Capability] = &[
    Capability::TopologyMultiProcess,
    Capability::TopologySingleHost,
    Capability::TransferZeroCopy,
    Capability::TransferLoaned,
    Capability::ObservabilityHealth,
];

/// 判断当前实现是否认识某个 backend 名称。
pub fn is_known_backend(name: &str) -> bool {
    BackendKind::parse(name).is_some()
}

/// 返回某个 backend 提供的 capability atoms。
pub fn backend_capabilities(name: &str) -> Option<Vec<CapabilityAtom>> {
    BackendKind::parse(name).map(|backend| backend.spec().capabilities())
}

/// deployment capability 决策结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentCapabilityDecision {
    /// selected backend 是否在内建 backend catalog 中已知。
    pub selected_backend_known: bool,
    /// target 声明的 backends 中是否包含 selected backend。
    pub target_supports_selected_backend: bool,
    /// selected backend 在 required capabilities 中缺失的能力，按 required 顺序返回。
    pub missing_required_capabilities: Vec<CapabilityAtom>,
    /// 该 deployment 是否满足 backend、target 和 capability 三方面约束。
    pub satisfied: bool,
}

/// 依据 selected backend、target 声明和 required capabilities 推导 deployment capability 决策。
///
/// unknown backend 会显式返回 `selected_backend_known = false`，并且不会把缺失能力伪装成
/// “已知 backend 但 capability 为空”。
pub fn deployment_capability_decision(
    selected_backend: &crate::BackendName,
    target_backends: &[crate::BackendName],
    required_capabilities: &[CapabilityAtom],
) -> DeploymentCapabilityDecision {
    let selected_backend_known = is_known_backend(&selected_backend.0);
    let target_supports_selected_backend = target_backends
        .iter()
        .any(|backend| backend.0 == selected_backend.0);
    let missing_required_capabilities = if selected_backend_known {
        let backend_capabilities = backend_capabilities(&selected_backend.0)
            .expect("known backend must resolve to capability catalog entry");
        required_capabilities
            .iter()
            .filter(|required| !backend_capabilities.contains(required))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let satisfied = selected_backend_known
        && target_supports_selected_backend
        && missing_required_capabilities.is_empty();

    DeploymentCapabilityDecision {
        selected_backend_known,
        target_supports_selected_backend,
        missing_required_capabilities,
        satisfied,
    }
}

/// v0.1 deployment 在 graph-specific policy 之外必须满足的基础能力。
pub fn base_deployment_capabilities() -> Vec<CapabilityAtom> {
    let mut capabilities = CapabilityList::new();
    capabilities.extend(BASE_DEPLOYMENT_CAPABILITIES);
    capabilities.into_atoms()
}

/// 依据 graph 的 process 边界推导 topology capability。
///
/// 只有当 bind 跨越不同 process group 时，deployment 才需要 `topology:multi_process`。
/// 单 process graph 不额外要求 topology capability，避免把 backend 选择和 graph 结构
/// 绑成一条静态路径。
fn graph_topology_capabilities(graph: &GraphIr) -> CapabilityList {
    let mut capabilities = CapabilityList::new();
    let processes = graph
        .instances
        .iter()
        .map(|instance| {
            (
                instance.name.as_str(),
                instance.process.as_deref().unwrap_or("main"),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let has_cross_process_bind = graph.binds.iter().any(|bind| {
        let from_process = processes
            .get(bind.from.instance.name.as_str())
            .copied()
            .unwrap_or("main");
        let to_process = processes
            .get(bind.to.instance.name.as_str())
            .copied()
            .unwrap_or("main");
        from_process != to_process
    });

    if has_cross_process_bind {
        capabilities.push(Capability::TopologyMultiProcess);
    }

    capabilities
}

/// 返回某个 task trigger 所需的 capability atom。
pub fn trigger_capability(trigger: TriggerKind) -> CapabilityAtom {
    trigger_capability_kind(trigger).atom()
}

fn trigger_capability_kind(trigger: TriggerKind) -> Capability {
    match trigger {
        TriggerKind::Periodic => Capability::TriggerPeriodic,
        TriggerKind::OnMessage => Capability::TriggerOnMessage,
        TriggerKind::Startup => Capability::TriggerStartup,
        TriggerKind::Shutdown => Capability::TriggerShutdown,
    }
}

/// 推导 target 声明的 backend capability atoms。
pub fn target_capabilities(backends: &[crate::BackendName]) -> Vec<CapabilityAtom> {
    let mut capabilities = CapabilityList::new();
    for backend in backends
        .iter()
        .filter_map(|backend| BackendKind::parse(&backend.0))
    {
        backend.spec().extend_capabilities(&mut capabilities);
    }
    capabilities.into_atoms()
}

/// 推导 data channel policy 需要的 capability atoms。
pub fn channel_capabilities(
    channel: ChannelKind,
    overflow: OverflowPolicy,
    stale: StalePolicy,
) -> Vec<CapabilityAtom> {
    let mut capabilities = CapabilityList::new();
    capabilities.extend([
        channel_capability(channel),
        overflow_capability(overflow),
        stale_capability(stale),
    ]);
    capabilities.into_atoms()
}

/// 推导 graph deployment 需要的 capability atoms。
///
/// `types` 用于解析全量生成消息 ABI 能力，`components` 用于解析 graph 可达 port
/// 的直接 primitive 类型；graph 内 task 与 bind 仍提供调度、deadline 和 channel policy 能力。
pub fn graph_required_capabilities(
    graph: &GraphIr,
    types: &[TypeIr],
    components: &[ComponentIr],
) -> Vec<CapabilityAtom> {
    let mut capabilities = CapabilityList::new();
    capabilities.extend(BASE_DEPLOYMENT_CAPABILITIES);
    capabilities.extend_list(graph_topology_capabilities(graph));
    let components_by_name = components
        .iter()
        .map(|component| (component.name.as_str(), component))
        .collect::<BTreeMap<_, _>>();

    capabilities.extend_list(message_abi_capability_list(
        types,
        graph
            .instances
            .iter()
            .filter_map(|instance| components_by_name.get(instance.component.name.as_str()))
            .copied(),
    ));
    for task in &graph.tasks {
        capabilities.push(trigger_capability_kind(task.trigger));
        if task.deadline_ms.is_some() {
            capabilities.push(Capability::TimingDeadlineAware);
        }
    }
    for bind in &graph.binds {
        capabilities.extend([
            channel_capability(bind.channel),
            overflow_capability(bind.overflow),
            stale_capability(bind.stale),
        ]);
    }
    capabilities.into_atoms()
}

/// 推导 Contract IR message 与 component port 类型需要的 ABI capability atoms。
pub fn message_abi_capabilities<'a>(
    types: &[TypeIr],
    components: impl IntoIterator<Item = &'a ComponentIr>,
) -> Vec<CapabilityAtom> {
    message_abi_capability_list(types, components).into_atoms()
}

fn message_abi_capability_list<'a>(
    types: &[TypeIr],
    components: impl IntoIterator<Item = &'a ComponentIr>,
) -> CapabilityList {
    let types_by_name = types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    let mut required = CapabilityList::new();

    for ty in types {
        for field in &ty.fields {
            collect_type_expr_abi_capabilities(&field.ty, &types_by_name, &mut required);
        }
    }
    for component in components {
        for port in component.inputs.iter().chain(component.outputs.iter()) {
            collect_type_expr_abi_capabilities(&port.ty, &types_by_name, &mut required);
        }
    }

    required
}

fn collect_type_expr_abi_capabilities(
    expr: &TypeExpr,
    types_by_name: &BTreeMap<&str, &TypeIr>,
    required: &mut CapabilityList,
) {
    let mut visiting = BTreeSet::new();
    collect_type_expr_abi_capabilities_inner(expr, types_by_name, required, &mut visiting);
}

fn collect_type_expr_abi_capabilities_inner(
    expr: &TypeExpr,
    types_by_name: &BTreeMap<&str, &TypeIr>,
    required: &mut CapabilityList,
    visiting: &mut BTreeSet<String>,
) {
    match expr {
        TypeExpr::Primitive {
            name: PrimitiveType::U128 | PrimitiveType::I128,
        } => required.push(Capability::AbiInt128),
        TypeExpr::Primitive { .. } | TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } => {}
        TypeExpr::Named { name } => {
            if !visiting.insert(name.clone()) {
                return;
            }
            if let Some(ty) = types_by_name.get(name.as_str()) {
                for field in &ty.fields {
                    collect_type_expr_abi_capabilities_inner(
                        &field.ty,
                        types_by_name,
                        required,
                        visiting,
                    );
                }
            }
            visiting.remove(name);
        }
        TypeExpr::Array { element, .. } | TypeExpr::VarSequence { element, .. } => {
            collect_type_expr_abi_capabilities_inner(element, types_by_name, required, visiting);
        }
    }
}

fn channel_capability(channel: ChannelKind) -> Capability {
    match channel {
        ChannelKind::Latest => Capability::ChannelLatest,
        ChannelKind::Fifo => Capability::ChannelFifo,
    }
}

fn overflow_capability(policy: OverflowPolicy) -> Capability {
    match policy {
        OverflowPolicy::DropOldest => Capability::OverflowDropOldest,
        OverflowPolicy::DropNewest => Capability::OverflowDropNewest,
        OverflowPolicy::Error => Capability::OverflowError,
        OverflowPolicy::Block => Capability::OverflowBlock,
    }
}

fn stale_capability(policy: StalePolicy) -> Capability {
    match policy {
        StalePolicy::Warn => Capability::StaleWarn,
        StalePolicy::Drop => Capability::StaleDrop,
        StalePolicy::HoldLast => Capability::StaleHoldLast,
        StalePolicy::Error => Capability::StaleError,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inproc_supports_core_v0_1_capabilities() {
        let capabilities = backend_capabilities("inproc").unwrap();
        assert!(capabilities.contains(&CapabilityAtom("channel:latest".to_string())));
        assert!(capabilities.contains(&CapabilityAtom("trigger:on_message".to_string())));
        assert!(capabilities.contains(&CapabilityAtom("layout:native_layout".to_string())));
    }

    #[test]
    fn rejects_unknown_backend_names() {
        assert!(!is_known_backend("typo_backend"));
        assert!(backend_capabilities("typo_backend").is_none());
    }

    #[test]
    fn backend_capabilities_are_unique_and_canonical() {
        let capabilities = backend_capabilities("iox2").unwrap();
        let unique = capabilities.iter().collect::<BTreeSet<_>>();

        assert_eq!(capabilities.len(), unique.len());
        assert_eq!(
            capabilities,
            vec![
                Capability::AbiFixedSizePlainData.atom(),
                Capability::LayoutNativeLayout.atom(),
                Capability::AllocationBounded.atom(),
                Capability::GraphStaticGraph.atom(),
                Capability::TriggerPeriodic.atom(),
                Capability::TriggerOnMessage.atom(),
                Capability::TriggerStartup.atom(),
                Capability::TriggerShutdown.atom(),
                Capability::TimingDeadlineAware.atom(),
                Capability::ChannelLatest.atom(),
                Capability::ChannelFifo.atom(),
                Capability::OverflowDropOldest.atom(),
                Capability::OverflowDropNewest.atom(),
                Capability::OverflowError.atom(),
                Capability::OverflowBlock.atom(),
                Capability::StaleWarn.atom(),
                Capability::StaleDrop.atom(),
                Capability::StaleHoldLast.atom(),
                Capability::StaleError.atom(),
                Capability::TopologyMultiProcess.atom(),
                Capability::TopologySingleHost.atom(),
                Capability::TransferZeroCopy.atom(),
                Capability::TransferLoaned.atom(),
                Capability::ObservabilityHealth.atom(),
            ]
        );
    }

    #[test]
    fn target_capabilities_use_global_catalog_order() {
        let capabilities = target_capabilities(&[
            crate::BackendName("iox2".to_string()),
            crate::BackendName("inproc".to_string()),
        ]);
        let capabilities_from_reversed_backends = target_capabilities(&[
            crate::BackendName("inproc".to_string()),
            crate::BackendName("iox2".to_string()),
        ]);

        assert_eq!(capabilities, capabilities_from_reversed_backends);
        assert_eq!(
            capabilities,
            vec![
                Capability::AbiFixedSizePlainData.atom(),
                Capability::LayoutNativeLayout.atom(),
                Capability::AllocationBounded.atom(),
                Capability::GraphStaticGraph.atom(),
                Capability::TriggerPeriodic.atom(),
                Capability::TriggerOnMessage.atom(),
                Capability::TriggerStartup.atom(),
                Capability::TriggerShutdown.atom(),
                Capability::TimingDeadlineAware.atom(),
                Capability::ChannelLatest.atom(),
                Capability::ChannelFifo.atom(),
                Capability::OverflowDropOldest.atom(),
                Capability::OverflowDropNewest.atom(),
                Capability::OverflowError.atom(),
                Capability::OverflowBlock.atom(),
                Capability::StaleWarn.atom(),
                Capability::StaleDrop.atom(),
                Capability::StaleHoldLast.atom(),
                Capability::StaleError.atom(),
                Capability::TopologySingleProcess.atom(),
                Capability::TopologyMultiProcess.atom(),
                Capability::TopologySingleHost.atom(),
                Capability::TransferCopy.atom(),
                Capability::TransferZeroCopy.atom(),
                Capability::TransferLoaned.atom(),
                Capability::ObservabilityHealth.atom(),
            ]
        );
    }

    #[test]
    fn deployment_capability_decision_exposes_known_target_support_and_missing_caps() {
        let ready = deployment_capability_decision(
            &crate::BackendName("inproc".to_string()),
            &[crate::BackendName("inproc".to_string())],
            &[
                Capability::AbiFixedSizePlainData.atom(),
                Capability::LayoutNativeLayout.atom(),
            ],
        );
        assert!(ready.selected_backend_known);
        assert!(ready.target_supports_selected_backend);
        assert!(ready.missing_required_capabilities.is_empty());
        assert!(ready.satisfied);

        let unsupported_target = deployment_capability_decision(
            &crate::BackendName("inproc".to_string()),
            &[crate::BackendName("iox2".to_string())],
            &[
                Capability::AbiInt128.atom(),
                Capability::TransferZeroCopy.atom(),
                Capability::TransferLoaned.atom(),
            ],
        );
        assert!(unsupported_target.selected_backend_known);
        assert!(!unsupported_target.target_supports_selected_backend);
        assert_eq!(
            unsupported_target.missing_required_capabilities,
            vec![
                Capability::AbiInt128.atom(),
                Capability::TransferZeroCopy.atom(),
                Capability::TransferLoaned.atom(),
            ]
        );
        assert!(!unsupported_target.satisfied);

        let missing = deployment_capability_decision(
            &crate::BackendName("inproc".to_string()),
            &[crate::BackendName("inproc".to_string())],
            &[
                Capability::AbiInt128.atom(),
                Capability::TransferZeroCopy.atom(),
                Capability::TransferLoaned.atom(),
            ],
        );
        assert!(missing.selected_backend_known);
        assert!(missing.target_supports_selected_backend);
        assert_eq!(
            missing.missing_required_capabilities,
            vec![
                Capability::AbiInt128.atom(),
                Capability::TransferZeroCopy.atom(),
                Capability::TransferLoaned.atom(),
            ]
        );
        assert!(!missing.satisfied);

        let unknown = deployment_capability_decision(
            &crate::BackendName("typo_backend".to_string()),
            &[crate::BackendName("typo_backend".to_string())],
            &[
                Capability::AbiInt128.atom(),
                Capability::TransferZeroCopy.atom(),
                Capability::TransferLoaned.atom(),
            ],
        );
        assert!(!unknown.selected_backend_known);
        assert!(unknown.target_supports_selected_backend);
        assert!(unknown.missing_required_capabilities.is_empty());
        assert!(!unknown.satisfied);
    }

    #[test]
    fn graph_required_capabilities_use_global_catalog_order() {
        let instance = crate::EntityRef {
            id: crate::EntityId("instance_0000000000000001".to_string()),
            name: "worker".to_string(),
        };
        let port = |name: &str| crate::PortRef {
            instance: instance.clone(),
            port: name.to_string(),
        };
        let policy_source = crate::ChannelPolicySourceIr {
            overflow: crate::PolicyValueSource::Explicit,
            stale: crate::PolicyValueSource::Explicit,
            max_age_ms: crate::PolicyValueSource::Explicit,
        };
        let mut graph = GraphIr {
            id: crate::EntityId("graph_0000000000000001".to_string()),
            name: "default".to_string(),
            instances: vec![],
            tasks: vec![
                crate::TaskIr {
                    id: crate::EntityId("task_0000000000000001".to_string()),
                    instance: instance.clone(),
                    trigger: TriggerKind::Shutdown,
                    period_ms: None,
                    deadline_ms: Some(10),
                    priority: None,
                    inputs: vec![],
                    outputs: vec![],
                },
                crate::TaskIr {
                    id: crate::EntityId("task_0000000000000002".to_string()),
                    instance: instance.clone(),
                    trigger: TriggerKind::Periodic,
                    period_ms: Some(10),
                    deadline_ms: None,
                    priority: None,
                    inputs: vec![],
                    outputs: vec![],
                },
            ],
            binds: vec![
                crate::ChannelEdgeIr {
                    id: crate::EntityId("bind_0000000000000001".to_string()),
                    from: port("fifo_out"),
                    to: port("fifo_in"),
                    channel: ChannelKind::Fifo,
                    depth: Some(2),
                    overflow: OverflowPolicy::Block,
                    stale: StalePolicy::Error,
                    max_age_ms: Some(10),
                    policy_source,
                    capability_requirements: vec![],
                },
                crate::ChannelEdgeIr {
                    id: crate::EntityId("bind_0000000000000002".to_string()),
                    from: port("latest_out"),
                    to: port("latest_in"),
                    channel: ChannelKind::Latest,
                    depth: Some(1),
                    overflow: OverflowPolicy::DropNewest,
                    stale: StalePolicy::HoldLast,
                    max_age_ms: Some(10),
                    policy_source,
                    capability_requirements: vec![],
                },
            ],
        };
        let types = [TypeIr {
            id: crate::EntityId("type_0000000000000001".to_string()),
            name: "WideIntegers".to_string(),
            fields: vec![
                crate::FieldIr {
                    name: "signed".to_string(),
                    ty: TypeExpr::Primitive {
                        name: PrimitiveType::I128,
                    },
                    default: None,
                },
                crate::FieldIr {
                    name: "unsigned".to_string(),
                    ty: TypeExpr::Primitive {
                        name: PrimitiveType::U128,
                    },
                    default: None,
                },
            ],
        }];

        let capabilities = graph_required_capabilities(&graph, &types, &[]);
        graph.tasks.reverse();
        graph.binds.reverse();

        assert_eq!(
            capabilities,
            graph_required_capabilities(&graph, &types, &[])
        );
        assert_eq!(
            capabilities,
            vec![
                Capability::AbiFixedSizePlainData.atom(),
                Capability::AbiInt128.atom(),
                Capability::LayoutNativeLayout.atom(),
                Capability::AllocationBounded.atom(),
                Capability::GraphStaticGraph.atom(),
                Capability::TriggerPeriodic.atom(),
                Capability::TriggerShutdown.atom(),
                Capability::TimingDeadlineAware.atom(),
                Capability::ChannelLatest.atom(),
                Capability::ChannelFifo.atom(),
                Capability::OverflowDropNewest.atom(),
                Capability::OverflowBlock.atom(),
                Capability::StaleHoldLast.atom(),
                Capability::StaleError.atom(),
            ]
        );
    }

    #[test]
    fn graph_required_capabilities_require_multi_process_for_cross_process_binds() {
        let source_component = crate::ComponentIr {
            id: crate::EntityId("component_0000000000000001".to_string()),
            name: "source".to_string(),
            language: crate::LanguageKind::Rust,
            kind: crate::ComponentKind::Native,
            inputs: vec![],
            outputs: vec![crate::PortIr {
                name: "sample".to_string(),
                ty: TypeExpr::Primitive {
                    name: PrimitiveType::U32,
                },
            }],
            params: vec![],
            lifecycle: crate::LifecycleSurface::reserved_v0_1(),
        };
        let sink_component = crate::ComponentIr {
            id: crate::EntityId("component_0000000000000002".to_string()),
            name: "sink".to_string(),
            language: crate::LanguageKind::Rust,
            kind: crate::ComponentKind::Native,
            inputs: vec![crate::PortIr {
                name: "sample".to_string(),
                ty: TypeExpr::Primitive {
                    name: PrimitiveType::U32,
                },
            }],
            outputs: vec![],
            params: vec![],
            lifecycle: crate::LifecycleSurface::reserved_v0_1(),
        };
        let source = crate::EntityRef {
            id: crate::EntityId("instance_0000000000000001".to_string()),
            name: "source".to_string(),
        };
        let sink = crate::EntityRef {
            id: crate::EntityId("instance_0000000000000002".to_string()),
            name: "sink".to_string(),
        };
        let graph = GraphIr {
            id: crate::EntityId("graph_0000000000000001".to_string()),
            name: "default".to_string(),
            instances: vec![
                crate::InstanceIr {
                    id: source.id.clone(),
                    name: source.name.clone(),
                    component: crate::EntityRef {
                        id: source_component.id.clone(),
                        name: source_component.name.clone(),
                    },
                    params: vec![],
                    process: Some("producer".to_string()),
                    target: None,
                },
                crate::InstanceIr {
                    id: sink.id.clone(),
                    name: sink.name.clone(),
                    component: crate::EntityRef {
                        id: sink_component.id.clone(),
                        name: sink_component.name.clone(),
                    },
                    params: vec![],
                    process: Some("consumer".to_string()),
                    target: None,
                },
            ],
            tasks: vec![],
            binds: vec![crate::ChannelEdgeIr {
                id: crate::EntityId("bind_0000000000000001".to_string()),
                from: crate::PortRef {
                    instance: source.clone(),
                    port: "sample".to_string(),
                },
                to: crate::PortRef {
                    instance: sink.clone(),
                    port: "sample".to_string(),
                },
                channel: ChannelKind::Latest,
                depth: Some(1),
                overflow: OverflowPolicy::DropOldest,
                stale: StalePolicy::Warn,
                max_age_ms: None,
                policy_source: crate::ChannelPolicySourceIr {
                    overflow: crate::PolicyValueSource::Explicit,
                    stale: crate::PolicyValueSource::Explicit,
                    max_age_ms: crate::PolicyValueSource::Explicit,
                },
                capability_requirements: vec![],
            }],
        };

        let capabilities = graph_required_capabilities(
            &graph,
            &[],
            &[source_component.clone(), sink_component.clone()],
        );

        assert!(capabilities.contains(&CapabilityAtom("topology:multi_process".to_string())));
        assert!(!capabilities.contains(&CapabilityAtom("topology:single_process".to_string())));
    }

    #[test]
    fn message_abi_capabilities_include_nested_int128_usage() {
        let types = vec![TypeIr {
            id: crate::EntityId("type_0000000000000001".to_string()),
            name: "Nested".to_string(),
            fields: vec![crate::FieldIr {
                name: "value".to_string(),
                ty: TypeExpr::Primitive {
                    name: PrimitiveType::I128,
                },
                default: None,
            }],
        }];
        let components = [ComponentIr {
            id: crate::EntityId("component_0000000000000001".to_string()),
            name: "producer".to_string(),
            language: crate::LanguageKind::Rust,
            kind: crate::ComponentKind::Native,
            inputs: vec![],
            outputs: vec![crate::PortIr {
                name: "sample".to_string(),
                ty: TypeExpr::Array {
                    element: Box::new(TypeExpr::Named {
                        name: "Nested".to_string(),
                    }),
                    len: 2,
                },
            }],
            params: vec![],
            lifecycle: crate::LifecycleSurface::reserved_v0_1(),
        }];

        assert_eq!(
            message_abi_capabilities(&types, components.iter()),
            vec![CapabilityAtom("abi:int128".to_string())]
        );
    }
}
