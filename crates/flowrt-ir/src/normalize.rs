use std::collections::BTreeMap;

use flowrt_rsdl::{RawDocument, RawPort, RawProfile, RawTarget};
use sha2::{Digest, Sha256};

use crate::{
    BackendName, CONTRACT_IR_VERSION, CONTRACT_SCHEMA_VERSION, ChannelBackendSource, ChannelEdgeIr,
    ChannelKind, ChannelPolicySourceIr, ComponentIr, ComponentKind, ContractIr, DeploymentIr,
    EntityId, EntityRef, FieldIr, GraphIr, ImportIr, InstanceIr, IrError, LanguageKind,
    LifecycleSurface, OverflowPolicy, PackageIr, PolicyDefaults, PolicyValueSource, PortIr,
    PortRef, ProfileIr, Result, RouteTopology, StalePolicy, TargetIr, TaskIr, TriggerKind,
    TypeExpr, TypeIr, channel_capabilities, channel_route_capabilities,
    deployment_capability_decision, graph_required_capabilities, parse_type_expr,
    target_capabilities,
};

mod params;

use params::{merge_instance_params, normalize_component_params};
pub use params::{param_value_compatible, param_value_kind};

/// 计算稳定的 SHA-256 源文本哈希。
pub fn hash_source(source: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// 将已解析的 RSDL 文档归一化为 Contract IR。
pub fn normalize_document(document: &RawDocument, source_hash: String) -> Result<ContractIr> {
    let package_qualified_name = format!(
        "{}@{}",
        document.package.name,
        document.package.version.as_deref().unwrap_or("0.0.0")
    );
    let package_id = entity_id("package", &package_qualified_name);

    let package = PackageIr {
        name: document.package.name.clone(),
        version: document.package.version.clone(),
        rsdl_version: document.package.rsdl_version.clone(),
        imports: document
            .package
            .imports
            .iter()
            .map(|(kind, patterns)| {
                let mut patterns = patterns.clone();
                patterns.sort();
                ImportIr {
                    kind: kind.clone(),
                    patterns,
                }
            })
            .collect(),
    };

    let types = normalize_types(document)?;
    let type_ids = types
        .iter()
        .map(|ty| (ty.name.clone(), ty.id.clone()))
        .collect::<BTreeMap<_, _>>();

    let components = normalize_components(document, &type_ids)?;
    let component_ids = components
        .iter()
        .map(|component| (component.name.clone(), component.id.clone()))
        .collect::<BTreeMap<_, _>>();

    let profiles = normalize_profiles(document)?;
    let targets = normalize_targets(document)?;
    let target_ids = targets
        .iter()
        .map(|target| (target.name.clone(), target.id.clone()))
        .collect::<BTreeMap<_, _>>();

    let graph_id = entity_id("graph", "default");
    let graph_name = "default".to_string();
    let (instances, mut tasks) =
        normalize_instances(document, &component_ids, &target_ids, &graph_name)?;
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

    let binds = normalize_binds(
        document,
        &instance_refs,
        &types,
        &components,
        &instances,
        &profiles,
    )?;
    let graph = GraphIr {
        id: graph_id.clone(),
        name: graph_name.clone(),
        instances,
        tasks,
        binds,
    };

    let deployments = normalize_deployments(&graph, &types, &components, &profiles, &targets);

    Ok(ContractIr {
        ir_version: CONTRACT_IR_VERSION.to_string(),
        schema_version: CONTRACT_SCHEMA_VERSION.to_string(),
        source_hash,
        package_id,
        package,
        types,
        components,
        graphs: vec![graph],
        profiles,
        targets,
        deployments,
    })
}

/// 依据 profile 名称投影出一个只包含目标 profile 的 Contract IR 副本。
///
/// `None` 优先选择 `default` profile，没有时选择首个 profile；`Some(name)` 会要求
/// contract 中存在该 profile。返回值只保留选中 profile，便于 codegen 和 CLI 统一生成产物。
pub fn project_contract_to_profile(
    contract: &ContractIr,
    profile_name: Option<&str>,
) -> Result<ContractIr> {
    let profile = match profile_name {
        Some(profile_name) => contract
            .profiles
            .iter()
            .find(|profile| profile.name == profile_name)
            .ok_or_else(|| crate::IrError::UnknownProfile {
                profile: profile_name.to_string(),
            })?,
        None => contract
            .profiles
            .iter()
            .find(|profile| profile.name == "default")
            .or_else(|| contract.profiles.first())
            .ok_or_else(|| crate::IrError::UnknownProfile {
                profile: "default".to_string(),
            })?,
    };

    let mut projected = contract.clone();
    projected.profiles = vec![profile.clone()];
    projected
        .deployments
        .retain(|deployment| deployment.profile.name == profile.name);
    apply_profile_defaults_to_binds(&mut projected, profile);
    refresh_projected_deployments(&mut projected);
    Ok(projected)
}

fn apply_profile_defaults_to_binds(contract: &mut ContractIr, profile: &ProfileIr) {
    for graph in &mut contract.graphs {
        let type_lookup = source_port_types_by_endpoint(&contract.components, &graph.instances);
        let topology_lookup = route_topology_by_bind_id(graph);
        for bind in &mut graph.binds {
            let source_type =
                type_lookup.get(&(bind.from.instance.name.clone(), bind.from.port.clone()));
            let topology = topology_lookup
                .get(&bind.id)
                .copied()
                .unwrap_or_else(RouteTopology::local);
            if bind.policy_source.overflow == PolicyValueSource::ProfileDefault {
                bind.overflow = profile.defaults.default_overflow;
            }
            if bind.policy_source.stale == PolicyValueSource::ProfileDefault {
                bind.stale = profile.defaults.default_stale_policy;
            }
            if bind.policy_source.max_age_ms == PolicyValueSource::ProfileDefault {
                bind.max_age_ms = profile.defaults.max_age_ms;
            }
            if bind.backend_source != ChannelBackendSource::Explicit {
                let resolved = resolve_channel_backend(
                    profile.backend.0.as_str(),
                    source_type,
                    &contract.types,
                );
                bind.backend = BackendName(resolved.backend);
                bind.backend_source = resolved.source;
            }
            bind.capability_requirements = match source_type {
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
        }
    }
}

fn refresh_projected_deployments(contract: &mut ContractIr) {
    let graph_capabilities = contract
        .graphs
        .iter()
        .map(|graph| {
            (
                graph.name.clone(),
                graph_required_capabilities(graph, &contract.types, &contract.components),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let profile_by_name = contract
        .profiles
        .iter()
        .map(|profile| (profile.name.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let target_by_name = contract
        .targets
        .iter()
        .map(|target| (target.name.as_str(), target))
        .collect::<BTreeMap<_, _>>();

    for deployment in &mut contract.deployments {
        if let Some(required) = graph_capabilities.get(&deployment.graph.name) {
            deployment.required_capabilities = required.clone();
        }
        let Some(profile) = profile_by_name.get(deployment.profile.name.as_str()) else {
            continue;
        };
        let Some(target) = target_by_name.get(deployment.target.name.as_str()) else {
            continue;
        };
        deployment.backend = profile.backend.clone();
        deployment.satisfied = deployment_capability_decision(
            &deployment.backend,
            &target.backends,
            &deployment.required_capabilities,
        )
        .satisfied;
    }
}

fn normalize_types(document: &RawDocument) -> Result<Vec<TypeIr>> {
    document
        .types
        .iter()
        .map(|(name, raw)| {
            Ok(TypeIr {
                id: entity_id("type", name),
                name: name.clone(),
                fields: raw
                    .fields
                    .iter()
                    .map(|field| {
                        Ok(FieldIr {
                            name: field.name.clone(),
                            ty: parse_type_expr(&field.ty)?,
                            default: None,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            })
        })
        .collect()
}

fn normalize_components(
    document: &RawDocument,
    _type_ids: &BTreeMap<String, EntityId>,
) -> Result<Vec<ComponentIr>> {
    document
        .components
        .iter()
        .map(|(name, raw)| {
            Ok(ComponentIr {
                id: entity_id("component", name),
                name: name.clone(),
                language: parse_language(&format!("component.{name}.language"), &raw.language)?,
                kind: match raw.kind.as_deref() {
                    Some(kind) => parse_component_kind(&format!("component.{name}.kind"), kind)?,
                    None => ComponentKind::Native,
                },
                inputs: normalize_ports(&raw.input)?,
                outputs: normalize_ports(&raw.output)?,
                params: normalize_component_params(name, raw)?,
                lifecycle: LifecycleSurface::reserved_v0_1(),
            })
        })
        .collect()
}

fn normalize_ports(ports: &[RawPort]) -> Result<Vec<PortIr>> {
    ports
        .iter()
        .map(|port| {
            Ok(PortIr {
                name: port.name.clone(),
                ty: parse_type_expr(&port.ty)?,
            })
        })
        .collect()
}

fn normalize_profiles(document: &RawDocument) -> Result<Vec<ProfileIr>> {
    if document.profiles.is_empty() {
        return Ok(vec![ProfileIr {
            id: entity_id("profile", "default"),
            name: "default".to_string(),
            backend: BackendName("inproc".to_string()),
            defaults: PolicyDefaults {
                default_overflow: OverflowPolicy::DropOldest,
                default_stale_policy: StalePolicy::Warn,
                max_age_ms: None,
            },
        }]);
    }

    document
        .profiles
        .iter()
        .map(|(name, raw)| {
            Ok(ProfileIr {
                id: entity_id("profile", name),
                name: name.clone(),
                backend: BackendName(raw.backend.clone().unwrap_or_else(|| "inproc".to_string())),
                defaults: normalize_policy_defaults(raw, &format!("profile.{name}"))?,
            })
        })
        .collect()
}

fn normalize_policy_defaults(raw: &RawProfile, context: &str) -> Result<PolicyDefaults> {
    Ok(PolicyDefaults {
        default_overflow: match raw.default_overflow.as_deref() {
            Some(value) => parse_overflow_policy(&format!("{context}.default_overflow"), value)?,
            None => OverflowPolicy::DropOldest,
        },
        default_stale_policy: match raw.default_stale_policy.as_deref() {
            Some(value) => parse_stale_policy(&format!("{context}.default_stale_policy"), value)?,
            None => StalePolicy::Warn,
        },
        max_age_ms: raw.max_age_ms,
    })
}

fn normalize_targets(document: &RawDocument) -> Result<Vec<TargetIr>> {
    document
        .targets
        .iter()
        .map(|(name, raw)| {
            let mut backends = raw
                .backends
                .iter()
                .cloned()
                .map(BackendName)
                .collect::<Vec<_>>();
            backends.sort();
            Ok(TargetIr {
                id: entity_id("target", name),
                name: name.clone(),
                platform: raw.platform.clone(),
                runtime: normalize_target_runtime(name, raw)?,
                capabilities: target_capabilities(&backends),
                backends,
            })
        })
        .collect()
}

fn normalize_target_runtime(target_name: &str, raw: &RawTarget) -> Result<Vec<LanguageKind>> {
    let mut runtime = raw
        .runtime
        .iter()
        .map(|language| parse_language(&format!("target.{target_name}.runtime"), language))
        .collect::<Result<Vec<_>>>()?;
    runtime.sort_by_key(|language| match language {
        LanguageKind::Cpp => 0,
        LanguageKind::Rust => 1,
    });
    Ok(runtime)
}

fn normalize_instances(
    document: &RawDocument,
    component_ids: &BTreeMap<String, EntityId>,
    target_ids: &BTreeMap<String, EntityId>,
    graph_name: &str,
) -> Result<(Vec<InstanceIr>, Vec<TaskIr>)> {
    let mut instances = Vec::with_capacity(document.instances.len());
    let mut tasks = Vec::new();
    let mut component_param_schemas = BTreeMap::new();
    for (name, component) in &document.components {
        let params = normalize_component_params(name, component)?
            .into_iter()
            .map(|param| (param.name.clone(), param))
            .collect::<BTreeMap<_, _>>();
        component_param_schemas.insert(name.as_str(), params);
    }

    for (name, raw) in &document.instances {
        let component_id = component_ids.get(&raw.component).cloned().ok_or_else(|| {
            IrError::UnknownComponent {
                instance: name.clone(),
                component: raw.component.clone(),
            }
        })?;
        let component_ref = EntityRef {
            id: component_id,
            name: raw.component.clone(),
        };
        let component = component_param_schemas
            .get(raw.component.as_str())
            .expect("component IDs and normalized components must be built from the same document");
        let params = merge_instance_params(name, raw, component)?;
        let target = raw
            .target
            .as_ref()
            .map(|target_name| {
                target_ids
                    .get(target_name)
                    .cloned()
                    .map(|id| EntityRef {
                        id,
                        name: target_name.clone(),
                    })
                    .ok_or_else(|| IrError::UnknownTarget {
                        instance: name.clone(),
                        target: target_name.clone(),
                    })
            })
            .transpose()?;

        let instance_id = entity_id("instance", &format!("{graph_name}.{name}"));
        let instance_ref = EntityRef {
            id: instance_id.clone(),
            name: name.clone(),
        };
        for (task_index, raw_task) in raw.tasks.iter().enumerate() {
            let task_name = raw_task
                .name
                .clone()
                .unwrap_or_else(|| default_task_name(task_index));
            tasks.push(TaskIr {
                id: entity_id("task", &format!("{graph_name}.{name}.{task_name}")),
                name: task_name,
                instance: instance_ref.clone(),
                trigger: parse_trigger(
                    &format!("instance.{name}.task.trigger"),
                    &raw_task.trigger,
                )?,
                period_ms: raw_task.period_ms,
                deadline_ms: raw_task.deadline_ms,
                priority: raw_task.priority,
                inputs: raw_task.input.clone(),
                outputs: raw_task.output.clone(),
            });
        }

        instances.push(InstanceIr {
            id: instance_id,
            name: name.clone(),
            component: component_ref,
            params,
            process: raw.process.clone(),
            target,
        });
    }

    Ok((instances, tasks))
}

fn default_task_name(index: usize) -> String {
    if index == 0 {
        "main".to_string()
    } else {
        format!("task_{index}")
    }
}

fn normalize_binds(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    types: &[TypeIr],
    components: &[ComponentIr],
    instances: &[InstanceIr],
    profiles: &[ProfileIr],
) -> Result<Vec<ChannelEdgeIr>> {
    let default_policy = profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or(profiles.first());
    let default_overflow = default_policy
        .map(|profile| profile.defaults.default_overflow)
        .unwrap_or(OverflowPolicy::DropOldest);
    let default_stale = default_policy
        .map(|profile| profile.defaults.default_stale_policy)
        .unwrap_or(StalePolicy::Warn);
    let default_max_age = default_policy.and_then(|profile| profile.defaults.max_age_ms);
    let default_backend = default_policy
        .map(|profile| profile.backend.0.as_str())
        .unwrap_or("inproc");
    let source_port_types = source_port_types_by_endpoint(components, instances);
    let instances_by_name = instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();

    let mut binds = document
        .binds
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let channel =
                parse_channel_kind(&format!("bind.dataflow[{index}].channel"), &raw.channel)?;
            let depth = raw.depth.or(match channel {
                ChannelKind::Latest => Some(1),
                ChannelKind::Fifo => None,
            });
            let overflow = match raw.overflow.as_deref() {
                Some(value) => {
                    parse_overflow_policy(&format!("bind.dataflow[{index}].overflow"), value)?
                }
                None => default_overflow,
            };
            let stale = match raw.stale_policy.as_deref() {
                Some(value) => {
                    parse_stale_policy(&format!("bind.dataflow[{index}].stale_policy"), value)?
                }
                None => default_stale,
            };
            let from = parse_port_ref(&raw.from, instance_refs)?;
            let to = parse_port_ref(&raw.to, instance_refs)?;
            let source_type =
                source_port_types.get(&(from.instance.name.clone(), from.port.clone()));
            let topology = route_topology(&instances_by_name, &from, &to);
            let backend_seed = match raw.backend.as_deref() {
                Some("auto") | None => default_backend,
                Some(backend) => backend,
            };
            let resolved_backend = resolve_channel_backend(backend_seed, source_type, types);
            let backend_source = if raw.backend.is_some()
                && raw.backend.as_deref() != Some("auto")
                && resolved_backend.source != ChannelBackendSource::AutoFallback
            {
                ChannelBackendSource::Explicit
            } else {
                resolved_backend.source
            };

            Ok(ChannelEdgeIr {
                id: entity_id("bind", &format!("{}->{}", raw.from, raw.to)),
                from,
                to,
                backend: BackendName(resolved_backend.backend),
                backend_source,
                channel,
                depth,
                overflow,
                stale,
                max_age_ms: raw.max_age_ms.or(default_max_age),
                policy_source: ChannelPolicySourceIr {
                    overflow: if raw.overflow.is_some() {
                        PolicyValueSource::Explicit
                    } else {
                        PolicyValueSource::ProfileDefault
                    },
                    stale: if raw.stale_policy.is_some() {
                        PolicyValueSource::Explicit
                    } else {
                        PolicyValueSource::ProfileDefault
                    },
                    max_age_ms: if raw.max_age_ms.is_some() {
                        PolicyValueSource::Explicit
                    } else {
                        PolicyValueSource::ProfileDefault
                    },
                },
                capability_requirements: match source_type {
                    Some(source_type) => channel_route_capabilities(
                        types,
                        source_type,
                        channel,
                        overflow,
                        stale,
                        topology,
                    ),
                    None => channel_capabilities(channel, overflow, stale),
                },
            })
        })
        .collect::<Result<Vec<_>>>()?;
    binds.sort_by(|left, right| {
        (
            &left.from.instance.name,
            &left.from.port,
            &left.to.instance.name,
            &left.to.port,
        )
            .cmp(&(
                &right.from.instance.name,
                &right.from.port,
                &right.to.instance.name,
                &right.to.port,
            ))
    });
    Ok(binds)
}

struct ResolvedChannelBackend {
    backend: String,
    source: ChannelBackendSource,
}

fn resolve_channel_backend(
    requested_backend: &str,
    source_type: Option<&TypeExpr>,
    types: &[TypeIr],
) -> ResolvedChannelBackend {
    if requested_backend == "iox2"
        && source_type.is_some_and(|ty| type_expr_contains_variable_data(ty, types))
    {
        return ResolvedChannelBackend {
            backend: "zenoh".to_string(),
            source: ChannelBackendSource::AutoFallback,
        };
    }
    ResolvedChannelBackend {
        backend: requested_backend.to_string(),
        source: ChannelBackendSource::ProfileDefault,
    }
}

fn source_port_types_by_endpoint(
    components: &[ComponentIr],
    instances: &[InstanceIr],
) -> BTreeMap<(String, String), TypeExpr> {
    let components = components
        .iter()
        .map(|component| (component.name.as_str(), component))
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

fn route_topology_by_bind_id(graph: &GraphIr) -> BTreeMap<EntityId, RouteTopology> {
    let instances = graph
        .instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    graph
        .binds
        .iter()
        .map(|bind| {
            (
                bind.id.clone(),
                route_topology(&instances, &bind.from, &bind.to),
            )
        })
        .collect()
}

fn route_topology(
    instances: &BTreeMap<&str, &InstanceIr>,
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
    RouteTopology::new(
        from_process != to_process,
        from_target.is_some() && to_target.is_some() && from_target != to_target,
    )
}

fn type_expr_contains_variable_data(expr: &TypeExpr, types: &[TypeIr]) -> bool {
    let types_by_name = types
        .iter()
        .map(|ty| (ty.name.as_str(), ty))
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
        TypeExpr::VarBytes { .. } | TypeExpr::VarString { .. } | TypeExpr::VarSequence { .. } => {
            true
        }
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

fn parse_port_ref(endpoint: &str, instance_refs: &BTreeMap<String, EntityRef>) -> Result<PortRef> {
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
    Ok(PortRef {
        instance,
        port: port.to_string(),
    })
}

fn normalize_deployments(
    graph: &GraphIr,
    types: &[TypeIr],
    components: &[ComponentIr],
    profiles: &[ProfileIr],
    targets: &[TargetIr],
) -> Vec<DeploymentIr> {
    let graph_ref = EntityRef {
        id: graph.id.clone(),
        name: graph.name.clone(),
    };
    let mut deployments = Vec::new();
    let required_capabilities = graph_required_capabilities(graph, types, components);

    for profile in profiles {
        for target in targets {
            deployments.push(DeploymentIr {
                id: entity_id(
                    "deployment",
                    &format!("{}.{}.{}", graph.name, profile.name, target.name),
                ),
                graph: graph_ref.clone(),
                profile: EntityRef {
                    id: profile.id.clone(),
                    name: profile.name.clone(),
                },
                target: EntityRef {
                    id: target.id.clone(),
                    name: target.name.clone(),
                },
                backend: profile.backend.clone(),
                required_capabilities: required_capabilities.clone(),
                satisfied: deployment_capability_decision(
                    &profile.backend,
                    &target.backends,
                    &required_capabilities,
                )
                .satisfied,
            });
        }
    }

    deployments
}

fn entity_id(kind: &str, qualified_name: &str) -> EntityId {
    let mut hasher = Sha256::new();
    hasher.update(kind.as_bytes());
    hasher.update(b":");
    hasher.update(qualified_name.as_bytes());
    let digest = hasher.finalize();
    EntityId(format!("{kind}_{}", hex_prefix(&digest)))
}

fn hex_prefix(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn parse_language(context: &str, value: &str) -> Result<LanguageKind> {
    match value {
        "cpp" => Ok(LanguageKind::Cpp),
        "rust" => Ok(LanguageKind::Rust),
        _ => Err(invalid_enum(context, "language", value)),
    }
}

fn parse_component_kind(context: &str, value: &str) -> Result<ComponentKind> {
    match value {
        "native" => Ok(ComponentKind::Native),
        "adapter" => Ok(ComponentKind::Adapter),
        "external" => Ok(ComponentKind::External),
        _ => Err(invalid_enum(context, "component kind", value)),
    }
}

fn parse_trigger(context: &str, value: &str) -> Result<TriggerKind> {
    match value {
        "periodic" => Ok(TriggerKind::Periodic),
        "on_message" => Ok(TriggerKind::OnMessage),
        "startup" => Ok(TriggerKind::Startup),
        "shutdown" => Ok(TriggerKind::Shutdown),
        _ => Err(invalid_enum(context, "trigger", value)),
    }
}

fn parse_channel_kind(context: &str, value: &str) -> Result<ChannelKind> {
    match value {
        "latest" => Ok(ChannelKind::Latest),
        "fifo" => Ok(ChannelKind::Fifo),
        _ => Err(invalid_enum(context, "channel", value)),
    }
}

fn parse_overflow_policy(context: &str, value: &str) -> Result<OverflowPolicy> {
    match value {
        "drop_oldest" => Ok(OverflowPolicy::DropOldest),
        "drop_newest" => Ok(OverflowPolicy::DropNewest),
        "error" => Ok(OverflowPolicy::Error),
        "block" => Ok(OverflowPolicy::Block),
        _ => Err(invalid_enum(context, "overflow policy", value)),
    }
}

fn parse_stale_policy(context: &str, value: &str) -> Result<StalePolicy> {
    match value {
        "warn" => Ok(StalePolicy::Warn),
        "drop" => Ok(StalePolicy::Drop),
        "hold_last" => Ok(StalePolicy::HoldLast),
        "error" => Ok(StalePolicy::Error),
        _ => Err(invalid_enum(context, "stale policy", value)),
    }
}

fn invalid_enum(context: &str, kind: &'static str, value: &str) -> IrError {
    IrError::InvalidEnum {
        context: context.to_string(),
        kind,
        value: value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use flowrt_rsdl::parse_str;

    use super::*;
    use crate::{
        CapabilityAtom, ChannelKind, ParamType, ParamUpdatePolicy, ParamValue, PrimitiveType,
        TypeExpr, channel_route_capabilities, deployment_capability_decision,
    };

    #[test]
    fn normalizes_minimal_document() {
        let source = r#"
[package]
name = "robot_demo"
version = "0.1.0"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.imu_sim]
language = "rust"
output = ["imu:Imu"]

[instance.imu_sim]
component = "imu_sim"
process = "main"
target = "linux"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[target.linux]
platform = "linux-x86_64"
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert_eq!(ir.package.name, "robot_demo");
        assert_eq!(
            ir.types[0].fields[0].ty,
            TypeExpr::Primitive {
                name: PrimitiveType::U64
            }
        );
        assert_eq!(ir.graphs[0].tasks[0].period_ms, Some(5));
    }

    #[test]
    fn normalizes_named_task_array_for_one_instance() {
        let source = r#"
[package]
name = "multi_task_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"
output = ["fast:u32", "slow:u32"]

[instance.worker]
component = "worker"

[[instance.worker.task]]
name = "fast_loop"
trigger = "periodic"
period_ms = 5
output = ["fast"]

[[instance.worker.task]]
name = "slow_loop"
trigger = "periodic"
period_ms = 100
output = ["slow"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let tasks = &ir.graphs[0].tasks;

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "fast_loop");
        assert_eq!(tasks[1].name, "slow_loop");
        assert_ne!(tasks[0].id, tasks[1].id);
        assert_eq!(tasks[0].outputs, vec!["fast"]);
        assert_eq!(tasks[1].outputs, vec!["slow"]);
    }

    #[test]
    fn expands_dataflow_binds() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"

[component.producer]
language = "rust"
output = ["imu:Imu"]

[component.consumer]
language = "rust"
input = ["imu:Imu"]

[instance.producer]
component = "producer"

[instance.consumer]
component = "consumer"

[[bind.dataflow]]
from = "producer.imu"
to = "consumer.imu"
channel = "latest"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert_eq!(ir.graphs[0].binds[0].channel, ChannelKind::Latest);
        assert_eq!(ir.graphs[0].binds[0].depth, Some(1));
    }

    #[test]
    fn canonicalizes_bind_order_independent_of_source_order() {
        let source_a = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.alpha]
language = "rust"
input = ["sample:Sample"]

[component.beta]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"

[instance.alpha]
component = "alpha"

[instance.beta]
component = "beta"

[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest"
"#;
        let source_b = source_a.replace(
            r#"[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest""#,
            r#"[[bind.dataflow]]
from = "producer.sample"
to = "alpha.sample"
channel = "latest"

[[bind.dataflow]]
from = "producer.sample"
to = "beta.sample"
channel = "latest""#,
        );
        let raw_a = parse_str(source_a).unwrap();
        let raw_b = parse_str(&source_b).unwrap();
        let source_hash = hash_source("same logical source");

        let ir_a = normalize_document(&raw_a, source_hash.clone()).unwrap();
        let ir_b = normalize_document(&raw_b, source_hash).unwrap();

        assert_eq!(ir_a, ir_b);
    }

    #[test]
    fn canonicalizes_target_set_order_independent_of_source_order() {
        let source_a = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["iox2", "inproc"]
"#;
        let source_b = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["inproc", "iox2"]
"#;
        let raw_a = parse_str(source_a).unwrap();
        let raw_b = parse_str(source_b).unwrap();
        let source_hash = hash_source("same logical source");

        let ir_a = normalize_document(&raw_a, source_hash.clone()).unwrap();
        let ir_b = normalize_document(&raw_b, source_hash).unwrap();

        assert_eq!(ir_a, ir_b);
    }

    #[test]
    fn canonicalizes_import_pattern_order_independent_of_source_order() {
        let source_a = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/b.rsdl", "types/a.rsdl"]
components = ["components/b.rsdl", "components/a.rsdl"]
"#;
        let source_b = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[package.imports]
types = ["types/a.rsdl", "types/b.rsdl"]
components = ["components/a.rsdl", "components/b.rsdl"]
"#;
        let raw_a = parse_str(source_a).unwrap();
        let raw_b = parse_str(source_b).unwrap();
        let source_hash = hash_source("same logical source");

        let ir_a = normalize_document(&raw_a, source_hash.clone()).unwrap();
        let ir_b = normalize_document(&raw_b, source_hash).unwrap();

        assert_eq!(ir_a, ir_b);
    }

    #[test]
    fn deadline_tasks_require_deadline_aware_backend_capability() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
deadline_ms = 2

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert!(
            ir.deployments[0]
                .required_capabilities
                .contains(&CapabilityAtom("timing:deadline_aware".to_string()))
        );
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn int128_component_ports_require_route_abi_capability() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["sample:u128"]

[component.consumer]
language = "rust"
input = ["sample:u128"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert!(
            ir.graphs[0].binds[0]
                .capability_requirements
                .contains(&CapabilityAtom("abi:int128".to_string()))
        );
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn declared_int128_message_types_do_not_affect_unused_routes() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.UnusedWide]
value = "u128"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert!(
            !ir.graphs[0].binds[0]
                .capability_requirements
                .contains(&CapabilityAtom("abi:int128".to_string()))
        );
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn iox2_route_records_int128_abi_capability() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.producer]
language = "rust"
output = ["sample:i128"]

[component.consumer]
language = "rust"
input = ["sample:i128"]

[instance.producer]
component = "producer"
target = "linux"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.consumer]
component = "consumer"
target = "linux"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert!(
            ir.graphs[0].binds[0]
                .capability_requirements
                .contains(&CapabilityAtom("abi:int128".to_string()))
        );
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn normalized_deployment_satisfied_matches_shared_capability_decision() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.WideSample]
value = "i128"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 5

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let deployment = &ir.deployments[0];
        let decision = deployment_capability_decision(
            &deployment.backend,
            &ir.targets[0].backends,
            &deployment.required_capabilities,
        );

        assert!(decision.selected_backend_known);
        assert!(decision.target_supports_selected_backend);
        assert!(decision.missing_required_capabilities.is_empty());
        assert_eq!(deployment.satisfied, decision.satisfied);
        assert!(deployment.satisfied);
    }

    #[test]
    fn inserts_implicit_default_profile_when_source_omits_profiles() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();

        assert_eq!(ir.profiles.len(), 1);
        assert_eq!(ir.profiles[0].name, "default");
        assert_eq!(ir.profiles[0].backend.0, "inproc");
        assert_eq!(ir.deployments.len(), 1);
        assert_eq!(ir.deployments[0].profile.name, "default");
        assert_eq!(ir.deployments[0].backend.0, "inproc");
        assert!(ir.deployments[0].satisfied);
    }

    #[test]
    fn rejects_instance_param_overrides_with_incompatible_types() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = 1.0
enabled = true
gains = [1.0, 2.0]

[component.controller.params.limits]
max = 10

[instance.controller]
component = "controller"

[instance.controller.params]
kp = "fast"
enabled = 1
gains = [true]

[instance.controller.params.limits]
max = false
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("incompatible parameter overrides should fail");

        assert!(matches!(
            error,
            IrError::IncompatibleParamOverride {
                instance,
                component,
                ..
            } if instance == "controller" && component == "controller"
        ));
    }

    #[test]
    fn rejects_non_empty_array_override_for_empty_default_array() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
gains = []

[instance.controller]
component = "controller"

[instance.controller.params]
gains = [true]
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("non-empty override for empty array default should fail");

        assert!(matches!(
            error,
            IrError::IncompatibleParamOverride {
                instance,
                component,
                param,
                ..
            } if instance == "controller" && component == "controller" && param == "gains"
        ));
    }

    #[test]
    fn normalizes_parameter_schema_and_legacy_defaults() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }
mode = { type = "string", default = "normal", enum = ["normal", "safe"], update = "on_tick" }
legacy_gain = 2.0

[instance.controller]
component = "controller"

[instance.controller.params]
kp = 2.5
mode = "safe"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let component = &ir.components[0];

        assert_eq!(component.params[0].name, "kp");
        assert_eq!(component.params[0].ty, ParamType::F32);
        assert_eq!(component.params[0].default, ParamValue::Float(1.0));
        assert_eq!(component.params[0].update, ParamUpdatePolicy::OnTick);
        assert_eq!(component.params[0].min, Some(ParamValue::Float(0.0)));
        assert_eq!(component.params[0].max, Some(ParamValue::Float(10.0)));

        assert_eq!(component.params[1].name, "legacy_gain");
        assert_eq!(component.params[1].ty, ParamType::F64);
        assert_eq!(component.params[1].update, ParamUpdatePolicy::Startup);

        assert_eq!(component.params[2].name, "mode");
        assert_eq!(component.params[2].ty, ParamType::String);
        assert_eq!(component.params[2].choices.len(), 2);
        assert_eq!(component.params[2].update, ParamUpdatePolicy::OnTick);

        let instance = &ir.graphs[0].instances[0];
        assert_eq!(instance.params[0].value, ParamValue::Float(2.5));
        assert_eq!(instance.params[1].value, ParamValue::Float(2.0));
        assert_eq!(
            instance.params[2].value,
            ParamValue::String("safe".to_string())
        );
    }

    #[test]
    fn rejects_parameter_override_outside_schema_range() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.params]
kp = 12.0
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("out-of-range parameter override should fail");

        assert!(matches!(
            error,
            IrError::InvalidParamSchema {
                component,
                param,
                ..
            } if component == "controller" && param == "kp"
        ));
    }

    #[test]
    fn rejects_unknown_parameter_update_policy() {
        let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = { type = "f32", default = 1.0, update = "immediate" }

[instance.controller]
component = "controller"
"#;
        let raw = parse_str(source).unwrap();
        let error = normalize_document(&raw, hash_source(source))
            .expect_err("unknown parameter update policy should fail");

        assert!(matches!(
            error,
            IrError::InvalidEnum {
                context,
                kind: "parameter update policy",
                value
            } if context == "component.controller.params.kp.update" && value == "immediate"
        ));
    }

    #[test]
    fn projects_selected_profile_without_touching_other_profiles() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, Some("iox2")).unwrap();

        assert_eq!(ir.profiles.len(), 2);
        assert_eq!(projected.profiles.len(), 1);
        assert_eq!(projected.profiles[0].name, "iox2");
        assert_eq!(projected.profiles[0].backend.0, "iox2");
        assert_eq!(projected.deployments.len(), 1);
        assert_eq!(projected.deployments[0].profile.name, "iox2");
        assert_eq!(projected.deployments[0].backend.0, "iox2");
    }

    #[test]
    fn projects_selected_profile_channel_policy_defaults() {
        let source = r#"
[package]
name = "profile_policy_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["defaulted:Sample", "explicit:Sample"]

[component.consumer]
language = "rust"
input = ["defaulted:Sample", "explicit:Sample"]

[instance.producer]
component = "producer"
target = "linux"

[instance.consumer]
component = "consumer"
target = "linux"

[[bind.dataflow]]
from = "producer.defaulted"
to = "consumer.defaulted"
channel = "fifo"
depth = 2

[[bind.dataflow]]
from = "producer.explicit"
to = "consumer.explicit"
channel = "latest"
overflow = "drop_newest"
stale_policy = "hold_last"
max_age_ms = 7

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[profile.safety]
backend = "inproc"
default_overflow = "error"
default_stale_policy = "drop"
max_age_ms = 25

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, Some("safety")).unwrap();
        let defaulted = projected.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.to.port == "defaulted")
            .unwrap();
        let explicit = projected.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.to.port == "explicit")
            .unwrap();

        assert_eq!(defaulted.overflow, OverflowPolicy::Error);
        assert_eq!(defaulted.stale, StalePolicy::Drop);
        assert_eq!(defaulted.max_age_ms, Some(25));
        assert_eq!(
            defaulted.capability_requirements,
            channel_route_capabilities(
                &projected.types,
                &TypeExpr::Primitive {
                    name: PrimitiveType::U32
                },
                defaulted.channel,
                defaulted.overflow,
                defaulted.stale,
                RouteTopology::local()
            )
        );

        assert_eq!(explicit.overflow, OverflowPolicy::DropNewest);
        assert_eq!(explicit.stale, StalePolicy::HoldLast);
        assert_eq!(explicit.max_age_ms, Some(7));
    }

    #[test]
    fn projects_auto_route_backend_and_falls_back_for_variable_frames() {
        let source = r#"
[package]
name = "route_backend_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes<max=64>"

[type.Counter]
value = "u32"

[component.producer]
language = "rust"
output = ["packet:Packet", "counter:Counter"]

[component.consumer]
language = "rust"
input = ["packet:Packet", "counter:Counter"]

[instance.producer]
component = "producer"
target = "linux"

[instance.consumer]
component = "consumer"
target = "linux"

[[bind.dataflow]]
from = "producer.packet"
to = "consumer.packet"
channel = "latest"
backend = "auto"

[[bind.dataflow]]
from = "producer.counter"
to = "consumer.counter"
channel = "latest"

[profile.default]
backend = "inproc"

[profile.ipc]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2", "zenoh"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, Some("ipc")).unwrap();
        let packet = projected.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.from.port == "packet")
            .unwrap();
        let counter = projected.graphs[0]
            .binds
            .iter()
            .find(|bind| bind.from.port == "counter")
            .unwrap();

        assert_eq!(packet.backend.0, "zenoh");
        assert_eq!(packet.backend_source, ChannelBackendSource::AutoFallback);
        assert_eq!(counter.backend.0, "iox2");
        assert_eq!(counter.backend_source, ChannelBackendSource::ProfileDefault);
    }

    #[test]
    fn explicit_route_backend_survives_profile_projection() {
        let source = r#"
[package]
name = "explicit_route_backend_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes<max=64>"

[component.producer]
language = "rust"
output = ["packet:Packet"]

[component.consumer]
language = "rust"
input = ["packet:Packet"]

[instance.producer]
component = "producer"
target = "linux"

[instance.consumer]
component = "consumer"
target = "linux"

[[bind.dataflow]]
from = "producer.packet"
to = "consumer.packet"
channel = "latest"
backend = "zenoh"

[profile.default]
backend = "inproc"

[profile.ipc]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2", "zenoh"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, Some("ipc")).unwrap();
        let bind = &projected.graphs[0].binds[0];

        assert_eq!(bind.backend.0, "zenoh");
        assert_eq!(bind.backend_source, ChannelBackendSource::Explicit);
    }

    #[test]
    fn projects_default_profile_when_selection_is_omitted() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, None).unwrap();

        assert_eq!(projected.profiles.len(), 1);
        assert_eq!(projected.profiles[0].name, "default");
        assert_eq!(projected.deployments.len(), 1);
        assert_eq!(projected.deployments[0].profile.name, "default");
        assert_eq!(projected.deployments[0].backend.0, "inproc");
    }

    #[test]
    fn projects_first_profile_when_selection_is_omitted_and_default_is_absent() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.alpha]
backend = "inproc"

[profile.beta]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let projected = project_contract_to_profile(&ir, None).unwrap();

        assert_eq!(projected.profiles.len(), 1);
        assert_eq!(projected.profiles[0].name, "alpha");
        assert_eq!(projected.deployments.len(), 1);
        assert_eq!(projected.deployments[0].profile.name, "alpha");
        assert_eq!(projected.deployments[0].backend.0, "inproc");
    }

    #[test]
    fn rejects_unknown_profile_selection() {
        let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"
"#;
        let raw = parse_str(source).unwrap();
        let ir = normalize_document(&raw, hash_source(source)).unwrap();
        let error = project_contract_to_profile(&ir, Some("iox2"))
            .expect_err("unknown profile selection should fail");

        assert!(matches!(
            error,
            IrError::UnknownProfile { profile } if profile == "iox2"
        ));
    }
}
