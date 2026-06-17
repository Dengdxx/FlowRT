use std::collections::BTreeMap;

use flowrt_rsdl::{
    RawBoundaryEndpoint, RawDocument, RawExternalProcess, RawModuleDocument, RawProcess,
    RawResourceProvider,
};

use crate::{
    BackendName, BackendThreadAffinity, BoundaryDirection, BoundaryEndpointIr, CapabilityAtom,
    ChannelEdgeIr, ChannelKind, ChannelPolicySourceIr, ComponentIr, DeploymentIr, EntityId,
    EntityRef, ExternalHealthKind, ExternalProcessIr, ExternalWorkingDir, GraphIr, InstanceIr,
    IrError, OverflowPolicy, PolicyValueSource, PortRef, ProcessFailurePropagation, ProcessIr,
    ProcessReadinessGate, ProcessRestartPolicy, ProcessRestartPolicyKind, ProfileIr,
    ResourceProviderIr, ResourceProviderScope, ResourceSatisfactionIr, Result, RtPolicy,
    StalePolicy, SyncGroupIr, SyncLatePolicy, TargetIr, TaskConcurrency, TaskIr, TypeIr,
    channel_capabilities, channel_route_capabilities, deployment_capability_decision,
    graph_required_capabilities, parse_type_expr,
};

use super::backends::{resolve_channel_backend, route_topology, source_port_types_by_endpoint};
use super::ids::entity_id;
use super::modules::{
    normalize_capability_atom, parse_declared_concurrency, parse_readiness, parse_trigger,
};
use super::params::merge_instance_params;
use super::profiles::{parse_overflow_policy, parse_stale_policy};
use super::resolver::NameResolver;

pub(super) fn normalize_instances(
    document: &RawDocument,
    raw_modules: &[RawModuleDocument],
    resolver: &NameResolver,
    component_ids: &BTreeMap<String, EntityId>,
    component_concurrency: &BTreeMap<String, TaskConcurrency>,
    target_ids: &BTreeMap<String, EntityId>,
    graph_name: &str,
) -> Result<(Vec<InstanceIr>, Vec<TaskIr>)> {
    let mut instances = Vec::with_capacity(document.instances.len());
    let mut tasks = Vec::new();
    let mut component_param_schemas = BTreeMap::new();
    for module in raw_modules {
        for (name, component) in &module.components {
            let decl_name = format!("{}::{}", module.module.name, name);
            let symbol = resolver.component_info_for_decl(&decl_name);
            let params = super::params::normalize_component_params(name, component)?
                .into_iter()
                .map(|param| (param.name.clone(), param))
                .collect::<BTreeMap<_, _>>();
            component_param_schemas.insert(symbol.qualified_name, params);
        }
    }
    for (name, component) in &document.components {
        let symbol = resolver.component_info_for_decl(name);
        let params = super::params::normalize_component_params(name, component)?
            .into_iter()
            .map(|param| (param.name.clone(), param))
            .collect::<BTreeMap<_, _>>();
        component_param_schemas.insert(symbol.qualified_name, params);
    }

    for (name, raw) in &document.instances {
        let component_symbol = resolver.resolve_component(&raw.component)?;
        let component_id = component_ids
            .get(&component_symbol.qualified_name)
            .cloned()
            .ok_or_else(|| IrError::UnknownComponent {
                instance: name.clone(),
                component: raw.component.clone(),
            })?;
        let component_ref = EntityRef {
            id: component_id,
            name: component_symbol.qualified_name.clone(),
        };
        let resolved_component_concurrency = *component_concurrency
            .get(component_symbol.qualified_name.as_str())
            .expect("component concurrency must be derived from normalized components");
        let component = component_param_schemas
            .get(component_symbol.qualified_name.as_str())
            .expect("component IDs and normalized components must be built from the same document");
        let params = merge_instance_params(name, raw, component)?;
        let target = raw
            .target
            .as_ref()
            .map(|target_name| {
                target_ids
                    .get(target_name)
                    .cloned()
                    .map(|id| EntityRef {
                        id,
                        name: target_name.clone(),
                    })
                    .ok_or_else(|| IrError::UnknownTarget {
                        instance: name.clone(),
                        target: target_name.clone(),
                    })
            })
            .transpose()?;

        let instance_id = entity_id("instance", &format!("{graph_name}.{name}"));
        let instance_ref = EntityRef {
            id: instance_id.clone(),
            name: name.clone(),
        };
        for (task_index, raw_task) in raw.tasks.iter().enumerate() {
            let task_name = raw_task
                .name
                .clone()
                .unwrap_or_else(|| default_task_name(task_index));
            let declared_concurrency = parse_declared_concurrency(
                &format!("instance.{name}.task.concurrency"),
                "task concurrency",
                raw_task.concurrency.as_deref(),
            )?;
            tasks.push(TaskIr {
                id: entity_id("task", &format!("{graph_name}.{name}.{task_name}")),
                name: task_name,
                instance: instance_ref.clone(),
                trigger: parse_trigger(
                    &format!("instance.{name}.task.trigger"),
                    &raw_task.trigger,
                )?,
                concurrency: declared_concurrency.unwrap_or(resolved_component_concurrency),
                declared_concurrency,
                readiness: parse_readiness(
                    &format!("instance.{name}.task.readiness"),
                    raw_task.readiness.as_deref(),
                )?,
                period_ms: raw_task.period_ms,
                deadline_ms: raw_task.deadline_ms,
                lane: raw_task.lane.clone(),
                priority: raw_task.priority,
                inputs: raw_task.input.clone(),
                outputs: raw_task.output.clone(),
                sync_group: raw_task.sync.as_ref().map(|group| EntityRef {
                    id: entity_id("sync", &format!("{graph_name}.{group}")),
                    name: group.clone(),
                }),
            });
        }

        instances.push(InstanceIr {
            id: instance_id,
            name: name.clone(),
            component: component_ref,
            params,
            process: raw.process.clone(),
            target,
        });
    }

    Ok((instances, tasks))
}

fn default_task_name(index: usize) -> String {
    if index == 0 {
        "main".to_string()
    } else {
        format!("task_{index}")
    }
}

pub(super) fn normalize_processes(
    document: &RawDocument,
    instances: &[InstanceIr],
) -> Result<Vec<ProcessIr>> {
    let used_processes = instances
        .iter()
        .map(|instance| {
            instance
                .process
                .clone()
                .unwrap_or_else(|| "main".to_string())
        })
        .collect::<std::collections::BTreeSet<_>>();

    let mut declared = BTreeMap::<String, &RawProcess>::new();
    for raw in &document.processes {
        if declared.insert(raw.name.clone(), raw).is_some() {
            return Err(IrError::InvalidValue {
                context: format!("process.{}", raw.name),
                message: "process orchestration is declared more than once".to_string(),
            });
        }
        if !used_processes.contains(&raw.name) {
            return Err(IrError::InvalidValue {
                context: format!("process.{}", raw.name),
                message: "process is not used by any instance".to_string(),
            });
        }
    }

    let mut processes = Vec::with_capacity(used_processes.len());
    for name in used_processes {
        let raw = declared.get(&name).copied();
        let mut seen_dependencies = std::collections::BTreeSet::new();
        let mut depends_on = raw
            .map(|raw| raw.depends_on.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|dependency| {
                if !seen_dependencies.insert(dependency.clone()) {
                    return Err(IrError::InvalidValue {
                        context: format!("process.{name}.depends_on"),
                        message: format!("duplicate dependency `{dependency}`"),
                    });
                }
                if dependency == name {
                    return Err(IrError::InvalidValue {
                        context: format!("process.{name}.depends_on"),
                        message: "process must not depend on itself".to_string(),
                    });
                }
                if !instances.iter().any(|instance| {
                    instance.process.as_deref().unwrap_or("main") == dependency.as_str()
                }) {
                    return Err(IrError::InvalidValue {
                        context: format!("process.{name}.depends_on"),
                        message: format!("unknown process `{dependency}`"),
                    });
                }
                Ok(dependency)
            })
            .collect::<Result<Vec<_>>>()?;
        depends_on.sort();

        processes.push(ProcessIr {
            name: name.clone(),
            depends_on,
            restart: normalize_process_restart(&name, raw)?,
            failure_propagation: normalize_failure_propagation(&name, raw)?,
            readiness: normalize_process_readiness(&name, raw)?,
            startup_delay_ms: raw.and_then(|r| r.startup_delay_ms).unwrap_or(0),
            env: raw.map(|r| r.env.clone()).unwrap_or_default(),
            cpu_affinity: raw.map(|r| r.cpu_affinity.clone()).unwrap_or_default(),
            nice: raw.and_then(|r| r.nice),
            rt_policy: raw
                .map(|r| normalize_rt_policy(&name, r.rt_policy.as_deref()))
                .transpose()?
                .flatten(),
            rt_priority: raw.and_then(|r| r.rt_priority),
        });
    }
    Ok(processes)
}

pub(super) fn normalize_external_processes(
    document: &RawDocument,
) -> Result<Vec<ExternalProcessIr>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut processes = document
        .external_processes
        .iter()
        .map(|raw| normalize_external_process(raw, &mut seen))
        .collect::<Result<Vec<_>>>()?;
    processes.sort_by(|left, right| left.process.cmp(&right.process));
    Ok(processes)
}

pub(super) fn normalize_resource_providers(
    document: &RawDocument,
    graph_name: &str,
    target_ids: &BTreeMap<String, EntityId>,
    processes: &[ProcessIr],
    external_processes: &[ExternalProcessIr],
) -> Result<Vec<ResourceProviderIr>> {
    let process_names = processes
        .iter()
        .map(|process| process.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let external_packages = external_processes
        .iter()
        .map(|process| process.package.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    let mut seen = std::collections::BTreeSet::new();
    let mut providers = document
        .resource_providers
        .iter()
        .map(|raw| {
            if !seen.insert(raw.name.clone()) {
                return Err(IrError::InvalidValue {
                    context: format!("resource.provider.{}", raw.name),
                    message: "resource provider is declared more than once".to_string(),
                });
            }
            normalize_resource_provider(
                raw,
                graph_name,
                target_ids,
                &process_names,
                &external_packages,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    providers.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(providers)
}

fn normalize_resource_provider(
    raw: &RawResourceProvider,
    graph_name: &str,
    target_ids: &BTreeMap<String, EntityId>,
    process_names: &std::collections::BTreeSet<&str>,
    external_packages: &std::collections::BTreeSet<&str>,
) -> Result<ResourceProviderIr> {
    let mut capabilities = raw
        .capabilities
        .iter()
        .map(|capability| {
            normalize_capability_atom(
                &format!("resource.provider.{}.capabilities", raw.name),
                capability,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    capabilities.sort();
    capabilities.dedup();

    let target = raw
        .target
        .as_ref()
        .map(|target_name| {
            target_ids
                .get(target_name)
                .cloned()
                .map(|id| EntityRef {
                    id,
                    name: target_name.clone(),
                })
                .ok_or_else(|| IrError::InvalidValue {
                    context: format!("resource.provider.{}.target", raw.name),
                    message: format!("unknown target `{target_name}`"),
                })
        })
        .transpose()?;
    if let Some(process) = &raw.process
        && !process_names.contains(process.as_str())
    {
        return Err(IrError::InvalidValue {
            context: format!("resource.provider.{}.process", raw.name),
            message: format!("unknown process `{process}`"),
        });
    }
    if let Some(package) = &raw.external_package
        && !external_packages.contains(package.as_str())
    {
        return Err(IrError::InvalidValue {
            context: format!("resource.provider.{}.external_package", raw.name),
            message: format!("unknown external package `{package}`"),
        });
    }

    Ok(ResourceProviderIr {
        id: entity_id("resource_provider", &format!("{graph_name}.{}", raw.name)),
        name: raw.name.clone(),
        capabilities,
        scope: parse_resource_provider_scope(
            &format!("resource.provider.{}.scope", raw.name),
            &raw.scope,
        )?,
        target,
        process: raw.process.clone(),
        external_package: raw.external_package.clone(),
        health_source: raw.health_source.clone(),
        readiness_source: raw.readiness_source.clone(),
    })
}

fn parse_resource_provider_scope(context: &str, value: &str) -> Result<ResourceProviderScope> {
    match value {
        "target" => Ok(ResourceProviderScope::Target),
        "process" => Ok(ResourceProviderScope::Process),
        "external_package" => Ok(ResourceProviderScope::ExternalPackage),
        value => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "resource provider scope",
            value: value.to_string(),
        }),
    }
}

pub fn derive_resource_satisfactions(
    graph_name: &str,
    instances: &[InstanceIr],
    components: &[ComponentIr],
    providers: &[ResourceProviderIr],
) -> Vec<ResourceSatisfactionIr> {
    let components_by_name = components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();
    let mut satisfactions = Vec::new();

    for instance in instances {
        let Some(component) = components_by_name
            .get(instance.component.name.as_str())
            .copied()
        else {
            continue;
        };
        for requirement in &component.resources {
            let provider = providers.iter().find(|provider| {
                provider_satisfies_instance_requirement(provider, instance, &requirement.capability)
            });
            let provider_ref = provider.map(|provider| EntityRef {
                id: provider.id.clone(),
                name: provider.name.clone(),
            });
            let satisfied = provider_ref.is_some();
            let requirement_name = format!("{}.{}", component.qualified_name, requirement.name);
            satisfactions.push(ResourceSatisfactionIr {
                id: entity_id(
                    "resource_satisfaction",
                    &format!("{graph_name}.{}.{}", instance.name, requirement.name),
                ),
                instance: EntityRef {
                    id: instance.id.clone(),
                    name: instance.name.clone(),
                },
                component: EntityRef {
                    id: component.id.clone(),
                    name: component.qualified_name.clone(),
                },
                requirement: EntityRef {
                    id: requirement.id.clone(),
                    name: requirement_name,
                },
                resource: requirement.name.clone(),
                capability: requirement.capability.clone(),
                access: requirement.access,
                required: requirement.required,
                readiness: requirement.readiness,
                health: requirement.health,
                on_failure: requirement.on_failure,
                satisfied,
                provider: provider_ref,
                diagnostic: if satisfied {
                    None
                } else {
                    Some(format!(
                        "{} resource requirement `{}` capability `{}` has no provider",
                        if requirement.required {
                            "required"
                        } else {
                            "optional"
                        },
                        requirement.name,
                        requirement.capability.0
                    ))
                },
            });
        }
    }
    satisfactions.sort_by(|left, right| {
        (&left.instance.name, &left.resource).cmp(&(&right.instance.name, &right.resource))
    });
    satisfactions
}

pub fn provider_satisfies_instance_requirement(
    provider: &ResourceProviderIr,
    instance: &InstanceIr,
    capability: &CapabilityAtom,
) -> bool {
    provider
        .capabilities
        .iter()
        .any(|candidate| candidate == capability)
        && match provider.scope {
            ResourceProviderScope::Target => provider.target.as_ref().is_none_or(|target| {
                instance
                    .target
                    .as_ref()
                    .is_some_and(|instance_target| instance_target.name == target.name)
            }),
            ResourceProviderScope::Process => provider.process.as_ref().is_none_or(|process| {
                instance.process.as_deref().unwrap_or("main") == process.as_str()
            }),
            ResourceProviderScope::ExternalPackage => true,
        }
}

fn normalize_external_process(
    raw: &RawExternalProcess,
    seen: &mut std::collections::BTreeSet<String>,
) -> Result<ExternalProcessIr> {
    if !seen.insert(raw.process.clone()) {
        return Err(IrError::InvalidValue {
            context: format!("external_process.{}", raw.process),
            message: "external process is declared more than once".to_string(),
        });
    }
    Ok(ExternalProcessIr {
        process: raw.process.clone(),
        package: raw.package.clone(),
        executable: raw.executable.clone(),
        args: raw.args.clone(),
        working_dir: normalize_external_working_dir(raw)?,
        health: normalize_external_health(raw)?,
        required_backends: {
            let mut backends = raw
                .required_backends
                .iter()
                .cloned()
                .map(BackendName)
                .collect::<Vec<_>>();
            backends.sort();
            backends.dedup();
            backends
        },
    })
}

fn normalize_external_working_dir(raw: &RawExternalProcess) -> Result<ExternalWorkingDir> {
    match raw.working_dir.as_deref().unwrap_or("package") {
        "package" => Ok(ExternalWorkingDir::Package),
        "workspace" => Ok(ExternalWorkingDir::Workspace),
        value => Err(IrError::InvalidEnum {
            context: format!("external_process.{}.working_dir", raw.process),
            kind: "external process working directory",
            value: value.to_string(),
        }),
    }
}

fn normalize_external_health(raw: &RawExternalProcess) -> Result<ExternalHealthKind> {
    match raw.health.as_deref().unwrap_or("runtime_socket") {
        "process_started" => Ok(ExternalHealthKind::ProcessStarted),
        "runtime_socket" => Ok(ExternalHealthKind::RuntimeSocket),
        value => Err(IrError::InvalidEnum {
            context: format!("external_process.{}.health", raw.process),
            kind: "external process health",
            value: value.to_string(),
        }),
    }
}

fn normalize_process_restart(
    process_name: &str,
    raw: Option<&RawProcess>,
) -> Result<ProcessRestartPolicy> {
    let policy = match raw.and_then(|raw| raw.restart.as_deref()) {
        Some("never") => ProcessRestartPolicyKind::Never,
        Some("on_failure") | None => ProcessRestartPolicyKind::OnFailure,
        Some("always") => ProcessRestartPolicyKind::Always,
        Some(value) => {
            return Err(IrError::InvalidEnum {
                context: format!("process.{process_name}.restart"),
                kind: "process restart policy",
                value: value.to_string(),
            });
        }
    };
    let max_restarts = if policy == ProcessRestartPolicyKind::Never {
        0
    } else {
        raw.and_then(|raw| raw.max_restarts).unwrap_or(3)
    };
    let initial_delay_ms = raw.and_then(|raw| raw.initial_delay_ms).unwrap_or(100);
    let max_delay_ms = raw.and_then(|raw| raw.max_delay_ms).unwrap_or(1_000);
    if initial_delay_ms == 0 {
        return Err(IrError::InvalidValue {
            context: format!("process.{process_name}.initial_delay_ms"),
            message: "`initial_delay_ms` must be greater than zero".to_string(),
        });
    }
    if max_delay_ms < initial_delay_ms {
        return Err(IrError::InvalidValue {
            context: format!("process.{process_name}.max_delay_ms"),
            message: "`max_delay_ms` must be greater than or equal to initial_delay_ms".to_string(),
        });
    }
    Ok(ProcessRestartPolicy {
        policy,
        max_restarts,
        initial_delay_ms,
        max_delay_ms,
    })
}

fn normalize_failure_propagation(
    process_name: &str,
    raw: Option<&RawProcess>,
) -> Result<ProcessFailurePropagation> {
    match raw.and_then(|raw| raw.failure.as_deref()) {
        Some("propagate") | None => Ok(ProcessFailurePropagation::Propagate),
        Some("isolate") => Ok(ProcessFailurePropagation::Isolate),
        Some(value) => Err(IrError::InvalidEnum {
            context: format!("process.{process_name}.failure"),
            kind: "process failure propagation",
            value: value.to_string(),
        }),
    }
}

fn normalize_process_readiness(
    process_name: &str,
    raw: Option<&RawProcess>,
) -> Result<ProcessReadinessGate> {
    match raw.and_then(|raw| raw.readiness.as_deref()) {
        Some("process_started") | None => Ok(ProcessReadinessGate::ProcessStarted),
        Some("runtime_ready") => Ok(ProcessReadinessGate::RuntimeReady),
        Some("service_ready") => Ok(ProcessReadinessGate::ServiceReady),
        Some(value) => Err(IrError::InvalidEnum {
            context: format!("process.{process_name}.readiness"),
            kind: "process readiness gate",
            value: value.to_string(),
        }),
    }
}

fn normalize_rt_policy(process_name: &str, value: Option<&str>) -> Result<Option<RtPolicy>> {
    match value {
        None => Ok(None),
        Some("fifo") => Ok(Some(RtPolicy::Fifo)),
        Some("round_robin") => Ok(Some(RtPolicy::RoundRobin)),
        Some(other) => Err(IrError::InvalidEnum {
            context: format!("process.{process_name}.rt_policy"),
            kind: "RT scheduling policy",
            value: other.to_string(),
        }),
    }
}

pub(super) fn normalize_binds(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    types: &[TypeIr],
    components: &[ComponentIr],
    instances: &[InstanceIr],
    profiles: &[ProfileIr],
) -> Result<Vec<ChannelEdgeIr>> {
    let default_policy = profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or(profiles.first());
    let default_overflow = default_policy
        .map(|profile| profile.defaults.default_overflow)
        .unwrap_or(OverflowPolicy::DropOldest);
    let default_stale = default_policy
        .map(|profile| profile.defaults.default_stale_policy)
        .unwrap_or(StalePolicy::Warn);
    let default_max_age = default_policy.and_then(|profile| profile.defaults.max_age_ms);
    let default_backend = default_policy
        .map(|profile| profile.backend.0.as_str())
        .unwrap_or("inproc");
    let source_port_types = source_port_types_by_endpoint(components, instances);
    let instances_by_name = instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let components_by_name = components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();

    let mut binds = document
        .binds
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let channel =
                parse_channel_kind(&format!("bind.dataflow[{index}].channel"), &raw.channel)?;
            let depth = raw.depth.or(match channel {
                ChannelKind::Latest => Some(1),
                ChannelKind::Fifo => None,
            });
            let overflow = match raw.overflow.as_deref() {
                Some(value) => {
                    parse_overflow_policy(&format!("bind.dataflow[{index}].overflow"), value)?
                }
                None => default_overflow,
            };
            let stale = match raw.stale_policy.as_deref() {
                Some(value) => {
                    parse_stale_policy(&format!("bind.dataflow[{index}].stale_policy"), value)?
                }
                None => default_stale,
            };
            let from = parse_port_ref(&raw.from, instance_refs)?;
            let to = parse_port_ref(&raw.to, instance_refs)?;
            let source_type =
                source_port_types.get(&(from.instance.name.clone(), from.port.clone()));
            let topology =
                route_topology(&instances_by_name, Some(&components_by_name), &from, &to);
            let backend_seed = match raw.backend.as_deref() {
                Some("auto") | None => default_backend,
                Some(backend) => backend,
            };
            let explicit_backend = raw.backend.is_some() && raw.backend.as_deref() != Some("auto");
            let resolved_backend = resolve_channel_backend(
                backend_seed,
                source_type,
                types,
                topology,
                explicit_backend,
            )?;
            let thread_affinity = BackendThreadAffinity::for_backend(&resolved_backend.backend);

            Ok(ChannelEdgeIr {
                id: entity_id("bind", &format!("{}->{}", raw.from, raw.to)),
                from,
                to,
                backend: BackendName(resolved_backend.backend),
                backend_policy_source: if explicit_backend {
                    PolicyValueSource::Explicit
                } else {
                    PolicyValueSource::ProfileDefault
                },
                backend_source: resolved_backend.source,
                thread_affinity,
                channel,
                depth,
                overflow,
                stale,
                max_age_ms: raw.max_age_ms.or(default_max_age),
                policy_source: ChannelPolicySourceIr {
                    overflow: if raw.overflow.is_some() {
                        PolicyValueSource::Explicit
                    } else {
                        PolicyValueSource::ProfileDefault
                    },
                    stale: if raw.stale_policy.is_some() {
                        PolicyValueSource::Explicit
                    } else {
                        PolicyValueSource::ProfileDefault
                    },
                    max_age_ms: if raw.max_age_ms.is_some() {
                        PolicyValueSource::Explicit
                    } else {
                        PolicyValueSource::ProfileDefault
                    },
                },
                capability_requirements: match source_type {
                    Some(source_type) => channel_route_capabilities(
                        types,
                        source_type,
                        channel,
                        overflow,
                        stale,
                        topology,
                    ),
                    None => channel_capabilities(channel, overflow, stale),
                },
                feedback: raw.feedback,
                init: raw
                    .init
                    .as_ref()
                    .map(super::params::convert_param_value_table),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    binds.sort_by(|left, right| {
        (
            &left.from.instance.name,
            &left.from.port,
            &left.to.instance.name,
            &left.to.port,
        )
            .cmp(&(
                &right.from.instance.name,
                &right.from.port,
                &right.to.instance.name,
                &right.to.port,
            ))
    });
    Ok(binds)
}

pub(super) fn normalize_boundary_endpoints(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    resolver: &NameResolver,
    graph_name: &str,
) -> Result<Vec<BoundaryEndpointIr>> {
    let mut endpoints =
        Vec::with_capacity(document.boundary_inputs.len() + document.boundary_outputs.len());
    endpoints.extend(
        document
            .boundary_inputs
            .iter()
            .map(|raw| {
                normalize_boundary_endpoint(
                    raw,
                    BoundaryDirection::Input,
                    instance_refs,
                    resolver,
                    graph_name,
                )
            })
            .collect::<Result<Vec<_>>>()?,
    );
    endpoints.extend(
        document
            .boundary_outputs
            .iter()
            .map(|raw| {
                normalize_boundary_endpoint(
                    raw,
                    BoundaryDirection::Output,
                    instance_refs,
                    resolver,
                    graph_name,
                )
            })
            .collect::<Result<Vec<_>>>()?,
    );
    endpoints
        .sort_by(|left, right| (left.direction, &left.name).cmp(&(right.direction, &right.name)));
    Ok(endpoints)
}

fn normalize_boundary_endpoint(
    raw: &RawBoundaryEndpoint,
    direction: BoundaryDirection,
    instance_refs: &BTreeMap<String, EntityRef>,
    resolver: &NameResolver,
    graph_name: &str,
) -> Result<BoundaryEndpointIr> {
    let direction_name = match direction {
        BoundaryDirection::Input => "input",
        BoundaryDirection::Output => "output",
    };
    Ok(BoundaryEndpointIr {
        id: entity_id(
            "boundary",
            &format!("{graph_name}.{direction_name}.{}", raw.name),
        ),
        name: raw.name.clone(),
        direction,
        port: parse_port_ref(&raw.port, instance_refs)?,
        ty: resolver.resolve_type_expr_in_module(parse_type_expr(&raw.ty)?, None)?,
    })
}

pub(super) fn normalize_sync_groups(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    graph_name: &str,
) -> Result<Vec<SyncGroupIr>> {
    let mut groups =
        document
            .sync_groups
            .iter()
            .map(|raw| {
                let instance = instance_refs.get(&raw.instance).cloned().ok_or_else(|| {
                    IrError::InvalidValue {
                        context: format!("sync.{}", raw.name),
                        message: format!("unknown instance `{}`", raw.instance),
                    }
                })?;
                Ok(SyncGroupIr {
                    id: entity_id("sync", &format!("{graph_name}.{}", raw.name)),
                    name: raw.name.clone(),
                    instance,
                    inputs: raw.inputs.clone(),
                    tolerance_ms: raw.tolerance_ms.unwrap_or(0),
                    late_policy: SyncLatePolicy::DropLate,
                })
            })
            .collect::<Result<Vec<_>>>()?;
    groups.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(groups)
}

pub(super) fn normalize_deployments(
    graph: &GraphIr,
    types: &[TypeIr],
    components: &[ComponentIr],
    profiles: &[ProfileIr],
    targets: &[TargetIr],
) -> Vec<DeploymentIr> {
    let graph_ref = EntityRef {
        id: graph.id.clone(),
        name: graph.name.clone(),
    };
    let mut deployments = Vec::new();
    let required_capabilities = graph_required_capabilities(graph, types, components);

    for profile in profiles {
        for target in targets {
            deployments.push(DeploymentIr {
                id: entity_id(
                    "deployment",
                    &format!("{}.{}.{}", graph.name, profile.name, target.name),
                ),
                graph: graph_ref.clone(),
                profile: EntityRef {
                    id: profile.id.clone(),
                    name: profile.name.clone(),
                },
                target: EntityRef {
                    id: target.id.clone(),
                    name: target.name.clone(),
                },
                backend: profile.backend.clone(),
                required_capabilities: required_capabilities.clone(),
                satisfied: deployment_capability_decision(
                    &profile.backend,
                    &target.backends,
                    &required_capabilities,
                )
                .satisfied,
            });
        }
    }

    deployments
}

fn parse_channel_kind(context: &str, value: &str) -> Result<ChannelKind> {
    match value {
        "latest" => Ok(ChannelKind::Latest),
        "fifo" => Ok(ChannelKind::Fifo),
        _ => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "channel",
            value: value.to_string(),
        }),
    }
}

fn parse_port_ref(endpoint: &str, instance_refs: &BTreeMap<String, EntityRef>) -> Result<PortRef> {
    let Some((instance_name, port)) = endpoint.split_once('.') else {
        return Err(IrError::InvalidPortEndpoint {
            endpoint: endpoint.to_string(),
        });
    };
    let instance = instance_refs.get(instance_name).cloned().ok_or_else(|| {
        IrError::UnknownEndpointInstance {
            endpoint: endpoint.to_string(),
            instance: instance_name.to_string(),
        }
    })?;
    Ok(PortRef {
        instance,
        port: port.to_string(),
    })
}
