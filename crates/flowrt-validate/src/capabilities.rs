use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    CapabilityAtom, ChannelEdgeIr, ContractIr, LanguageKind, PolicyValueSource,
    channel_capabilities, deployment_capability_decision, graph_required_capabilities,
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
        for bind in &graph.binds {
            let expected = expected_bind_capabilities(bind);
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

pub(crate) fn expected_bind_capabilities(bind: &ChannelEdgeIr) -> Vec<CapabilityAtom> {
    channel_capabilities(bind.channel, bind.overflow, bind.stale)
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

fn capabilities_match(actual: &[CapabilityAtom], expected: &[CapabilityAtom]) -> bool {
    actual == expected
}
