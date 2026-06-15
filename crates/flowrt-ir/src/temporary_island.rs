use std::collections::{BTreeMap, BTreeSet};

use crate::{
    BoundaryDirection, BoundaryEndpointIr, ClockSourceKind, ContractArtifactIr, ContractIr,
    EntityId, EntityRef, GraphMode, IrError, PortRef, Result, TemporaryOverlayBoundaryMappingIr,
    TemporaryOverlayGenerationIr, TemporaryOverlayIr, TypeExpr,
};

use crate::normalize::entity_id_for_projection;

/// CLI temporary island overlay 的一条显式 boundary 映射。
///
/// `endpoint` 使用 `<instance>.<port>`，类型从目标 component port 推导，避免测试命令和
/// RSDL contract 之间出现重复事实源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporaryBoundaryMapping {
    pub name: String,
    pub endpoint: String,
}

/// 归一化 IR 上的一次性 test-only island 投影。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemporaryIslandOverlay {
    pub boundary_inputs: Vec<TemporaryBoundaryMapping>,
    pub boundary_outputs: Vec<TemporaryBoundaryMapping>,
    pub generated_by: TemporaryOverlayGenerationIr,
}

/// 将已按 profile 投影的 strict Contract IR 转成 test-only island 产物。
///
/// 该函数不修改源 RSDL；调用方必须在 projection 后重新运行 validator，确保 codegen
/// 仍只消费合法 Contract IR。
pub fn apply_temporary_island_overlay(
    contract: &ContractIr,
    overlay: &TemporaryIslandOverlay,
) -> Result<ContractIr> {
    if overlay.boundary_inputs.is_empty() && overlay.boundary_outputs.is_empty() {
        return Err(IrError::InvalidValue {
            context: "temporary island overlay".to_string(),
            message: "at least one --boundary-input or --boundary-output mapping is required"
                .to_string(),
        });
    }
    if contract.profiles.len() != 1 {
        return Err(IrError::InvalidValue {
            context: "temporary island overlay".to_string(),
            message: "overlay must be applied after selecting exactly one profile".to_string(),
        });
    }

    let mut projected = contract.clone();
    let original_profile_mode = projected
        .profiles
        .first()
        .map(|profile| profile.mode)
        .unwrap_or(GraphMode::Strict);
    let boundary_mappings = overlay
        .boundary_inputs
        .iter()
        .map(|mapping| temporary_overlay_mapping(mapping, BoundaryDirection::Input))
        .chain(
            overlay
                .boundary_outputs
                .iter()
                .map(|mapping| temporary_overlay_mapping(mapping, BoundaryDirection::Output)),
        )
        .collect();
    projected.artifact = ContractArtifactIr {
        mode: GraphMode::Island,
        temporary_island: true,
        test_only: true,
        temporary_overlay: Some(TemporaryOverlayIr {
            kind: "temporary_island".to_string(),
            original_profile_mode,
            generated_by: overlay.generated_by.clone(),
            boundary_mappings,
        }),
        clock_source: ClockSourceKind::SimulatedReplay,
    };
    for profile in &mut projected.profiles {
        profile.mode = GraphMode::Island;
    }

    let component_ports = component_ports_by_name(contract);
    for graph in &mut projected.graphs {
        apply_overlay_to_graph(
            graph,
            &component_ports,
            &overlay.boundary_inputs,
            BoundaryDirection::Input,
        )?;
        apply_overlay_to_graph(
            graph,
            &component_ports,
            &overlay.boundary_outputs,
            BoundaryDirection::Output,
        )?;
        graph.boundary_endpoints.sort_by(|left, right| {
            (left.direction, &left.name).cmp(&(right.direction, &right.name))
        });
    }

    Ok(projected)
}

fn temporary_overlay_mapping(
    mapping: &TemporaryBoundaryMapping,
    direction: BoundaryDirection,
) -> TemporaryOverlayBoundaryMappingIr {
    TemporaryOverlayBoundaryMappingIr {
        direction,
        name: mapping.name.clone(),
        endpoint: mapping.endpoint.clone(),
        source: match direction {
            BoundaryDirection::Input => "--boundary-input",
            BoundaryDirection::Output => "--boundary-output",
        }
        .to_string(),
    }
}

fn apply_overlay_to_graph(
    graph: &mut crate::GraphIr,
    component_ports: &BTreeMap<String, ComponentPortLookup>,
    mappings: &[TemporaryBoundaryMapping],
    direction: BoundaryDirection,
) -> Result<()> {
    if mappings.is_empty() {
        return Ok(());
    }

    let instances = graph
        .instances
        .iter()
        .map(|instance| {
            (
                instance.name.as_str(),
                InstanceLookup {
                    id: instance.id.clone(),
                    name: instance.name.clone(),
                    component: instance.component.name.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut seen_names = graph
        .boundary_endpoints
        .iter()
        .map(|endpoint| endpoint.name.clone())
        .collect::<BTreeSet<_>>();

    for mapping in mappings {
        let name = mapping.name.trim();
        if name.is_empty() {
            return Err(IrError::InvalidValue {
                context: "temporary island overlay".to_string(),
                message: "boundary name must not be empty".to_string(),
            });
        }
        if !seen_names.insert(name.to_string()) {
            return Err(IrError::InvalidValue {
                context: "temporary island overlay".to_string(),
                message: format!("duplicate temporary boundary name `{name}`"),
            });
        }
        let port_ref = parse_overlay_port_ref(&mapping.endpoint, &instances)?;
        if direction == BoundaryDirection::Input {
            reject_input_with_existing_bind(graph, &port_ref)?;
        }
        let ty = resolve_component_port_type(component_ports, &instances, &port_ref, direction)?;
        let direction_name = boundary_direction_name(direction);
        graph.boundary_endpoints.push(BoundaryEndpointIr {
            id: entity_id_for_projection(
                "boundary",
                &format!("{}.temporary_{}.{}", graph.name, direction_name, name),
            ),
            name: name.to_string(),
            direction,
            port: port_ref,
            ty,
        });
    }

    Ok(())
}

fn parse_overlay_port_ref(
    endpoint: &str,
    instances: &BTreeMap<&str, InstanceLookup>,
) -> Result<PortRef> {
    let Some((instance_name, port)) = endpoint.split_once('.') else {
        return Err(IrError::InvalidPortEndpoint {
            endpoint: endpoint.to_string(),
        });
    };
    if instance_name.is_empty() || port.is_empty() {
        return Err(IrError::InvalidPortEndpoint {
            endpoint: endpoint.to_string(),
        });
    }
    let instance =
        instances
            .get(instance_name)
            .ok_or_else(|| IrError::UnknownEndpointInstance {
                endpoint: endpoint.to_string(),
                instance: instance_name.to_string(),
            })?;
    Ok(PortRef {
        instance: EntityRef {
            id: instance.id.clone(),
            name: instance.name.clone(),
        },
        port: port.to_string(),
    })
}

fn reject_input_with_existing_bind(graph: &crate::GraphIr, port_ref: &PortRef) -> Result<()> {
    let has_dataflow = graph
        .binds
        .iter()
        .any(|bind| bind.to.instance.id == port_ref.instance.id && bind.to.port == port_ref.port);
    if has_dataflow {
        return Err(IrError::InvalidValue {
            context: "temporary island overlay".to_string(),
            message: format!(
                "input `{}` already has an incoming dataflow bind; temporary boundary input would create multiple sources",
                endpoint_name(port_ref)
            ),
        });
    }
    Ok(())
}

fn resolve_component_port_type(
    component_ports: &BTreeMap<String, ComponentPortLookup>,
    instances: &BTreeMap<&str, InstanceLookup>,
    port_ref: &PortRef,
    direction: BoundaryDirection,
) -> Result<TypeExpr> {
    let instance = instances
        .get(port_ref.instance.name.as_str())
        .ok_or_else(|| IrError::UnknownEndpointInstance {
            endpoint: endpoint_name(port_ref),
            instance: port_ref.instance.name.clone(),
        })?;
    let component = component_ports
        .get(instance.component.as_str())
        .ok_or_else(|| IrError::UnknownComponent {
            instance: instance.name.clone(),
            component: instance.component.clone(),
        })?;
    let ports = match direction {
        BoundaryDirection::Input => &component.inputs,
        BoundaryDirection::Output => &component.outputs,
    };
    ports
        .get(port_ref.port.as_str())
        .cloned()
        .ok_or_else(|| IrError::InvalidValue {
            context: "temporary island overlay".to_string(),
            message: format!(
                "instance `{}` component `{}` has no {} port `{}`",
                instance.name,
                component.name,
                boundary_direction_name(direction),
                port_ref.port
            ),
        })
}

#[derive(Debug, Clone)]
struct ComponentPortLookup {
    name: String,
    inputs: BTreeMap<String, TypeExpr>,
    outputs: BTreeMap<String, TypeExpr>,
}

#[derive(Debug, Clone)]
struct InstanceLookup {
    id: EntityId,
    name: String,
    component: String,
}

fn component_ports_by_name(contract: &ContractIr) -> BTreeMap<String, ComponentPortLookup> {
    contract
        .components
        .iter()
        .map(|component| {
            (
                component.qualified_name.clone(),
                ComponentPortLookup {
                    name: component.name.clone(),
                    inputs: component
                        .inputs
                        .iter()
                        .map(|port| (port.name.clone(), port.ty.clone()))
                        .collect(),
                    outputs: component
                        .outputs
                        .iter()
                        .map(|port| (port.name.clone(), port.ty.clone()))
                        .collect(),
                },
            )
        })
        .collect()
}

fn endpoint_name(port_ref: &PortRef) -> String {
    format!("{}.{}", port_ref.instance.name, port_ref.port)
}

fn boundary_direction_name(direction: BoundaryDirection) -> &'static str {
    match direction {
        BoundaryDirection::Input => "input",
        BoundaryDirection::Output => "output",
    }
}
