use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CapabilityAtom, ChannelKind, ComponentIr, GraphIr, OverflowPolicy, PrimitiveType, StalePolicy,
    TriggerKind, TypeExpr, TypeIr,
};

const IMPLEMENTED_BACKENDS: &[&str] = &["inproc", "iox2"];

const COMMON_CAPABILITIES: &[&str] = &[
    "abi:fixed_size_plain_data",
    "layout:native_layout",
    "allocation:bounded",
    "graph:static_graph",
    "trigger:periodic",
    "trigger:on_message",
    "trigger:startup",
    "trigger:shutdown",
    "timing:deadline_aware",
    "channel:latest",
    "channel:fifo",
    "overflow:drop_oldest",
    "overflow:drop_newest",
    "overflow:error",
    "overflow:block",
    "stale:warn",
    "stale:drop",
    "stale:hold_last",
    "stale:error",
];

/// 判断当前实现是否认识某个 backend 名称。
pub fn is_known_backend(name: &str) -> bool {
    IMPLEMENTED_BACKENDS.contains(&name)
}

/// 返回某个 backend 提供的 capability atoms。
pub fn backend_capabilities(name: &str) -> Option<Vec<CapabilityAtom>> {
    let specific = match name {
        "inproc" => &[
            "topology:single_process",
            "transfer:copy",
            "observability:health",
        ][..],
        "iox2" => &[
            "topology:multi_process",
            "topology:single_host",
            "transfer:zero_copy",
            "transfer:loaned",
            "observability:health",
            "timing:deadline_aware",
        ][..],
        _ => return None,
    };

    Some(
        COMMON_CAPABILITIES
            .iter()
            .chain(specific.iter())
            .map(|capability| CapabilityAtom((*capability).to_string()))
            .collect(),
    )
}

/// v0.1 deployment 在 graph-specific policy 之外必须满足的基础能力。
pub fn base_deployment_capabilities() -> Vec<CapabilityAtom> {
    [
        "abi:fixed_size_plain_data",
        "layout:native_layout",
        "allocation:bounded",
        "graph:static_graph",
    ]
    .into_iter()
    .map(|capability| CapabilityAtom(capability.to_string()))
    .collect()
}

/// 返回某个 task trigger 所需的 capability atom。
pub fn trigger_capability(trigger: TriggerKind) -> CapabilityAtom {
    let name = match trigger {
        TriggerKind::Periodic => "trigger:periodic",
        TriggerKind::OnMessage => "trigger:on_message",
        TriggerKind::Startup => "trigger:startup",
        TriggerKind::Shutdown => "trigger:shutdown",
    };
    CapabilityAtom(name.to_string())
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
        CapabilityAtom(channel_capability_name(channel).to_string()),
        CapabilityAtom(overflow_capability_name(overflow).to_string()),
        CapabilityAtom(stale_capability_name(stale).to_string()),
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
            capabilities.push(CapabilityAtom("timing:deadline_aware".to_string()));
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
        } => required.push(CapabilityAtom("abi:int128".to_string())),
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

fn channel_capability_name(channel: ChannelKind) -> &'static str {
    match channel {
        ChannelKind::Latest => "channel:latest",
        ChannelKind::Fifo => "channel:fifo",
    }
}

fn overflow_capability_name(policy: OverflowPolicy) -> &'static str {
    match policy {
        OverflowPolicy::DropOldest => "overflow:drop_oldest",
        OverflowPolicy::DropNewest => "overflow:drop_newest",
        OverflowPolicy::Error => "overflow:error",
        OverflowPolicy::Block => "overflow:block",
    }
}

fn stale_capability_name(policy: StalePolicy) -> &'static str {
    match policy {
        StalePolicy::Warn => "stale:warn",
        StalePolicy::Drop => "stale:drop",
        StalePolicy::HoldLast => "stale:hold_last",
        StalePolicy::Error => "stale:error",
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
