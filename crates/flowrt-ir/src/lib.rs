//! Contract IR 模型和归一化入口。
//!
//! Contract IR 是 FlowRT 工具链、validator、codegen 和 runtime shell 共享的语义合同。
//! RSDL 文本必须先归一化为强类型 IR，后续阶段不应直接依赖源文件的表结构。

mod backend;
mod error;
mod model;
mod normalize;
mod type_expr;

pub use backend::{
    DeploymentCapabilityDecision, OPERATION_DEFAULT_MAX_IN_FLIGHT, OPERATION_DEFAULT_QUEUE_DEPTH,
    OPERATION_DEFAULT_RESULT_RETENTION_MS, OPERATION_DEFAULT_TIMEOUT_MS, RouteTopology,
    SERVICE_DEFAULT_MAX_IN_FLIGHT, SERVICE_DEFAULT_QUEUE_DEPTH, SERVICE_DEFAULT_TIMEOUT_MS,
    backend_capabilities, base_deployment_capabilities, channel_capabilities,
    channel_route_capabilities, deployment_capability_decision, graph_required_capabilities,
    is_known_backend, is_known_operation_backend, is_known_service_backend,
    message_abi_capabilities, resolve_channel_backend, target_capabilities, trigger_capability,
};
pub use error::{IrError, Result};
pub use model::*;
pub use normalize::{
    hash_source, normalize_document, normalize_loaded_document, param_value_compatible,
    param_value_kind, project_contract_to_profile,
};
pub use type_expr::{PrimitiveType, StringEncoding, TypeExpr, parse_type_expr};

/// 当前工具链支持的 RSDL 源语言版本。
pub const RSDL_VERSION: &str = "0.1";

/// 当前工具链支持的 Contract IR 语义版本。
pub const CONTRACT_IR_VERSION: &str = "0.1";

/// 当前工具链支持的 Contract IR canonical JSON schema 版本。
pub const CONTRACT_SCHEMA_VERSION: &str = "0.1";

/// 根据 module/name 派生 codegen 使用的 canonical generated symbol。
pub fn canonical_generated_symbol(module: Option<&str>, name: &str) -> String {
    let Some(module) = module else {
        return name.to_string();
    };

    let mut output = String::new();
    let mut capitalize_next = true;
    for ch in module
        .chars()
        .chain(std::iter::once('_'))
        .chain(name.chars())
    {
        if ch.is_ascii_alphanumeric() {
            if capitalize_next {
                output.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                output.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }
    if output.is_empty() {
        name.to_string()
    } else {
        output
    }
}
