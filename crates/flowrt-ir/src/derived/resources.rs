use crate::{
    ComponentIr, GraphIr, ResourceSatisfactionIr,
    derive_resource_satisfactions as derive_graph_resource_satisfactions,
};

/// 单个 graph 的 resource satisfaction 事实和摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDerivedFacts {
    /// 每个 active instance requirement 的重新推导满足状态。
    pub satisfactions: Vec<ResourceSatisfactionIr>,
    /// 已满足的 requirement 数量。
    pub satisfied_count: usize,
    /// 未满足且 required 的 requirement 数量。
    pub required_unsatisfied_count: usize,
    /// 未满足且 optional 的 requirement 数量。
    pub optional_unsatisfied_count: usize,
}

pub(super) fn derive_resource_facts(
    graph: &GraphIr,
    components: &[ComponentIr],
) -> ResourceDerivedFacts {
    let satisfactions = derive_graph_resource_satisfactions(
        &graph.name,
        &graph.instances,
        components,
        &graph.resource_providers,
    );
    let satisfied_count = satisfactions
        .iter()
        .filter(|satisfaction| satisfaction.satisfied)
        .count();
    let required_unsatisfied_count = satisfactions
        .iter()
        .filter(|satisfaction| satisfaction.required && !satisfaction.satisfied)
        .count();
    let optional_unsatisfied_count = satisfactions
        .iter()
        .filter(|satisfaction| !satisfaction.required && !satisfaction.satisfied)
        .count();

    ResourceDerivedFacts {
        satisfactions,
        satisfied_count,
        required_unsatisfied_count,
        optional_unsatisfied_count,
    }
}
