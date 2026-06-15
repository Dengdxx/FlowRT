//! Contract IR 派生事实入口。
//!
//! 该模块从 normalized `ContractIr` 重新推导 backend、capability、deployment 和 resource
//! 等事实，供 validator/codegen 后续统一消费。调用方不需要知道各个散函数的调用顺序。

use std::collections::BTreeMap;

use crate::{
    CapabilityAtom, ContractIr, EntityRef, GraphIr, Result,
    graph_required_capabilities as derive_graph_required_capabilities,
    message_abi_capabilities as derive_message_abi_capabilities,
};

mod backends;
mod deployment;
mod resources;

pub use backends::RouteDerivedFacts;
pub use deployment::{DeploymentDerivedFacts, TargetDerivedFacts};
pub use resources::ResourceDerivedFacts;

/// 整个 Contract IR 可重新推导的事实集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractDerivedFacts {
    /// Contract 中所有 message 与 component port 类型需要的 ABI 能力。
    pub message_abi_capabilities: Vec<CapabilityAtom>,
    /// 每个 graph 的 route、deployment required capability 和 resource 事实。
    pub graphs: Vec<GraphDerivedFacts>,
    /// 每个 target 从 backend catalog 重新推导的 capability。
    pub targets: Vec<TargetDerivedFacts>,
    /// 每个 deployment 从 graph/profile/target 重新推导的 capability 决策。
    pub deployments: Vec<DeploymentDerivedFacts>,
}

/// 单个 graph 的派生事实集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphDerivedFacts {
    /// Graph 引用。
    pub graph: EntityRef,
    /// Graph 内 dataflow route 的 backend、topology 和 capability 事实。
    pub routes: Vec<RouteDerivedFacts>,
    /// Graph deployment 需要的基础和 task capability。
    pub required_capabilities: Vec<CapabilityAtom>,
    /// Graph resource requirement 的满足状态。
    pub resources: ResourceDerivedFacts,
}

/// 从 Contract IR 重新推导 backend/capability/deployment/resource 事实。
pub fn derive_contract_facts(contract: &ContractIr) -> Result<ContractDerivedFacts> {
    let message_abi_capabilities =
        derive_message_abi_capabilities(&contract.types, contract.components.iter());
    let mut graph_required_capabilities = BTreeMap::new();
    let mut graphs = Vec::with_capacity(contract.graphs.len());

    for graph in &contract.graphs {
        let required_capabilities =
            derive_graph_required_capabilities(graph, &contract.types, &contract.components);
        graph_required_capabilities.insert(graph.name.clone(), required_capabilities.clone());
        graphs.push(GraphDerivedFacts {
            graph: graph_ref(graph),
            routes: backends::derive_route_facts(contract, graph)?,
            required_capabilities,
            resources: resources::derive_resource_facts(graph, &contract.components),
        });
    }

    Ok(ContractDerivedFacts {
        message_abi_capabilities,
        graphs,
        targets: deployment::derive_target_facts(&contract.targets),
        deployments: deployment::derive_deployment_facts(contract, &graph_required_capabilities)?,
    })
}

fn graph_ref(graph: &GraphIr) -> EntityRef {
    EntityRef {
        id: graph.id.clone(),
        name: graph.name.clone(),
    }
}

#[cfg(test)]
mod tests;
