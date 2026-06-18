pub(crate) use std::collections::BTreeMap;

pub(crate) use flowrt_rsdl::{load_file, parse_str};

pub(crate) use super::{
    hash_source, normalize_document, normalize_loaded_document, project_contract_to_profile,
};
pub(crate) use crate::{
    BackendThreadAffinity, BoundaryDirection, CapabilityAtom, ChannelBackendSource, ChannelKind,
    ClockSourceKind, DeterminismMode, DeterminismTimeoutPolicy, FaultInjectionScenario,
    FaultInjectionScenarioPoint, GraphFaultReaction, GraphMode, IrError, OperationBackendSource,
    OperationConcurrencyPolicy, OperationFeedbackPolicy, OperationPreemptPolicy, OverflowPolicy,
    ParamType, ParamUpdatePolicy, ParamValue, PolicyValueSource, PrimitiveType,
    ProcessFailurePropagation, ProcessReadinessGate, ProcessRestartPolicyKind, RedundancyMode,
    RedundancyTrigger, Ros2BridgeDirection, RouteTopology, RtPolicy, ServiceBackendSource,
    ServiceOverflowPolicy, StalePolicy, TaskReadiness, TemporaryBoundaryMapping,
    TemporaryIslandOverlay, TypeExpr, apply_fault_injection_overlay,
    apply_temporary_island_overlay, channel_route_capabilities, deployment_capability_decision,
};

mod core;
mod fault_injection;
mod params_profiles;
mod resources_processes;
mod routes_capabilities;
mod services_operations;
mod workspace_island;

fn unique_temp_dir() -> std::path::PathBuf {
    let suffix = format!(
        "flowrt-ir-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    std::env::temp_dir().join(suffix)
}
