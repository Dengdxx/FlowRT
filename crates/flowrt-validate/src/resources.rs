use std::collections::{BTreeMap, BTreeSet};

use flowrt_ir::{
    CapabilityAtom, ComponentIr, ContractIr, GraphIr, ResourceAccess, ResourceProviderIr,
    ResourceProviderScope, derive_resource_satisfactions,
};

use crate::ValidationError;

pub(crate) fn validate_resources(ir: &ContractIr, errors: &mut Vec<ValidationError>) {
    for component in &ir.components {
        validate_component_resource_requirements(component, errors);
    }
    for graph in &ir.graphs {
        validate_resource_providers(graph, ir, errors);
        let expected = derive_resource_satisfactions(
            &graph.name,
            &graph.instances,
            &ir.components,
            &graph.resource_providers,
        );
        if graph.resource_satisfactions != expected {
            errors.push(ValidationError::new(format!(
                "graph `{}` resource satisfaction metadata is not canonical",
                graph.name
            )));
        }
        validate_resource_satisfaction_rules(graph, &expected, errors);
    }
}

fn validate_component_resource_requirements(
    component: &ComponentIr,
    errors: &mut Vec<ValidationError>,
) {
    for resource in &component.resources {
        validate_capability_atom(
            &format!(
                "component `{}` resource `{}`",
                component.name, resource.name
            ),
            &resource.capability,
            errors,
        );
    }
}

fn validate_resource_providers(
    graph: &GraphIr,
    ir: &ContractIr,
    errors: &mut Vec<ValidationError>,
) {
    let target_names = ir
        .targets
        .iter()
        .map(|target| target.name.as_str())
        .collect::<BTreeSet<_>>();
    let process_names = graph
        .processes
        .iter()
        .map(|process| process.name.as_str())
        .collect::<BTreeSet<_>>();
    let external_packages = graph
        .external_processes
        .iter()
        .map(|process| process.package.as_str())
        .collect::<BTreeSet<_>>();

    for provider in &graph.resource_providers {
        if provider.capabilities.is_empty() {
            errors.push(ValidationError::new(format!(
                "resource provider `{}` must declare at least one capability",
                provider.name
            )));
        }
        for capability in &provider.capabilities {
            validate_capability_atom(
                &format!("resource provider `{}` capability", provider.name),
                capability,
                errors,
            );
        }
        validate_provider_source(
            &format!("resource provider `{}` health_source", provider.name),
            &provider.health_source,
            errors,
        );
        validate_provider_source(
            &format!("resource provider `{}` readiness_source", provider.name),
            &provider.readiness_source,
            errors,
        );
        validate_provider_scope_refs(
            provider,
            &target_names,
            &process_names,
            &external_packages,
            errors,
        );
    }
}

fn validate_provider_scope_refs(
    provider: &ResourceProviderIr,
    target_names: &BTreeSet<&str>,
    process_names: &BTreeSet<&str>,
    external_packages: &BTreeSet<&str>,
    errors: &mut Vec<ValidationError>,
) {
    match provider.scope {
        ResourceProviderScope::Target => {
            if let Some(target) = &provider.target
                && !target_names.contains(target.name.as_str())
            {
                errors.push(ValidationError::new(format!(
                    "resource provider `{}` target reference references unknown target `{}`",
                    provider.name, target.name
                )));
            }
            if provider.process.is_some() || provider.external_package.is_some() {
                errors.push(ValidationError::new(format!(
                    "resource provider `{}` has target scope but declares process or external_package owner",
                    provider.name
                )));
            }
        }
        ResourceProviderScope::Process => {
            if let Some(process) = &provider.process
                && !process_names.contains(process.as_str())
            {
                errors.push(ValidationError::new(format!(
                    "resource provider `{}` process reference points to unknown process `{process}`",
                    provider.name
                )));
            }
            if provider.target.is_some() || provider.external_package.is_some() {
                errors.push(ValidationError::new(format!(
                    "resource provider `{}` has process scope but declares target or external_package owner",
                    provider.name
                )));
            }
        }
        ResourceProviderScope::ExternalPackage => {
            if let Some(package) = &provider.external_package
                && !external_packages.contains(package.as_str())
            {
                errors.push(ValidationError::new(format!(
                    "resource provider `{}` external package reference points to unknown external package `{package}`",
                    provider.name
                )));
            }
            if provider.target.is_some() || provider.process.is_some() {
                errors.push(ValidationError::new(format!(
                    "resource provider `{}` has external_package scope but declares target or process owner",
                    provider.name
                )));
            }
        }
    }
}

fn validate_resource_satisfaction_rules(
    graph: &GraphIr,
    expected: &[flowrt_ir::ResourceSatisfactionIr],
    errors: &mut Vec<ValidationError>,
) {
    for satisfaction in expected {
        if satisfaction.required && !satisfaction.satisfied {
            errors.push(ValidationError::new(format!(
                "instance `{}` resource `{}` requires capability `{}` but no provider satisfies it",
                satisfaction.instance.name, satisfaction.resource, satisfaction.capability.0
            )));
        }
    }

    let providers = graph
        .resource_providers
        .iter()
        .map(|provider| (provider.name.as_str(), provider))
        .collect::<BTreeMap<_, _>>();
    let mut consumers = BTreeMap::<(&str, &str, &'static str), Vec<(&str, ResourceAccess)>>::new();
    for satisfaction in expected {
        if !satisfaction.satisfied {
            continue;
        }
        let Some(provider_ref) = &satisfaction.provider else {
            continue;
        };
        let Some(provider) = providers.get(provider_ref.name.as_str()).copied() else {
            continue;
        };
        consumers
            .entry((
                provider.name.as_str(),
                satisfaction.capability.0.as_str(),
                provider_scope_name(provider.scope),
            ))
            .or_default()
            .push((satisfaction.instance.name.as_str(), satisfaction.access));
    }
    for ((provider, capability, scope), consumers) in consumers {
        if consumers.len() > 1
            && consumers
                .iter()
                .any(|(_, access)| *access == ResourceAccess::Exclusive)
        {
            let names = consumers
                .iter()
                .map(|(instance, _)| *instance)
                .collect::<Vec<_>>();
            errors.push(ValidationError::new(format!(
                "exclusive resource provider `{provider}` capability `{capability}` is shared by multiple active instances in scope `{scope}`: {}",
                names.join(", ")
            )));
        }
    }
}

fn provider_scope_name(scope: ResourceProviderScope) -> &'static str {
    match scope {
        ResourceProviderScope::Target => "target",
        ResourceProviderScope::Process => "process",
        ResourceProviderScope::ExternalPackage => "external_package",
    }
}

fn validate_capability_atom(
    context: &str,
    capability: &CapabilityAtom,
    errors: &mut Vec<ValidationError>,
) {
    if capability.0.trim().is_empty() {
        errors.push(ValidationError::new(format!(
            "{context} capability must not be empty"
        )));
        return;
    }
    if let Some(term) = concrete_resource_term(&capability.0) {
        errors.push(ValidationError::new(format!(
            "{context} capability `{}` contains concrete hardware/protocol term `{term}`",
            capability.0
        )));
    }
}

fn validate_provider_source(context: &str, source: &str, errors: &mut Vec<ValidationError>) {
    if source.trim().is_empty() {
        errors.push(ValidationError::new(format!("{context} must not be empty")));
        return;
    }
    if let Some(term) = concrete_resource_term(source) {
        errors.push(ValidationError::new(format!(
            "{context} `{source}` contains concrete hardware/protocol term `{term}`"
        )));
    }
}

fn concrete_resource_term(value: &str) -> Option<&'static str> {
    const BANNED: &[&str] = &[
        "serial", "tcp", "udp", "usb", "v4l2", "rknn", "cuda", "tty", "baud", "sdk",
    ];
    let lower = value.to_ascii_lowercase();
    if lower.contains("/dev/") {
        return Some("/dev");
    }
    for concrete_phrase in ["device_path", "dev_path", "port_number", "board_sdk"] {
        if lower.contains(concrete_phrase) {
            return Some(concrete_phrase);
        }
    }
    lower
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .find_map(|token| BANNED.iter().copied().find(|banned| token == *banned))
}
