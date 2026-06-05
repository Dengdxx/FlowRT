//! normalized Contract IR 的校验 passes。
//!
//! 本 crate 只校验已经归一化后的 IR，不直接读取 RSDL 源文本。校验失败时会聚合多个错误，
//! 便于 CLI 一次性报告 contract 中的结构问题。

mod capabilities;
mod components;
mod contract;
mod graphs;
mod names;
mod types;

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{Display, Formatter};

use capabilities::{
    validate_channel_policy_sources, validate_declared_backends, validate_deployment_matrix,
    validate_deployments, validate_derived_capabilities,
};
use components::validate_components;
use contract::{
    validate_contract_canonical_fields, validate_contract_canonical_ordering,
    validate_contract_shape, validate_contract_versions, validate_entity_id_uniqueness,
    validate_entity_name_uniqueness, validate_entity_references,
};
use flowrt_ir::ContractIr;
use graphs::validate_graphs;
use names::validate_names;
use types::{
    validate_message_abi, validate_message_type_cycles, validate_message_types,
    validate_variable_frame_shapes,
};

/// validation passes 返回的结果类型。
pub type Result<T> = std::result::Result<T, ValidationReport>;

/// validation report，可同时包含多个 contract 错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    pub errors: Vec<ValidationError>,
}

impl ValidationReport {
    /// 判断报告是否不包含任何错误。
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }
}

impl Display for ValidationReport {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            formatter,
            "validation failed with {} error(s)",
            self.errors.len()
        )?;
        for error in &self.errors {
            writeln!(formatter, "- {}", error.message)?;
        }
        Ok(())
    }
}

impl Error for ValidationReport {}

/// 单个 contract 校验错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub message: String,
}

impl ValidationError {
    /// 构造一个校验错误。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// 校验一个 normalized Contract IR 文档。
pub fn validate_contract(ir: &ContractIr) -> Result<()> {
    let mut errors = Vec::new();

    let type_names = ir
        .types
        .iter()
        .map(|ty| ty.name.as_str())
        .collect::<BTreeSet<_>>();
    validate_contract_versions(ir, &mut errors);
    validate_contract_shape(ir, &mut errors);
    validate_contract_canonical_fields(ir, &mut errors);
    validate_contract_canonical_ordering(ir, &mut errors);
    validate_entity_name_uniqueness(ir, &mut errors);
    validate_entity_id_uniqueness(ir, &mut errors);
    validate_entity_references(ir, &mut errors);
    validate_deployment_matrix(ir, &mut errors);
    validate_derived_capabilities(ir, &mut errors);
    validate_channel_policy_sources(ir, &mut errors);
    validate_names(ir, &mut errors);
    validate_message_types(ir, &type_names, &mut errors);
    validate_variable_frame_shapes(ir, &mut errors);
    validate_message_type_cycles(ir, &mut errors);
    validate_message_abi(ir, &mut errors);
    validate_components(ir, &type_names, &mut errors);
    validate_graphs(ir, &mut errors);
    validate_declared_backends(ir, &mut errors);
    validate_deployments(ir, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ValidationReport { errors })
    }
}

#[cfg(test)]
mod tests;
