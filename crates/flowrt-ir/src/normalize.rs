use std::collections::BTreeMap;

use flowrt_rsdl::{RawComponent, RawDocument, RawPort, RawProfile, RawTarget, RawValue};
use sha2::{Digest, Sha256};

use crate::{
    BackendName, CONTRACT_IR_VERSION, CONTRACT_SCHEMA_VERSION, CapabilityAtom, ChannelEdgeIr,
    ChannelKind, ChannelPolicySourceIr, ComponentIr, ComponentKind, ContractIr, DeploymentIr,
    EntityId, EntityRef, FieldIr, GraphIr, ImportIr, InstanceIr, IrError, LanguageKind,
    LifecycleSurface, OverflowPolicy, PackageIr, ParamIr, ParamValue, ParamValueIr, PolicyDefaults,
    PolicyValueSource, PortIr, PortRef, ProfileIr, Result, StalePolicy, TargetIr, TaskIr,
    TriggerKind, TypeIr, backend_capabilities, channel_capabilities, graph_required_capabilities,
    parse_type_expr, target_capabilities,
};

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
    let (instances, tasks) =
        normalize_instances(document, &component_ids, &target_ids, &graph_name)?;
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

    let binds = normalize_binds(document, &instance_refs, &profiles)?;
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
        for bind in &mut graph.binds {
            if bind.policy_source.overflow == PolicyValueSource::ProfileDefault {
                bind.overflow = profile.defaults.default_overflow;
            }
            if bind.policy_source.stale == PolicyValueSource::ProfileDefault {
                bind.stale = profile.defaults.default_stale_policy;
            }
            if bind.policy_source.max_age_ms == PolicyValueSource::ProfileDefault {
                bind.max_age_ms = profile.defaults.max_age_ms;
            }
            bind.capability_requirements =
                channel_capabilities(bind.channel, bind.overflow, bind.stale);
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
        deployment.satisfied =
            deployment_satisfied(target, profile, &deployment.required_capabilities);
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
                params: normalize_component_params(raw),
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

fn normalize_component_params(raw: &RawComponent) -> Vec<ParamIr> {
    raw.params
        .iter()
        .map(|(name, value)| ParamIr {
            name: name.clone(),
            default: convert_param_value(value),
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
        let raw_component = &document.components[&raw.component];
        let params = merge_instance_params(name, raw, raw_component)?;
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
        if let Some(raw_task) = &raw.task {
            tasks.push(TaskIr {
                id: entity_id("task", &format!("{graph_name}.{name}.task")),
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

fn merge_instance_params(
    instance_name: &str,
    raw: &flowrt_rsdl::RawInstance,
    component: &RawComponent,
) -> Result<Vec<ParamValueIr>> {
    for (param, override_value) in &raw.params {
        let Some(default_value) = component.params.get(param) else {
            return Err(IrError::UnknownParamOverride {
                instance: instance_name.to_string(),
                component: raw.component.clone(),
                param: param.clone(),
            });
        };
        validate_param_override_type(
            instance_name,
            &raw.component,
            param,
            default_value,
            override_value,
        )?;
    }

    let mut merged = component.params.clone();
    for (name, value) in &raw.params {
        merged.insert(name.clone(), value.clone());
    }

    Ok(merged
        .iter()
        .map(|(name, value)| ParamValueIr {
            name: name.clone(),
            value: convert_param_value(value),
        })
        .collect())
}

fn validate_param_override_type(
    instance_name: &str,
    component_name: &str,
    param_name: &str,
    default_value: &RawValue,
    override_value: &RawValue,
) -> Result<()> {
    let default_value = convert_param_value(default_value);
    let override_value = convert_param_value(override_value);
    if param_value_compatible(&default_value, &override_value) {
        Ok(())
    } else {
        Err(IrError::IncompatibleParamOverride {
            instance: instance_name.to_string(),
            component: component_name.to_string(),
            param: param_name.to_string(),
            expected: param_value_kind(&default_value),
            actual: param_value_kind(&override_value),
        })
    }
}

/// 判断一个参数值是否可覆盖另一个参数值。
pub fn param_value_compatible(default_value: &ParamValue, override_value: &ParamValue) -> bool {
    match (default_value, override_value) {
        (ParamValue::Bool(_), ParamValue::Bool(_)) => true,
        (ParamValue::Integer(_), ParamValue::Integer(_)) => true,
        (ParamValue::Float(_), ParamValue::Float(_) | ParamValue::Integer(_)) => true,
        (ParamValue::String(_), ParamValue::String(_)) => true,
        (ParamValue::Array(default_values), ParamValue::Array(override_values)) => {
            array_param_compatible(default_values, override_values)
        }
        (ParamValue::Table(default_values), ParamValue::Table(override_values)) => override_values
            .iter()
            .all(|(name, value)| match default_values.get(name) {
                Some(default_value) => param_value_compatible(default_value, value),
                None => false,
            }),
        _ => false,
    }
}

fn array_param_compatible(default_values: &[ParamValue], override_values: &[ParamValue]) -> bool {
    if default_values.is_empty() {
        return override_values.is_empty();
    }
    if override_values.is_empty() {
        return true;
    }
    let Some(default_sample) = default_values.first() else {
        return false;
    };
    override_values
        .iter()
        .all(|value| param_value_compatible(default_sample, value))
}

/// 返回参数值的类别名称。
pub fn param_value_kind(value: &ParamValue) -> &'static str {
    match value {
        ParamValue::Bool(_) => "bool",
        ParamValue::Integer(_) => "integer",
        ParamValue::Float(_) => "float",
        ParamValue::String(_) => "string",
        ParamValue::Array(_) => "array",
        ParamValue::Table(_) => "table",
    }
}

fn normalize_binds(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
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

            Ok(ChannelEdgeIr {
                id: entity_id("bind", &format!("{}->{}", raw.from, raw.to)),
                from: parse_port_ref(&raw.from, instance_refs)?,
                to: parse_port_ref(&raw.to, instance_refs)?,
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
                capability_requirements: channel_capabilities(channel, overflow, stale),
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
                satisfied: deployment_satisfied(target, profile, &required_capabilities),
            });
        }
    }

    deployments
}

fn deployment_satisfied(
    target: &TargetIr,
    profile: &ProfileIr,
    required_capabilities: &[CapabilityAtom],
) -> bool {
    let backend_supported_by_target = target
        .backends
        .iter()
        .any(|backend| backend.0 == profile.backend.0);
    let backend_capabilities = backend_capabilities(&profile.backend.0).unwrap_or_default();
    let capabilities_satisfied = required_capabilities
        .iter()
        .all(|required| backend_capabilities.contains(required));
    backend_supported_by_target && capabilities_satisfied
}

fn convert_param_value(value: &RawValue) -> ParamValue {
    match value {
        RawValue::Bool(value) => ParamValue::Bool(*value),
        RawValue::Integer(value) => ParamValue::Integer(*value),
        RawValue::Float(value) => ParamValue::Float(*value),
        RawValue::String(value) => ParamValue::String(value.clone()),
        RawValue::Array(values) => {
            ParamValue::Array(values.iter().map(convert_param_value).collect())
        }
        RawValue::Table(values) => ParamValue::Table(
            values
                .iter()
                .map(|(name, value)| (name.clone(), convert_param_value(value)))
                .collect(),
        ),
    }
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
    use crate::{ChannelKind, PrimitiveType, TypeExpr};

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
    fn int128_component_ports_require_abi_capability() {
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
            ir.deployments[0]
                .required_capabilities
                .contains(&CapabilityAtom("abi:int128".to_string()))
        );
        assert!(!ir.deployments[0].satisfied);
    }

    #[test]
    fn declared_int128_message_types_require_abi_capability_even_when_not_port_reachable() {
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
            ir.deployments[0]
                .required_capabilities
                .contains(&CapabilityAtom("abi:int128".to_string()))
        );
        assert!(!ir.deployments[0].satisfied);
    }

    #[test]
    fn iox2_does_not_satisfy_int128_abi_capability() {
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
            ir.deployments[0]
                .required_capabilities
                .contains(&CapabilityAtom("abi:int128".to_string()))
        );
        assert!(!ir.deployments[0].satisfied);
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
            channel_capabilities(defaulted.channel, defaulted.overflow, defaulted.stale)
        );

        assert_eq!(explicit.overflow, OverflowPolicy::DropNewest);
        assert_eq!(explicit.stale, StalePolicy::HoldLast);
        assert_eq!(explicit.max_age_ms, Some(7));
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
