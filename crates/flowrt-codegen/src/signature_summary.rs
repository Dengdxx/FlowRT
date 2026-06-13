use flowrt_ir::{ComponentIr, ComponentKind, ContractIr, LanguageKind};

use crate::runtime_plan::{
    OperationRuntimePlan, ServiceRuntimePlan, operation_runtime_plans, service_runtime_plans,
};

/// 生成用户实现入口的只读摘要。
///
/// 该摘要供 `flowrt check` 提前暴露 generated API 形状，避免用户只能在编译
/// 失败后再翻生成文件。它不写入任何 FlowRT 管理产物，也不改变 codegen 结果。
pub fn handler_signature_summary(contract: &ContractIr) -> String {
    let mut output = String::from("generated user API summary:");
    for graph in &contract.graphs {
        output.push_str(&format!("\ngraph {}", graph.name));
        let service_plans = service_runtime_plans(contract, graph);
        let operation_plans = operation_runtime_plans(contract, graph);
        for component in &contract.components {
            output.push_str(&component_signature_summary(
                component,
                &service_plans,
                &operation_plans,
            ));
        }
    }
    output
}

fn component_signature_summary(
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> String {
    let mut output = format!(
        "\n  component {} language={} kind={}",
        component.name,
        language_name(component.language),
        component_kind_name(component.kind)
    );
    output.push_str("\n    user handlers:");
    output.push_str(&format!(
        "\n      {}",
        on_tick_signature(component, service_plans, operation_plans)
    ));
    if !component.params.is_empty() {
        output.push_str(&format!("\n      {}", params_update_signature(component)));
    }
    output
}

pub(crate) fn on_tick_signature(
    component: &ComponentIr,
    service_plans: &[ServiceRuntimePlan],
    operation_plans: &[OperationRuntimePlan],
) -> String {
    match component.language {
        LanguageKind::C => {
            "C callback table adapter declared in flowrt_app/c_components.h".to_string()
        }
        LanguageKind::Rust => {
            let args =
                crate::rust_shell::rust_callback_args(component, service_plans, operation_plans);
            if args.is_empty() {
                "fn on_tick(&mut self) -> flowrt::Status".to_string()
            } else {
                format!(
                    "fn on_tick(&mut self, {}) -> flowrt::Status",
                    args.join(", ")
                )
            }
        }
        LanguageKind::Cpp => {
            let args =
                crate::cpp_shell::cpp_callback_args(component, service_plans, operation_plans);
            if args.is_empty() {
                "flowrt::Status on_tick()".to_string()
            } else {
                format!("flowrt::Status on_tick({})", args.join(", "))
            }
        }
        LanguageKind::External => "no generated on_tick handler".to_string(),
    }
}

pub(crate) fn params_update_signature(component: &ComponentIr) -> String {
    match component.language {
        LanguageKind::C => "no generated C params handler yet".to_string(),
        LanguageKind::Rust => {
            let name = crate::component_rust_name(component);
            format!(
                "fn on_params_update(&mut self, old_params: &{name}Params, new_params: &{name}Params, context: &mut flowrt::Context) -> flowrt::Status"
            )
        }
        LanguageKind::Cpp => {
            let name = crate::cpp_shell::component_cpp_name(component);
            format!(
                "flowrt::Status on_params_update(const {name}Params& old_params, const {name}Params& new_params, flowrt::Context& context)"
            )
        }
        LanguageKind::External => "no generated params handler".to_string(),
    }
}

pub(crate) fn component_kind_name(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Native => "native",
        ComponentKind::IoBoundary => "io_boundary",
        ComponentKind::External => "external",
    }
}

pub(crate) fn language_name(language: LanguageKind) -> &'static str {
    match language {
        LanguageKind::C => "c",
        LanguageKind::Rust => "rust",
        LanguageKind::Cpp => "cpp",
        LanguageKind::External => "external",
    }
}
