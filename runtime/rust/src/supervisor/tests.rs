mod support;

mod prelude {
    pub(super) use std::collections::{BTreeMap, BTreeSet};
    pub(super) use std::path::{Path, PathBuf};
    pub(super) use std::process::Command;
    pub(super) use std::time::{Duration, Instant};

    pub(super) use crate::ShutdownToken;
    pub(super) use crate::introspection::{IntrospectionResourceStatus, IntrospectionState};

    pub(super) use super::super::command::binary_name;
    pub(super) use super::super::launch_loop::{
        SupervisedChild, child_dependencies_satisfied, record_child_health,
        record_child_reported_resource_statuses, refresh_child_health, supervise_children,
    };
    pub(super) use super::super::manifest::{effective_readiness, parse_launch_manifest};
    pub(super) use super::super::readiness::{
        READINESS_POLL_INTERVAL, READINESS_TIMEOUT, ReadinessConfig, expected_services_for_process,
        expected_services_ready, readiness_gate_label, wait_for_readiness, wait_for_runtime_ready,
        wait_for_service_ready, wait_for_startup_delay,
    };
    pub(super) use super::super::resource_placement::{ResourcePlacement, ResourcePlacementStatus};
    pub(super) use super::super::resources::{
        ProcessResourceGate, ResourceGateAction, ResourceGatePhase, evaluate_child_resource_gates,
        evaluate_process_resource_gates, process_resource_gate,
    };
    pub(super) use super::super::time::unix_time_ms;
    pub(super) use super::super::zenoh::reserve_zenoh_port_lease;
    pub(super) use super::super::*;
    pub(super) use super::support::*;
}

mod command_tests;
mod dependency_zenoh_tests;
mod global_tick_tests;
mod lifecycle_tests;
mod manifest_resource_tests;
mod readiness_tests;
