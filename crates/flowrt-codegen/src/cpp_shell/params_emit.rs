use super::*;

pub(super) fn cpp_params_struct(component: &ComponentIr) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "struct {}Params {{\n",
        component_cpp_name(component)
    ));
    for param in &component.params {
        output.push_str(&format!(
            "    {} {}{{}};\n",
            cpp_param_type(param.ty),
            param.name
        ));
    }
    output.push_str("};\n\n");
    output
}

pub(super) fn cpp_params_initializer(component: &ComponentIr, instance: &InstanceIr) -> String {
    let mut output = format!("{}Params{{", component_cpp_name(component));
    for param in &component.params {
        let value = param_value_for_instance(instance, param);
        output.push_str(&format!(
            "\n        .{} = {},",
            param.name,
            cpp_param_literal(param, value)
        ));
    }
    output.push_str("\n    }");
    output
}

pub(super) fn cpp_params_update_signature(component: &ComponentIr) -> String {
    if component.params.is_empty() {
        return String::new();
    }
    let params_ty = format!("{}Params", component_cpp_name(component));
    format!(
        "    /**\n     * @brief 参数 pending 值在 tick 边界通过校验后调用。\n     *\n     * @param old_params 当前已生效参数快照。\n     * @param new_params 即将生效的新参数快照。\n     * @param context runtime 上下文。\n     * @return 返回 `Ok` 后 shell 才会提交新参数。\n     */\n    virtual flowrt::Status on_params_update(\n        const {params_ty}& old_params,\n        const {params_ty}& new_params,\n        flowrt::Context& context) {{\n        (void)old_params;\n        (void)new_params;\n        (void)context;\n        return flowrt::ok();\n    }}\n"
    )
}

pub(super) fn emit_cpp_introspection_param_registration(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        for param in &component.params {
            output.push_str(&format!(
                "    introspection_state.register_param(flowrt::IntrospectionParamSchema{{\n        .name = {},\n        .ty = {},\n        .update = {},\n        .current = {},\n        .min = {},\n        .max = {},\n        .choices = {},\n    }});\n",
                cpp_string_literal(&runtime_param_name(instance, param)),
                cpp_string_literal(param_type_name(param.ty)),
                cpp_string_literal(param_update_name(param.update)),
                cpp_string_literal(&param_json_literal(param_value_for_instance(instance, param))),
                cpp_optional_json_literal(param.min.as_ref()),
                cpp_optional_json_literal(param.max.as_ref()),
                cpp_json_fragment_vector_literal(&param.choices),
            ));
        }
    }
    output
}

pub(super) fn emit_cpp_param_constraint_helpers(
    order: &[&InstanceIr],
    contract: &ContractIr,
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        for param in &component.params {
            if param.update != ParamUpdatePolicy::OnTick || !param_has_constraints(param) {
                continue;
            }
            let helper = cpp_param_constraint_helper_name(instance, param);
            let ty = cpp_param_type(param.ty);
            let checks = cpp_param_constraint_checks(param);
            output.push_str(&format!(
                "bool {helper}(const {ty}& value) {{\n    return {checks};\n}}\n\n"
            ));
        }
    }
    output
}

pub(super) fn cpp_apply_pending_params(
    instance: &InstanceIr,
    component: &ComponentIr,
    nested: bool,
    context_name: &str,
) -> String {
    let mut output = String::new();
    let indent = step_indent(nested);
    let inner_indent = nested_step_indent(nested);
    for param in &component.params {
        if param.update != ParamUpdatePolicy::OnTick {
            continue;
        }
        let runtime_name = runtime_param_name(instance, param);
        let pending = format!("{}_{}_pending", instance.name, param.name);
        let next = format!("{}_{}_next_params", instance.name, param.name);
        output.push_str(&format!(
            "{indent}if (const auto {pending} = introspection_state.peek_pending_param({runtime_name}); {pending}.has_value()) {{\n",
            runtime_name = cpp_string_literal(&runtime_name)
        ));
        output.push_str(&format!(
            "{inner_indent}auto {next} = {instance}_params_;\n",
            instance = instance.name
        ));
        output.push_str(&format!(
            "{inner_indent}if (!decode_flowrt_param_value(*{pending}, {next}.{field})) {{\n{deep_indent}introspection_state.record_param_rejected({runtime_name}, *{pending}, \"decode_failed\");\n",
            field = param.name,
            runtime_name = cpp_string_literal(&runtime_name),
            deep_indent = if nested { "                " } else { "            " }
        ));
        if param_has_constraints(param) {
            output.push_str(&format!(
                "{inner_indent}}} else if (!{}({next}.{field})) {{\n{deep_indent}introspection_state.record_param_rejected({runtime_name}, *{pending}, \"constraint_failed\");\n",
                cpp_param_constraint_helper_name(instance, param),
                field = param.name,
                runtime_name = cpp_string_literal(&runtime_name),
                deep_indent = if nested { "                " } else { "            " }
            ));
        }
        output.push_str(&format!(
            "{inner_indent}}} else if ({instance}_ && {instance}_->on_params_update({instance}_params_, {next}, {context_name}) != flowrt::Status::Ok) {{\n{deep_indent}introspection_state.record_param_rejected({runtime_name}, *{pending}, \"callback_rejected\");\n",
            instance = instance.name,
            runtime_name = cpp_string_literal(&runtime_name),
            deep_indent = if nested { "                " } else { "            " }
        ));
        output.push_str(&format!(
            "{inner_indent}}} else {{\n{deep_indent}{instance}_params_ = std::move({next});\n{deep_indent}introspection_state.record_param_applied({runtime_name}, *{pending});\n{inner_indent}}}\n",
            instance = instance.name,
            runtime_name = cpp_string_literal(&runtime_name),
            deep_indent = if nested { "                " } else { "            " }
        ));
        output.push_str(&format!("{indent}}}\n"));
    }
    output
}

pub(super) fn param_has_constraints(param: &ParamIr) -> bool {
    param.min.is_some() || param.max.is_some() || !param.choices.is_empty()
}

pub(super) fn cpp_param_constraint_helper_name(instance: &InstanceIr, param: &ParamIr) -> String {
    format!(
        "flowrt_validate_pending_param_{}_{}",
        crate::snake_identifier(&instance.name),
        crate::snake_identifier(&param.name)
    )
}

pub(super) fn cpp_param_constraint_checks(param: &ParamIr) -> String {
    let mut checks = Vec::new();
    if param_type_supports_range(param.ty)
        && let Some(min) = &param.min
    {
        if let Some(literal) = cpp_param_constraint_literal(param, min) {
            checks.push(format!("value >= {literal}"));
        }
    }
    if param_type_supports_range(param.ty)
        && let Some(max) = &param.max
    {
        if let Some(literal) = cpp_param_constraint_literal(param, max) {
            checks.push(format!("value <= {literal}"));
        }
    }
    let choices = param
        .choices
        .iter()
        .filter_map(|choice| cpp_param_constraint_literal(param, choice))
        .map(|literal| format!("value == {literal}"))
        .collect::<Vec<_>>();
    if !choices.is_empty() {
        checks.push(format!("({})", choices.join(" || ")));
    }
    if checks.is_empty() {
        "true".to_string()
    } else {
        checks.join(" && ")
    }
}

pub(super) fn param_type_supports_range(ty: ParamType) -> bool {
    matches!(
        ty,
        ParamType::U8
            | ParamType::U16
            | ParamType::U32
            | ParamType::U64
            | ParamType::I8
            | ParamType::I16
            | ParamType::I32
            | ParamType::I64
            | ParamType::F32
            | ParamType::F64
            | ParamType::String
    )
}

pub(super) fn cpp_param_constraint_literal(param: &ParamIr, value: &ParamValue) -> Option<String> {
    match (param.ty, value) {
        (
            ParamType::Bool
            | ParamType::U8
            | ParamType::U16
            | ParamType::U32
            | ParamType::U64
            | ParamType::I8
            | ParamType::I16
            | ParamType::I32
            | ParamType::I64
            | ParamType::F32
            | ParamType::F64
            | ParamType::String,
            _,
        ) => Some(cpp_param_literal(param, value)),
        (ParamType::Array | ParamType::Table, _) => Some(cpp_param_literal(param, value)),
    }
}

pub(super) fn cpp_optional_json_literal(value: Option<&ParamValue>) -> String {
    match value {
        Some(value) => format!(
            "std::optional<std::string>{{{}}}",
            cpp_string_literal(&param_json_literal(value))
        ),
        None => "std::nullopt".to_string(),
    }
}

pub(super) fn cpp_json_fragment_vector_literal(values: &[ParamValue]) -> String {
    if values.is_empty() {
        return "{}".to_string();
    }
    let values = values
        .iter()
        .map(|value| cpp_string_literal(&param_json_literal(value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("std::vector<std::string>{{{values}}}")
}

pub(super) fn emit_cpp_apply_pending_params_for_order(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = component_by_name(contract, &instance.component.name);
        if !component.params.is_empty() {
            output.push_str(&cpp_apply_pending_params(
                instance,
                component,
                false,
                "lifecycle_context",
            ));
        }
    }
    output
}
