use std::collections::BTreeMap;

use crate::{
    ComponentIr, EntityId, GraphIr, InstanceIr, LanguageKind, PortRef, RouteTopology, TypeExpr,
};

pub(super) use crate::resolve_channel_backend;

pub(crate) fn source_port_types_by_endpoint(
    components: &[ComponentIr],
    instances: &[InstanceIr],
) -> BTreeMap<(String, String), TypeExpr> {
    let components = components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    let mut ports = BTreeMap::new();
    for instance in instances {
        let Some(component) = components.get(instance.component.name.as_str()) else {
            continue;
        };
        for output in &component.outputs {
            ports.insert(
                (instance.name.clone(), output.name.clone()),
                output.ty.clone(),
            );
        }
    }
    ports
}

pub(crate) fn route_topology_by_bind_id(
    graph: &GraphIr,
    components: &[ComponentIr],
) -> BTreeMap<EntityId, RouteTopology> {
    let instances = graph
        .instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let components = components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    graph
        .binds
        .iter()
        .map(|bind| {
            (
                bind.id.clone(),
                route_topology(&instances, Some(&components), &bind.from, &bind.to),
            )
        })
        .collect()
}

pub(crate) fn route_topology(
    instances: &BTreeMap<&str, &InstanceIr>,
    components: Option<&BTreeMap<&str, &ComponentIr>>,
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
    let touches_external = components.is_some_and(|components| {
        [from_instance, to_instance].iter().any(|instance| {
            instance
                .and_then(|instance| components.get(instance.component.name.as_str()))
                .is_some_and(|component| component.language == LanguageKind::External)
        })
    });
    let crosses_process = from_process != to_process;
    let crosses_target = from_target.is_some() && to_target.is_some() && from_target != to_target;
    if touches_external {
        RouteTopology::with_external(crosses_process, crosses_target)
    } else {
        RouteTopology::new(crosses_process, crosses_target)
    }
}
