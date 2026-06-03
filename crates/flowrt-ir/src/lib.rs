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
    backend_capabilities, base_deployment_capabilities, is_known_backend, trigger_capability,
};
pub use error::{IrError, Result};
pub use model::*;
pub use normalize::{hash_source, normalize_document};
pub use type_expr::{PrimitiveType, TypeExpr, parse_type_expr};
