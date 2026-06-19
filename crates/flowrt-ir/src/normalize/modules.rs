use std::collections::BTreeMap;

use flowrt_rsdl::{
    RawComponent, RawDocument, RawModuleDocument, RawOperationPort, RawPort, RawResourceDescriptor,
    RawResourceRequirement, RawServicePort, RawTimestampSource,
};

use crate::{
    CapabilityAtom, ComponentBuildIr, ComponentIr, ComponentKind, DescriptorPayloadCapture,
    EntityId, FieldIr, IoBoundaryHealth, IoBoundaryIr, IoBoundaryReadiness, IoBoundaryShutdown,
    IoSideEffect, IrError, LanguageKind, LifecycleSurface, ModuleIr, OperationPortIr, PortIr,
    ResourceAccess, ResourceDescriptorKind, ResourceDescriptorSchemaIr, ResourceFailurePolicy,
    ResourceHealthPolicy, ResourceReadinessGate, ResourceRequirementIr, Result, ServicePortIr,
    TaskConcurrency, TimestampEpoch, TimestampSourceIr, TimestampUnit, TypeIr, parse_type_expr,
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
                empty: raw.empty,
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
                timestamp: raw
                    .timestamp
                    .as_ref()
                    .map(normalize_timestamp_source)
                    .transpose()?,
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|mut types| {
            types.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
            types
        })
}

/// 归一化 `[type.<Name>.timestamp]`：unit/epoch 字符串映射为枚举（未知值拒绝），
/// 缺省 unit=ns、epoch=monotonic、clock_domain=sensor。
fn normalize_timestamp_source(raw: &RawTimestampSource) -> Result<TimestampSourceIr> {
    let context = || format!("type.timestamp field `{}`", raw.field);
    let unit = match raw.unit.as_deref() {
        None | Some("ns") => TimestampUnit::Ns,
        Some("us") => TimestampUnit::Us,
        Some("ms") => TimestampUnit::Ms,
        Some(value) => {
            return Err(IrError::InvalidEnum {
                context: context(),
                kind: "timestamp unit",
                value: value.to_string(),
            });
        }
    };
    let epoch = match raw.epoch.as_deref() {
        None | Some("monotonic") => TimestampEpoch::Monotonic,
        Some("unix") => TimestampEpoch::Unix,
        Some(value) => {
            return Err(IrError::InvalidEnum {
                context: context(),
                kind: "timestamp epoch",
                value: value.to_string(),
            });
        }
    };
    let clock_domain = raw
        .clock_domain
        .clone()
        .unwrap_or_else(|| "sensor".to_string());
    Ok(TimestampSourceIr {
        field: raw.field.clone(),
        unit,
        epoch,
        clock_domain,
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
            let declared_concurrency = parse_declared_concurrency(
                &format!("component.{name}.concurrency"),
                "component concurrency",
                raw.concurrency.as_deref(),
            )?;
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
                concurrency: declared_concurrency.unwrap_or(TaskConcurrency::Exclusive),
                declared_concurrency,
                build: normalize_component_build(raw),
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
                resources: normalize_resources(&symbol.qualified_name, &raw.resources)?,
                io_boundary: normalize_io_boundary(raw)?,
                lifecycle: LifecycleSurface::reserved_v0_1(),
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|mut components| {
            components.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
            components
        })
}

fn normalize_component_build(raw: &RawComponent) -> ComponentBuildIr {
    let mut pkg_config = raw.build.pkg_config.clone();
    pkg_config.sort();
    ComponentBuildIr { pkg_config }
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
        "c" => Ok(LanguageKind::C),
        "cpp" => Ok(LanguageKind::Cpp),
        "rust" => Ok(LanguageKind::Rust),
        "external" => Ok(LanguageKind::External),
        _ => Err(invalid_enum(context, "language", value)),
    }
}

pub(super) fn parse_component_kind(context: &str, value: &str) -> Result<ComponentKind> {
    match value {
        "native" => Ok(ComponentKind::Native),
        "io_boundary" => Ok(ComponentKind::IoBoundary),
        "external" => Ok(ComponentKind::External),
        _ => Err(invalid_enum(context, "component kind", value)),
    }
}

fn normalize_resources(
    component_qualified_name: &str,
    raw: &[RawResourceRequirement],
) -> Result<Vec<ResourceRequirementIr>> {
    let mut resources = raw
        .iter()
        .map(|resource| {
            Ok(ResourceRequirementIr {
                id: entity_id(
                    "resource_requirement",
                    &format!("{component_qualified_name}.{}", resource.name),
                ),
                name: resource.name.clone(),
                capability: normalize_capability_atom(
                    &format!("component.resource.{}.capability", resource.name),
                    &resource.capability,
                )?,
                access: parse_resource_access(
                    &format!("component.resource.{}.access", resource.name),
                    resource.access.as_deref(),
                )?,
                required: resource.required,
                readiness: parse_resource_readiness(
                    &format!("component.resource.{}.readiness", resource.name),
                    resource.readiness.as_deref(),
                )?,
                health: parse_resource_health(
                    &format!("component.resource.{}.health", resource.name),
                    resource.health.as_deref(),
                )?,
                on_failure: parse_resource_failure(
                    &format!("component.resource.{}.on_failure", resource.name),
                    resource.on_failure.as_deref(),
                )?,
                descriptor: normalize_resource_descriptor(
                    &format!("component.resource.{}.descriptor", resource.name),
                    resource.descriptor.as_ref(),
                )?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    resources.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(resources)
}

fn normalize_resource_descriptor(
    context: &str,
    raw: Option<&RawResourceDescriptor>,
) -> Result<Option<ResourceDescriptorSchemaIr>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let port = raw
        .port
        .as_deref()
        .map(str::trim)
        .filter(|port| !port.is_empty())
        .ok_or_else(|| IrError::InvalidValue {
            context: format!("{context}.port"),
            message: "frame descriptor schema must bind to an output port".to_string(),
        })?
        .to_string();
    Ok(Some(ResourceDescriptorSchemaIr {
        kind: parse_resource_descriptor_kind(&format!("{context}.kind"), &raw.kind)?,
        port,
        format: raw.format.clone(),
        encoding: raw.encoding.clone(),
        metadata: raw.metadata.clone(),
        record_payload: raw.record_payload,
        payload_capture: raw
            .payload_capture
            .as_deref()
            .map(|value| {
                parse_descriptor_payload_capture(&format!("{context}.payload_capture"), value)
            })
            .transpose()?
            .unwrap_or_default(),
    }))
}

fn normalize_io_boundary(raw: &RawComponent) -> Result<Option<IoBoundaryIr>> {
    if raw.kind.as_deref() != Some("io_boundary") {
        if !raw.io_side_effect.is_empty()
            || raw.io_readiness.is_some()
            || raw.io_health.is_some()
            || raw.io_shutdown.is_some()
        {
            return Err(IrError::InvalidValue {
                context: "component.io_boundary".to_string(),
                message: "I/O boundary policy fields require `kind = \"io_boundary\"`".to_string(),
            });
        }
        return Ok(None);
    }
    let mut side_effects = raw
        .io_side_effect
        .iter()
        .map(|effect| parse_io_side_effect("component.io_side_effect", effect))
        .collect::<Result<Vec<_>>>()?;
    side_effects.sort();
    side_effects.dedup();
    Ok(Some(IoBoundaryIr {
        side_effects,
        readiness: parse_io_readiness(
            "component.io_readiness",
            raw.io_readiness.as_deref().unwrap_or("resource_ready"),
        )?,
        health: parse_io_health(
            "component.io_health",
            raw.io_health.as_deref().unwrap_or("runtime_reported"),
        )?,
        shutdown: parse_io_shutdown(
            "component.io_shutdown",
            raw.io_shutdown.as_deref().unwrap_or("cooperative"),
        )?,
    }))
}

pub(super) fn normalize_capability_atom(context: &str, value: &str) -> Result<CapabilityAtom> {
    let value = value.trim();
    if value.is_empty() {
        return Err(IrError::InvalidValue {
            context: context.to_string(),
            message: "resource capability must not be empty".to_string(),
        });
    }
    Ok(CapabilityAtom(value.to_string()))
}

fn parse_resource_access(context: &str, value: Option<&str>) -> Result<ResourceAccess> {
    match value.unwrap_or("read_write") {
        "read" => Ok(ResourceAccess::Read),
        "write" => Ok(ResourceAccess::Write),
        "read_write" => Ok(ResourceAccess::ReadWrite),
        "exclusive" => Ok(ResourceAccess::Exclusive),
        value => Err(invalid_enum(context, "resource access", value)),
    }
}

fn parse_resource_readiness(context: &str, value: Option<&str>) -> Result<ResourceReadinessGate> {
    match value.unwrap_or("before_start") {
        "before_init" => Ok(ResourceReadinessGate::BeforeInit),
        "before_start" => Ok(ResourceReadinessGate::BeforeStart),
        "lazy" => Ok(ResourceReadinessGate::Lazy),
        value => Err(invalid_enum(context, "resource readiness", value)),
    }
}

fn parse_resource_health(context: &str, value: Option<&str>) -> Result<ResourceHealthPolicy> {
    match value.unwrap_or("required") {
        "required" => Ok(ResourceHealthPolicy::Required),
        "optional" => Ok(ResourceHealthPolicy::Optional),
        "ignored" => Ok(ResourceHealthPolicy::Ignored),
        value => Err(invalid_enum(context, "resource health", value)),
    }
}

fn parse_resource_failure(context: &str, value: Option<&str>) -> Result<ResourceFailurePolicy> {
    match value.unwrap_or("stop_process") {
        "stop_process" => Ok(ResourceFailurePolicy::StopProcess),
        "restart_process" => Ok(ResourceFailurePolicy::RestartProcess),
        "degrade" => Ok(ResourceFailurePolicy::Degrade),
        "stop_graph" => Ok(ResourceFailurePolicy::StopGraph),
        value => Err(invalid_enum(context, "resource failure policy", value)),
    }
}

fn parse_resource_descriptor_kind(context: &str, value: &str) -> Result<ResourceDescriptorKind> {
    match value {
        "frame" => Ok(ResourceDescriptorKind::Frame),
        _ => Err(invalid_enum(context, "resource descriptor kind", value)),
    }
}

fn parse_descriptor_payload_capture(
    context: &str,
    value: &str,
) -> Result<DescriptorPayloadCapture> {
    match value {
        "none" => Ok(DescriptorPayloadCapture::None),
        "boundary" => Ok(DescriptorPayloadCapture::Boundary),
        "external" => Ok(DescriptorPayloadCapture::External),
        _ => Err(invalid_enum(context, "descriptor payload capture", value)),
    }
}

fn parse_io_side_effect(context: &str, value: &str) -> Result<IoSideEffect> {
    match value {
        "read" => Ok(IoSideEffect::Read),
        "write" => Ok(IoSideEffect::Write),
        "network" => Ok(IoSideEffect::Network),
        "filesystem" => Ok(IoSideEffect::Filesystem),
        "device" => Ok(IoSideEffect::Device),
        "compute" => Ok(IoSideEffect::Compute),
        _ => Err(invalid_enum(context, "I/O side effect", value)),
    }
}

fn parse_io_readiness(context: &str, value: &str) -> Result<IoBoundaryReadiness> {
    match value {
        "component_started" => Ok(IoBoundaryReadiness::ComponentStarted),
        "resource_ready" => Ok(IoBoundaryReadiness::ResourceReady),
        _ => Err(invalid_enum(context, "I/O readiness", value)),
    }
}

fn parse_io_health(context: &str, value: &str) -> Result<IoBoundaryHealth> {
    match value {
        "runtime_reported" => Ok(IoBoundaryHealth::RuntimeReported),
        "process_status" => Ok(IoBoundaryHealth::ProcessStatus),
        _ => Err(invalid_enum(context, "I/O health", value)),
    }
}

fn parse_io_shutdown(context: &str, value: &str) -> Result<IoBoundaryShutdown> {
    match value {
        "cooperative" => Ok(IoBoundaryShutdown::Cooperative),
        "best_effort" => Ok(IoBoundaryShutdown::BestEffort),
        _ => Err(invalid_enum(context, "I/O shutdown", value)),
    }
}

pub(super) fn parse_trigger(context: &str, value: &str) -> Result<crate::TriggerKind> {
    match value {
        "periodic" => Ok(crate::TriggerKind::Periodic),
        "on_message" => Ok(crate::TriggerKind::OnMessage),
        "startup" => Ok(crate::TriggerKind::Startup),
        "shutdown" => Ok(crate::TriggerKind::Shutdown),
        "on_synchronized" => Ok(crate::TriggerKind::OnSynchronized),
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

pub(super) fn parse_declared_concurrency(
    context: &str,
    kind: &'static str,
    value: Option<&str>,
) -> Result<Option<crate::TaskConcurrency>> {
    value
        .map(|value| parse_concurrency(context, kind, value))
        .transpose()
}

fn parse_concurrency(
    context: &str,
    kind: &'static str,
    value: &str,
) -> Result<crate::TaskConcurrency> {
    match value {
        "exclusive" => Ok(crate::TaskConcurrency::Exclusive),
        "parallel" => Ok(crate::TaskConcurrency::Parallel),
        _ => Err(invalid_enum(context, kind, value)),
    }
}

fn invalid_enum(context: &str, kind: &'static str, value: &str) -> IrError {
    IrError::InvalidEnum {
        context: context.to_string(),
        kind,
        value: value.to_string(),
    }
}
