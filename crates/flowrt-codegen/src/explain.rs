use flowrt_ir::ContractIr;

use crate::app_api::{AppApiManifest, app_api_manifest, format_app_api_manifest_text};

/// `flowrt explain` 输出的结构化报告，复用 `flowrt prepare` 生成的 App API 模型。
pub type ExplainReport = AppApiManifest;

/// 生成 `flowrt explain` 的结构化报告。
pub fn explain_report(contract: &ContractIr) -> ExplainReport {
    app_api_manifest(contract)
}

/// 将 `flowrt explain --format text` 报告渲染为稳定文本。
pub fn format_explain_report_text(report: &ExplainReport) -> String {
    format_app_api_manifest_text(report)
}
