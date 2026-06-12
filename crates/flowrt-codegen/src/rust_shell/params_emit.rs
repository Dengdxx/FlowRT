use flowrt_ir::{
    ComponentIr, ContractIr, InstanceIr, ParamIr, ParamType, ParamUpdatePolicy, ParamValue,
};

use crate::runtime_plan::runtime_param_name;

pub(super) fn rust_params_struct(component: &ComponentIr) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "#[derive(Clone, Debug, PartialEq)]\npub struct {}Params {{\n",
        crate::component_rust_name(component)
    ));
    for param in &component.params {
        output.push_str(&format!(
            "    pub {}: {},\n",
            param.name,
            rust_param_type(param.ty)
        ));
    }
    output.push_str("}\n\n");
    output
}

pub(super) fn rust_params_initializer(component: &ComponentIr, instance: &InstanceIr) -> String {
    let mut output = format!("{}Params {{", crate::component_rust_name(component));
    for param in &component.params {
        let value = crate::param_value_for_instance(instance, param);
        output.push_str(&format!(
            "\n                {}: {},",
            param.name,
            rust_param_literal(param, value)
        ));
    }
    output.push_str("\n            }");
    output
}

pub(super) fn rust_params_update_signature(component: &ComponentIr) -> String {
    if component.params.is_empty() {
        return String::new();
    }
    let params_ty = format!("{}Params", crate::component_rust_name(component));
    format!(
        "    /// 参数 pending 值在 tick 边界通过校验后调用。\n    ///\n    /// 返回 `Ok` 后 shell 才会提交新参数。\n    fn on_params_update(\n        &mut self,\n        _old: &{params_ty},\n        _new: &{params_ty},\n        _context: &mut flowrt::Context,\n    ) -> flowrt::Status {{\n        flowrt::Status::ok()\n    }}\n\n"
    )
}

pub(crate) fn emit_rust_introspection_param_registration(
    contract: &ContractIr,
    order: &[&InstanceIr],
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = crate::component_by_name(contract, &instance.component.name);
        for param in &component.params {
            output.push_str(&format!(
                "        introspection_state.register_param(flowrt::IntrospectionParamSchema {{\n            name: {}.to_string(),\n            ty: {}.to_string(),\n            update: {}.to_string(),\n            current: {},\n            min: {},\n            max: {},\n            choices: {},\n        }});\n",
                crate::rust_string_literal(&runtime_param_name(instance, param)),
                crate::rust_string_literal(crate::param_type_name(param.ty)),
                crate::rust_string_literal(crate::param_update_name(param.update)),
                crate::param_json_value_literal(crate::param_value_for_instance(instance, param)),
                rust_optional_param_json_value(param.min.as_ref()),
                rust_optional_param_json_value(param.max.as_ref()),
                rust_param_json_vec(&param.choices),
            ));
        }
    }
    output
}

pub(super) fn emit_rust_param_constraint_helpers(
    order: &[&InstanceIr],
    contract: &ContractIr,
) -> String {
    let mut output = String::new();
    for instance in order {
        let component = crate::component_by_name(contract, &instance.component.name);
        for param in &component.params {
            if param.update != ParamUpdatePolicy::OnTick || !param_has_constraints(param) {
                continue;
            }
            let helper = rust_param_constraint_helper_name(instance, param);
            let ty = rust_param_type(param.ty);
            let checks = rust_param_constraint_checks(param);
            output.push_str(&format!(
                "fn {helper}(value: &{ty}) -> bool {{\n    {checks}\n}}\n\n"
            ));
        }
    }
    output
}

pub(super) fn rust_apply_pending_params(
    instance: &InstanceIr,
    component: &ComponentIr,
    nested: bool,
    context_name: &str,
) -> String {
    let mut output = String::new();
    let indent = crate::runtime_plan::rust_step_indent(nested);
    let inner_indent = crate::runtime_plan::rust_nested_step_indent(nested);
    let deep_indent = if nested {
        "                    "
    } else {
        "                "
    };
    for param in &component.params {
        if param.update != ParamUpdatePolicy::OnTick {
            continue;
        }
        let runtime_name = runtime_param_name(instance, param);
        let pending = format!("{}_{}_pending", instance.name, param.name);
        let next = format!("{}_{}_next_params", instance.name, param.name);
        output.push_str(&format!(
            "{indent}if let Some({pending}) = introspection_state.take_pending_param({}) {{\n",
            crate::rust_string_literal(&runtime_name)
        ));
        output.push_str(&format!(
            "{inner_indent}let mut {next} = self.{}_params.clone();\n",
            instance.name
        ));
        output.push_str(&format!(
            "{inner_indent}{next}.{field} = match decode_flowrt_param_value::<{}>({pending}.clone()) {{\n{deep_indent}Ok(value) => value,\n{deep_indent}Err(_) => return flowrt::Status::Error,\n{inner_indent}}};\n",
            rust_param_type(param.ty),
            field = param.name
        ));
        if param_has_constraints(param) {
            output.push_str(&format!(
                "{inner_indent}if !{}(&{next}.{field}) {{\n{deep_indent}return flowrt::Status::Error;\n{inner_indent}}}\n",
                rust_param_constraint_helper_name(instance, param),
                field = param.name
            ));
        }
        output.push_str(&format!(
            "{inner_indent}if self.{instance}.on_params_update(&self.{instance}_params, &{next}, {context_name}) != flowrt::Status::Ok {{\n{deep_indent}return flowrt::Status::Error;\n{inner_indent}}}\n",
            instance = instance.name
        ));
        output.push_str(&format!(
            "{inner_indent}self.{instance}_params = {next};\n{inner_indent}introspection_state.record_param_applied({}, {pending});\n",
            crate::rust_string_literal(&runtime_name),
            instance = instance.name
        ));
        output.push_str(&format!("{indent}}}\n"));
    }
    output
}

fn param_has_constraints(param: &ParamIr) -> bool {
    param.min.is_some() || param.max.is_some() || !param.choices.is_empty()
}

fn rust_param_constraint_helper_name(instance: &InstanceIr, param: &ParamIr) -> String {
    format!(
        "flowrt_validate_pending_param_{}_{}",
        crate::snake_identifier(&instance.name),
        crate::snake_identifier(&param.name)
    )
}

fn rust_param_constraint_checks(param: &ParamIr) -> String {
    let mut checks = Vec::new();
    if param_type_supports_range(param.ty)
        && let Some(min) = &param.min
    {
        if let Some(literal) = rust_param_constraint_literal(param, min) {
            checks.push(format!("*value >= {literal}"));
        }
    }
    if param_type_supports_range(param.ty)
        && let Some(max) = &param.max
    {
        if let Some(literal) = rust_param_constraint_literal(param, max) {
            checks.push(format!("*value <= {literal}"));
        }
    }
    let choices = param
        .choices
        .iter()
        .filter_map(|choice| rust_param_constraint_literal(param, choice))
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

fn param_type_supports_range(ty: ParamType) -> bool {
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

fn rust_param_constraint_literal(param: &ParamIr, value: &ParamValue) -> Option<String> {
    match (param.ty, value) {
        (ParamType::String, ParamValue::String(value)) => Some(crate::rust_string_literal(value)),
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
            | ParamType::F64,
            _,
        ) => Some(rust_param_literal(param, value)),
        (ParamType::Array | ParamType::Table, _) => Some(rust_param_literal(param, value)),
        _ => None,
    }
}

fn rust_optional_param_json_value(value: Option<&ParamValue>) -> String {
    match value {
        Some(value) => format!("Some({})", crate::param_json_value_literal(value)),
        None => "None".to_string(),
    }
}

fn rust_param_json_vec(values: &[ParamValue]) -> String {
    if values.is_empty() {
        return "Vec::new()".to_string();
    }
    let values = values
        .iter()
        .map(crate::param_json_value_literal)
        .collect::<Vec<_>>()
        .join(", ");
    format!("vec![{values}]")
}

fn rust_param_type(ty: ParamType) -> &'static str {
    match ty {
        ParamType::Bool => "bool",
        ParamType::U8 => "u8",
        ParamType::U16 => "u16",
        ParamType::U32 => "u32",
        ParamType::U64 => "u64",
        ParamType::I8 => "i8",
        ParamType::I16 => "i16",
        ParamType::I32 => "i32",
        ParamType::I64 => "i64",
        ParamType::F32 => "f32",
        ParamType::F64 => "f64",
        ParamType::String => "String",
        ParamType::Array | ParamType::Table => "serde_json::Value",
    }
}

fn rust_param_literal(param: &ParamIr, value: &ParamValue) -> String {
    match (param.ty, value) {
        (ParamType::Bool, ParamValue::Bool(value)) => value.to_string(),
        (
            ParamType::U8
            | ParamType::U16
            | ParamType::U32
            | ParamType::U64
            | ParamType::I8
            | ParamType::I16
            | ParamType::I32
            | ParamType::I64,
            ParamValue::Integer(value),
        ) => format!("{} as {}", value, rust_param_type(param.ty)),
        (ParamType::F32, ParamValue::Float(value)) => {
            format!("{}f32", crate::float_literal(*value))
        }
        (ParamType::F32, ParamValue::Integer(value)) => format!("{}f32", value),
        (ParamType::F64, ParamValue::Float(value)) => {
            format!("{}f64", crate::float_literal(*value))
        }
        (ParamType::F64, ParamValue::Integer(value)) => format!("{}f64", value),
        (ParamType::String, ParamValue::String(value)) => {
            format!("{}.to_string()", crate::rust_string_literal(value))
        }
        (ParamType::Array | ParamType::Table, _) => param_json_value_literal(value),
        _ => param_json_value_literal(value),
    }
}

fn param_json_value_literal(value: &ParamValue) -> String {
    format!("serde_json::json!({})", crate::param_json_literal(value))
}
