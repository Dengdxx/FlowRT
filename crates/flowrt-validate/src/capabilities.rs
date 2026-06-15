use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    BackendName, CapabilityAtom, ChannelEdgeIr, ContractIr, GraphIr, LanguageKind,
    PolicyValueSource, backend_capabilities, deployment_capability_decision,
    derived::{ContractDerivedFacts, GraphDerivedFacts, derive_contract_facts},
    is_known_backend,
};

use crate::ValidationError;

pub(crate) fn validate_deployment_matrix(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let mut rows = BTreeSet::new();
    for deployment in &ir.deployments {
        let row = (
            deployment.graph.name.as_str(),
            deployment.profile.name.as_str(),
            deployment.target.name.as_str(),
        );
        if !rows.insert(row) {
            errors.push(ValidationError::new(format!(
                "contract has duplicate deployment for graph `{}`, profile `{}`, target `{}`",
                deployment.graph.name, deployment.profile.name, deployment.target.name
            )));
        }
    }

    for graph in &ir.graphs {
        for profile in &ir.profiles {
            for target in &ir.targets {
                let row = (
                    graph.name.as_str(),
                    profile.name.as_str(),
                    target.name.as_str(),
                );
                if !rows.contains(&row) {
                    errors.push(ValidationError::new(format!(
                        "contract is missing deployment for graph `{}`, profile `{}`, target `{}`",
                        graph.name, profile.name, target.name
                    )));
                }
            }
        }
    }
}

pub(crate) fn validate_derived_capabilities(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let Some(facts) = derive_facts(ir, errors) else {
        return;
    };
    let facts_by_graph = graph_facts_by_name(&facts);
    let facts_by_target = facts
        .targets
        .iter()
        .map(|facts| (facts.target.name.as_str(), facts))
        .collect::<BTreeMap<_, _>>();
    let facts_by_deployment = facts
        .deployments
        .iter()
        .map(|facts| (facts.deployment.id.0.as_str(), facts))
        .collect::<BTreeMap<_, _>>();

    for graph in &ir.graphs {
        let Some(graph_facts) = facts_by_graph.get(graph.name.as_str()).copied() else {
            continue;
        };
        let route_facts = graph_facts
            .routes
            .iter()
            .map(|route| (route.bind_id.0.as_str(), route))
            .collect::<BTreeMap<_, _>>();
        for bind in &graph.binds {
            let Some(route) = route_facts.get(bind.id.0.as_str()).copied() else {
                continue;
            };
            if bind.thread_affinity != route.thread_affinity {
                errors.push(ValidationError::new(format!(
                    "bind `{}.{}` -> `{}.{}` thread affinity metadata is inconsistent with selected backend",
                    bind.from.instance.name,
                    bind.from.port,
                    bind.to.instance.name,
                    bind.to.port
                )));
            }
            if !capabilities_match(
                &bind.capability_requirements,
                &route.capability_requirements,
            ) {
                errors.push(ValidationError::new(format!(
                    "bind `{}.{}` -> `{}.{}` capability requirements do not match channel policy",
                    bind.from.instance.name, bind.from.port, bind.to.instance.name, bind.to.port
                )));
            }
        }
    }

    for target in &ir.targets {
        let Some(facts) = facts_by_target.get(target.name.as_str()).copied() else {
            continue;
        };
        if !capabilities_match(&target.capabilities, &facts.capabilities) {
            errors.push(ValidationError::new(format!(
                "target `{}` capabilities do not match declared backends",
                target.name
            )));
        }
    }

    for deployment in &ir.deployments {
        let Some(facts) = facts_by_deployment.get(deployment.id.0.as_str()).copied() else {
            continue;
        };
        if !capabilities_match(
            &deployment.required_capabilities,
            &facts.required_capabilities,
        ) {
            errors.push(ValidationError::new(format!(
                "deployment `{} / {} / {}` required capabilities do not match graph `{}`",
                deployment.graph.name,
                deployment.profile.name,
                deployment.target.name,
                deployment.graph.name
            )));
        }
    }
}

pub(crate) fn validate_channel_policy_sources(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let Some(profile) = policy_anchor_profile(ir) else {
        return;
    };

    for graph in &ir.graphs {
        for bind in &graph.binds {
            if bind.policy_source.overflow == PolicyValueSource::ProfileDefault
                && bind.overflow != profile.defaults.default_overflow
            {
                push_policy_source_error(bind, errors);
                continue;
            }
            if bind.policy_source.stale == PolicyValueSource::ProfileDefault
                && bind.stale != profile.defaults.default_stale_policy
            {
                push_policy_source_error(bind, errors);
                continue;
            }
            if bind.policy_source.max_age_ms == PolicyValueSource::ProfileDefault
                && bind.max_age_ms != profile.defaults.max_age_ms
            {
                push_policy_source_error(bind, errors);
            }
        }
    }
}

pub(crate) fn validate_channel_backend_sources(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let Some(facts) = derive_facts(ir, errors) else {
        return;
    };
    let facts_by_graph = graph_facts_by_name(&facts);

    for graph in &ir.graphs {
        let Some(graph_facts) = facts_by_graph.get(graph.name.as_str()).copied() else {
            continue;
        };
        let route_facts = graph_facts
            .routes
            .iter()
            .map(|route| (route.bind_id.0.as_str(), route))
            .collect::<BTreeMap<_, _>>();
        for bind in &graph.binds {
            let Some(route) = route_facts.get(bind.id.0.as_str()).copied() else {
                continue;
            };
            if bind.backend.0 != route.backend.0 || bind.backend_source != route.backend_source {
                push_backend_source_error(bind, errors);
            }
        }
    }
}

pub(crate) fn validate_deployments(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let profiles = ir
        .profiles
        .iter()
        .map(|profile| (profile.name.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let targets = ir
        .targets
        .iter()
        .map(|target| (target.name.as_str(), target))
        .collect::<BTreeMap<_, _>>();
    let Some(facts) = derive_facts(ir, errors) else {
        return;
    };
    let facts_by_deployment = facts
        .deployments
        .iter()
        .map(|facts| (facts.deployment.id.0.as_str(), facts))
        .collect::<BTreeMap<_, _>>();

    for deployment in &ir.deployments {
        if !is_known_backend(&deployment.backend.0) {
            errors.push(ValidationError::new(format!(
                "deployment for graph `{}` selects unknown backend `{}`",
                deployment.graph.name, deployment.backend.0
            )));
            continue;
        }

        let Some(profile) = profiles.get(deployment.profile.name.as_str()) else {
            continue;
        };
        if profile.backend.0 != deployment.backend.0 {
            errors.push(ValidationError::new(format!(
                "deployment backend `{}` does not match profile `{}` backend `{}`",
                deployment.backend.0, deployment.profile.name, profile.backend.0
            )));
        }

        if !targets.contains_key(deployment.target.name.as_str()) {
            continue;
        }

        let Some(facts) = facts_by_deployment.get(deployment.id.0.as_str()).copied() else {
            continue;
        };

        let decision = &facts.decision;

        if !decision.target_supports_selected_backend {
            errors.push(ValidationError::new(format!(
                "target `{}` does not support backend `{}` selected by profile `{}`",
                deployment.target.name, facts.backend.0, deployment.profile.name
            )));
        }
        if decision.selected_backend_known && !decision.missing_required_capabilities.is_empty() {
            errors.push(ValidationError::new(format!(
                "backend `{}` selected by profile `{}` cannot satisfy required capabilities for graph `{}`",
                facts.backend.0, deployment.profile.name, deployment.graph.name
            )));
        }

        let expected_satisfied = profile.backend.0 == deployment.backend.0 && decision.satisfied;
        if deployment.satisfied != expected_satisfied {
            errors.push(ValidationError::new(format!(
                "deployment `{} / {} / {}` has inconsistent satisfied flag; expected {}",
                deployment.graph.name,
                deployment.profile.name,
                deployment.target.name,
                expected_satisfied
            )));
        }
    }
}

pub(crate) fn validate_declared_backends(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    for profile in &ir.profiles {
        if !is_known_backend(&profile.backend.0) {
            errors.push(ValidationError::new(format!(
                "profile `{}` selects unknown backend `{}`",
                profile.name, profile.backend.0
            )));
        }
    }

    for target in &ir.targets {
        for backend in &target.backends {
            if !is_known_backend(&backend.0) {
                errors.push(ValidationError::new(format!(
                    "target `{}` declares unknown backend `{}`",
                    target.name, backend.0
                )));
            }
        }

        let mut runtimes = BTreeSet::new();
        for runtime in &target.runtime {
            let runtime_name = match runtime {
                LanguageKind::C => "c",
                LanguageKind::Cpp => "cpp",
                LanguageKind::Rust => "rust",
                LanguageKind::External => "external",
            };
            if !runtimes.insert(runtime_name) {
                errors.push(ValidationError::new(format!(
                    "target `{}` has duplicate runtime `{runtime_name}`",
                    target.name
                )));
            }
        }

        let mut backends = BTreeSet::new();
        for backend in &target.backends {
            if !backends.insert(backend.0.as_str()) {
                errors.push(ValidationError::new(format!(
                    "target `{}` has duplicate backend `{}`",
                    target.name, backend.0
                )));
            }
        }
    }
}

pub(crate) fn validate_route_backends(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    let Some(facts) = derive_facts(ir, errors) else {
        return;
    };
    let facts_by_graph = graph_facts_by_name(&facts);
    for graph in &ir.graphs {
        let Some(graph_facts) = facts_by_graph.get(graph.name.as_str()).copied() else {
            continue;
        };
        let route_facts = graph_facts
            .routes
            .iter()
            .map(|route| (route.bind_id.0.as_str(), route))
            .collect::<BTreeMap<_, _>>();
        for bind in &graph.binds {
            let mut checked_targets = BTreeSet::new();
            if !is_known_backend(&bind.backend.0) {
                errors.push(ValidationError::new(format!(
                    "bind `{}.{}` -> `{}.{}` selects unknown backend `{}`",
                    bind.from.instance.name,
                    bind.from.port,
                    bind.to.instance.name,
                    bind.to.port,
                    bind.backend.0
                )));
                continue;
            }
            let Some(route) = route_facts.get(bind.id.0.as_str()).copied() else {
                continue;
            };
            if let Some(backend_capabilities) = backend_capabilities(&route.backend.0) {
                let missing_required_capabilities = route
                    .capability_requirements
                    .iter()
                    .filter(|required| !backend_capabilities.contains(required))
                    .collect::<Vec<_>>();
                if !missing_required_capabilities.is_empty() {
                    errors.push(ValidationError::new(format!(
                        "backend `{}` selected by bind `{}.{}` -> `{}.{}` cannot satisfy route capabilities",
                        route.backend.0,
                        bind.from.instance.name,
                        bind.from.port,
                        bind.to.instance.name,
                        bind.to.port
                    )));
                }
            }
            let Some(source_target) = instance_target(ir, graph, &bind.from.instance.name) else {
                continue;
            };
            let Some(target_target) = instance_target(ir, graph, &bind.to.instance.name) else {
                continue;
            };
            for target in [source_target, target_target] {
                if !checked_targets.insert(target.name.as_str()) {
                    continue;
                }
                let decision = deployment_capability_decision(
                    &route.backend,
                    &target.backends,
                    &route.capability_requirements,
                );
                if !decision.target_supports_selected_backend {
                    errors.push(ValidationError::new(format!(
                        "target `{}` does not support backend `{}` selected by bind `{}.{}` -> `{}.{}`",
                        target.name,
                        route.backend.0,
                        bind.from.instance.name,
                        bind.from.port,
                        bind.to.instance.name,
                        bind.to.port
                    )));
                }
                if decision.selected_backend_known
                    && !decision.missing_required_capabilities.is_empty()
                {
                    continue;
                }
            }
        }

        for service in &graph.services {
            validate_endpoint_backend_targets(
                ir,
                graph,
                &service.backend,
                &service.client.instance.name,
                &service.server.instance.name,
                &format!(
                    "service bind `{}.{} -> {}.{}`",
                    service.client.instance.name,
                    service.client.port,
                    service.server.instance.name,
                    service.server.port
                ),
                errors,
            );
        }

        for operation in &graph.operations {
            validate_endpoint_backend_targets(
                ir,
                graph,
                &operation.backend,
                &operation.client.instance.name,
                &operation.server.instance.name,
                &format!(
                    "operation bind `{}.{} -> {}.{}`",
                    operation.client.instance.name,
                    operation.client.port,
                    operation.server.instance.name,
                    operation.server.port
                ),
                errors,
            );
        }

        for external in &graph.external_processes {
            for backend in &external.required_backends {
                if !is_known_backend(&backend.0) {
                    continue;
                }
                let mut checked_targets = BTreeSet::new();
                for target in targets_for_process(ir, graph, &external.process) {
                    if !checked_targets.insert(target.name.as_str()) {
                        continue;
                    }
                    let decision = deployment_capability_decision(backend, &target.backends, &[]);
                    if !decision.target_supports_selected_backend {
                        errors.push(ValidationError::new(format!(
                            "target `{}` does not support backend `{}` required by external_process `{}`",
                            target.name, backend.0, external.process
                        )));
                    }
                }
            }
        }
    }
}

fn validate_endpoint_backend_targets(
    ir: &ContractIr,
    graph: &GraphIr,
    backend: &BackendName,
    first_instance: &str,
    second_instance: &str,
    description: &str,
    errors: &mut Vec<ValidationError>,
) {
    if !is_known_backend(&backend.0) {
        return;
    }
    let mut checked_targets = BTreeSet::new();
    for instance_name in [first_instance, second_instance] {
        let Some(target) = instance_target(ir, graph, instance_name) else {
            continue;
        };
        if !checked_targets.insert(target.name.as_str()) {
            continue;
        }
        let decision = deployment_capability_decision(backend, &target.backends, &[]);
        if !decision.target_supports_selected_backend {
            errors.push(ValidationError::new(format!(
                "target `{}` does not support backend `{}` selected by {description}",
                target.name, backend.0
            )));
        }
    }
}

fn targets_for_process<'a>(
    ir: &'a ContractIr,
    graph: &'a GraphIr,
    process_name: &str,
) -> Vec<&'a flowrt_ir::TargetIr> {
    graph
        .instances
        .iter()
        .filter(|instance| instance.process.as_deref().unwrap_or("main") == process_name)
        .filter_map(|instance| instance_target(ir, graph, &instance.name))
        .collect()
}

fn policy_anchor_profile(ir: &ContractIr) -> Option<&flowrt_ir::ProfileIr> {
    ir.profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| ir.profiles.first())
}

fn push_policy_source_error(bind: &ChannelEdgeIr, errors: &mut Vec<ValidationError>) {
    errors.push(ValidationError::new(format!(
        "bind `{}.{}` -> `{}.{}` policy source metadata is inconsistent with selected profile defaults",
        bind.from.instance.name, bind.from.port, bind.to.instance.name, bind.to.port
    )));
}

fn push_backend_source_error(bind: &ChannelEdgeIr, errors: &mut Vec<ValidationError>) {
    errors.push(ValidationError::new(format!(
        "bind `{}.{}` -> `{}.{}` backend source metadata is inconsistent with backend resolver semantics",
        bind.from.instance.name, bind.from.port, bind.to.instance.name, bind.to.port
    )));
}

fn capabilities_match(actual: &[CapabilityAtom], expected: &[CapabilityAtom]) -> bool {
    actual == expected
}

fn derive_facts(
    ir: &ContractIr,
    errors: &mut Vec<ValidationError>,
) -> Option<ContractDerivedFacts> {
    match derive_contract_facts(ir) {
        Ok(facts) => Some(facts),
        Err(error) => {
            let message = format!("contract derived facts could not be recomputed: {error}");
            if !errors.iter().any(|error| error.message == message) {
                errors.push(ValidationError::new(message));
            }
            None
        }
    }
}

fn graph_facts_by_name(facts: &ContractDerivedFacts) -> BTreeMap<&str, &GraphDerivedFacts> {
    facts
        .graphs
        .iter()
        .map(|facts| (facts.graph.name.as_str(), facts))
        .collect()
}

fn instance_target<'a>(
    ir: &'a ContractIr,
    graph: &'a GraphIr,
    instance_name: &str,
) -> Option<&'a flowrt_ir::TargetIr> {
    let instance = graph
        .instances
        .iter()
        .find(|instance| instance.name == instance_name)?;
    let target_name = instance
        .target
        .as_ref()
        .map(|target| target.name.as_str())?;
    ir.targets.iter().find(|target| target.name == target_name)
}
