use std::collections::BTreeMap;

use flowrt_rsdl::{RawComponent, RawDocument, RawPort, RawProfile, RawTarget, RawValue};
use sha2::{Digest, Sha256};

use crate::{
    BackendName, CapabilityAtom, ChannelEdgeIr, ChannelKind, ComponentIr, ComponentKind,
    ContractIr, DeploymentIr, EntityId, EntityRef, FieldIr, GraphIr, ImportIr, InstanceIr, IrError,
    LanguageKind, LifecycleSurface, OverflowPolicy, PackageIr, ParamIr, ParamValue, ParamValueIr,
    PolicyDefaults, PortIr, PortRef, ProfileIr, Result, StalePolicy, TargetIr, TaskIr, TriggerKind,
    TypeIr, backend_capabilities, base_deployment_capabilities, parse_type_expr,
    trigger_capability,
};

const IR_VERSION: &str = "0.1";
const SCHEMA_VERSION: &str = "0.1";

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
            .map(|(kind, patterns)| ImportIr {
                kind: kind.clone(),
                patterns: patterns.clone(),
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

    let deployments = normalize_deployments(&graph, &profiles, &targets);

    Ok(ContractIr {
        ir_version: IR_VERSION.to_string(),
        schema_version: SCHEMA_VERSION.to_string(),
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
/// `None` 保持原始 contract 不变；`Some(name)` 会要求 contract 中存在该 profile，
/// 并返回仅保留该 profile 的克隆版本，便于 codegen 和 CLI 统一按选定 profile 生成产物。
pub fn project_contract_to_profile(
    contract: &ContractIr,
    profile_name: Option<&str>,
) -> Result<ContractIr> {
    let Some(profile_name) = profile_name else {
        return Ok(contract.clone());
    };

    let Some(profile) = contract
        .profiles
        .iter()
        .find(|profile| profile.name == profile_name)
    else {
        return Err(crate::IrError::UnknownProfile {
            profile: profile_name.to_string(),
        });
    };

    let mut projected = contract.clone();
    projected.profiles = vec![profile.clone()];
    projected
        .deployments
        .retain(|deployment| deployment.profile.name == profile_name);
    Ok(projected)
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
            Ok(TargetIr {
                id: entity_id("target", name),
                name: name.clone(),
                platform: raw.platform.clone(),
                runtime: normalize_target_runtime(name, raw)?,
                backends: raw.backends.iter().cloned().map(BackendName).collect(),
                capabilities: target_capabilities(&raw.backends),
            })
        })
        .collect()
}

fn target_capabilities(backends: &[String]) -> Vec<CapabilityAtom> {
    let capabilities = backends
        .iter()
        .filter_map(|backend| backend_capabilities(backend))
        .flatten()
        .collect::<Vec<_>>();
    dedupe_capabilities(capabilities)
}

fn normalize_target_runtime(target_name: &str, raw: &RawTarget) -> Result<Vec<LanguageKind>> {
    raw.runtime
        .iter()
        .map(|language| parse_language(&format!("target.{target_name}.runtime"), language))
        .collect()
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
    if param_value_compatible(default_value, override_value) {
        Ok(())
    } else {
        Err(IrError::IncompatibleParamOverride {
            instance: instance_name.to_string(),
            component: component_name.to_string(),
            param: param_name.to_string(),
            expected: param_value_kind(default_value),
            actual: param_value_kind(override_value),
        })
    }
}

fn param_value_compatible(default_value: &RawValue, override_value: &RawValue) -> bool {
    match (default_value, override_value) {
        (RawValue::Bool(_), RawValue::Bool(_)) => true,
        (RawValue::Integer(_), RawValue::Integer(_)) => true,
        (RawValue::Float(_), RawValue::Float(_) | RawValue::Integer(_)) => true,
        (RawValue::String(_), RawValue::String(_)) => true,
        (RawValue::Array(default_values), RawValue::Array(override_values)) => {
            array_param_compatible(default_values, override_values)
        }
        (RawValue::Table(default_values), RawValue::Table(override_values)) => override_values
            .iter()
            .all(|(name, value)| match default_values.get(name) {
                Some(default_value) => param_value_compatible(default_value, value),
                None => false,
            }),
        _ => false,
    }
}

fn array_param_compatible(default_values: &[RawValue], override_values: &[RawValue]) -> bool {
    if default_values.is_empty() || override_values.is_empty() {
        return true;
    }
    let Some(default_sample) = default_values.first() else {
        return true;
    };
    override_values
        .iter()
        .all(|value| param_value_compatible(default_sample, value))
}

fn param_value_kind(value: &RawValue) -> &'static str {
    match value {
        RawValue::Bool(_) => "bool",
        RawValue::Integer(_) => "integer",
        RawValue::Float(_) => "float",
        RawValue::String(_) => "string",
        RawValue::Array(_) => "array",
        RawValue::Table(_) => "table",
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

    document
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
                capability_requirements: channel_capabilities(channel, overflow, stale),
            })
        })
        .collect()
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
    profiles: &[ProfileIr],
    targets: &[TargetIr],
) -> Vec<DeploymentIr> {
    let graph_ref = EntityRef {
        id: graph.id.clone(),
        name: graph.name.clone(),
    };
    let mut deployments = Vec::new();
    let required_capabilities = graph_required_capabilities(graph);

    for profile in profiles {
        for target in targets {
            let backend_supported_by_target = target
                .backends
                .iter()
                .any(|backend| backend.0 == profile.backend.0);
            let backend_capabilities = backend_capabilities(&profile.backend.0).unwrap_or_default();
            let capabilities_satisfied = required_capabilities
                .iter()
                .all(|required| backend_capabilities.contains(required));
            let satisfied = backend_supported_by_target && capabilities_satisfied;
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
                satisfied,
            });
        }
    }

    deployments
}

fn graph_required_capabilities(graph: &GraphIr) -> Vec<CapabilityAtom> {
    let mut capabilities = base_deployment_capabilities();
    for task in &graph.tasks {
        capabilities.push(trigger_capability(task.trigger));
        if task.deadline_ms.is_some() {
            capabilities.push(CapabilityAtom("timing:deadline_aware".to_string()));
        }
    }
    for bind in &graph.binds {
        capabilities.extend(bind.capability_requirements.clone());
    }
    dedupe_capabilities(capabilities)
}

fn dedupe_capabilities(capabilities: Vec<CapabilityAtom>) -> Vec<CapabilityAtom> {
    let mut seen = std::collections::BTreeSet::new();
    capabilities
        .into_iter()
        .filter(|capability| seen.insert(capability.clone()))
        .collect()
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

fn channel_capabilities(
    channel: ChannelKind,
    overflow: OverflowPolicy,
    stale: StalePolicy,
) -> Vec<CapabilityAtom> {
    vec![
        CapabilityAtom(channel_capability_name(channel).to_string()),
        CapabilityAtom(overflow_capability_name(overflow).to_string()),
        CapabilityAtom(stale_capability_name(stale).to_string()),
    ]
}

fn channel_capability_name(channel: ChannelKind) -> &'static str {
    match channel {
        ChannelKind::Latest => "channel:latest",
        ChannelKind::Fifo => "channel:fifo",
    }
}

fn overflow_capability_name(policy: OverflowPolicy) -> &'static str {
    match policy {
        OverflowPolicy::DropOldest => "overflow:drop_oldest",
        OverflowPolicy::DropNewest => "overflow:drop_newest",
        OverflowPolicy::Error => "overflow:error",
        OverflowPolicy::Block => "overflow:block",
    }
}

fn stale_capability_name(policy: StalePolicy) -> &'static str {
    match policy {
        StalePolicy::Warn => "stale:warn",
        StalePolicy::Drop => "stale:drop",
        StalePolicy::HoldLast => "stale:hold_last",
        StalePolicy::Error => "stale:error",
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
