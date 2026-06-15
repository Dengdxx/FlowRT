use serde::Serialize;

use super::model::{
    IntrospectionDiagnostic, IntrospectionDiagnosticMetric, IntrospectionInputStatus,
    IntrospectionStatus,
};

pub(super) fn derive_diagnostics(status: &IntrospectionStatus) -> Vec<IntrospectionDiagnostic> {
    let mut diagnostics = Vec::new();
    let clock_ms = status.clock.tick_time_ms;
    diagnostics.push(diagnostic(
        "clock",
        "clock",
        &status.clock.source,
        &status.clock.source,
        if status.clock.source == "realtime" {
            "info"
        } else {
            "warn"
        },
        Some(format!("{} time source", status.clock.source)),
        None,
        None,
        clock_ms,
        vec![
            metric("tick_time_ms", status.clock.tick_time_ms),
            metric("unit", status.clock.unit.clone()),
            metric("field", status.clock.field.clone()),
        ],
    ));

    for channel in &status.channels {
        let dropped = channel.dropped_samples;
        diagnostics.push(diagnostic(
            "channel",
            "channel",
            &channel.name,
            if dropped > 0 { "dropping" } else { "ok" },
            if dropped > 0 { "warn" } else { "info" },
            (dropped > 0).then(|| "channel probe dropped samples".to_string()),
            None,
            None,
            clock_ms,
            vec![
                metric("published_count", channel.published_count),
                metric("last_payload_len", channel.last_payload_len),
                metric("active_observers", channel.active_observers),
                metric("dropped_samples", dropped),
            ],
        ));
    }

    for input in &status.inputs {
        let severity = if !input.present {
            "error"
        } else if input.stale {
            "warn"
        } else {
            "info"
        };
        let reason = if !input.present {
            Some("input has no latest sample".to_string())
        } else if input.stale {
            Some("input latest sample is stale".to_string())
        } else {
            None
        };
        diagnostics.push(diagnostic(
            "input",
            "input",
            &input_status_key(input),
            if !input.present {
                "missing"
            } else if input.stale {
                "stale"
            } else {
                "present"
            },
            severity,
            reason,
            None,
            input.updated_unix_ms,
            input.last_read_ms.or(clock_ms),
            vec![
                metric("present", input.present),
                metric("stale", input.stale),
                metric("last_revision", input.last_revision),
                metric("last_read_ms", input.last_read_ms),
                metric("dropped_samples", input.dropped_samples),
                metric("backpressure_count", input.backpressure_count),
                metric("overflow_count", input.overflow_count),
            ],
        ));
    }

    for route in &status.routes {
        let has_error = route.last_error.is_some();
        let has_loss =
            route.dropped_samples > 0 || route.backpressure_count > 0 || route.overflow_count > 0;
        let mut metrics = vec![
            metric("published_count", route.published_count),
            metric("dropped_samples", route.dropped_samples),
            metric("backpressure_count", route.backpressure_count),
            metric("overflow_count", route.overflow_count),
            metric("last_publish_ms", route.last_publish_ms),
        ];
        if let (Some(now), Some(last)) = (clock_ms, route.last_publish_ms)
            && now >= last
        {
            metrics.push(metric("latest_age_ms", now - last));
        }
        diagnostics.push(diagnostic(
            "route",
            "route",
            &route.name,
            if has_error {
                "error"
            } else if has_loss {
                "degraded"
            } else {
                "selected"
            },
            if has_error {
                "error"
            } else if has_loss {
                "warn"
            } else {
                "info"
            },
            route
                .last_error
                .clone()
                .or_else(|| Some(format!("backend selected: {}", route.selected_reason))),
            None,
            None,
            route.last_publish_ms.or(clock_ms),
            {
                metrics.push(metric("backend", route.backend.clone()));
                metrics.push(metric("selected_reason", route.selected_reason.clone()));
                metrics
            },
        ));
    }

    for process in &status.processes {
        let bad_state = !matches!(
            process.state.as_str(),
            "running" | "ready" | "started" | "ok"
        );
        let severity = if process.exit_code.is_some() || process.tick_stale {
            "error"
        } else if bad_state || process.restart_count > 0 {
            "warn"
        } else {
            "info"
        };
        let reason = process
            .readiness_wait
            .as_ref()
            .map(|wait| format!("waiting for {wait}"))
            .or_else(|| process.exit_code.map(|code| format!("exit_code={code}")))
            .or_else(|| {
                process
                    .tick_stale
                    .then(|| "process tick is stale".to_string())
            });
        diagnostics.push(diagnostic(
            "process",
            "process",
            &process.name,
            &process.state,
            severity,
            reason,
            None,
            process.last_seen_unix_ms,
            process.tick_count.or(clock_ms),
            vec![
                metric("pid", process.pid),
                metric("restart_count", process.restart_count),
                metric("tick_count", process.tick_count),
                metric("tick_stale", process.tick_stale),
                metric("exit_code", process.exit_code),
            ],
        ));
    }

    for resource in &status.resources {
        let satisfied = resource
            .satisfied
            .unwrap_or(matches!(resource.state.as_str(), "ready"));
        let severity = match resource.state.as_str() {
            "failed" => "error",
            "degraded" | "pending" if resource.required => "warn",
            _ if resource.required && !satisfied => "error",
            _ => "info",
        };
        diagnostics.push(diagnostic(
            "resource",
            "resource",
            &resource.name,
            &resource.state,
            severity,
            resource
                .last_error
                .clone()
                .or_else(|| resource.diagnostic.clone())
                .or_else(|| resource.contract_status.clone()),
            resource.suggestion.clone(),
            resource.updated_unix_ms,
            clock_ms,
            vec![
                metric("capability", resource.capability.clone()),
                metric("required", resource.required),
                metric("readiness", resource.readiness.clone()),
                metric("health", resource.health.clone()),
                metric("on_failure", resource.on_failure.clone()),
                metric("satisfied", resource.satisfied),
                metric("provider", resource.provider.clone()),
            ],
        ));
    }

    for boundary in &status.io_boundaries {
        let severity = if !boundary.healthy {
            "error"
        } else if !boundary.ready {
            "warn"
        } else {
            "info"
        };
        diagnostics.push(diagnostic(
            "io_boundary",
            "io_boundary",
            &boundary.name,
            if boundary.ready && boundary.healthy {
                "ready"
            } else if !boundary.healthy {
                "unhealthy"
            } else {
                "not_ready"
            },
            severity,
            boundary.last_error.clone(),
            None,
            boundary.updated_unix_ms,
            clock_ms,
            vec![
                metric("ready", boundary.ready),
                metric("healthy", boundary.healthy),
                metric("resource_count", boundary.resources.len()),
            ],
        ));
        for resource in &boundary.resources {
            diagnostics.push(diagnostic(
                "io_boundary",
                "io_boundary_resource",
                &format!("{}.{}", boundary.name, resource.name),
                if resource.ready { "ready" } else { "not_ready" },
                if resource.ready { "info" } else { "warn" },
                resource
                    .last_error
                    .clone()
                    .or_else(|| resource.message.clone()),
                None,
                resource.updated_unix_ms,
                clock_ms,
                vec![
                    metric("kind", resource.kind.clone()),
                    metric("ready", resource.ready),
                ],
            ));
        }
    }

    for param in &status.params {
        diagnostics.push(diagnostic(
            "param",
            "param",
            &param.name,
            &param.apply_state,
            if param.apply_state == "rejected" {
                "error"
            } else if param.pending.is_some() {
                "warn"
            } else {
                "info"
            },
            param.last_reject_reason.clone(),
            None,
            param.updated_unix_ms,
            clock_ms,
            vec![
                metric("update", param.update.clone()),
                metric("pending", param.pending.clone()),
                metric("current", param.current.clone()),
            ],
        ));
    }

    for service in &status.services {
        let failures = service.timeout_count
            + service.busy_count
            + service.unavailable_count
            + service.late_drop_count;
        diagnostics.push(diagnostic(
            "service",
            "service",
            &service.name,
            if service.ready {
                "ready"
            } else {
                "unavailable"
            },
            if !service.ready || failures > 0 {
                "warn"
            } else {
                "info"
            },
            (!service.ready)
                .then(|| "service endpoint is not ready".to_string())
                .or_else(|| (failures > 0).then(|| "service has failed requests".to_string())),
            None,
            None,
            clock_ms,
            vec![
                metric("in_flight", service.in_flight),
                metric("queued", service.queued),
                metric("total_requests", service.total_requests),
                metric("timeout_count", service.timeout_count),
                metric("busy_count", service.busy_count),
                metric("unavailable_count", service.unavailable_count),
                metric("late_drop_count", service.late_drop_count),
            ],
        ));
    }

    for operation in &status.operations {
        let failed = operation.failed_count + operation.timeout_count + operation.preempted_count;
        diagnostics.push(diagnostic(
            "operation",
            "operation",
            &operation.name,
            operation
                .current_state
                .as_deref()
                .unwrap_or(if operation.ready {
                    "ready"
                } else {
                    "unavailable"
                }),
            if operation.last_error.is_some() || failed > 0 {
                "error"
            } else if operation.running > 0 || operation.queued > 0 {
                "warn"
            } else {
                "info"
            },
            operation
                .last_error
                .clone()
                .or_else(|| operation.last_event.clone()),
            None,
            operation.last_transition_ms,
            operation.current_deadline_ms.or(clock_ms),
            vec![
                metric("ready", operation.ready),
                metric("running", operation.running),
                metric("queued", operation.queued),
                metric("total_started", operation.total_started),
                metric("succeeded_count", operation.succeeded_count),
                metric("failed_count", operation.failed_count),
                metric("timeout_count", operation.timeout_count),
                metric("current_deadline_ms", operation.current_deadline_ms),
            ],
        ));
    }

    for task in &status.tasks {
        let has_failure = task.consecutive_failures > 0;
        let has_runtime_timing_issue = task.lateness_ms.unwrap_or(0) > 0
            || task.missed_periods.unwrap_or(0) > 0
            || task.overrun.unwrap_or(false);
        let has_counter_issue = task.deadline_missed > 0
            || task.stale_input > 0
            || task.backpressure > 0
            || task.overflow > 0;
        let has_timing_issue = has_runtime_timing_issue || has_counter_issue;
        diagnostics.push(diagnostic(
            "task",
            "task",
            &task.name,
            if has_failure {
                "failing"
            } else if has_timing_issue {
                "degraded"
            } else {
                "ok"
            },
            if has_failure {
                "error"
            } else if has_timing_issue {
                "warn"
            } else {
                "info"
            },
            has_failure
                .then(|| "task has consecutive failures".to_string())
                .or_else(|| {
                    has_runtime_timing_issue
                        .then(|| "runtime observed task timing issue".to_string())
                })
                .or_else(|| {
                    has_counter_issue.then(|| "task timing/input counters are non-zero".to_string())
                }),
            has_runtime_timing_issue.then(|| {
                "timing is runtime-observed scheduler time, not a hard realtime guarantee"
                    .to_string()
            }),
            task.last_run_ms.or(task.observed_time_ms),
            task.observed_time_ms.or(task.last_run_ms).or(clock_ms),
            vec![
                metric("lane", task.lane.clone()),
                metric("inflight", task.inflight),
                metric("scheduled_time_ms", task.scheduled_time_ms),
                metric("observed_time_ms", task.observed_time_ms),
                metric("lateness_ms", task.lateness_ms),
                metric("missed_periods", task.missed_periods),
                metric("overrun", task.overrun),
                metric("deadline_missed", task.deadline_missed),
                metric("stale_input", task.stale_input),
                metric("backpressure", task.backpressure),
                metric("overflow", task.overflow),
                metric("run_count", task.run_count),
                metric("success_count", task.success_count),
                metric("consecutive_failures", task.consecutive_failures),
            ],
        ));
    }

    diagnostics
}

#[allow(clippy::too_many_arguments)]
fn diagnostic(
    category: &str,
    entity_kind: &str,
    entity_id: &str,
    state: &str,
    severity: &str,
    reason: Option<String>,
    suggestion: Option<String>,
    updated_unix_ms: Option<u64>,
    observed_ms: Option<u64>,
    metrics: Vec<IntrospectionDiagnosticMetric>,
) -> IntrospectionDiagnostic {
    IntrospectionDiagnostic {
        category: category.to_string(),
        entity_kind: entity_kind.to_string(),
        entity_id: entity_id.to_string(),
        state: state.to_string(),
        severity: severity.to_string(),
        reason,
        suggestion,
        updated_unix_ms,
        observed_ms,
        metrics,
    }
}

fn metric(name: &str, value: impl Serialize) -> IntrospectionDiagnosticMetric {
    IntrospectionDiagnosticMetric {
        name: name.to_string(),
        value: serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
    }
}
pub(super) fn input_status_key(status: &IntrospectionInputStatus) -> String {
    if status.task.is_empty() {
        status.input.clone()
    } else if status.input.is_empty() {
        status.task.clone()
    } else {
        format!("{}.{}", status.task, status.input)
    }
}
