use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CapabilityAtom, ChannelKind, ComponentIr, GraphIr, OverflowPolicy, PrimitiveType, StalePolicy,
    TriggerKind, TypeExpr, TypeIr,
};

/// 单条 dataflow route 的拓扑边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteTopology {
    pub crosses_process: bool,
    pub crosses_target: bool,
}

impl RouteTopology {
    /// 构造本进程、本目标内的 route 拓扑。
    pub const fn local() -> Self {
        Self {
            crosses_process: false,
            crosses_target: false,
        }
    }

    /// 构造显式 route 拓扑。
    pub const fn new(crosses_process: bool, crosses_target: bool) -> Self {
        Self {
            crosses_process,
            crosses_target,
        }
    }
}

// 枚举声明顺序是所有已知 capability 的全局 canonical 顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Capability {
    AbiFixedSizePlainData,
    AbiInt128,
    AbiVariablePayloadFrame,
    LayoutNativeLayout,
    AllocationBounded,
    AllocationUnboundedDynamic,
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
    TopologyMultiHost,
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
            Capability::AbiVariablePayloadFrame => "abi:variable_payload_frame",
            Capability::LayoutNativeLayout => "layout:native_layout",
            Capability::AllocationBounded => "allocation:bounded",
            Capability::AllocationUnboundedDynamic => "allocation:unbounded_dynamic",
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
            Capability::TopologyMultiHost => "topology:multi_host",
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
    Zenoh,
}

impl BackendKind {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "inproc" => Some(Self::Inproc),
            "iox2" => Some(Self::Iox2),
            "zenoh" => Some(Self::Zenoh),
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
            Self::Zenoh => BackendSpec {
                capabilities: &[&COMMON_BACKEND_CAPABILITIES, ZENOH_BACKEND_CAPABILITIES],
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

const COMMON_BACKEND_CAPABILITIES: [Capability; 16] = [
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
    Capability::StaleWarn,
    Capability::StaleDrop,
    Capability::StaleHoldLast,
    Capability::StaleError,
];

const INPROC_BACKEND_CAPABILITIES: &[Capability] = &[
    Capability::AbiVariablePayloadFrame,
    Capability::AllocationUnboundedDynamic,
    Capability::OverflowDropNewest,
    Capability::OverflowError,
    Capability::OverflowBlock,
    Capability::TopologySingleProcess,
    Capability::TransferCopy,
    Capability::ObservabilityHealth,
];

const IOX2_BACKEND_CAPABILITIES: &[Capability] = &[
    Capability::OverflowDropNewest,
    Capability::OverflowError,
    Capability::OverflowBlock,
    Capability::TopologyMultiProcess,
    Capability::TopologySingleHost,
    Capability::TransferZeroCopy,
    Capability::TransferLoaned,
    Capability::ObservabilityHealth,
];

const ZENOH_BACKEND_CAPABILITIES: &[Capability] = &[
    Capability::AbiVariablePayloadFrame,
    Capability::AllocationUnboundedDynamic,
    Capability::TopologyMultiProcess,
    Capability::TopologyMultiHost,
    Capability::TransferCopy,
    Capability::ObservabilityHealth,
];

/// 判断当前实现是否认识某个 backend 名称。
pub fn is_known_backend(name: &str) -> bool {
    BackendKind::parse(name).is_some()
}

/// service 默认超时时间（毫秒）。
pub const SERVICE_DEFAULT_TIMEOUT_MS: u64 = 5000;

/// service 默认队列深度。
pub const SERVICE_DEFAULT_QUEUE_DEPTH: u32 = 32;

/// service 默认最大并发请求数。
pub const SERVICE_DEFAULT_MAX_IN_FLIGHT: u32 = 64;

/// 判断某个 backend 名称是否可作为 service backend 使用。
///
/// service backend 只支持 `inproc` 和 `zenoh`，不支持 `iox2`。
pub fn is_known_service_backend(name: &str) -> bool {
    matches!(name, "inproc" | "zenoh")
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

/// 依据单条 route 的 process/target 边界推导 topology capability。
fn route_topology_capabilities(topology: RouteTopology) -> CapabilityList {
    let mut capabilities = CapabilityList::new();
    if topology.crosses_process || topology.crosses_target {
        capabilities.push(Capability::TopologyMultiProcess);
    }
    if topology.crosses_target {
        capabilities.push(Capability::TopologyMultiHost);
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

/// 推导单条 route 的 backend capability requirements。
///
/// route backend 必须同时满足消息 ABI 额外能力和 channel policy；deployment backend
/// 只负责 profile 的调度与进程拓扑，不再为每条消息承担 ABI 能力。
pub fn channel_route_capabilities(
    types: &[TypeIr],
    source_type: &TypeExpr,
    channel: ChannelKind,
    overflow: OverflowPolicy,
    stale: StalePolicy,
    topology: RouteTopology,
) -> Vec<CapabilityAtom> {
    let mut capabilities = CapabilityList::new();
    capabilities.extend_list(type_expr_abi_capability_list(types, source_type));
    capabilities.extend([
        channel_capability(channel),
        overflow_capability(overflow),
        stale_capability(stale),
    ]);
    capabilities.extend_list(route_topology_capabilities(topology));
    capabilities.into_atoms()
}

/// 推导 graph deployment 需要的 capability atoms。
///
/// 消息 ABI、channel policy 和 dataflow topology 由 `ChannelEdgeIr.capability_requirements`
/// 绑定到单条 route backend；deployment 只承担 profile backend 的调度和基础 graph 能力。
pub fn graph_required_capabilities(
    graph: &GraphIr,
    _types: &[TypeIr],
    _components: &[ComponentIr],
) -> Vec<CapabilityAtom> {
    let mut capabilities = CapabilityList::new();
    capabilities.extend(BASE_DEPLOYMENT_CAPABILITIES);
    for task in &graph.tasks {
        capabilities.push(trigger_capability_kind(task.trigger));
        if task.deadline_ms.is_some() {
            capabilities.push(Capability::TimingDeadlineAware);
        }
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
        .map(|ty| (ty.qualified_name.as_str(), ty))
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

fn type_expr_abi_capability_list(types: &[TypeIr], expr: &TypeExpr) -> CapabilityList {
    let types_by_name = types
        .iter()
        .map(|ty| (ty.qualified_name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    let mut required = CapabilityList::new();
    let mut visiting = BTreeSet::new();
    collect_type_expr_abi_capabilities_inner(expr, &types_by_name, &mut required, &mut visiting);
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
        TypeExpr::Primitive { .. } => {}
        TypeExpr::VarBytes | TypeExpr::VarString { .. } => {
            required.extend([
                Capability::AbiVariablePayloadFrame,
                Capability::AllocationUnboundedDynamic,
            ]);
        }
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
        TypeExpr::Array { element, .. } => {
            collect_type_expr_abi_capabilities_inner(element, types_by_name, required, visiting);
        }
        TypeExpr::VarSequence { element, .. } => {
            required.extend([
                Capability::AbiVariablePayloadFrame,
                Capability::AllocationUnboundedDynamic,
            ]);
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

    fn test_type(name: &str, fields: Vec<crate::FieldIr>) -> TypeIr {
        TypeIr {
            id: crate::EntityId("type_0000000000000001".to_string()),
            module: None,
            name: name.to_string(),
            qualified_name: name.to_string(),
            generated_name: name.to_string(),
            fields,
        }
    }

    fn test_component(name: &str, outputs: Vec<crate::PortIr>) -> ComponentIr {
        ComponentIr {
            id: crate::EntityId("component_0000000000000001".to_string()),
            module: None,
            name: name.to_string(),
            qualified_name: name.to_string(),
            generated_name: name.to_string(),
            language: crate::LanguageKind::Rust,
            kind: crate::ComponentKind::Native,
            inputs: vec![],
            outputs,
            service_clients: vec![],
            service_servers: vec![],
            params: vec![],
            lifecycle: crate::LifecycleSurface::reserved_v0_1(),
        }
    }

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
    fn zenoh_supports_cross_host_copy_transport_capabilities() {
        assert!(is_known_backend("zenoh"));

        let capabilities = backend_capabilities("zenoh").unwrap();

        assert!(capabilities.contains(&CapabilityAtom("topology:multi_process".to_string())));
        assert!(capabilities.contains(&CapabilityAtom("topology:multi_host".to_string())));
        assert!(capabilities.contains(&CapabilityAtom("transfer:copy".to_string())));
        assert!(capabilities.contains(&CapabilityAtom("overflow:drop_oldest".to_string())));
        assert!(!capabilities.contains(&CapabilityAtom("overflow:drop_newest".to_string())));
        assert!(!capabilities.contains(&CapabilityAtom("overflow:error".to_string())));
        assert!(!capabilities.contains(&CapabilityAtom("overflow:block".to_string())));
        assert!(!capabilities.contains(&CapabilityAtom("transfer:zero_copy".to_string())));
        assert!(!capabilities.contains(&CapabilityAtom("transfer:loaned".to_string())));
    }

    #[test]
    fn inproc_and_zenoh_support_unbounded_variable_frames_but_iox2_does_not() {
        for backend in ["inproc", "zenoh"] {
            let capabilities = backend_capabilities(backend).unwrap();
            assert!(
                capabilities.contains(&CapabilityAtom("abi:variable_payload_frame".to_string()))
            );
            assert!(
                capabilities.contains(&CapabilityAtom("allocation:unbounded_dynamic".to_string()))
            );
        }
        let iox2_capabilities = backend_capabilities("iox2").unwrap();
        assert!(
            !iox2_capabilities.contains(&CapabilityAtom("abi:variable_payload_frame".to_string()))
        );
        assert!(
            !iox2_capabilities
                .contains(&CapabilityAtom("allocation:unbounded_dynamic".to_string()))
        );
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
                Capability::AbiVariablePayloadFrame.atom(),
                Capability::LayoutNativeLayout.atom(),
                Capability::AllocationBounded.atom(),
                Capability::AllocationUnboundedDynamic.atom(),
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
            processes: vec![],
            tasks: vec![
                crate::TaskIr {
                    id: crate::EntityId("task_0000000000000001".to_string()),
                    name: "shutdown".to_string(),
                    instance: instance.clone(),
                    trigger: TriggerKind::Shutdown,
                    readiness: crate::TaskReadiness::AnyReady,
                    period_ms: None,
                    deadline_ms: Some(10),
                    lane: None,
                    priority: None,
                    inputs: vec![],
                    outputs: vec![],
                },
                crate::TaskIr {
                    id: crate::EntityId("task_0000000000000002".to_string()),
                    name: "main".to_string(),
                    instance: instance.clone(),
                    trigger: TriggerKind::Periodic,
                    readiness: crate::TaskReadiness::AnyReady,
                    period_ms: Some(10),
                    deadline_ms: None,
                    lane: None,
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
                    backend: crate::BackendName("inproc".to_string()),
                    backend_source: crate::ChannelBackendSource::Explicit,
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
                    backend: crate::BackendName("inproc".to_string()),
                    backend_source: crate::ChannelBackendSource::Explicit,
                    channel: ChannelKind::Latest,
                    depth: Some(1),
                    overflow: OverflowPolicy::DropNewest,
                    stale: StalePolicy::HoldLast,
                    max_age_ms: Some(10),
                    policy_source,
                    capability_requirements: vec![],
                },
            ],
            services: vec![],
            ros2_bridges: vec![],
        };
        let types = [test_type(
            "WideIntegers",
            vec![
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
        )];

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
                Capability::LayoutNativeLayout.atom(),
                Capability::AllocationBounded.atom(),
                Capability::GraphStaticGraph.atom(),
                Capability::TriggerPeriodic.atom(),
                Capability::TriggerShutdown.atom(),
                Capability::TimingDeadlineAware.atom(),
            ]
        );
    }

    #[test]
    fn route_capabilities_require_multi_process_for_cross_process_binds() {
        let capabilities = channel_route_capabilities(
            &[],
            &TypeExpr::Primitive {
                name: PrimitiveType::U32,
            },
            ChannelKind::Latest,
            OverflowPolicy::DropOldest,
            StalePolicy::Warn,
            RouteTopology::new(true, false),
        );

        assert!(capabilities.contains(&CapabilityAtom("topology:multi_process".to_string())));
        assert!(!capabilities.contains(&CapabilityAtom("topology:multi_host".to_string())));
    }

    #[test]
    fn route_capabilities_require_multi_host_for_cross_target_binds() {
        let capabilities = channel_route_capabilities(
            &[],
            &TypeExpr::Primitive {
                name: PrimitiveType::U32,
            },
            ChannelKind::Latest,
            OverflowPolicy::DropOldest,
            StalePolicy::Warn,
            RouteTopology::new(true, true),
        );

        assert!(capabilities.contains(&CapabilityAtom("topology:multi_process".to_string())));
        assert!(capabilities.contains(&CapabilityAtom("topology:multi_host".to_string())));
    }

    #[test]
    fn message_abi_capabilities_include_nested_int128_usage() {
        let types = vec![test_type(
            "Nested",
            vec![crate::FieldIr {
                name: "value".to_string(),
                ty: TypeExpr::Primitive {
                    name: PrimitiveType::I128,
                },
                default: None,
            }],
        )];
        let components = [test_component(
            "producer",
            vec![crate::PortIr {
                name: "sample".to_string(),
                ty: TypeExpr::Array {
                    element: Box::new(TypeExpr::Named {
                        name: "Nested".to_string(),
                    }),
                    len: 2,
                },
            }],
        )];

        assert_eq!(
            message_abi_capabilities(&types, components.iter()),
            vec![CapabilityAtom("abi:int128".to_string())]
        );
    }

    #[test]
    fn message_abi_capabilities_include_unbounded_variable_frame_requirements() {
        let types = vec![test_type(
            "Packet",
            vec![
                crate::FieldIr {
                    name: "payload".to_string(),
                    ty: TypeExpr::VarBytes,
                    default: None,
                },
                crate::FieldIr {
                    name: "samples".to_string(),
                    ty: TypeExpr::VarSequence {
                        element: Box::new(TypeExpr::Primitive {
                            name: PrimitiveType::U32,
                        }),
                    },
                    default: None,
                },
            ],
        )];

        assert_eq!(
            message_abi_capabilities(&types, std::iter::empty()),
            vec![
                Capability::AbiVariablePayloadFrame.atom(),
                Capability::AllocationUnboundedDynamic.atom(),
            ]
        );
    }
}
