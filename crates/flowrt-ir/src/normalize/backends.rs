use std::collections::BTreeMap;

use crate::{
    ChannelBackendSource, ComponentIr, EntityId, GraphIr, InstanceIr, IrError, LanguageKind,
    PortRef, Result, RouteTopology, TypeExpr, TypeIr,
};

pub(super) struct ResolvedChannelBackend {
    pub(super) backend: String,
    pub(super) source: ChannelBackendSource,
}

pub(super) fn resolve_channel_backend(
    requested_backend: &str,
    source_type: Option<&TypeExpr>,
    types: &[TypeIr],
    topology: RouteTopology,
    explicit: bool,
) -> Result<ResolvedChannelBackend> {
    if topology.touches_external {
        if matches!(requested_backend, "inproc" | "iox2") && explicit {
            return Err(IrError::InvalidValue {
                context: "bind.dataflow.backend".to_string(),
                message: format!(
                    "external dataflow route cannot use `{requested_backend}` backend"
                ),
            });
        }
        return Ok(ResolvedChannelBackend {
            backend: "zenoh".to_string(),
            source: ChannelBackendSource::AutoFallback,
        });
    }
    if requested_backend == "iox2"
        && source_type.is_some_and(|ty| type_expr_contains_variable_data(ty, types))
    {
        return Ok(ResolvedChannelBackend {
            backend: "zenoh".to_string(),
            source: ChannelBackendSource::AutoFallback,
        });
    }
    Ok(ResolvedChannelBackend {
        backend: requested_backend.to_string(),
        source: ChannelBackendSource::ProfileDefault,
    })
}

pub(super) fn source_port_types_by_endpoint(
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

pub(super) fn route_topology_by_bind_id(
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

pub(super) fn route_topology(
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

fn type_expr_contains_variable_data(expr: &TypeExpr, types: &[TypeIr]) -> bool {
    let types_by_name = types
        .iter()
        .map(|ty| (ty.qualified_name.as_str(), ty))
        .collect::<BTreeMap<_, _>>();
    let mut visiting = std::collections::BTreeSet::new();
    type_expr_contains_variable_data_inner(expr, &types_by_name, &mut visiting)
}

fn type_expr_contains_variable_data_inner(
    expr: &TypeExpr,
    types_by_name: &BTreeMap<&str, &TypeIr>,
    visiting: &mut std::collections::BTreeSet<String>,
) -> bool {
    match expr {
        TypeExpr::VarBytes | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => true,
        TypeExpr::Array { element, .. } => {
            type_expr_contains_variable_data_inner(element, types_by_name, visiting)
        }
        TypeExpr::Named { name } => {
            if !visiting.insert(name.clone()) {
                return false;
            }
            let contains = types_by_name.get(name.as_str()).is_some_and(|ty| {
                ty.fields.iter().any(|field| {
                    type_expr_contains_variable_data_inner(&field.ty, types_by_name, visiting)
                })
            });
            visiting.remove(name);
            contains
        }
        TypeExpr::Primitive { .. } => false,
    }
}
