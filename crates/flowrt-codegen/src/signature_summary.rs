use flowrt_ir::ContractIr;

use crate::app_api::{app_api_manifest, format_app_api_signature_summary};

/// 生成用户实现入口的只读摘要。
///
/// 该摘要供 `flowrt check` 提前暴露 App API 形状，避免用户只能在编译失败后再翻生成文件。
/// 它不写入任何 FlowRT 管理产物，也不改变 codegen 结果。
pub fn handler_signature_summary(contract: &ContractIr) -> String {
    let manifest = app_api_manifest(contract);
    format_app_api_signature_summary(&manifest)
}
