use flowrt_rsdl::{LoadedDocument, RawDocument};

use crate::{ContractIr, Result};

mod assembly;
mod backends;
mod graphs;
mod ids;
mod modules;
mod operations;
mod package;
mod params;
mod profiles;
mod resolver;
mod services;
mod targets;

pub use ids::hash_source;
pub use params::{param_value_compatible, param_value_kind};
pub use profiles::project_contract_to_profile;

pub fn derive_resource_satisfactions(
    graph_name: &str,
    instances: &[crate::InstanceIr],
    components: &[crate::ComponentIr],
    providers: &[crate::ResourceProviderIr],
) -> Vec<crate::ResourceSatisfactionIr> {
    graphs::derive_resource_satisfactions(graph_name, instances, components, providers)
}

pub fn provider_satisfies_instance_requirement(
    provider: &crate::ResourceProviderIr,
    instance: &crate::InstanceIr,
    capability: &crate::CapabilityAtom,
) -> bool {
    graphs::provider_satisfies_instance_requirement(provider, instance, capability)
}

pub(crate) fn entity_id_for_projection(kind: &str, qualified_name: &str) -> crate::EntityId {
    ids::entity_id(kind, qualified_name)
}

/// 将已解析的 RSDL 文档归一化为 Contract IR。
pub fn normalize_document(document: &RawDocument, source_hash: String) -> Result<ContractIr> {
    assembly::normalize_document_with_modules(document, &[], source_hash)
}

/// 将带 workspace/module 边界的 RSDL 文档归一化为 Contract IR。
pub fn normalize_loaded_document(
    loaded: &LoadedDocument,
    source_hash: String,
) -> Result<ContractIr> {
    assembly::normalize_document_with_modules(&loaded.document, &loaded.modules, source_hash)
}

#[cfg(test)]
mod tests;
