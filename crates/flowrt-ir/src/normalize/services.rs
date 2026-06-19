use std::collections::BTreeMap;

use flowrt_rsdl::RawDocument;

use crate::{
    BackendName, BoundaryDirection, BoundaryEndpointIr, ComponentIr, EntityRef, InstanceIr,
    IrError, LanguageKind, PolicyValueSource, ProfileIr, Result, Ros2BridgeDirection, Ros2BridgeIr,
    SERVICE_DEFAULT_MAX_IN_FLIGHT, SERVICE_DEFAULT_QUEUE_DEPTH, SERVICE_DEFAULT_TIMEOUT_MS,
    ServiceEdgeIr, ServiceOverflowPolicy, ServicePolicyIr, ServicePolicySourceIr, ServicePortIr,
    ServicePortRef, TypeExpr, TypeIr,
};

use super::ids::entity_id;

pub(super) fn normalize_service_binds(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    instances: &[InstanceIr],
    components: &[ComponentIr],
    types: &[TypeIr],
    profiles: &[ProfileIr],
    graph_name: &str,
) -> Result<Vec<ServiceEdgeIr>> {
    let default_backend = profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or(profiles.first())
        .map(|profile| profile.backend.0.as_str())
        .unwrap_or("inproc");
    let instances_by_name = instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let components_by_name = components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();

    let mut services = document
        .service_binds
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let client = parse_service_port_ref(&raw.client, instance_refs)?;
            let server = parse_service_port_ref(&raw.server, instance_refs)?;
            let context = format!("bind.service[{index}]");

            let topology =
                service_route_topology(&instances_by_name, &components_by_name, &client, &server);
            let (request_type, response_type) =
                service_port_types(&instances_by_name, &components_by_name, &client, &server)
                    .map(|(request, response)| (Some(request), Some(response)))
                    .unwrap_or((None, None));
            let requested = raw.backend.as_deref().unwrap_or(default_backend);
            let explicit = raw.backend.is_some();
            let resolution = crate::backend::resolve_service_backend(
                requested,
                request_type,
                response_type,
                types,
                topology,
                explicit,
            )?;

            let policy = parse_service_policy(&context, raw)?;
            let policy_source = service_policy_source(raw);

            Ok(ServiceEdgeIr {
                id: entity_id("service", &format!("{}->{}", raw.client, raw.server)),
                client,
                server,
                backend: BackendName(resolution.backend),
                backend_source: resolution.source,
                policy,
                policy_source,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    services.sort_by(|left, right| {
        (
            &left.client.instance.name,
            &left.client.port,
            &left.server.instance.name,
            &left.server.port,
        )
            .cmp(&(
                &right.client.instance.name,
                &right.client.port,
                &right.server.instance.name,
                &right.server.port,
            ))
    });
    for service in &mut services {
        service.id = entity_id(
            "service",
            &format!(
                "{graph_name}.{}.{}->{}.{}",
                service.client.instance.name,
                service.client.port,
                service.server.instance.name,
                service.server.port
            ),
        );
    }
    Ok(services)
}

/// 推导 service route 的拓扑边界。
pub(crate) fn service_route_topology(
    instances: &BTreeMap<&str, &InstanceIr>,
    components: &BTreeMap<&str, &ComponentIr>,
    client: &ServicePortRef,
    server: &ServicePortRef,
) -> crate::backend::RouteTopology {
    let client_instance = instances.get(client.instance.name.as_str());
    let server_instance = instances.get(server.instance.name.as_str());
    let client_process = client_instance
        .and_then(|i| i.process.as_deref())
        .unwrap_or("main");
    let server_process = server_instance
        .and_then(|i| i.process.as_deref())
        .unwrap_or("main");
    let client_target = client_instance
        .and_then(|i| i.target.as_ref())
        .map(|t| t.name.as_str());
    let server_target = server_instance
        .and_then(|i| i.target.as_ref())
        .map(|t| t.name.as_str());
    let crosses_process = client_process != server_process;
    let crosses_target =
        client_target.is_some() && server_target.is_some() && client_target != server_target;
    let touches_external = [client_instance, server_instance].iter().any(|instance| {
        instance
            .and_then(|instance| components.get(instance.component.name.as_str()))
            .is_some_and(|component| component.language == LanguageKind::External)
    });
    crate::backend::RouteTopology {
        crosses_process,
        crosses_target,
        touches_external,
    }
}

pub(crate) fn service_port_types<'a>(
    instances: &BTreeMap<&str, &InstanceIr>,
    components: &BTreeMap<&str, &'a ComponentIr>,
    client: &ServicePortRef,
    server: &ServicePortRef,
) -> Option<(&'a TypeExpr, &'a TypeExpr)> {
    let client_port = service_port(
        components,
        instances.get(client.instance.name.as_str())?,
        client,
        true,
    )?;
    let server_port = service_port(
        components,
        instances.get(server.instance.name.as_str())?,
        server,
        false,
    )?;
    if client_port.request == server_port.request && client_port.response == server_port.response {
        Some((&client_port.request, &client_port.response))
    } else {
        Some((&server_port.request, &server_port.response))
    }
}

fn service_port<'a>(
    components: &BTreeMap<&str, &'a ComponentIr>,
    instance: &InstanceIr,
    port_ref: &ServicePortRef,
    client: bool,
) -> Option<&'a ServicePortIr> {
    let component = components.get(instance.component.name.as_str())?;
    let ports = if client {
        &component.service_clients
    } else {
        &component.service_servers
    };
    ports.iter().find(|port| port.name == port_ref.port)
}

/// 解析 service policy 字段，使用默认值填充缺失项。
fn parse_service_policy(
    context: &str,
    raw: &flowrt_rsdl::RawServiceBind,
) -> Result<ServicePolicyIr> {
    let timeout_ms = raw.timeout_ms.unwrap_or(SERVICE_DEFAULT_TIMEOUT_MS);
    if raw.timeout_ms.is_some() && timeout_ms == 0 {
        return Err(IrError::InvalidValue {
            context: format!("{context}.timeout_ms"),
            message: "service timeout_ms must be greater than zero".to_string(),
        });
    }

    let queue_depth = raw.queue_depth.unwrap_or(SERVICE_DEFAULT_QUEUE_DEPTH);
    if raw.queue_depth.is_some() && queue_depth == 0 {
        return Err(IrError::InvalidValue {
            context: format!("{context}.queue_depth"),
            message: "service queue_depth must be greater than zero".to_string(),
        });
    }

    let overflow = match raw.overflow.as_deref() {
        Some("busy") | None => ServiceOverflowPolicy::Busy,
        Some("error") => ServiceOverflowPolicy::Error,
        Some(other) => {
            return Err(IrError::InvalidEnum {
                context: format!("{context}.overflow"),
                kind: "service overflow policy",
                value: other.to_string(),
            });
        }
    };

    let max_in_flight = raw.max_in_flight.unwrap_or(SERVICE_DEFAULT_MAX_IN_FLIGHT);
    if raw.max_in_flight.is_some() && max_in_flight == 0 {
        return Err(IrError::InvalidValue {
            context: format!("{context}.max_in_flight"),
            message: "service max_in_flight must be greater than zero".to_string(),
        });
    }

    Ok(ServicePolicyIr {
        timeout_ms,
        queue_depth,
        overflow,
        lane: raw.lane.clone(),
        max_in_flight,
    })
}

/// 构建 service policy source 元数据。
fn service_policy_source(raw: &flowrt_rsdl::RawServiceBind) -> ServicePolicySourceIr {
    ServicePolicySourceIr {
        backend: if raw.backend.is_some() {
            PolicyValueSource::Explicit
        } else {
            PolicyValueSource::ProfileDefault
        },
        timeout_ms: if raw.timeout_ms.is_some() {
            PolicyValueSource::Explicit
        } else {
            PolicyValueSource::ProfileDefault
        },
        queue_depth: if raw.queue_depth.is_some() {
            PolicyValueSource::Explicit
        } else {
            PolicyValueSource::ProfileDefault
        },
        overflow: if raw.overflow.is_some() {
            PolicyValueSource::Explicit
        } else {
            PolicyValueSource::ProfileDefault
        },
        lane: if raw.lane.is_some() {
            PolicyValueSource::Explicit
        } else {
            PolicyValueSource::ProfileDefault
        },
        max_in_flight: if raw.max_in_flight.is_some() {
            PolicyValueSource::Explicit
        } else {
            PolicyValueSource::ProfileDefault
        },
    }
}

pub(super) fn normalize_ros2_bridges(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    boundary_endpoints: &[BoundaryEndpointIr],
    graph_name: &str,
) -> Result<Vec<Ros2BridgeIr>> {
    let boundary_by_name = boundary_endpoints
        .iter()
        .map(|endpoint| (endpoint.name.as_str(), endpoint))
        .collect::<BTreeMap<_, _>>();
    document
        .ros2_bridges
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let direction = parse_ros2_bridge_direction(
                &format!("bridge.ros2[{index}].direction"),
                &raw.direction,
            )?;
            let (flowrt, boundary_endpoint) = resolve_ros2_flowrt_ref(
                index,
                &raw.flowrt,
                direction,
                instance_refs,
                &boundary_by_name,
            )?;
            let name = format!("ros2_bridge_{index}");
            Ok(Ros2BridgeIr {
                id: entity_id("bridge", &format!("{graph_name}.{name}")),
                name,
                flowrt,
                boundary_endpoint,
                ros2_topic: raw.ros2_topic.clone(),
                ros2_type: raw.ros2_type.clone(),
                direction,
                field: raw.field.clone().unwrap_or_else(|| "data".to_string()),
                backend: BackendName("zenoh".to_string()),
            })
        })
        .collect()
}

fn resolve_ros2_flowrt_ref(
    index: usize,
    endpoint: &str,
    direction: Ros2BridgeDirection,
    instance_refs: &BTreeMap<String, EntityRef>,
    boundary_by_name: &BTreeMap<&str, &BoundaryEndpointIr>,
) -> Result<(crate::PortRef, Option<EntityRef>)> {
    if endpoint.contains('.') {
        return Ok((parse_port_ref(endpoint, instance_refs)?, None));
    }

    let Some(boundary) = boundary_by_name.get(endpoint).copied() else {
        return Err(IrError::InvalidValue {
            context: format!("bridge.ros2[{index}].flowrt"),
            message: format!(
                "expected `<instance>.<port>` or boundary endpoint name; unknown boundary endpoint `{endpoint}`"
            ),
        });
    };

    let expected = match direction {
        Ros2BridgeDirection::FlowrtToRos2 => BoundaryDirection::Output,
        Ros2BridgeDirection::Ros2ToFlowrt => BoundaryDirection::Input,
    };
    if boundary.direction != expected {
        return Err(IrError::InvalidValue {
            context: format!("bridge.ros2[{index}].flowrt"),
            message: format!(
                "boundary endpoint `{endpoint}` direction `{}` is incompatible with ROS2 bridge direction `{}`; expected boundary {}",
                boundary_direction_name(boundary.direction),
                ros2_bridge_direction_name(direction),
                boundary_direction_name(expected)
            ),
        });
    }

    Ok((
        boundary.port.clone(),
        Some(EntityRef {
            id: boundary.id.clone(),
            name: boundary.name.clone(),
        }),
    ))
}

fn boundary_direction_name(direction: BoundaryDirection) -> &'static str {
    match direction {
        BoundaryDirection::Input => "input",
        BoundaryDirection::Output => "output",
    }
}

fn ros2_bridge_direction_name(direction: Ros2BridgeDirection) -> &'static str {
    match direction {
        Ros2BridgeDirection::FlowrtToRos2 => "flowrt_to_ros2",
        Ros2BridgeDirection::Ros2ToFlowrt => "ros2_to_flowrt",
    }
}

fn parse_service_port_ref(
    endpoint: &str,
    instance_refs: &BTreeMap<String, EntityRef>,
) -> Result<ServicePortRef> {
    let Some((instance_name, port)) = endpoint.split_once('.') else {
        return Err(IrError::InvalidPortEndpoint {
            endpoint: endpoint.to_string(),
        });
    };
    let instance = instance_refs.get(instance_name).cloned().ok_or_else(|| {
        IrError::UnknownEndpointInstance {
            endpoint: endpoint.to_string(),
            instance: instance_name.to_string(),
        }
    })?;
    Ok(ServicePortRef {
        instance,
        port: port.to_string(),
    })
}

fn parse_port_ref(
    endpoint: &str,
    instance_refs: &BTreeMap<String, EntityRef>,
) -> Result<crate::PortRef> {
    let Some((instance_name, port)) = endpoint.split_once('.') else {
        return Err(IrError::InvalidPortEndpoint {
            endpoint: endpoint.to_string(),
        });
    };
    let instance = instance_refs.get(instance_name).cloned().ok_or_else(|| {
        IrError::UnknownEndpointInstance {
            endpoint: endpoint.to_string(),
            instance: instance_name.to_string(),
        }
    })?;
    Ok(crate::PortRef {
        instance,
        port: port.to_string(),
    })
}

fn parse_ros2_bridge_direction(context: &str, value: &str) -> Result<Ros2BridgeDirection> {
    match value {
        "flowrt_to_ros2" => Ok(Ros2BridgeDirection::FlowrtToRos2),
        "ros2_to_flowrt" => Ok(Ros2BridgeDirection::Ros2ToFlowrt),
        _ => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "ROS2 bridge direction",
            value: value.to_string(),
        }),
    }
}
