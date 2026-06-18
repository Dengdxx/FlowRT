use std::collections::BTreeMap;

use flowrt_rsdl::{RawDocument, RawModuleDocument};

use crate::{ContractIr, EntityRef, GraphIr, Result};

use super::{graphs, ids, modules, operations, package, profiles, resolver, services, targets};

pub(super) fn normalize_document_with_modules(
    document: &RawDocument,
    raw_modules: &[RawModuleDocument],
    source_hash: String,
) -> Result<ContractIr> {
    let normalized_package = package::normalize_package(document);
    let mut name_resolver = resolver::NameResolver::new(raw_modules);
    name_resolver.register_document_symbols(document);

    let normalized_modules = modules::normalize_modules(raw_modules);
    let types = modules::normalize_types(document, raw_modules, &name_resolver)?;
    let type_ids = types
        .iter()
        .map(|ty| (ty.qualified_name.clone(), ty.id.clone()))
        .collect::<BTreeMap<_, _>>();
    let components =
        modules::normalize_components(document, raw_modules, &name_resolver, &type_ids)?;

    let profiles = profiles::normalize_profiles(document)?;
    let targets = targets::normalize_targets(document)?;
    let graph = normalize_default_graph(
        document,
        raw_modules,
        &name_resolver,
        &types,
        &components,
        &profiles,
        &targets,
    )?;
    let deployments =
        graphs::normalize_deployments(&graph, &types, &components, &profiles, &targets);

    Ok(ContractIr {
        ir_version: crate::CONTRACT_IR_VERSION.to_string(),
        schema_version: crate::CONTRACT_SCHEMA_VERSION.to_string(),
        source_hash,
        artifact: Default::default(),
        package_id: normalized_package.id,
        package: normalized_package.package,
        modules: normalized_modules,
        types,
        components,
        graphs: vec![graph],
        profiles,
        targets,
        deployments,
    })
}

fn normalize_default_graph(
    document: &RawDocument,
    raw_modules: &[RawModuleDocument],
    name_resolver: &resolver::NameResolver,
    types: &[crate::TypeIr],
    components: &[crate::ComponentIr],
    profiles: &[crate::ProfileIr],
    targets: &[crate::TargetIr],
) -> Result<GraphIr> {
    let component_ids = components
        .iter()
        .map(|component| (component.qualified_name.clone(), component.id.clone()))
        .collect::<BTreeMap<_, _>>();
    let component_concurrency = components
        .iter()
        .map(|component| (component.qualified_name.clone(), component.concurrency))
        .collect::<BTreeMap<_, _>>();
    let target_ids = targets
        .iter()
        .map(|target| (target.name.clone(), target.id.clone()))
        .collect::<BTreeMap<_, _>>();

    let graph_id = ids::entity_id("graph", "default");
    let graph_name = "default".to_string();
    let (instances, mut tasks) = graphs::normalize_instances(
        document,
        raw_modules,
        name_resolver,
        &component_ids,
        &component_concurrency,
        &target_ids,
        &graph_name,
    )?;
    tasks.sort_by(|left, right| {
        (&left.instance.name, &left.name).cmp(&(&right.instance.name, &right.name))
    });
    let instance_refs = instances
        .iter()
        .map(|instance| {
            (
                instance.name.clone(),
                EntityRef {
                    id: instance.id.clone(),
                    name: instance.name.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let binds = graphs::normalize_binds(
        document,
        &instance_refs,
        types,
        components,
        &instances,
        profiles,
    )?;
    let processes = graphs::normalize_processes(document, &instances)?;
    let external_processes = graphs::normalize_external_processes(document)?;
    let resource_providers = graphs::normalize_resource_providers(
        document,
        &graph_name,
        &target_ids,
        &processes,
        &external_processes,
    )?;
    let resource_satisfactions = graphs::derive_resource_satisfactions(
        &graph_name,
        &instances,
        components,
        &resource_providers,
    );
    let service_edges = services::normalize_service_binds(
        document,
        &instance_refs,
        &instances,
        components,
        &graph_name,
    )?;
    let operation_edges = operations::normalize_operation_binds(
        document,
        &instance_refs,
        &instances,
        components,
        &graph_name,
    )?;
    let boundary_endpoints =
        graphs::normalize_boundary_endpoints(document, &instance_refs, name_resolver, &graph_name)?;
    let ros2_bridges = services::normalize_ros2_bridges(
        document,
        &instance_refs,
        &boundary_endpoints,
        &graph_name,
    )?;
    let sync_groups = graphs::normalize_sync_groups(document, &instance_refs, &graph_name)?;
    let redundancy_groups =
        graphs::normalize_redundancy_groups(document, &instance_refs, &graph_name)?;

    Ok(GraphIr {
        id: graph_id,
        name: graph_name,
        health: graphs::normalize_graph_health(document, &instance_refs)?,
        instances,
        processes,
        external_processes,
        resource_providers,
        resource_satisfactions,
        tasks,
        binds,
        services: service_edges,
        operations: operation_edges,
        boundary_endpoints,
        ros2_bridges,
        sync_groups,
        redundancy_groups,
    })
}
