use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    BackendName, CapabilityAtom, ChannelBackendSource, ChannelEdgeIr, ContractIr, GraphIr,
    InstanceIr, LanguageKind, PolicyValueSource, PortRef, RouteTopology, backend_capabilities,
    channel_route_capabilities, deployment_capability_decision, graph_required_capabilities,
    is_known_backend, target_capabilities,
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
    let expected_capabilities_by_target = ir
        .targets
        .iter()
        .map(|target| (target.name.as_str(), target_capabilities(&target.backends)))
        .collect::<BTreeMap<_, _>>();
    let expected_capabilities_by_graph = ir
        .graphs
        .iter()
        .map(|graph| {
            (
                graph.name.as_str(),
                graph_required_capabilities(graph, &ir.types, &ir.components),
            )
        })
        .collect::<BTreeMap<_, _>>();

    for graph in &ir.graphs {
        let source_types = source_types_by_route(ir, graph);
        let route_topologies = route_topology_by_bind_id(ir, graph);
        for bind in &graph.binds {
            let Some(source_type) = source_types.get(&bind.id) else {
                continue;
            };
            let topology = route_topologies
                .get(&bind.id)
                .copied()
                .unwrap_or_else(RouteTopology::local);
            let expected = expected_bind_capabilities(ir, bind, source_type, topology);
            if !capabilities_match(&bind.capability_requirements, &expected) {
                errors.push(ValidationError::new(format!(
                    "bind `{}.{}` -> `{}.{}` capability requirements do not match channel policy",
                    bind.from.instance.name, bind.from.port, bind.to.instance.name, bind.to.port
                )));
            }
        }
    }

    for target in &ir.targets {
        let expected = &expected_capabilities_by_target[target.name.as_str()];
        if !capabilities_match(&target.capabilities, expected) {
            errors.push(ValidationError::new(format!(
                "target `{}` capabilities do not match declared backends",
                target.name
            )));
        }
    }

    for deployment in &ir.deployments {
        if let Some(expected) = expected_capabilities_by_graph.get(deployment.graph.name.as_str()) {
            if !capabilities_match(&deployment.required_capabilities, expected) {
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
    let Some(profile) = policy_anchor_profile(ir) else {
        return;
    };

    for graph in &ir.graphs {
        let source_types = source_types_by_route(ir, graph);
        let route_topologies = route_topology_by_bind_id(ir, graph);
        for bind in &graph.binds {
            match bind.backend_policy_source {
                PolicyValueSource::Explicit => {
                    let Some(source_type) = source_types.get(&bind.id) else {
                        continue;
                    };
                    let topology = route_topologies
                        .get(&bind.id)
                        .copied()
                        .unwrap_or_else(RouteTopology::local);
                    let expected = expected_explicit_backend(
                        bind.backend.0.as_str(),
                        ir,
                        source_type,
                        topology,
                    );
                    let explicit_semantics_match = expected
                        .as_ref()
                        .map(|expected| {
                            bind.backend.0 == expected.backend
                                && bind.backend_source == expected.source
                        })
                        .unwrap_or(false);
                    if bind.backend_source != ChannelBackendSource::Explicit
                        || !explicit_semantics_match
                    {
                        push_backend_source_error(bind, errors);
                    }
                }
                PolicyValueSource::ProfileDefault => {
                    let Some(source_type) = source_types.get(&bind.id) else {
                        continue;
                    };
                    let topology = route_topologies
                        .get(&bind.id)
                        .copied()
                        .unwrap_or_else(RouteTopology::local);
                    let expected = expected_profile_default_backend(
                        profile.backend.0.as_str(),
                        ir,
                        source_type,
                        topology,
                    );
                    if bind.backend.0 != expected.backend || bind.backend_source != expected.source
                    {
                        push_backend_source_error(bind, errors);
                    }
                }
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
    let graphs = ir
        .graphs
        .iter()
        .map(|graph| (graph.name.as_str(), graph))
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

        let Some(target) = targets.get(deployment.target.name.as_str()) else {
            continue;
        };

        let Some(graph) = graphs.get(deployment.graph.name.as_str()) else {
            continue;
        };

        let expected_required_capabilities =
            graph_required_capabilities(graph, &ir.types, &ir.components);
        let decision = deployment_capability_decision(
            &deployment.backend,
            &target.backends,
            &expected_required_capabilities,
        );

        if !decision.target_supports_selected_backend {
            errors.push(ValidationError::new(format!(
                "target `{}` does not support backend `{}` selected by profile `{}`",
                deployment.target.name, deployment.backend.0, deployment.profile.name
            )));
        }
        if decision.selected_backend_known && !decision.missing_required_capabilities.is_empty() {
            errors.push(ValidationError::new(format!(
                "backend `{}` selected by profile `{}` cannot satisfy required capabilities for graph `{}`",
                deployment.backend.0, deployment.profile.name, deployment.graph.name
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
    for graph in &ir.graphs {
        let source_types = source_types_by_route(ir, graph);
        let route_topologies = route_topology_by_bind_id(ir, graph);
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
            let Some(source_type) = source_types.get(&bind.id) else {
                continue;
            };
            let topology = route_topologies
                .get(&bind.id)
                .copied()
                .unwrap_or_else(RouteTopology::local);
            let expected = expected_bind_capabilities(ir, bind, source_type, topology);
            if let Some(backend_capabilities) = backend_capabilities(&bind.backend.0) {
                let missing_required_capabilities = expected
                    .iter()
                    .filter(|required| !backend_capabilities.contains(required))
                    .collect::<Vec<_>>();
                if !missing_required_capabilities.is_empty() {
                    errors.push(ValidationError::new(format!(
                        "backend `{}` selected by bind `{}.{}` -> `{}.{}` cannot satisfy route capabilities",
                        bind.backend.0,
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
                let decision =
                    deployment_capability_decision(&bind.backend, &target.backends, &expected);
                if !decision.target_supports_selected_backend {
                    errors.push(ValidationError::new(format!(
                        "target `{}` does not support backend `{}` selected by bind `{}.{}` -> `{}.{}`",
                        target.name,
                        bind.backend.0,
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

pub(crate) fn expected_bind_capabilities(
    ir: &ContractIr,
    bind: &ChannelEdgeIr,
    source_type: &flowrt_ir::TypeExpr,
    topology: RouteTopology,
) -> Vec<CapabilityAtom> {
    channel_route_capabilities(
        &ir.types,
        source_type,
        bind.channel,
        bind.overflow,
        bind.stale,
        topology,
    )
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

struct ExpectedChannelBackend {
    backend: String,
    source: ChannelBackendSource,
}

fn expected_profile_default_backend(
    profile_backend: &str,
    ir: &ContractIr,
    source_type: &flowrt_ir::TypeExpr,
    topology: RouteTopology,
) -> ExpectedChannelBackend {
    let resolved = flowrt_ir::resolve_channel_backend(
        profile_backend,
        Some(source_type),
        &ir.types,
        topology,
        false,
    )
    .expect("profile default backend resolution should not fail without explicit backend");
    ExpectedChannelBackend {
        backend: resolved.backend,
        source: resolved.source,
    }
}

fn expected_explicit_backend(
    backend: &str,
    ir: &ContractIr,
    source_type: &flowrt_ir::TypeExpr,
    topology: RouteTopology,
) -> Option<ExpectedChannelBackend> {
    let resolved =
        flowrt_ir::resolve_channel_backend(backend, Some(source_type), &ir.types, topology, true)
            .ok()?;
    Some(ExpectedChannelBackend {
        backend: resolved.backend,
        source: resolved.source,
    })
}

fn capabilities_match(actual: &[CapabilityAtom], expected: &[CapabilityAtom]) -> bool {
    actual == expected
}

fn source_types_by_route(
    ir: &ContractIr,
    graph: &GraphIr,
) -> BTreeMap<flowrt_ir::EntityId, flowrt_ir::TypeExpr> {
    let components = ir
        .components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    let instances = graph
        .instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    graph
        .binds
        .iter()
        .filter_map(|bind| {
            let instance = instances.get(bind.from.instance.name.as_str())?;
            let component = components.get(instance.component.name.as_str())?;
            let port = component
                .outputs
                .iter()
                .find(|port| port.name == bind.from.port)?;
            Some((bind.id.clone(), port.ty.clone()))
        })
        .collect()
}

fn route_topology_by_bind_id(
    ir: &ContractIr,
    graph: &GraphIr,
) -> BTreeMap<flowrt_ir::EntityId, RouteTopology> {
    let instances = graph
        .instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let components = ir
        .components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    graph
        .binds
        .iter()
        .map(|bind| {
            (
                bind.id.clone(),
                route_topology(&instances, &components, &bind.from, &bind.to),
            )
        })
        .collect()
}

fn route_topology(
    instances: &BTreeMap<&str, &InstanceIr>,
    components: &BTreeMap<&str, &flowrt_ir::ComponentIr>,
    from: &PortRef,
    to: &PortRef,
) -> RouteTopology {
    let from_instance = instances.get(from.instance.name.as_str()).copied();
    let to_instance = instances.get(to.instance.name.as_str()).copied();
    let from_process = from_instance
        .and_then(|instance| instance.process.as_deref())
        .unwrap_or("main");
    let to_process = to_instance
        .and_then(|instance| instance.process.as_deref())
        .unwrap_or("main");
    let from_target = from_instance
        .and_then(|instance| instance.target.as_ref())
        .map(|target| target.name.as_str());
    let to_target = to_instance
        .and_then(|instance| instance.target.as_ref())
        .map(|target| target.name.as_str());
    let crosses_process = from_process != to_process;
    let crosses_target = from_target.is_some() && to_target.is_some() && from_target != to_target;
    let touches_external = [from_instance, to_instance].iter().any(|instance| {
        instance
            .and_then(|instance| components.get(instance.component.name.as_str()))
            .is_some_and(|component| component.language == LanguageKind::External)
    });
    if touches_external {
        RouteTopology::with_external(crosses_process, crosses_target)
    } else {
        RouteTopology::new(crosses_process, crosses_target)
    }
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
