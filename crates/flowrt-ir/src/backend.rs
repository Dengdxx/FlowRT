use crate::{CapabilityAtom, TriggerKind};

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
}
