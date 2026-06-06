use std::collections::BTreeMap;

use flowrt_rsdl::RawDocument;

use crate::{
    BackendName, EntityRef, IrError, Result, Ros2BridgeDirection, Ros2BridgeIr, ServiceEdgeIr,
    ServicePortRef,
};

use super::ids::entity_id;

pub(super) fn normalize_service_binds(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    graph_name: &str,
) -> Result<Vec<ServiceEdgeIr>> {
    let mut services = document
        .service_binds
        .iter()
        .map(|raw| {
            let client = parse_service_port_ref(&raw.client, instance_refs)?;
            let server = parse_service_port_ref(&raw.server, instance_refs)?;
            Ok(ServiceEdgeIr {
                id: entity_id("service", &format!("{}->{}", raw.client, raw.server)),
                client,
                server,
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
