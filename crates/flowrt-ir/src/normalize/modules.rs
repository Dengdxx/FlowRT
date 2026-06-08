use std::collections::BTreeMap;

use flowrt_rsdl::{RawDocument, RawModuleDocument, RawOperationPort, RawPort, RawServicePort};

use crate::{
    ComponentIr, ComponentKind, EntityId, FieldIr, IrError, LanguageKind, LifecycleSurface,
    ModuleIr, OperationPortIr, PortIr, Result, ServicePortIr, TypeIr, parse_type_expr,
};

use super::ids::entity_id;
use super::resolver::NameResolver;

pub(super) fn normalize_modules(raw_modules: &[RawModuleDocument]) -> Vec<ModuleIr> {
    let mut modules = raw_modules
        .iter()
        .map(|module| ModuleIr {
            name: module.module.name.clone(),
            source: module.source.to_string_lossy().replace('\\', "/"),
        })
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| left.name.cmp(&right.name));
    modules
}

pub(super) fn normalize_types(
    document: &RawDocument,
    raw_modules: &[RawModuleDocument],
    resolver: &NameResolver,
) -> Result<Vec<TypeIr>> {
    let mut raw_types = Vec::new();
    for module in raw_modules {
        for (name, raw) in &module.types {
            raw_types.push((format!("{}::{}", module.module.name, name), name, raw));
        }
    }
    for (name, raw) in &document.types {
        raw_types.push((name.clone(), name, raw));
    }

    raw_types
        .into_iter()
        .map(|(decl_name, _name, raw)| {
            let symbol = resolver.type_info_for_decl(&decl_name);
            let current_module = symbol.module.as_deref();
            Ok(TypeIr {
                id: entity_id("type", &symbol.qualified_name),
                module: symbol.module.clone(),
                name: symbol.name,
                qualified_name: symbol.qualified_name,
                generated_name: symbol.generated_name,
                fields: raw
                    .fields
                    .iter()
                    .map(|field| {
                        Ok(FieldIr {
                            name: field.name.clone(),
                            ty: resolver.resolve_type_expr_in_module(
                                parse_type_expr(&field.ty)?,
                                current_module,
                            )?,
                            default: None,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|mut types| {
            types.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
            types
        })
}

pub(super) fn normalize_components(
    document: &RawDocument,
    raw_modules: &[RawModuleDocument],
    resolver: &NameResolver,
    _type_ids: &BTreeMap<String, EntityId>,
) -> Result<Vec<ComponentIr>> {
    let mut raw_components = Vec::new();
    for module in raw_modules {
        for (name, raw) in &module.components {
            raw_components.push((format!("{}::{}", module.module.name, name), name, raw));
        }
    }
    for (name, raw) in &document.components {
        raw_components.push((name.clone(), name, raw));
    }

    raw_components
        .into_iter()
        .map(|(decl_name, name, raw)| {
            let symbol = resolver.component_info_for_decl(&decl_name);
            let current_module = symbol.module.as_deref();
            Ok(ComponentIr {
                id: entity_id("component", &symbol.qualified_name),
                module: symbol.module.clone(),
                name: symbol.name,
                qualified_name: symbol.qualified_name.clone(),
                generated_name: symbol.generated_name,
                language: parse_language(&format!("component.{name}.language"), &raw.language)?,
                kind: match raw.kind.as_deref() {
                    Some(kind) => parse_component_kind(&format!("component.{name}.kind"), kind)?,
                    None => ComponentKind::Native,
                },
                inputs: normalize_ports(&raw.input, resolver, current_module)?,
                outputs: normalize_ports(&raw.output, resolver, current_module)?,
                service_clients: normalize_service_ports(
                    &raw.service_clients,
                    resolver,
                    current_module,
                )?,
                service_servers: normalize_service_ports(
                    &raw.service_servers,
                    resolver,
                    current_module,
                )?,
                operation_clients: normalize_operation_ports(
                    &raw.operation_clients,
                    resolver,
                    current_module,
                )?,
                operation_servers: normalize_operation_ports(
                    &raw.operation_servers,
                    resolver,
                    current_module,
                )?,
                params: super::params::normalize_component_params(name, raw)?,
                lifecycle: LifecycleSurface::reserved_v0_1(),
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|mut components| {
            components.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
            components
        })
}

pub(super) fn normalize_ports(
    ports: &[RawPort],
    resolver: &NameResolver,
    current_module: Option<&str>,
) -> Result<Vec<PortIr>> {
    ports
        .iter()
        .map(|port| {
            Ok(PortIr {
                name: port.name.clone(),
                ty: resolver
                    .resolve_type_expr_in_module(parse_type_expr(&port.ty)?, current_module)?,
            })
        })
        .collect()
}

pub(super) fn normalize_service_ports(
    ports: &[RawServicePort],
    resolver: &NameResolver,
    current_module: Option<&str>,
) -> Result<Vec<ServicePortIr>> {
    ports
        .iter()
        .map(|port| {
            Ok(ServicePortIr {
                name: port.name.clone(),
                request: resolver
                    .resolve_type_expr_in_module(parse_type_expr(&port.request)?, current_module)?,
                response: resolver.resolve_type_expr_in_module(
                    parse_type_expr(&port.response)?,
                    current_module,
                )?,
            })
        })
        .collect()
}

pub(super) fn normalize_operation_ports(
    ports: &[RawOperationPort],
    resolver: &NameResolver,
    current_module: Option<&str>,
) -> Result<Vec<OperationPortIr>> {
    ports
        .iter()
        .map(|port| {
            Ok(OperationPortIr {
                name: port.name.clone(),
                goal: resolver
                    .resolve_type_expr_in_module(parse_type_expr(&port.goal)?, current_module)?,
                feedback: resolver.resolve_type_expr_in_module(
                    parse_type_expr(&port.feedback)?,
                    current_module,
                )?,
                result: resolver
                    .resolve_type_expr_in_module(parse_type_expr(&port.result)?, current_module)?,
            })
        })
        .collect()
}

pub(super) fn parse_language(context: &str, value: &str) -> Result<LanguageKind> {
    match value {
        "cpp" => Ok(LanguageKind::Cpp),
        "rust" => Ok(LanguageKind::Rust),
        "external" => Ok(LanguageKind::External),
        _ => Err(invalid_enum(context, "language", value)),
    }
}

pub(super) fn parse_component_kind(context: &str, value: &str) -> Result<ComponentKind> {
    match value {
        "native" => Ok(ComponentKind::Native),
        "adapter" => Ok(ComponentKind::Adapter),
        "external" => Ok(ComponentKind::External),
        _ => Err(invalid_enum(context, "component kind", value)),
    }
}

pub(super) fn parse_trigger(context: &str, value: &str) -> Result<crate::TriggerKind> {
    match value {
        "periodic" => Ok(crate::TriggerKind::Periodic),
        "on_message" => Ok(crate::TriggerKind::OnMessage),
        "startup" => Ok(crate::TriggerKind::Startup),
        "shutdown" => Ok(crate::TriggerKind::Shutdown),
        _ => Err(invalid_enum(context, "trigger", value)),
    }
}

pub(super) fn parse_readiness(context: &str, value: Option<&str>) -> Result<crate::TaskReadiness> {
    match value.unwrap_or("any_ready") {
        "any_ready" => Ok(crate::TaskReadiness::AnyReady),
        "all_ready" => Ok(crate::TaskReadiness::AllReady),
        other => Err(invalid_enum(context, "readiness", other)),
    }
}

fn invalid_enum(context: &str, kind: &'static str, value: &str) -> IrError {
    IrError::InvalidEnum {
        context: context.to_string(),
        kind,
        value: value.to_string(),
    }
}
