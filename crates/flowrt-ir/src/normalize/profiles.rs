use flowrt_rsdl::{RawDocument, RawGraphMode, RawProfile};

use crate::{
    BackendName, ContractIr, GraphMode, IrError, OverflowPolicy, PolicyDefaults, PolicyValueSource,
    ProfileIr, Result, RouteTopology, SchedulerDefaults, StalePolicy, channel_capabilities,
    channel_route_capabilities, deployment_capability_decision, graph_required_capabilities,
};

use super::backends::{
    resolve_channel_backend, route_topology_by_bind_id, source_port_types_by_endpoint,
};
use super::ids::entity_id;

pub(super) fn normalize_profiles(document: &RawDocument) -> Result<Vec<ProfileIr>> {
    if document.profiles.is_empty() {
        return Ok(vec![ProfileIr {
            id: entity_id("profile", "default"),
            name: "default".to_string(),
            mode: GraphMode::Strict,
            backend: BackendName("inproc".to_string()),
            scheduler: SchedulerDefaults { worker_threads: 1 },
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
                mode: normalize_graph_mode(raw.mode),
                backend: BackendName(raw.backend.clone().unwrap_or_else(|| "inproc".to_string())),
                scheduler: normalize_scheduler_defaults(raw, &format!("profile.{name}"))?,
                defaults: normalize_policy_defaults(raw, &format!("profile.{name}"))?,
            })
        })
        .collect()
}

fn normalize_graph_mode(mode: RawGraphMode) -> GraphMode {
    match mode {
        RawGraphMode::Strict => GraphMode::Strict,
        RawGraphMode::Island => GraphMode::Island,
    }
}

fn normalize_scheduler_defaults(raw: &RawProfile, context: &str) -> Result<SchedulerDefaults> {
    let worker_threads = raw.worker_threads.unwrap_or(1);
    if worker_threads == 0 {
        return Err(IrError::InvalidValue {
            context: format!("{context}.worker_threads"),
            message: "`worker_threads` must be greater than zero".to_string(),
        });
    }
    Ok(SchedulerDefaults { worker_threads })
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
        let topology_lookup = route_topology_by_bind_id(graph, &contract.components);
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
            if bind.backend_policy_source != PolicyValueSource::Explicit {
                let resolved = resolve_channel_backend(
                    profile.backend.0.as_str(),
                    source_type,
                    &contract.types,
                    topology,
                    false,
                )
                .expect(
                    "profile default backend resolution should not fail without explicit backend",
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
    use std::collections::BTreeMap;

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

pub(super) fn parse_overflow_policy(context: &str, value: &str) -> Result<OverflowPolicy> {
    match value {
        "drop_oldest" => Ok(OverflowPolicy::DropOldest),
        "drop_newest" => Ok(OverflowPolicy::DropNewest),
        "error" => Ok(OverflowPolicy::Error),
        "block" => Ok(OverflowPolicy::Block),
        _ => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "overflow policy",
            value: value.to_string(),
        }),
    }
}

pub(super) fn parse_stale_policy(context: &str, value: &str) -> Result<StalePolicy> {
    match value {
        "warn" => Ok(StalePolicy::Warn),
        "drop" => Ok(StalePolicy::Drop),
        "hold_last" => Ok(StalePolicy::HoldLast),
        "error" => Ok(StalePolicy::Error),
        _ => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "stale policy",
            value: value.to_string(),
        }),
    }
}
