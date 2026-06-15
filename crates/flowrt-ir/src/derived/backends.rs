use crate::{
    BackendName, BackendThreadAffinity, CapabilityAtom, ChannelBackendSource, ContractIr, EntityId,
    GraphIr, PolicyValueSource, PortRef, Result, RouteTopology, channel_capabilities,
    channel_route_capabilities, normalize::backends::route_topology_by_bind_id,
    normalize::backends::source_port_types_by_endpoint, resolve_channel_backend,
};

/// 单条 dataflow route 重新推导得到的 backend、topology 和 capability 事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDerivedFacts {
    /// Route 对应的 bind id。
    pub bind_id: EntityId,
    /// Route source 端口。
    pub from: PortRef,
    /// Route sink 端口。
    pub to: PortRef,
    /// Route 是否跨 process、target 或 external process 边界。
    pub topology: RouteTopology,
    /// Route 实际应使用的 backend。
    pub backend: BackendName,
    /// Route backend 来源。
    pub backend_source: ChannelBackendSource,
    /// Route backend 的线程亲和事实。
    pub thread_affinity: Option<BackendThreadAffinity>,
    /// Route backend 必须满足的 capability requirements。
    pub capability_requirements: Vec<CapabilityAtom>,
}

pub(super) fn derive_route_facts(
    contract: &ContractIr,
    graph: &GraphIr,
) -> Result<Vec<RouteDerivedFacts>> {
    let source_port_types = source_port_types_by_endpoint(&contract.components, &graph.instances);
    let route_topologies = route_topology_by_bind_id(graph, &contract.components);
    let default_backend = contract
        .profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| contract.profiles.first())
        .map(|profile| profile.backend.0.as_str())
        .unwrap_or("inproc");

    graph
        .binds
        .iter()
        .map(|bind| {
            let topology = route_topologies
                .get(&bind.id)
                .copied()
                .unwrap_or_else(RouteTopology::local);
            let source_type =
                source_port_types.get(&(bind.from.instance.name.clone(), bind.from.port.clone()));
            let explicit_backend = bind.backend_policy_source == PolicyValueSource::Explicit;
            let requested_backend = if explicit_backend {
                bind.backend.0.as_str()
            } else {
                default_backend
            };
            let resolved_backend = resolve_channel_backend(
                requested_backend,
                source_type,
                &contract.types,
                topology,
                explicit_backend,
            )?;
            let capability_requirements = match source_type {
                Some(source_type) => channel_route_capabilities(
                    &contract.types,
                    source_type,
                    bind.channel,
                    bind.overflow,
                    bind.stale,
                    topology,
                ),
                None => channel_capabilities(bind.channel, bind.overflow, bind.stale),
            };

            Ok(RouteDerivedFacts {
                bind_id: bind.id.clone(),
                from: bind.from.clone(),
                to: bind.to.clone(),
                topology,
                backend: BackendName(resolved_backend.backend.clone()),
                backend_source: resolved_backend.source,
                thread_affinity: BackendThreadAffinity::for_backend(&resolved_backend.backend),
                capability_requirements,
            })
        })
        .collect()
}
