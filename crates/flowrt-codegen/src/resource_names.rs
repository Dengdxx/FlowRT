use flowrt_ir::{
    ResourceAccess, ResourceDescriptorKind, ResourceFailurePolicy, ResourceHealthPolicy,
    ResourceProviderScope, ResourceReadinessGate, ResourceSatisfactionIr,
};

pub(crate) fn resource_descriptor_kind_name(kind: ResourceDescriptorKind) -> &'static str {
    match kind {
        ResourceDescriptorKind::Frame => "frame",
    }
}

pub(crate) fn resource_access_name(kind: ResourceAccess) -> &'static str {
    match kind {
        ResourceAccess::Read => "read",
        ResourceAccess::Write => "write",
        ResourceAccess::ReadWrite => "read_write",
        ResourceAccess::Exclusive => "exclusive",
    }
}

pub(crate) fn resource_readiness_name(kind: ResourceReadinessGate) -> &'static str {
    match kind {
        ResourceReadinessGate::BeforeInit => "before_init",
        ResourceReadinessGate::BeforeStart => "before_start",
        ResourceReadinessGate::Lazy => "lazy",
    }
}

pub(crate) fn resource_health_name(kind: ResourceHealthPolicy) -> &'static str {
    match kind {
        ResourceHealthPolicy::Required => "required",
        ResourceHealthPolicy::Optional => "optional",
        ResourceHealthPolicy::Ignored => "ignored",
    }
}

pub(crate) fn resource_failure_name(kind: ResourceFailurePolicy) -> &'static str {
    match kind {
        ResourceFailurePolicy::StopProcess => "stop_process",
        ResourceFailurePolicy::RestartProcess => "restart_process",
        ResourceFailurePolicy::Degrade => "degrade",
        ResourceFailurePolicy::StopGraph => "stop_graph",
    }
}

pub(crate) fn resource_provider_scope_name(kind: ResourceProviderScope) -> &'static str {
    match kind {
        ResourceProviderScope::Target => "target",
        ResourceProviderScope::Process => "process",
        ResourceProviderScope::ExternalPackage => "external_package",
    }
}

pub(crate) fn resource_satisfaction_status(satisfaction: &ResourceSatisfactionIr) -> &'static str {
    if satisfaction.satisfied {
        "satisfied"
    } else if satisfaction
        .diagnostic
        .as_deref()
        .is_some_and(|diagnostic| diagnostic.contains("conflict"))
    {
        "conflict"
    } else if !satisfaction.required {
        "optional_unsatisfied"
    } else {
        "unsatisfied"
    }
}
