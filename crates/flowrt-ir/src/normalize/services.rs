use std::collections::BTreeMap;

use flowrt_rsdl::RawDocument;

use crate::{
    BackendName, EntityRef, InstanceIr, IrError, PolicyValueSource, Result, Ros2BridgeDirection,
    Ros2BridgeIr, ServiceBackendSource, ServiceEdgeIr, ServiceOverflowPolicy, ServicePolicyIr,
    ServicePolicySourceIr, ServicePortRef, SERVICE_DEFAULT_MAX_IN_FLIGHT,
    SERVICE_DEFAULT_QUEUE_DEPTH, SERVICE_DEFAULT_TIMEOUT_MS,
};

use super::ids::entity_id;

pub(super) fn normalize_service_binds(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    instances: &[InstanceIr],
    graph_name: &str,
) -> Result<Vec<ServiceEdgeIr>> {
    let instances_by_name = instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();

    let mut services = document
        .service_binds
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let client = parse_service_port_ref(&raw.client, instance_refs)?;
            let server = parse_service_port_ref(&raw.server, instance_refs)?;
            let context = format!("bind.service[{index}]");

            let topology = service_route_topology(&instances_by_name, &client, &server);
            let (backend, backend_source) =
                resolve_service_backend(&context, raw.backend.as_deref(), topology)?;

            let policy = parse_service_policy(&context, raw)?;
            let policy_source = service_policy_source(raw);

            Ok(ServiceEdgeIr {
                id: entity_id("service", &format!("{}->{}", raw.client, raw.server)),
                client,
                server,
                backend: BackendName(backend),
                backend_source,
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
fn service_route_topology(
    instances: &BTreeMap<&str, &InstanceIr>,
    client: &ServicePortRef,
    server: &ServicePortRef,
) -> (bool, bool) {
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
    (crosses_process, crosses_target)
}

/// 解析 service backend：auto 根据拓扑选择，显式值校验合法性。
fn resolve_service_backend(
    context: &str,
    requested: Option<&str>,
    (crosses_process, crosses_target): (bool, bool),
) -> Result<(String, ServiceBackendSource)> {
    match requested {
        None | Some("auto") => {
            let resolved = if crosses_process || crosses_target {
                "zenoh"
            } else {
                "inproc"
            };
            Ok((resolved.to_string(), ServiceBackendSource::AutoResolved))
        }
        Some("inproc") => {
            if crosses_process || crosses_target {
                Err(IrError::InvalidValue {
                    context: context.to_string(),
                    message: "explicit `inproc` service backend cannot span process or target boundaries"
                        .to_string(),
                })
            } else {
                Ok(("inproc".to_string(), ServiceBackendSource::Explicit))
            }
        }
        Some("zenoh") => Ok(("zenoh".to_string(), ServiceBackendSource::Explicit)),
        Some("iox2") => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "service backend",
            value: "iox2".to_string(),
        }),
        Some(other) => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "service backend",
            value: other.to_string(),
        }),
    }
}

/// 解析 service policy 字段，使用默认值填充缺失项。
fn parse_service_policy(context: &str, raw: &flowrt_rsdl::RawServiceBind) -> Result<ServicePolicyIr> {
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
    graph_name: &str,
) -> Result<Vec<Ros2BridgeIr>> {
    document
        .ros2_bridges
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let flowrt = parse_port_ref(&raw.flowrt, instance_refs)?;
            let direction = parse_ros2_bridge_direction(
                &format!("bridge.ros2[{index}].direction"),
                &raw.direction,
            )?;
            let name = format!("ros2_bridge_{index}");
            Ok(Ros2BridgeIr {
                id: entity_id("bridge", &format!("{graph_name}.{name}")),
                name,
                flowrt,
                ros2_topic: raw.ros2_topic.clone(),
                ros2_type: raw.ros2_type.clone(),
                direction,
                field: raw.field.clone().unwrap_or_else(|| "data".to_string()),
                backend: BackendName("zenoh".to_string()),
            })
        })
        .collect()
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
        _ => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "ROS2 bridge direction",
            value: value.to_string(),
        }),
    }
}
