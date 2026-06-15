//! Resource contract gate、状态记录和 placement 应用。

use std::collections::{BTreeMap, BTreeSet};

use crate::introspection::{
    IntrospectionProcessStatus, IntrospectionResourceStatus, IntrospectionState,
};

use super::launch_loop::SupervisedChild;
use super::manifest::{LaunchGraph, LaunchProcess};
use super::resource_placement::{
    self, ResourceApplied, ResourcePlacement, ResourcePlacementStatus,
};
use super::time::unix_time_ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResourceGatePhase {
    Startup,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResourceGateAction {
    Start,
    Degrade,
    WaitRestart,
    StopProcess,
    StopGraph,
}

#[derive(Debug, Clone)]
pub(super) struct ResourceGateDecision {
    pub(super) action: ResourceGateAction,
    pub(super) statuses: Vec<IntrospectionResourceStatus>,
    blocking: Option<ResourceGateBlock>,
}

#[derive(Debug, Clone)]
pub(super) struct ResourceGateBlock {
    pub(super) resource: String,
    pub(super) capability: String,
    pub(super) readiness: String,
    pub(super) on_failure: String,
    pub(super) diagnostic: Option<String>,
}

impl ResourceGateDecision {
    pub(super) fn error_message(&self, process_name: &str) -> Option<String> {
        let block = self.blocking.as_ref()?;
        let diagnostic = block
            .diagnostic
            .as_deref()
            .unwrap_or("resource is not ready");
        Some(format!(
            "FlowRT resource gate blocked process `{process_name}`: resource `{}` capability `{}` readiness={} policy={} diagnostic={diagnostic}",
            block.resource, block.capability, block.readiness, block.on_failure
        ))
    }

    pub(super) fn wait_label(&self) -> Option<String> {
        let block = self.blocking.as_ref()?;
        Some(format!(
            "resource={} capability={} readiness={} policy={}",
            block.resource, block.capability, block.readiness, block.on_failure
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct ProcessResourceGate {
    pub(super) resources: Vec<ProcessResourceContract>,
}

#[derive(Debug, Clone)]
pub(super) struct ProcessResourceContract {
    pub(super) instance: String,
    pub(super) resource: String,
    pub(super) capability: String,
    pub(super) access: String,
    pub(super) required: bool,
    pub(super) readiness: String,
    pub(super) health: String,
    pub(super) on_failure: String,
    pub(super) contract_status: String,
    pub(super) satisfied: bool,
    pub(super) provider: Option<String>,
    pub(super) diagnostic: Option<String>,
    pub(super) provider_scope: Option<String>,
    pub(super) provider_readiness_source: Option<String>,
    pub(super) provider_health_source: Option<String>,
}

pub(super) fn evaluate_process_resource_gates(
    graph: &LaunchGraph,
    process: &LaunchProcess,
    phase: ResourceGatePhase,
) -> ResourceGateDecision {
    let gate = process_resource_gate(graph, process);
    let mut decision = evaluate_resource_gate(&gate, phase);
    for status in &mut decision.statuses {
        status.owner_process = Some(process.name.clone());
    }
    decision
}

pub(super) fn evaluate_child_resource_gates(
    supervisor_state: &IntrospectionState,
    child: &SupervisedChild,
    phase: ResourceGatePhase,
) -> ResourceGateDecision {
    let mut gate = child.resource_gate.clone();
    apply_latest_resource_statuses(&mut gate, &supervisor_state.status().resources);
    let mut decision = evaluate_resource_gate(&gate, phase);
    for status in &mut decision.statuses {
        status.owner_process = Some(child.name.clone());
    }
    decision
}

fn apply_latest_resource_statuses(
    gate: &mut ProcessResourceGate,
    latest: &[IntrospectionResourceStatus],
) {
    for resource in &mut gate.resources {
        let resource_name = format!("{}.{}", resource.instance, resource.resource);
        let Some(status) = latest.iter().find(|status| status.name == resource_name) else {
            continue;
        };
        let Some(satisfied) = resource_status_satisfied(status) else {
            continue;
        };
        resource.satisfied = satisfied;
        resource.contract_status = status
            .contract_status
            .clone()
            .unwrap_or_else(|| status.state.clone());
        resource.diagnostic = status
            .diagnostic
            .clone()
            .or_else(|| status.last_error.clone())
            .or_else(|| resource.diagnostic.clone());
        if status.provider.is_some() {
            resource.provider = status.provider.clone();
        }
        if status.provider_scope.is_some() {
            resource.provider_scope = status.provider_scope.clone();
        }
        if status.provider_readiness_source.is_some() {
            resource.provider_readiness_source = status.provider_readiness_source.clone();
        }
        if status.provider_health_source.is_some() {
            resource.provider_health_source = status.provider_health_source.clone();
        }
    }
}

fn resource_status_satisfied(status: &IntrospectionResourceStatus) -> Option<bool> {
    status.satisfied.or(match status.state.as_str() {
        "ready" => Some(true),
        "pending" | "degraded" | "failed" | "unknown" => Some(false),
        _ => None,
    })
}

pub(super) fn process_resource_gate(
    graph: &LaunchGraph,
    process: &LaunchProcess,
) -> ProcessResourceGate {
    let process_instances = process
        .instances
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let providers = graph
        .resource_contract
        .providers
        .iter()
        .map(|provider| (provider.name.as_str(), provider))
        .collect::<BTreeMap<_, _>>();
    let resources = graph
        .resource_contract
        .satisfactions
        .iter()
        .filter(|satisfaction| process_instances.contains(satisfaction.instance.as_str()))
        .map(|satisfaction| {
            let provider = satisfaction
                .provider
                .as_deref()
                .and_then(|name| providers.get(name).copied());
            ProcessResourceContract {
                instance: satisfaction.instance.clone(),
                resource: satisfaction.resource.clone(),
                capability: satisfaction.capability.clone(),
                access: satisfaction.access.clone(),
                required: satisfaction.required,
                readiness: satisfaction.readiness.clone(),
                health: satisfaction.health.clone(),
                on_failure: satisfaction.on_failure.clone(),
                contract_status: satisfaction.status.clone(),
                satisfied: satisfaction.satisfied,
                provider: satisfaction.provider.clone(),
                diagnostic: satisfaction.diagnostic.clone(),
                provider_scope: provider.map(|provider| provider.scope.clone()),
                provider_readiness_source: provider.and_then(|provider| {
                    provider
                        .readiness_source
                        .clone()
                        .or_else(|| provider.process.clone())
                        .or_else(|| provider.target.clone())
                        .or_else(|| provider.external_package.clone())
                }),
                provider_health_source: provider
                    .and_then(|provider| provider.health_source.clone()),
            }
        })
        .collect();
    ProcessResourceGate { resources }
}

fn evaluate_resource_gate(
    gate: &ProcessResourceGate,
    phase: ResourceGatePhase,
) -> ResourceGateDecision {
    let mut action = ResourceGateAction::Start;
    let mut blocking = None;
    let mut statuses = Vec::new();

    for resource in &gate.resources {
        let (resource_action, state) = resource_gate_resource_action(resource, phase);
        let status = resource_status_from_contract(resource, &state);
        if should_replace_action(action, resource_action) {
            action = resource_action;
            if matches!(
                resource_action,
                ResourceGateAction::StopProcess
                    | ResourceGateAction::StopGraph
                    | ResourceGateAction::WaitRestart
            ) {
                blocking = Some(ResourceGateBlock {
                    resource: status.name.clone(),
                    capability: status.capability.clone(),
                    readiness: resource.readiness.clone(),
                    on_failure: resource.on_failure.clone(),
                    diagnostic: resource.diagnostic.clone(),
                });
            }
        }
        statuses.push(status);
    }

    ResourceGateDecision {
        action,
        statuses,
        blocking,
    }
}

fn resource_gate_resource_action(
    resource: &ProcessResourceContract,
    phase: ResourceGatePhase,
) -> (ResourceGateAction, String) {
    if resource.satisfied {
        return (ResourceGateAction::Start, "ready".to_string());
    }
    if resource.readiness == "lazy" {
        return (ResourceGateAction::Start, "pending".to_string());
    }
    if !resource.required || resource.health == "optional" {
        return (ResourceGateAction::Degrade, "degraded".to_string());
    }
    match resource.on_failure.as_str() {
        "stop_graph" => (ResourceGateAction::StopGraph, "failed".to_string()),
        "restart_process" if phase == ResourceGatePhase::Restart => {
            (ResourceGateAction::WaitRestart, "pending".to_string())
        }
        "restart_process" => (ResourceGateAction::StopProcess, "pending".to_string()),
        "degrade" => (ResourceGateAction::Degrade, "degraded".to_string()),
        _ => (ResourceGateAction::StopProcess, "failed".to_string()),
    }
}

fn should_replace_action(current: ResourceGateAction, candidate: ResourceGateAction) -> bool {
    resource_action_rank(candidate) > resource_action_rank(current)
}

fn resource_action_rank(action: ResourceGateAction) -> u8 {
    match action {
        ResourceGateAction::Start => 0,
        ResourceGateAction::Degrade => 1,
        ResourceGateAction::WaitRestart => 2,
        ResourceGateAction::StopProcess => 3,
        ResourceGateAction::StopGraph => 4,
    }
}

pub(super) fn resource_status_from_contract(
    resource: &ProcessResourceContract,
    state: &str,
) -> IntrospectionResourceStatus {
    let last_error = (!resource.satisfied)
        .then(|| {
            resource
                .diagnostic
                .clone()
                .unwrap_or_else(|| "resource is not ready".to_string())
        })
        .filter(|_| state != "ready");
    IntrospectionResourceStatus {
        name: format!("{}.{}", resource.instance, resource.resource),
        capability: resource.capability.clone(),
        access: Some(resource.access.clone()),
        state: state.to_string(),
        required: resource.required,
        readiness: Some(resource.readiness.clone()),
        health: Some(resource.health.clone()),
        on_failure: Some(resource.on_failure.clone()),
        contract_status: Some(resource.contract_status.clone()),
        satisfied: Some(resource.satisfied),
        provider: resource.provider.clone(),
        provider_scope: resource.provider_scope.clone(),
        provider_readiness_source: resource.provider_readiness_source.clone(),
        provider_health_source: resource.provider_health_source.clone(),
        diagnostic: resource.diagnostic.clone(),
        suggestion: resource_gate_suggestion(resource),
        source: Some("contract".to_string()),
        owner_process: None,
        last_error,
        updated_unix_ms: Some(unix_time_ms()),
    }
}

fn resource_gate_suggestion(resource: &ProcessResourceContract) -> Option<String> {
    if resource.satisfied {
        return None;
    }
    match resource.provider_readiness_source.as_deref() {
        Some(source) => Some(format!("check provider readiness source `{source}`")),
        None if resource.required => {
            Some("configure a provider or relax the requirement".to_string())
        }
        None => {
            Some("configure an optional provider if degraded mode is not acceptable".to_string())
        }
    }
}

pub(super) fn record_resource_gate_statuses(
    supervisor_state: &IntrospectionState,
    statuses: &[IntrospectionResourceStatus],
) {
    for status in statuses {
        supervisor_state.record_resource_status(status.clone());
    }
}

fn record_process_resource_gate_status(
    supervisor_state: &IntrospectionState,
    process: &LaunchProcess,
    state: &str,
    decision: &ResourceGateDecision,
) {
    supervisor_state.record_process_health(IntrospectionProcessStatus {
        name: process.name.clone(),
        state: state.to_string(),
        pid: None,
        restart_count: 0,
        tick_count: None,
        last_seen_unix_ms: None,
        tick_stale: false,
        exit_code: None,
        readiness_wait: decision.wait_label(),
        resource_placement: if resource_placement::has_placement(&process.resource_placement) {
            Some(ResourcePlacementStatus {
                desired: process.resource_placement.clone(),
                applied: ResourceApplied::default(),
            })
        } else {
            None
        },
    });
}

pub(super) fn apply_startup_resource_gate(
    supervisor_state: &IntrospectionState,
    graph: &LaunchGraph,
    process: &LaunchProcess,
) -> Result<ResourceGateDecision, String> {
    let decision = evaluate_process_resource_gates(graph, process, ResourceGatePhase::Startup);
    record_resource_gate_statuses(supervisor_state, &decision.statuses);
    match decision.action {
        ResourceGateAction::Start | ResourceGateAction::Degrade => Ok(decision),
        ResourceGateAction::StopGraph => {
            record_process_resource_gate_status(supervisor_state, process, "stopped", &decision);
            Err(decision.error_message(&process.name).unwrap_or_else(|| {
                format!(
                    "FlowRT resource gate stopped graph before process `{}` startup",
                    process.name
                )
            }))
        }
        ResourceGateAction::StopProcess | ResourceGateAction::WaitRestart => {
            record_process_resource_gate_status(supervisor_state, process, "stopped", &decision);
            Err(decision.error_message(&process.name).unwrap_or_else(|| {
                format!(
                    "FlowRT resource gate stopped process `{}` before startup",
                    process.name
                )
            }))
        }
    }
}

pub(super) fn apply_resource_placement_to_pid(
    placement: &ResourcePlacement,
    pid: Option<u32>,
) -> ResourceApplied {
    if resource_placement::has_placement(placement) {
        resource_placement::apply_to_pid(placement, pid)
    } else {
        ResourceApplied::default()
    }
}
