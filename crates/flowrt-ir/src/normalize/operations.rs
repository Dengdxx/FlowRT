use std::collections::BTreeMap;

use flowrt_rsdl::{RawDocument, RawOperationBind};

use crate::{
    BackendName, ComponentIr, EntityRef, InstanceIr, IrError, LanguageKind,
    OPERATION_DEFAULT_MAX_IN_FLIGHT, OPERATION_DEFAULT_QUEUE_DEPTH,
    OPERATION_DEFAULT_RESULT_RETENTION_MS, OPERATION_DEFAULT_TIMEOUT_MS, OperationBackendSource,
    OperationConcurrencyPolicy, OperationEdgeIr, OperationFeedbackPolicy, OperationPolicyIr,
    OperationPolicySourceIr, OperationPortRef, OperationPreemptPolicy, PolicyValueSource, Result,
};

use super::ids::entity_id;

pub(super) fn normalize_operation_binds(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    instances: &[InstanceIr],
    components: &[ComponentIr],
    graph_name: &str,
) -> Result<Vec<OperationEdgeIr>> {
    let instances_by_name = instances
        .iter()
        .map(|instance| (instance.name.as_str(), instance))
        .collect::<BTreeMap<_, _>>();
    let components_by_name = components
        .iter()
        .map(|component| (component.qualified_name.as_str(), component))
        .collect::<BTreeMap<_, _>>();

    let mut operations = document
        .operation_binds
        .iter()
        .enumerate()
        .map(|(index, raw)| {
            let client = parse_operation_port_ref(&raw.client, instance_refs)?;
            let server = parse_operation_port_ref(&raw.server, instance_refs)?;
            let context = format!("bind.operation[{index}]");

            let topology =
                operation_route_topology(&instances_by_name, &components_by_name, &client, &server);
            let (backend, backend_source) =
                resolve_operation_backend(&context, raw.backend.as_deref(), topology)?;

            let policy = parse_operation_policy(&context, raw)?;
            let policy_source = operation_policy_source(raw);

            Ok(OperationEdgeIr {
                id: entity_id("operation", &format!("{}->{}", raw.client, raw.server)),
                client,
                server,
                backend: BackendName(backend),
                backend_source,
                policy,
                policy_source,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    operations.sort_by(|left, right| {
        (
            &left.client.instance.name,
            &left.client.port,
            &left.server.instance.name,
            &left.server.port,
        )
            .cmp(&(
                &right.client.instance.name,
                &right.client.port,
                &right.server.instance.name,
                &right.server.port,
            ))
    });
    for operation in &mut operations {
        operation.id = entity_id(
            "operation",
            &format!(
                "{graph_name}.{}.{}->{}.{}",
                operation.client.instance.name,
                operation.client.port,
                operation.server.instance.name,
                operation.server.port
            ),
        );
    }
    Ok(operations)
}

fn operation_route_topology(
    instances: &BTreeMap<&str, &InstanceIr>,
    components: &BTreeMap<&str, &ComponentIr>,
    client: &OperationPortRef,
    server: &OperationPortRef,
) -> (bool, bool, bool) {
    let client_instance = instances.get(client.instance.name.as_str());
    let server_instance = instances.get(server.instance.name.as_str());
    let client_process = client_instance
        .and_then(|i| i.process.as_deref())
        .unwrap_or("main");
    let server_process = server_instance
        .and_then(|i| i.process.as_deref())
        .unwrap_or("main");
    let client_target = client_instance
        .and_then(|i| i.target.as_ref())
        .map(|t| t.name.as_str());
    let server_target = server_instance
        .and_then(|i| i.target.as_ref())
        .map(|t| t.name.as_str());
    let crosses_process = client_process != server_process;
    let crosses_target =
        client_target.is_some() && server_target.is_some() && client_target != server_target;
    let touches_external = [client_instance, server_instance].iter().any(|instance| {
        instance
            .and_then(|instance| components.get(instance.component.name.as_str()))
            .is_some_and(|component| component.language == LanguageKind::External)
    });
    (crosses_process, crosses_target, touches_external)
}

fn resolve_operation_backend(
    context: &str,
    requested: Option<&str>,
    (crosses_process, crosses_target, touches_external): (bool, bool, bool),
) -> Result<(String, OperationBackendSource)> {
    match requested {
        None | Some("auto") => {
            let resolved = if touches_external || crosses_process || crosses_target {
                "zenoh"
            } else {
                "inproc"
            };
            Ok((resolved.to_string(), OperationBackendSource::AutoResolved))
        }
        Some("inproc") => {
            if touches_external {
                Err(IrError::InvalidValue {
                    context: context.to_string(),
                    message: "external operation route cannot use `inproc` backend".to_string(),
                })
            } else if crosses_process || crosses_target {
                Err(IrError::InvalidValue {
                    context: context.to_string(),
                    message:
                        "explicit `inproc` operation backend cannot span process or target boundaries"
                            .to_string(),
                })
            } else {
                Ok(("inproc".to_string(), OperationBackendSource::Explicit))
            }
        }
        Some("zenoh") => Ok(("zenoh".to_string(), OperationBackendSource::Explicit)),
        Some("iox2") => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "operation backend",
            value: "iox2".to_string(),
        }),
        Some(other) => Err(IrError::InvalidEnum {
            context: context.to_string(),
            kind: "operation backend",
            value: other.to_string(),
        }),
    }
}

fn parse_operation_policy(context: &str, raw: &RawOperationBind) -> Result<OperationPolicyIr> {
    let timeout_ms = raw.timeout_ms.unwrap_or(OPERATION_DEFAULT_TIMEOUT_MS);
    if raw.timeout_ms.is_some() && timeout_ms == 0 {
        return Err(IrError::InvalidValue {
            context: format!("{context}.timeout_ms"),
            message: "operation timeout_ms must be greater than zero".to_string(),
        });
    }

    let concurrency = match raw.concurrency.as_deref() {
        Some("reject") | None => OperationConcurrencyPolicy::Reject,
        Some("queue") => OperationConcurrencyPolicy::Queue,
        Some(other) => {
            return Err(IrError::InvalidEnum {
                context: format!("{context}.concurrency"),
                kind: "operation concurrency policy",
                value: other.to_string(),
            });
        }
    };

    let preempt = match raw.preempt.as_deref() {
        Some("reject") | None => OperationPreemptPolicy::Reject,
        Some("cancel_running") => OperationPreemptPolicy::CancelRunning,
        Some(other) => {
            return Err(IrError::InvalidEnum {
                context: format!("{context}.preempt"),
                kind: "operation preempt policy",
                value: other.to_string(),
            });
        }
    };

    let queue_depth = raw.queue_depth.unwrap_or(OPERATION_DEFAULT_QUEUE_DEPTH);
    if raw.queue_depth.is_some() && queue_depth == 0 {
        return Err(IrError::InvalidValue {
            context: format!("{context}.queue_depth"),
            message: "operation queue_depth must be greater than zero".to_string(),
        });
    }

    let max_in_flight = raw.max_in_flight.unwrap_or(OPERATION_DEFAULT_MAX_IN_FLIGHT);
    if raw.max_in_flight.is_some() && max_in_flight == 0 {
        return Err(IrError::InvalidValue {
            context: format!("{context}.max_in_flight"),
            message: "operation max_in_flight must be greater than zero".to_string(),
        });
    }

    let feedback = match raw.feedback.as_deref() {
        Some("latest") | None => OperationFeedbackPolicy::Latest,
        Some("fifo") => OperationFeedbackPolicy::Fifo,
        Some(other) => {
            return Err(IrError::InvalidEnum {
                context: format!("{context}.feedback"),
                kind: "operation feedback policy",
                value: other.to_string(),
            });
        }
    };

    Ok(OperationPolicyIr {
        timeout_ms,
        concurrency,
        preempt,
        queue_depth,
        max_in_flight,
        feedback,
        result_retention_ms: raw
            .result_retention_ms
            .unwrap_or(OPERATION_DEFAULT_RESULT_RETENTION_MS),
    })
}

fn operation_policy_source(raw: &RawOperationBind) -> OperationPolicySourceIr {
    OperationPolicySourceIr {
        backend: source(raw.backend.is_some()),
        timeout_ms: source(raw.timeout_ms.is_some()),
        concurrency: source(raw.concurrency.is_some()),
        preempt: source(raw.preempt.is_some()),
        queue_depth: source(raw.queue_depth.is_some()),
        max_in_flight: source(raw.max_in_flight.is_some()),
        feedback: source(raw.feedback.is_some()),
        result_retention_ms: source(raw.result_retention_ms.is_some()),
    }
}

fn source(explicit: bool) -> PolicyValueSource {
    if explicit {
        PolicyValueSource::Explicit
    } else {
        PolicyValueSource::ProfileDefault
    }
}

fn parse_operation_port_ref(
    endpoint: &str,
    instance_refs: &BTreeMap<String, EntityRef>,
) -> Result<OperationPortRef> {
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
    Ok(OperationPortRef {
        instance,
        port: port.to_string(),
    })
}
