use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    IntrospectionDiagnostic, IntrospectionDiagnosticMetric, IntrospectionFailoverEvent,
    IntrospectionOperationStatus, IntrospectionProcessStatus, IntrospectionRouteStatus,
    IntrospectionServiceStatus, IntrospectionStatus, IntrospectionTaskHealth,
};

/// tracing exporter 配置。
///
/// 默认关闭；关闭时 `TracingExporter::export_status` 只做一次布尔判断，不派生 span。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TracingExporterConfig {
    pub enabled: bool,
    pub endpoint: Option<String>,
}

/// FlowRT 从 introspection 快照派生出的稳定 span 表示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowrtSpan {
    pub name: String,
    pub entity_kind: String,
    pub entity_id: String,
    pub state: String,
    pub observed_ms: Option<u64>,
    #[serde(default)]
    pub attributes: BTreeMap<String, serde_json::Value>,
}

impl FlowrtSpan {
    fn new(
        name: &'static str,
        entity_kind: &'static str,
        entity_id: impl Into<String>,
        state: impl Into<String>,
        observed_ms: Option<u64>,
        attributes: BTreeMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            name: name.to_string(),
            entity_kind: entity_kind.to_string(),
            entity_id: entity_id.into(),
            state: state.into(),
            observed_ms,
            attributes,
        }
    }
}

/// FlowRT span 输出端。
///
/// runtime 只依赖该 trait，不持有 OpenTelemetry SDK 或 transport handle。
pub trait FlowrtSpanSink {
    fn emit(&self, span: FlowrtSpan) -> Result<(), String>;
}

/// 单次 tracing export 的结果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TracingExportReport {
    pub attempted: usize,
    pub emitted: usize,
    pub diagnostics: Vec<IntrospectionDiagnostic>,
}

/// 从 introspection status 派生并输出 FlowRT spans 的 additive exporter。
pub struct TracingExporter<S> {
    config: TracingExporterConfig,
    sink: S,
}

impl<S> TracingExporter<S>
where
    S: FlowrtSpanSink,
{
    pub fn new(config: TracingExporterConfig, sink: S) -> Self {
        Self { config, sink }
    }

    pub fn export_status(&self, status: &IntrospectionStatus) -> TracingExportReport {
        if !self.config.enabled {
            return TracingExportReport::default();
        }

        let spans = derive_tracing_spans(status);
        let attempted = spans.len();
        let mut emitted = 0;
        for span in spans {
            if let Err(error) = self.sink.emit(span) {
                return TracingExportReport {
                    attempted,
                    emitted,
                    diagnostics: vec![tracing_export_failure_diagnostic(
                        self.config.endpoint.as_deref(),
                        error,
                        attempted,
                        emitted,
                        status.clock.tick_time_ms,
                    )],
                };
            }
            emitted += 1;
        }

        TracingExportReport {
            attempted,
            emitted,
            diagnostics: Vec::new(),
        }
    }
}

/// 从 runtime status 快照派生 tracing spans。
pub fn derive_tracing_spans(status: &IntrospectionStatus) -> Vec<FlowrtSpan> {
    let mut spans = Vec::with_capacity(
        1 + status.processes.len()
            + status.tasks.len()
            + status.services.len()
            + status.operations.len()
            + status.routes.len()
            + status.failovers.len(),
    );
    spans.push(global_tick_span(status));
    spans.extend(status.processes.iter().map(process_span));
    spans.extend(status.tasks.iter().map(task_span));
    spans.extend(status.services.iter().map(service_span));
    spans.extend(status.operations.iter().map(operation_span));
    spans.extend(status.routes.iter().map(route_span));
    spans.extend(status.failovers.iter().map(failover_span));
    spans
}

fn global_tick_span(status: &IntrospectionStatus) -> FlowrtSpan {
    FlowrtSpan::new(
        "flowrt.global_tick",
        "clock",
        status.clock.source.clone(),
        status.graph_health.clone(),
        status.clock.tick_time_ms,
        attrs([
            attr("tick_count", status.tick_count),
            attr("clock_source", status.clock.source.clone()),
            attr("tick_time_ms", status.clock.tick_time_ms),
            attr("unit", status.clock.unit.clone()),
            attr("field", status.clock.field.clone()),
            attr("graph_health", status.graph_health.clone()),
            attr(
                "graph_critical_health",
                status.graph_critical_health.clone(),
            ),
        ]),
    )
}

fn process_span(process: &IntrospectionProcessStatus) -> FlowrtSpan {
    FlowrtSpan::new(
        "flowrt.process.lifecycle",
        "process",
        process.name.clone(),
        process.state.clone(),
        process.last_seen_unix_ms,
        attrs([
            attr("pid", process.pid),
            attr("restart_count", process.restart_count),
            attr("tick_count", process.tick_count),
            attr("tick_stale", process.tick_stale),
            attr("exit_code", process.exit_code),
            attr("readiness_wait", process.readiness_wait.clone()),
        ]),
    )
}

fn task_span(task: &IntrospectionTaskHealth) -> FlowrtSpan {
    let state = if task.inflight {
        "inflight"
    } else if task.consecutive_failures > 0 {
        "failing"
    } else if task.overrun.unwrap_or(false) || task.deadline_missed > 0 {
        "degraded"
    } else {
        "ok"
    };
    FlowrtSpan::new(
        "flowrt.task.execution",
        "task",
        task.name.clone(),
        state,
        task.last_run_ms.or(task.observed_time_ms),
        attrs([
            attr("lane", task.lane.clone()),
            attr("inflight", task.inflight),
            attr("scheduled_time_ms", task.scheduled_time_ms),
            attr("observed_time_ms", task.observed_time_ms),
            attr("lateness_ms", task.lateness_ms),
            attr("missed_periods", task.missed_periods),
            attr("overrun", task.overrun),
            attr("deadline_missed", task.deadline_missed),
            attr("stale_input", task.stale_input),
            attr("backpressure", task.backpressure),
            attr("overflow", task.overflow),
            attr("fairness_violations", task.fairness_violations),
            attr("run_count", task.run_count),
            attr("success_count", task.success_count),
            attr("consecutive_failures", task.consecutive_failures),
        ]),
    )
}

fn service_span(service: &IntrospectionServiceStatus) -> FlowrtSpan {
    FlowrtSpan::new(
        "flowrt.service.request",
        "service",
        service.name.clone(),
        if service.ready {
            "ready"
        } else {
            "unavailable"
        },
        None,
        attrs([
            attr("ready", service.ready),
            attr("in_flight", service.in_flight),
            attr("queued", service.queued),
            attr("total_requests", service.total_requests),
            attr("timeout_count", service.timeout_count),
            attr("busy_count", service.busy_count),
            attr("unavailable_count", service.unavailable_count),
            attr("late_drop_count", service.late_drop_count),
        ]),
    )
}

fn operation_span(operation: &IntrospectionOperationStatus) -> FlowrtSpan {
    FlowrtSpan::new(
        "flowrt.operation.invocation",
        "operation",
        operation.name.clone(),
        operation.current_state.clone().unwrap_or_else(|| {
            if operation.ready {
                "ready"
            } else {
                "unavailable"
            }
            .to_string()
        }),
        operation.last_transition_ms,
        attrs([
            attr("ready", operation.ready),
            attr("running", operation.running),
            attr("queued", operation.queued),
            attr(
                "current_operation_ids",
                operation.current_operation_ids.clone(),
            ),
            attr("total_started", operation.total_started),
            attr("succeeded_count", operation.succeeded_count),
            attr("failed_count", operation.failed_count),
            attr("canceled_count", operation.canceled_count),
            attr("timeout_count", operation.timeout_count),
            attr("preempted_count", operation.preempted_count),
            attr("current_owner", operation.current_owner.clone()),
            attr("current_deadline_ms", operation.current_deadline_ms),
            attr("last_event", operation.last_event.clone()),
            attr("last_error", operation.last_error.clone()),
        ]),
    )
}

fn route_span(route: &IntrospectionRouteStatus) -> FlowrtSpan {
    FlowrtSpan::new(
        "flowrt.route.publish",
        "route",
        route.name.clone(),
        route.backend_health_state.clone(),
        route.last_publish_ms,
        attrs([
            attr("from", route.from.clone()),
            attr("to", route.to.clone()),
            attr("message_type", route.message_type.clone()),
            attr("backend", route.backend.clone()),
            attr("selected_reason", route.selected_reason.clone()),
            attr("published_count", route.published_count),
            attr("dropped_samples", route.dropped_samples),
            attr("backpressure_count", route.backpressure_count),
            attr("overflow_count", route.overflow_count),
            attr("last_error", route.last_error.clone()),
            attr("backend_health_error", route.backend_health_error.clone()),
            attr("backend_reconnect_attempt", route.backend_reconnect_attempt),
            attr(
                "backend_next_retry_unix_ms",
                route.backend_next_retry_unix_ms,
            ),
            attr("backend_recoverable", route.backend_recoverable),
        ]),
    )
}

fn failover_span(failover: &IntrospectionFailoverEvent) -> FlowrtSpan {
    FlowrtSpan::new(
        "flowrt.failover.transition",
        "failover",
        failover.group.clone(),
        failover.event.clone(),
        Some(failover.tick_id),
        attrs([
            attr("old_active", failover.old_active.clone()),
            attr("new_active", failover.new_active.clone()),
            attr("tick_id", failover.tick_id),
            attr("reason", failover.reason.clone()),
        ]),
    )
}

fn tracing_export_failure_diagnostic(
    endpoint: Option<&str>,
    error: String,
    attempted: usize,
    emitted: usize,
    observed_ms: Option<u64>,
) -> IntrospectionDiagnostic {
    IntrospectionDiagnostic {
        category: "tracing".to_string(),
        entity_kind: "tracing_exporter".to_string(),
        entity_id: endpoint.unwrap_or("flowrt.tracing").to_string(),
        state: "export_failed".to_string(),
        severity: "warn".to_string(),
        reason: Some(error),
        suggestion: Some("check tracing sink endpoint and retry policy".to_string()),
        updated_unix_ms: None,
        observed_ms,
        metrics: vec![
            metric("attempted_spans", attempted),
            metric("emitted_spans", emitted),
        ],
    }
}

fn attrs<const N: usize>(
    items: [(String, serde_json::Value); N],
) -> BTreeMap<String, serde_json::Value> {
    items.into_iter().collect()
}

fn attr(name: &str, value: impl Serialize) -> (String, serde_json::Value) {
    (
        name.to_string(),
        serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
    )
}

fn metric(name: &str, value: impl Serialize) -> IntrospectionDiagnosticMetric {
    IntrospectionDiagnosticMetric {
        name: name.to_string(),
        value: serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
    }
}
