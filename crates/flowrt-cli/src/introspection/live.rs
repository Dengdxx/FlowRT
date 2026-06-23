use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use flowrt_selfdesc::{SelfDescription, SelfDescriptionResourceDescriptor};
use serde::Serialize;

use super::*;

pub(crate) fn live_status_summary(live_only: bool) -> Result<String> {
    let sockets = discover_cli_runtime_sockets()?;
    live_status_summary_for_sockets(sockets, live_only)
}

#[derive(Debug, Serialize)]
struct LiveStatusJsonEntry {
    socket: String,
    live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    handshake: Option<flowrt::IntrospectionHandshake>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<flowrt::IntrospectionStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub(crate) fn live_status_json(live_only: bool) -> Result<String> {
    let sockets = discover_cli_runtime_sockets()?;
    live_status_json_for_sockets(sockets, live_only)
}

pub(crate) fn live_status_json_for_sockets(
    sockets: Vec<PathBuf>,
    live_only: bool,
) -> Result<String> {
    let mut entries = Vec::new();
    for socket in sockets {
        match flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(flowrt::IntrospectionResponse::Status { handshake, status }) => {
                entries.push(LiveStatusJsonEntry {
                    socket: socket.display().to_string(),
                    live: true,
                    handshake: Some(handshake),
                    status: Some(status),
                    error: None,
                });
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) if !live_only => {
                entries.push(LiveStatusJsonEntry {
                    socket: socket.display().to_string(),
                    live: false,
                    handshake: None,
                    status: None,
                    error: Some(message),
                });
            }
            Ok(_) if !live_only => {
                entries.push(LiveStatusJsonEntry {
                    socket: socket.display().to_string(),
                    live: false,
                    handshake: None,
                    status: None,
                    error: Some("unexpected introspection response".to_string()),
                });
            }
            Err(error) if !live_only => {
                entries.push(LiveStatusJsonEntry {
                    socket: socket.display().to_string(),
                    live: false,
                    handshake: None,
                    status: None,
                    error: Some(error.to_string()),
                });
            }
            _ => {}
        }
    }
    serde_json::to_string_pretty(&entries).context("序列化 live status JSON 失败")
}

pub(crate) fn live_status_summary_for_sockets(
    sockets: Vec<PathBuf>,
    live_only: bool,
) -> Result<String> {
    let mut lines = Vec::new();
    for socket in sockets {
        match flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(flowrt::IntrospectionResponse::Status { handshake, status }) => {
                // static self-description 只提供合同关联信息；live status 仍是运行态事实源。
                let static_facts =
                    load_static_self_description_facts(&socket, &handshake.self_description_hash);
                let recorder = status.recorder.clone();

                let live_counts = live_status_counts(&status);
                let critical_instances = if status.critical_instances.is_empty() {
                    "none".to_string()
                } else {
                    status.critical_instances.join(",")
                };
                let temporary_overlay = static_facts.artifact.temporary_overlay.is_some();
                lines.push(format!(
                    "pid={} package={} process={} runtime={} selfdesc={} static_selfdesc={} ticks={} clock_source={} tick_time_ms={} clock_unit={} clock_field={} channels={} inputs={} routes={} graph_health={} graph_critical_health={} critical_instances={} observers={} dropped_samples={} artifact_mode={} temporary_island={} test_only={} temporary_overlay={} socket={}",
                    handshake.pid,
                    handshake.package,
                    handshake.process,
                    handshake.runtime,
                    handshake.self_description_hash,
                    static_facts.load_state_label(),
                    status.tick_count,
                    status.clock.source,
                    option_u64(status.clock.tick_time_ms),
                    status.clock.unit,
                    status.clock.field,
                    status.channels.len(),
                    status.inputs.len(),
                    status.routes.len(),
                    status.graph_health,
                    status.graph_critical_health,
                    critical_instances,
                    live_counts.active_observers,
                    live_counts.dropped_samples,
                    static_facts.artifact.mode,
                    static_facts.artifact.temporary_island,
                    static_facts.artifact.test_only,
                    temporary_overlay,
                    socket.display()
                ));
                for graph in &static_facts.graphs {
                    lines.push(format!(
                        "graph={} mode={} boundary_endpoints={} socket={}",
                        graph.name,
                        graph.mode,
                        graph.boundary_endpoint_count,
                        socket.display()
                    ));
                }
                for boundary in &static_facts.boundary_endpoints {
                    lines.push(format!(
                        "boundary_endpoint={} direction={} endpoint={} type={} graph={} mode={} socket={}",
                        boundary.name,
                        boundary.direction,
                        boundary.endpoint,
                        boundary.message_type,
                        boundary.graph,
                        boundary.graph_mode,
                        socket.display()
                    ));
                }
                for channel in &status.channels {
                    lines.push(format!(
                        "channel={} type={} published_count={} last_payload_len={} observers={} dropped_samples={} socket={}",
                        channel.name,
                        channel.message_type,
                        channel.published_count,
                        option_usize(channel.last_payload_len),
                        channel.active_observers,
                        channel.dropped_samples,
                        socket.display()
                    ));
                }
                for route in &status.routes {
                    let static_thread_affinity = static_facts
                        .route_thread_affinity
                        .get(&route_affinity_key(&route.from, &route.to))
                        .map(String::as_str)
                        .unwrap_or("none");
                    lines.push(format!(
                        "route={} from={} to={} type={} backend={} thread_affinity={} static_thread_affinity={} selected_reason={} published_count={} dropped_samples={} backpressure={} overflow={} last_publish_ms={} last_error={} backend_health={} backend_recoverable={} backend_reconnect_attempt={} backend_next_retry_unix_ms={} backend_health_error={} socket={}",
                        route.name,
                        route.from,
                        route.to,
                        route.message_type,
                        route.backend,
                        static_thread_affinity,
                        static_thread_affinity,
                        empty_as_none(&route.selected_reason),
                        route.published_count,
                        route.dropped_samples,
                        route.backpressure_count,
                        route.overflow_count,
                        option_u64(route.last_publish_ms),
                        option_str(route.last_error.as_deref()),
                        empty_as_none(&route.backend_health_state),
                        route.backend_recoverable,
                        route.backend_reconnect_attempt,
                        option_u64(route.backend_next_retry_unix_ms),
                        option_str(route.backend_health_error.as_deref()),
                        socket.display()
                    ));
                }
                for input in &status.inputs {
                    lines.push(format!(
                        "input={} task={} channel={} type={} present={} stale={} last_revision={} last_read_ms={} updated_unix_ms={} dropped_samples={} backpressure={} overflow={} socket={}",
                        input_display_name(input),
                        input.task,
                        input.channel,
                        input.message_type,
                        input.present,
                        input.stale,
                        option_u64(input.last_revision),
                        option_u64(input.last_read_ms),
                        option_u64(input.updated_unix_ms),
                        input.dropped_samples,
                        input.backpressure_count,
                        input.overflow_count,
                        socket.display()
                    ));
                }
                for instance in &status.instances {
                    lines.push(format!(
                        "instance={} lifecycle={} restart_count={} last_fault_reason={} last_fault_tick={} last_transition_tick={} socket={}",
                        instance.instance,
                        instance.lifecycle_state,
                        instance.restart_count,
                        option_str(instance.last_fault_reason.as_deref()),
                        option_u64(instance.last_fault_tick),
                        option_u64(instance.last_transition_tick),
                        socket.display()
                    ));
                }
                for param in &status.params {
                    lines.push(format!(
                        "param={} socket={}",
                        format_param_status(param),
                        socket.display()
                    ));
                }
                for process in status.processes {
                    let readiness_info = process
                        .readiness_wait
                        .as_deref()
                        .map(|wait| format!(" readiness_wait={wait}"))
                        .unwrap_or_default();
                    let resource_info = process
                        .resource_placement
                        .as_ref()
                        .and_then(|placement| serde_json::to_string(placement).ok())
                        .map(|placement| format!(" resource_placement={placement}"))
                        .unwrap_or_default();
                    lines.push(format!(
                        "supervisor_process={} state={} pid={} restarts={} ticks={} last_seen_ms={} tick_stale={} exit_code={}{}{} socket={}",
                        process.name,
                        process.state,
                        option_u32(process.pid),
                        process.restart_count,
                        option_u64(process.tick_count),
                        option_u64(process.last_seen_unix_ms),
                        process.tick_stale,
                        option_i32(process.exit_code),
                        readiness_info,
                        resource_info,
                        socket.display()
                    ));
                }
                for service in status.services {
                    let (client_inst, server_inst) = static_facts
                        .service_endpoints
                        .get(service.name.as_str())
                        .map(|ep| (ep.client_instance.as_str(), ep.server_instance.as_str()))
                        .unwrap_or(("", ""));
                    if client_inst.is_empty() {
                        lines.push(format!(
                            "service={} ready={} in_flight={} queued={} total_requests={} timeout={} busy={} unavailable={} late_drop={} socket={}",
                            service.name,
                            service.ready,
                            service.in_flight,
                            service.queued,
                            service.total_requests,
                            service.timeout_count,
                            service.busy_count,
                            service.unavailable_count,
                            service.late_drop_count,
                            socket.display()
                        ));
                    } else {
                        lines.push(format!(
                            "service={} client_instance={} server_instance={} ready={} in_flight={} queued={} total_requests={} timeout={} busy={} unavailable={} late_drop={} socket={}",
                            service.name,
                            client_inst,
                            server_inst,
                            service.ready,
                            service.in_flight,
                            service.queued,
                            service.total_requests,
                            service.timeout_count,
                            service.busy_count,
                            service.unavailable_count,
                            service.late_drop_count,
                            socket.display()
                        ));
                    }
                }
                for operation in status.operations {
                    lines.push(format_operation_status(&operation, Some(&socket)));
                }
                for resource in &status.resources {
                    lines.push(format!(
                        "resource={} capability={} access={} state={} required={} readiness={} health={} on_failure={} contract_status={} satisfied={} provider={} provider_scope={} provider_readiness_source={} provider_health_source={} source={} owner_process={} diagnostic={} suggestion={} last_error={} updated_unix_ms={} socket={}",
                        resource.name,
                        resource.capability,
                        option_str(resource.access.as_deref()),
                        resource.state,
                        resource.required,
                        option_str(resource.readiness.as_deref()),
                        option_str(resource.health.as_deref()),
                        option_str(resource.on_failure.as_deref()),
                        option_str(resource.contract_status.as_deref()),
                        option_bool(resource.satisfied),
                        option_str(resource.provider.as_deref()),
                        option_str(resource.provider_scope.as_deref()),
                        option_str(resource.provider_readiness_source.as_deref()),
                        option_str(resource.provider_health_source.as_deref()),
                        option_str(resource.source.as_deref()),
                        option_str(resource.owner_process.as_deref()),
                        option_str(resource.diagnostic.as_deref()),
                        option_str(resource.suggestion.as_deref()),
                        option_str(resource.last_error.as_deref()),
                        option_u64(resource.updated_unix_ms),
                        socket.display()
                    ));
                }
                for boundary in &status.io_boundaries {
                    lines.push(format!(
                        "io_boundary={} component={} ready={} healthy={} last_error={} updated_unix_ms={} socket={}",
                        boundary.name,
                        boundary.component,
                        boundary.ready,
                        boundary.healthy,
                        option_str(boundary.last_error.as_deref()),
                        option_u64(boundary.updated_unix_ms),
                        socket.display()
                    ));
                    for resource in &boundary.resources {
                        let descriptor_info = static_facts
                            .resource_descriptors
                            .get(&resource_descriptor_key(&boundary.name, &resource.name))
                            .map(format_descriptor_schema)
                            .unwrap_or_default();
                        lines.push(format!(
                            "io_boundary_resource={}.{} kind={} ready={} message={} last_error={} updated_unix_ms={}{} socket={}",
                            boundary.name,
                            resource.name,
                            resource.kind,
                            resource.ready,
                            option_str(resource.message.as_deref()),
                            option_str(resource.last_error.as_deref()),
                            option_u64(resource.updated_unix_ms),
                            descriptor_info,
                            socket.display()
                        ));
                    }
                }
                for task in &status.tasks {
                    let last_run = task
                        .last_run_ms
                        .map_or_else(|| "none".to_string(), |v| v.to_string());
                    let last_success = task
                        .last_success_ms
                        .map_or_else(|| "none".to_string(), |v| v.to_string());
                    let timing = if task.scheduled_time_ms.is_some()
                        || task.observed_time_ms.is_some()
                        || task.lateness_ms.is_some()
                        || task.missed_periods.is_some()
                        || task.overrun.is_some()
                    {
                        "runtime_observed"
                    } else {
                        "none"
                    };
                    lines.push(format!(
                        "task_health={} lane={} inflight={} scheduled_time_ms={} observed_time_ms={} lateness_ms={} missed_periods={} overrun={} timing={} deadline_missed={} stale_input={} backpressure={} overflow={} fairness_violations={} runs={} successes={} consecutive_failures={} last_run_ms={} last_success_ms={} socket={}",
                        task.name,
                        task.lane,
                        task.inflight,
                        option_u64(task.scheduled_time_ms),
                        option_u64(task.observed_time_ms),
                        option_u64(task.lateness_ms),
                        option_u64(task.missed_periods),
                        option_bool(task.overrun),
                        timing,
                        task.deadline_missed,
                        task.stale_input,
                        task.backpressure,
                        task.overflow,
                        task.fairness_violations,
                        task.run_count,
                        task.success_count,
                        task.consecutive_failures,
                        last_run,
                        last_success,
                        socket.display()
                    ));
                }
                for lane in &status.lanes {
                    lines.push(format!(
                        "lane_health={} queue_depth={} dispatched_count={} fairness_violations={} socket={}",
                        lane.name,
                        lane.queue_depth,
                        lane.dispatched_count,
                        lane.fairness_violations,
                        socket.display()
                    ));
                }
                for diagnostic in &status.diagnostics {
                    lines.push(format_diagnostic_status(diagnostic, &socket));
                }
                if recorder.enabled
                    || recorder.dropped_count != 0
                    || recorder.bytes_written != 0
                    || recorder.queued_events != 0
                {
                    let output = recorder.output.as_deref().unwrap_or("none");
                    lines.push(format!(
                        "recorder enabled={} output={} dropped_count={} bytes_written={} queued_events={} active_filters=[{}] socket={}",
                        recorder.enabled,
                        output,
                        recorder.dropped_count,
                        recorder.bytes_written,
                        recorder.queued_events,
                        recorder.active_filters.join(","),
                        socket.display()
                    ));
                }
            }
            Ok(flowrt::IntrospectionResponse::ChannelSnapshot { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected channel snapshot response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::SelfDescription { .. })
            | Ok(flowrt::IntrospectionResponse::ObserveReady { .. })
            | Ok(flowrt::IntrospectionResponse::OperationValue { .. })
            | Ok(flowrt::IntrospectionResponse::OperationResult { .. })
            | Ok(flowrt::IntrospectionResponse::OperationEvents { .. })
            | Ok(flowrt::IntrospectionResponse::BoundaryPublish { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected introspection response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::ParamList { .. })
            | Ok(flowrt::IntrospectionResponse::ParamValue { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected parameter response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::OperationStarted { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected operation response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::RecorderValue { .. })
            | Ok(flowrt::IntrospectionResponse::RecorderEvents { .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!(
                    "stale socket={} error=unexpected recorder response",
                    socket.display()
                ));
            }
            Ok(flowrt::IntrospectionResponse::Error { message, .. }) => {
                if live_only {
                    continue;
                }
                lines.push(format!("stale socket={} error={message}", socket.display()));
            }
            Err(error) => {
                if live_only {
                    continue;
                }
                lines.push(format!("stale socket={} error={error}", socket.display()));
            }
        }
    }
    if lines.is_empty() {
        Ok("no live FlowRT processes".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

/// service endpoint 关联信息（从 self-description 提取）。
struct ServiceEndpointAssoc {
    client_instance: String,
    server_instance: String,
}

/// graph 级静态模式摘要。
#[derive(Clone)]
struct GraphModeAssoc {
    name: String,
    mode: String,
    boundary_endpoint_count: usize,
}

/// island boundary endpoint 静态摘要。
#[derive(Clone)]
struct BoundaryEndpointAssoc {
    graph: String,
    graph_mode: String,
    name: String,
    direction: String,
    endpoint: String,
    message_type: String,
}

/// live status 输出中来自 static self-description 的合同事实。
#[derive(Default)]
struct StaticSelfDescriptionFacts {
    loaded: bool,
    artifact: flowrt_selfdesc::SelfDescriptionArtifact,
    graphs: Vec<GraphModeAssoc>,
    boundary_endpoints: Vec<BoundaryEndpointAssoc>,
    route_thread_affinity: BTreeMap<String, String>,
    service_endpoints: BTreeMap<String, ServiceEndpointAssoc>,
    resource_descriptors: BTreeMap<String, SelfDescriptionResourceDescriptor>,
}

impl StaticSelfDescriptionFacts {
    fn load_state_label(&self) -> &'static str {
        if self.loaded { "loaded" } else { "unavailable" }
    }
}

struct LiveStatusCounts {
    active_observers: u64,
    dropped_samples: u64,
}

fn live_status_counts(status: &flowrt::IntrospectionStatus) -> LiveStatusCounts {
    LiveStatusCounts {
        active_observers: status
            .channels
            .iter()
            .map(|channel| channel.active_observers)
            .sum(),
        dropped_samples: status
            .channels
            .iter()
            .map(|channel| channel.dropped_samples)
            .sum(),
    }
}

/// 从 runtime socket 请求 static self-description，构建 service/resource 关联映射。
///
/// 如果 self-description 请求失败（如 socket 不支持），返回空 map，不报错。
fn load_static_self_description_facts(
    socket: &Path,
    expected_hash: &str,
) -> StaticSelfDescriptionFacts {
    let Ok(response) =
        flowrt::request_self_description_with_timeout(socket, LOCAL_INTROSPECTION_TIMEOUT)
    else {
        return StaticSelfDescriptionFacts::default();
    };
    let flowrt::IntrospectionResponse::SelfDescription { handshake, json } = response else {
        return StaticSelfDescriptionFacts::default();
    };
    if handshake.self_description_hash != expected_hash
        || self_description_hash(json.as_bytes()) != expected_hash
    {
        return StaticSelfDescriptionFacts::default();
    }
    let Ok(sd) = serde_json::from_str::<SelfDescription>(&json) else {
        return StaticSelfDescriptionFacts::default();
    };
    let mut static_facts = StaticSelfDescriptionFacts {
        loaded: true,
        artifact: sd.artifact.clone(),
        ..StaticSelfDescriptionFacts::default()
    };
    for graph in &sd.graphs {
        static_facts.graphs.push(GraphModeAssoc {
            name: graph.name.clone(),
            mode: graph.mode.clone(),
            boundary_endpoint_count: graph.boundary_endpoints.len(),
        });
        for boundary in &graph.boundary_endpoints {
            static_facts.boundary_endpoints.push(BoundaryEndpointAssoc {
                graph: graph.name.clone(),
                graph_mode: graph.mode.clone(),
                name: boundary.name.clone(),
                direction: boundary.direction.clone(),
                endpoint: boundary.endpoint.clone(),
                message_type: boundary.message_type.clone(),
            });
        }
        for channel in &graph.channels {
            if channel.thread_affinity.is_empty() {
                continue;
            }
            static_facts.route_thread_affinity.insert(
                route_affinity_key(&channel.from, &channel.to),
                channel.thread_affinity.clone(),
            );
        }
        for ep in &graph.services {
            if !ep.client_instance.is_empty() && !ep.server_instance.is_empty() {
                static_facts.service_endpoints.insert(
                    ep.name.clone(),
                    ServiceEndpointAssoc {
                        client_instance: ep.client_instance.clone(),
                        server_instance: ep.server_instance.clone(),
                    },
                );
            }
        }
        let component_by_instance = graph
            .instances
            .iter()
            .map(|instance| (instance.name.as_str(), instance.component.as_str()))
            .collect::<BTreeMap<_, _>>();
        let component_types = sd
            .component_types
            .iter()
            .map(|component| (component.name.as_str(), component))
            .collect::<BTreeMap<_, _>>();
        for (instance, component_name) in component_by_instance {
            let Some(component) = component_types.get(component_name) else {
                continue;
            };
            for resource in &component.resources {
                let Some(descriptor) = &resource.descriptor else {
                    continue;
                };
                static_facts.resource_descriptors.insert(
                    resource_descriptor_key(instance, &resource.name),
                    descriptor.clone(),
                );
            }
        }
    }
    static_facts
}

fn route_affinity_key(from: &str, to: &str) -> String {
    format!("{from}->{to}")
}

fn resource_descriptor_key(boundary: &str, resource: &str) -> String {
    format!("{boundary}.{resource}")
}

pub(crate) fn live_hz_summary(
    channel: Option<&str>,
    socket: Option<&Path>,
    window_ms: u64,
) -> Result<String> {
    let sockets = match socket {
        Some(socket) => vec![socket.to_path_buf()],
        None => discover_cli_runtime_sockets()?,
    };
    live_hz_summary_for_sockets(channel, sockets, Duration::from_millis(window_ms))
}

pub(crate) fn live_hz_summary_for_sockets(
    channel: Option<&str>,
    sockets: Vec<PathBuf>,
    window: Duration,
) -> Result<String> {
    if sockets.is_empty() {
        return Ok("no live FlowRT processes".to_string());
    }

    let mut first = Vec::new();
    let mut lines = Vec::new();
    for socket in &sockets {
        match flowrt::request_status_with_timeout(socket, LOCAL_INTROSPECTION_TIMEOUT) {
            Ok(response) => {
                if let Some(status) = hz_status_or_stale(socket, response, &mut lines) {
                    first.push((socket.clone(), status));
                }
            }
            Err(error) => lines.push(format!("stale socket={} error={error}", socket.display())),
        }
    }
    if first.is_empty() {
        return Ok(lines.join("\n"));
    }
    let started = Instant::now();
    std::thread::sleep(window);
    let elapsed = started.elapsed();

    for (socket, first_status) in first {
        let second_status =
            match flowrt::request_status_with_timeout(&socket, LOCAL_INTROSPECTION_TIMEOUT) {
                Ok(response) => {
                    let Some(status) = hz_status_or_stale(&socket, response, &mut lines) else {
                        continue;
                    };
                    status
                }
                Err(error) => {
                    lines.push(format!("stale socket={} error={error}", socket.display()));
                    continue;
                }
            };
        let summary = format_hz_summary_from_status_pair(&first_status, &second_status, elapsed)?;
        for line in summary.lines() {
            if channel.is_none_or(|selected| hz_summary_line_matches_channel(line, selected)) {
                lines.push(format!("{line} socket={}", socket.display()));
            }
        }
    }

    if lines.is_empty() {
        match channel {
            Some(channel) => Ok(format!("no live FlowRT channel matched `{channel}`")),
            None => Ok("no live FlowRT channels".to_string()),
        }
    } else {
        Ok(lines.join("\n"))
    }
}

/// 从镜像文件计算 self-description hash，用于远程 discovery 匹配。
pub(crate) fn self_description_hash_for_image(image: &Path) -> Result<String> {
    let (_self_description, hash) = load_self_description_with_hash(image)?;
    Ok(hash)
}
