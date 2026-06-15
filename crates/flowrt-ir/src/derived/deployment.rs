use std::collections::BTreeMap;

use crate::{
    BackendName, CapabilityAtom, ContractIr, DeploymentCapabilityDecision, EntityRef, IrError,
    Result, TargetIr, deployment_capability_decision, target_capabilities,
};

/// 单个 target 从 backend catalog 重新推导得到的 capability 事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDerivedFacts {
    /// Target 引用。
    pub target: EntityRef,
    /// Target 声明的 backend 列表。
    pub backends: Vec<BackendName>,
    /// Target backend 集合提供的 capability。
    pub capabilities: Vec<CapabilityAtom>,
}

/// 单个 deployment 重新推导得到的 capability 决策事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentDerivedFacts {
    /// Deployment 引用。
    pub deployment: EntityRef,
    /// Deployment 选择的 graph。
    pub graph: EntityRef,
    /// Deployment 选择的 profile。
    pub profile: EntityRef,
    /// Deployment 选择的 target。
    pub target: EntityRef,
    /// Profile 重新推导出的 selected backend。
    pub backend: BackendName,
    /// Graph 重新推导出的 required capabilities。
    pub required_capabilities: Vec<CapabilityAtom>,
    /// Backend、target 和 required capability 的最终决策。
    pub decision: DeploymentCapabilityDecision,
}

pub(super) fn derive_target_facts(targets: &[TargetIr]) -> Vec<TargetDerivedFacts> {
    targets
        .iter()
        .map(|target| TargetDerivedFacts {
            target: EntityRef {
                id: target.id.clone(),
                name: target.name.clone(),
            },
            backends: target.backends.clone(),
            capabilities: target_capabilities(&target.backends),
        })
        .collect()
}

pub(super) fn derive_deployment_facts(
    contract: &ContractIr,
    graph_required_capabilities: &BTreeMap<String, Vec<CapabilityAtom>>,
) -> Result<Vec<DeploymentDerivedFacts>> {
    let profiles = contract
        .profiles
        .iter()
        .map(|profile| (profile.name.as_str(), profile))
        .collect::<BTreeMap<_, _>>();
    let targets = contract
        .targets
        .iter()
        .map(|target| (target.name.as_str(), target))
        .collect::<BTreeMap<_, _>>();

    contract
        .deployments
        .iter()
        .map(|deployment| {
            let required_capabilities = graph_required_capabilities
                .get(&deployment.graph.name)
                .cloned()
                .ok_or_else(|| invalid_deployment_ref("graph", &deployment.graph.name))?;
            let profile = profiles
                .get(deployment.profile.name.as_str())
                .copied()
                .ok_or_else(|| invalid_deployment_ref("profile", &deployment.profile.name))?;
            let target = targets
                .get(deployment.target.name.as_str())
                .copied()
                .ok_or_else(|| invalid_deployment_ref("target", &deployment.target.name))?;
            let decision = deployment_capability_decision(
                &profile.backend,
                &target.backends,
                &required_capabilities,
            );

            Ok(DeploymentDerivedFacts {
                deployment: EntityRef {
                    id: deployment.id.clone(),
                    name: format!(
                        "{}.{}.{}",
                        deployment.graph.name, deployment.profile.name, deployment.target.name
                    ),
                },
                graph: deployment.graph.clone(),
                profile: deployment.profile.clone(),
                target: deployment.target.clone(),
                backend: profile.backend.clone(),
                required_capabilities,
                decision,
            })
        })
        .collect()
}

fn invalid_deployment_ref(kind: &'static str, name: &str) -> IrError {
    IrError::InvalidValue {
        context: "contract.deployments".to_string(),
        message: format!("deployment references unknown {kind} `{name}`"),
    }
}
