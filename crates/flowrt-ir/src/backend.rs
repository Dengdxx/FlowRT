use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CapabilityAtom, ChannelKind, ComponentIr, GraphIr, OverflowPolicy, PrimitiveType, StalePolicy,
    TriggerKind, TypeExpr, TypeIr,
};

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
        for group in self.capabilities {
            capabilities.extend(group.iter().copied());
        }
        capabilities.into_atoms()
    }
}

struct CapabilityList {
    seen: BTreeSet<Capability>,
    items: Vec<Capability>,
}

impl CapabilityList {
    fn new() -> Self {
        Self {
            seen: BTreeSet::new(),
            items: Vec::new(),
        }
    }

    fn push(&mut self, capability: Capability) {
        if self.seen.insert(capability) {
            self.items.push(capability);
        }
    }

    fn extend(&mut self, capabilities: impl IntoIterator<Item = Capability>) {
        for capability in capabilities {
            self.push(capability);
        }
    }

    fn into_atoms(self) -> Vec<CapabilityAtom> {
        self.items
            .into_iter()
            .map(Capability::atom)
            .collect::<Vec<_>>()
    }
}

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

/// v0.1 deployment 在 graph-specific policy 之外必须满足的基础能力。
pub fn base_deployment_capabilities() -> Vec<CapabilityAtom> {
    [
        Capability::AbiFixedSizePlainData,
        Capability::LayoutNativeLayout,
        Capability::AllocationBounded,
        Capability::GraphStaticGraph,
    ]
    .into_iter()
    .map(Capability::atom)
    .collect()
}

/// 返回某个 task trigger 所需的 capability atom。
pub fn trigger_capability(trigger: TriggerKind) -> CapabilityAtom {
    let capability = match trigger {
        TriggerKind::Periodic => Capability::TriggerPeriodic,
        TriggerKind::OnMessage => Capability::TriggerOnMessage,
        TriggerKind::Startup => Capability::TriggerStartup,
        TriggerKind::Shutdown => Capability::TriggerShutdown,
    };
    capability.atom()
}

/// 推导 target 声明的 backend capability atoms。
pub fn target_capabilities(backends: &[crate::BackendName]) -> Vec<CapabilityAtom> {
    let capabilities = backends
        .iter()
        .filter_map(|backend| backend_capabilities(&backend.0))
        .flatten()
        .collect::<Vec<_>>();
    dedupe_capabilities(capabilities)
}

/// 推导 data channel policy 需要的 capability atoms。
pub fn channel_capabilities(
    channel: ChannelKind,
    overflow: OverflowPolicy,
    stale: StalePolicy,
) -> Vec<CapabilityAtom> {
    vec![
        channel_capability(channel).atom(),
        overflow_capability(overflow).atom(),
        stale_capability(stale).atom(),
    ]
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
    let mut capabilities = base_deployment_capabilities();
    let components_by_name = components
        .iter()
        .map(|component| (component.name.as_str(), component))
        .collect::<BTreeMap<_, _>>();

    capabilities.extend(message_abi_capabilities(
        types,
        graph
            .instances
            .iter()
            .filter_map(|instance| components_by_name.get(instance.component.name.as_str()))
            .copied(),
    ));
    for task in &graph.tasks {
        capabilities.push(trigger_capability(task.trigger));
        if task.deadline_ms.is_some() {
            capabilities.push(Capability::TimingDeadlineAware.atom());
        }
    }
    for bind in &graph.binds {
        capabilities.extend(channel_capabilities(
            bind.channel,
            bind.overflow,
            bind.stale,
        ));
    }
    dedupe_capabilities(capabilities)
}

/// 推导 Contract IR message 与 component port 类型需要的 ABI capability atoms。
pub fn message_abi_capabilities<'a>(
    types: &[TypeIr],
    components: impl IntoIterator<Item = &'a ComponentIr>,
) -> Vec<CapabilityAtom> {
    let types_by_name = types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    let mut required = Vec::new();

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

    dedupe_capabilities(required)
}

fn collect_type_expr_abi_capabilities(
    expr: &TypeExpr,
    types_by_name: &BTreeMap<&str, &TypeIr>,
    required: &mut Vec<CapabilityAtom>,
) {
    let mut visiting = BTreeSet::new();
    collect_type_expr_abi_capabilities_inner(expr, types_by_name, required, &mut visiting);
}

fn collect_type_expr_abi_capabilities_inner(
    expr: &TypeExpr,
    types_by_name: &BTreeMap<&str, &TypeIr>,
    required: &mut Vec<CapabilityAtom>,
    visiting: &mut BTreeSet<String>,
) {
    match expr {
        TypeExpr::Primitive {
            name: PrimitiveType::U128 | PrimitiveType::I128,
        } => required.push(Capability::AbiInt128.atom()),
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

/// 去重 capability atoms，并保留首次出现顺序作为 canonical 派生列表。
fn dedupe_capabilities(capabilities: Vec<CapabilityAtom>) -> Vec<CapabilityAtom> {
    let mut seen = BTreeSet::new();
    capabilities
        .into_iter()
        .filter(|capability| seen.insert(capability.clone()))
        .collect()
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
