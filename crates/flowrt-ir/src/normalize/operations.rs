use std::collections::BTreeMap;

use flowrt_rsdl::{RawDocument, RawOperationBind};

use crate::{
    BackendName, ComponentIr, EntityRef, InstanceIr, IrError, LanguageKind,
    OPERATION_DEFAULT_MAX_IN_FLIGHT, OPERATION_DEFAULT_QUEUE_DEPTH,
    OPERATION_DEFAULT_RESULT_RETENTION_MS, OPERATION_DEFAULT_TIMEOUT_MS,
    OperationConcurrencyPolicy, OperationEdgeIr, OperationFeedbackPolicy, OperationPolicyIr,
    OperationPolicySourceIr, OperationPortIr, OperationPortRef, OperationPreemptPolicy,
    PolicyValueSource, ProfileIr, Result, TypeExpr, TypeIr,
};

use super::ids::entity_id;

pub(super) fn normalize_operation_binds(
    document: &RawDocument,
    instance_refs: &BTreeMap<String, EntityRef>,
    instances: &[InstanceIr],
    components: &[ComponentIr],
    types: &[TypeIr],
    profiles: &[ProfileIr],
    graph_name: &str,
) -> Result<Vec<OperationEdgeIr>> {
    let default_backend = profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or(profiles.first())
        .map(|profile| profile.backend.0.as_str())
        .unwrap_or("inproc");
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
            let (goal_type, feedback_type, result_type) =
                operation_port_types(&instances_by_name, &components_by_name, &client, &server)
                    .map(|(goal, feedback, result)| (Some(goal), Some(feedback), Some(result)))
                    .unwrap_or((None, None, None));
            let requested = raw.backend.as_deref().unwrap_or(default_backend);
            let explicit = raw.backend.is_some();
            let resolution = crate::backend::resolve_operation_backend(
                requested,
                goal_type,
                feedback_type,
                result_type,
                types,
                topology,
                explicit,
            )?;

            let policy = parse_operation_policy(&context, raw)?;
            let policy_source = operation_policy_source(raw);

            Ok(OperationEdgeIr {
                id: entity_id("operation", &format!("{}->{}", raw.client, raw.server)),
                client,
                server,
                backend: BackendName(resolution.backend),
                backend_source: resolution.source,
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

pub(crate) fn operation_route_topology(
    instances: &BTreeMap<&str, &InstanceIr>,
    components: &BTreeMap<&str, &ComponentIr>,
    client: &OperationPortRef,
    server: &OperationPortRef,
) -> crate::backend::RouteTopology {
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
    crate::backend::RouteTopology {
        crosses_process,
        crosses_target,
        touches_external,
    }
}

pub(crate) fn operation_port_types<'a>(
    instances: &BTreeMap<&str, &InstanceIr>,
    components: &BTreeMap<&str, &'a ComponentIr>,
    client: &OperationPortRef,
    server: &OperationPortRef,
) -> Option<(&'a TypeExpr, &'a TypeExpr, &'a TypeExpr)> {
    let client_port = operation_port(
        components,
        instances.get(client.instance.name.as_str())?,
        client,
        true,
    )?;
    let server_port = operation_port(
        components,
        instances.get(server.instance.name.as_str())?,
        server,
        false,
    )?;
    if client_port.goal == server_port.goal
        && client_port.feedback == server_port.feedback
        && client_port.result == server_port.result
    {
        Some((
            &client_port.goal,
            &client_port.feedback,
            &client_port.result,
        ))
    } else {
        Some((
            &server_port.goal,
            &server_port.feedback,
            &server_port.result,
        ))
    }
}

fn operation_port<'a>(
    components: &BTreeMap<&str, &'a ComponentIr>,
    instance: &InstanceIr,
    port_ref: &OperationPortRef,
    client: bool,
) -> Option<&'a OperationPortIr> {
    let component = components.get(instance.component.name.as_str())?;
    let ports = if client {
        &component.operation_clients
    } else {
        &component.operation_servers
    };
    ports.iter().find(|port| port.name == port_ref.port)
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
