#pragma once

#include <cstdint>
#include <flowrt/introspection/model.hpp>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace flowrt {

namespace detail {

inline std::string json_string(std::string_view value) {
    static constexpr char kHex[] = "0123456789abcdef";
    std::string output;
    output.reserve(value.size() + 2);
    output.push_back('"');
    for (const unsigned char byte : value) {
        switch (byte) {
            case '"':
                output.append("\\\"");
                break;
            case '\\':
                output.append("\\\\");
                break;
            case '\b':
                output.append("\\b");
                break;
            case '\f':
                output.append("\\f");
                break;
            case '\n':
                output.append("\\n");
                break;
            case '\r':
                output.append("\\r");
                break;
            case '\t':
                output.append("\\t");
                break;
            default:
                if (byte < 0x20U) {
                    output.append("\\u00");
                    output.push_back(kHex[(byte >> 4U) & 0x0FU]);
                    output.push_back(kHex[byte & 0x0FU]);
                } else {
                    output.push_back(static_cast<char>(byte));
                }
                break;
        }
    }
    output.push_back('"');
    return output;
}

inline std::string handshake_json(const IntrospectionHandshake &handshake) {
    std::string output;
    output.append("{\"protocol_version\":");
    output.append(json_string(handshake.protocol_version));
    output.append(",\"pid\":");
    output.append(std::to_string(handshake.pid));
    output.append(",\"started_at_unix_ms\":");
    output.append(std::to_string(handshake.started_at_unix_ms));
    output.append(",\"self_description_hash\":");
    output.append(json_string(handshake.self_description_hash));
    output.append(",\"package\":");
    output.append(json_string(handshake.package));
    output.append(",\"process\":");
    output.append(json_string(handshake.process));
    output.append(",\"runtime\":");
    output.append(json_string(handshake.runtime));
    output.push_back('}');
    return output;
}

inline std::string channel_status_json(const IntrospectionChannelStatus &channel) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(channel.name));
    output.append(",\"message_type\":");
    output.append(json_string(channel.message_type));
    output.append(",\"published_count\":");
    output.append(std::to_string(channel.published_count));
    output.append(",\"last_payload_len\":");
    output.append(channel.last_payload_len ? std::to_string(*channel.last_payload_len) : "null");
    output.append(",\"active_observers\":");
    output.append(std::to_string(channel.active_observers));
    output.append(",\"dropped_samples\":");
    output.append(std::to_string(channel.dropped_samples));
    output.push_back('}');
    return output;
}

inline std::string service_status_json(const IntrospectionServiceStatus &service) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(service.name));
    output.append(",\"ready\":");
    output.append(service.ready ? "true" : "false");
    output.append(",\"in_flight\":");
    output.append(std::to_string(service.in_flight));
    output.append(",\"queued\":");
    output.append(std::to_string(service.queued));
    output.append(",\"total_requests\":");
    output.append(std::to_string(service.total_requests));
    output.append(",\"timeout_count\":");
    output.append(std::to_string(service.timeout_count));
    output.append(",\"busy_count\":");
    output.append(std::to_string(service.busy_count));
    output.append(",\"unavailable_count\":");
    output.append(std::to_string(service.unavailable_count));
    output.append(",\"late_drop_count\":");
    output.append(std::to_string(service.late_drop_count));
    output.push_back('}');
    return output;
}

inline std::string boundary_resource_status_json(const BoundaryResourceStatus &resource) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(resource.name));
    output.append(",\"kind\":");
    output.append(json_string(resource.kind));
    output.append(",\"ready\":");
    output.append(resource.ready ? "true" : "false");
    output.append(",\"message\":");
    output.append(resource.message ? json_string(*resource.message) : "null");
    output.append(",\"last_error\":");
    output.append(resource.last_error ? json_string(*resource.last_error) : "null");
    output.append(",\"updated_unix_ms\":");
    output.append(resource.updated_unix_ms ? std::to_string(*resource.updated_unix_ms) : "null");
    output.push_back('}');
    return output;
}

inline std::string boundary_status_json(const BoundaryStatus &boundary) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(boundary.name));
    output.append(",\"component\":");
    output.append(json_string(boundary.component));
    output.append(",\"ready\":");
    output.append(boundary.ready ? "true" : "false");
    output.append(",\"healthy\":");
    output.append(boundary.healthy ? "true" : "false");
    output.append(",\"last_error\":");
    output.append(boundary.last_error ? json_string(*boundary.last_error) : "null");
    output.append(",\"resources\":[");
    for (std::size_t index = 0; index < boundary.resources.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(boundary_resource_status_json(boundary.resources[index]));
    }
    output.append("],\"updated_unix_ms\":");
    output.append(boundary.updated_unix_ms ? std::to_string(*boundary.updated_unix_ms) : "null");
    output.push_back('}');
    return output;
}

inline std::string resource_status_json(const IntrospectionResourceStatus &resource) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(resource.name));
    output.append(",\"capability\":");
    output.append(json_string(resource.capability));
    output.append(",\"access\":");
    output.append(resource.access ? json_string(*resource.access) : "null");
    output.append(",\"state\":");
    output.append(json_string(resource.state));
    output.append(",\"required\":");
    output.append(resource.required ? "true" : "false");
    output.append(",\"readiness\":");
    output.append(resource.readiness ? json_string(*resource.readiness) : "null");
    output.append(",\"health\":");
    output.append(resource.health ? json_string(*resource.health) : "null");
    output.append(",\"on_failure\":");
    output.append(resource.on_failure ? json_string(*resource.on_failure) : "null");
    output.append(",\"contract_status\":");
    output.append(resource.contract_status ? json_string(*resource.contract_status) : "null");
    output.append(",\"satisfied\":");
    output.append(resource.satisfied ? (*resource.satisfied ? "true" : "false") : "null");
    output.append(",\"provider\":");
    output.append(resource.provider ? json_string(*resource.provider) : "null");
    output.append(",\"provider_scope\":");
    output.append(resource.provider_scope ? json_string(*resource.provider_scope) : "null");
    output.append(",\"provider_readiness_source\":");
    output.append(resource.provider_readiness_source
                      ? json_string(*resource.provider_readiness_source)
                      : "null");
    output.append(",\"provider_health_source\":");
    output.append(resource.provider_health_source ? json_string(*resource.provider_health_source)
                                                  : "null");
    output.append(",\"diagnostic\":");
    output.append(resource.diagnostic ? json_string(*resource.diagnostic) : "null");
    output.append(",\"suggestion\":");
    output.append(resource.suggestion ? json_string(*resource.suggestion) : "null");
    output.append(",\"source\":");
    output.append(resource.source ? json_string(*resource.source) : "null");
    output.append(",\"owner_process\":");
    output.append(resource.owner_process ? json_string(*resource.owner_process) : "null");
    output.append(",\"last_error\":");
    output.append(resource.last_error ? json_string(*resource.last_error) : "null");
    output.append(",\"updated_unix_ms\":");
    output.append(resource.updated_unix_ms ? std::to_string(*resource.updated_unix_ms) : "null");
    output.push_back('}');
    return output;
}

inline std::string optional_u64_json(const std::optional<std::uint64_t> &value) {
    return value ? std::to_string(*value) : "null";
}

inline std::string optional_bool_json(const std::optional<bool> &value) {
    return value ? (*value ? "true" : "false") : "null";
}

inline std::string json_string_array(const std::vector<std::string> &values) {
    std::string output;
    output.push_back('[');
    for (std::size_t index = 0; index < values.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(json_string(values[index]));
    }
    output.push_back(']');
    return output;
}

inline std::string recorder_status_json(const IntrospectionRecorderStatus &recorder) {
    std::string output;
    output.append("{\"enabled\":");
    output.append(recorder.enabled ? "true" : "false");
    output.append(",\"output\":");
    output.append(recorder.output ? json_string(*recorder.output) : "null");
    output.append(",\"dropped_count\":");
    output.append(std::to_string(recorder.dropped_count));
    output.append(",\"bytes_written\":");
    output.append(std::to_string(recorder.bytes_written));
    output.append(",\"active_filters\":");
    output.append(json_string_array(recorder.active_filters));
    output.append(",\"queued_events\":");
    output.append(std::to_string(recorder.queued_events));
    output.push_back('}');
    return output;
}

inline std::string boundary_publish_status_json(const IntrospectionBoundaryPublishStatus &status) {
    std::string output;
    output.append("{\"endpoint\":");
    output.append(json_string(status.endpoint));
    output.append(",\"message_type\":");
    output.append(json_string(status.message_type));
    output.append(",\"revision\":");
    output.append(std::to_string(status.revision));
    output.append(",\"published_at_ms\":");
    output.append(status.published_at_ms ? std::to_string(*status.published_at_ms) : "null");
    output.push_back('}');
    return output;
}

inline std::string operation_status_json(const IntrospectionOperationStatus &operation) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(operation.name));
    output.append(",\"ready\":");
    output.append(operation.ready ? "true" : "false");
    output.append(",\"running\":");
    output.append(std::to_string(operation.running));
    output.append(",\"queued\":");
    output.append(std::to_string(operation.queued));
    output.append(",\"current_operation_ids\":");
    output.append(json_string_array(operation.current_operation_ids));
    output.append(",\"total_started\":");
    output.append(std::to_string(operation.total_started));
    output.append(",\"succeeded_count\":");
    output.append(std::to_string(operation.succeeded_count));
    output.append(",\"failed_count\":");
    output.append(std::to_string(operation.failed_count));
    output.append(",\"canceled_count\":");
    output.append(std::to_string(operation.canceled_count));
    output.append(",\"timeout_count\":");
    output.append(std::to_string(operation.timeout_count));
    output.append(",\"preempted_count\":");
    output.append(std::to_string(operation.preempted_count));
    output.append(",\"current_state\":");
    output.append(operation.current_state ? json_string(*operation.current_state) : "null");
    output.append(",\"current_owner\":");
    output.append(operation.current_owner ? json_string(*operation.current_owner) : "null");
    output.append(",\"current_deadline_ms\":");
    output.append(optional_u64_json(operation.current_deadline_ms));
    output.append(",\"last_event\":");
    output.append(operation.last_event ? json_string(*operation.last_event) : "null");
    output.append(",\"last_error\":");
    output.append(operation.last_error ? json_string(*operation.last_error) : "null");
    output.append(",\"last_transition_ms\":");
    output.append(optional_u64_json(operation.last_transition_ms));
    output.push_back('}');
    return output;
}

inline std::string task_health_json(const IntrospectionTaskHealth &task) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(task.name));
    output.append(",\"lane\":");
    output.append(json_string(task.lane));
    output.append(",\"inflight\":");
    output.append(task.inflight ? "true" : "false");
    output.append(",\"scheduled_time_ms\":");
    output.append(optional_u64_json(task.scheduled_time_ms));
    output.append(",\"observed_time_ms\":");
    output.append(optional_u64_json(task.observed_time_ms));
    output.append(",\"lateness_ms\":");
    output.append(optional_u64_json(task.lateness_ms));
    output.append(",\"missed_periods\":");
    output.append(optional_u64_json(task.missed_periods));
    output.append(",\"overrun\":");
    output.append(optional_bool_json(task.overrun));
    output.append(",\"deadline_missed\":");
    output.append(std::to_string(task.deadline_missed));
    output.append(",\"stale_input\":");
    output.append(std::to_string(task.stale_input));
    output.append(",\"backpressure\":");
    output.append(std::to_string(task.backpressure));
    output.append(",\"overflow\":");
    output.append(std::to_string(task.overflow));
    output.append(",\"fairness_violations\":");
    output.append(std::to_string(task.fairness_violations));
    output.append(",\"run_count\":");
    output.append(std::to_string(task.run_count));
    output.append(",\"success_count\":");
    output.append(std::to_string(task.success_count));
    output.append(",\"consecutive_failures\":");
    output.append(std::to_string(task.consecutive_failures));
    output.append(",\"last_run_ms\":");
    output.append(optional_u64_json(task.last_run_ms));
    output.append(",\"last_success_ms\":");
    output.append(optional_u64_json(task.last_success_ms));
    output.push_back('}');
    return output;
}

inline std::string lane_health_json(const IntrospectionLaneHealth &lane) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(lane.name));
    output.append(",\"queue_depth\":");
    output.append(std::to_string(lane.queue_depth));
    output.append(",\"dispatched_count\":");
    output.append(std::to_string(lane.dispatched_count));
    output.append(",\"fairness_violations\":");
    output.append(std::to_string(lane.fairness_violations));
    output.push_back('}');
    return output;
}

inline std::string clock_status_json(const IntrospectionClockStatus &clock) {
    std::string output;
    output.append("{\"source\":");
    output.append(json_string(clock.source));
    output.append(",\"tick_time_ms\":");
    output.append(optional_u64_json(clock.tick_time_ms));
    output.append(",\"unit\":");
    output.append(json_string(clock.unit));
    output.append(",\"field\":");
    output.append(json_string(clock.field));
    output.push_back('}');
    return output;
}

inline std::string optional_json_fragment(const std::optional<std::string> &value) {
    return value ? *value : "null";
}

inline std::string json_fragment_array(const std::vector<std::string> &values) {
    std::string output;
    output.push_back('[');
    for (std::size_t index = 0; index < values.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(values[index]);
    }
    output.push_back(']');
    return output;
}

inline std::string param_status_json(const IntrospectionParamStatus &param) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(param.name));
    output.append(",\"type\":");
    output.append(json_string(param.ty));
    output.append(",\"update\":");
    output.append(json_string(param.update));
    output.append(",\"current\":");
    output.append(param.current);
    output.append(",\"pending\":");
    output.append(optional_json_fragment(param.pending));
    output.append(",\"apply_state\":");
    output.append(json_string(param.apply_state));
    output.append(",\"last_reject_reason\":");
    output.append(param.last_reject_reason ? json_string(*param.last_reject_reason) : "null");
    output.append(",\"updated_unix_ms\":");
    output.append(optional_u64_json(param.updated_unix_ms));
    output.append(",\"min\":");
    output.append(optional_json_fragment(param.min));
    output.append(",\"max\":");
    output.append(optional_json_fragment(param.max));
    output.append(",\"choices\":");
    output.append(json_fragment_array(param.choices));
    output.push_back('}');
    return output;
}

inline std::string diagnostic_metric_json(const IntrospectionDiagnosticMetric &metric) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(metric.name));
    output.append(",\"value\":");
    output.append(metric.value);
    output.push_back('}');
    return output;
}

inline std::string diagnostic_status_json(const IntrospectionDiagnostic &diagnostic) {
    std::string output;
    output.append("{\"category\":");
    output.append(json_string(diagnostic.category));
    output.append(",\"entity_kind\":");
    output.append(json_string(diagnostic.entity_kind));
    output.append(",\"entity_id\":");
    output.append(json_string(diagnostic.entity_id));
    output.append(",\"state\":");
    output.append(json_string(diagnostic.state));
    output.append(",\"severity\":");
    output.append(json_string(diagnostic.severity));
    output.append(",\"reason\":");
    output.append(diagnostic.reason ? json_string(*diagnostic.reason) : "null");
    output.append(",\"suggestion\":");
    output.append(diagnostic.suggestion ? json_string(*diagnostic.suggestion) : "null");
    output.append(",\"updated_unix_ms\":");
    output.append(optional_u64_json(diagnostic.updated_unix_ms));
    output.append(",\"observed_ms\":");
    output.append(optional_u64_json(diagnostic.observed_ms));
    output.append(",\"metrics\":[");
    for (std::size_t index = 0; index < diagnostic.metrics.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(diagnostic_metric_json(diagnostic.metrics[index]));
    }
    output.append("]}");
    return output;
}

inline std::string status_json(const IntrospectionStatus &status) {
    std::string output;
    output.append("{\"tick_count\":");
    output.append(std::to_string(status.tick_count));
    output.append(",\"clock\":");
    output.append(clock_status_json(status.clock));
    output.append(",\"recorder\":");
    output.append(recorder_status_json(status.recorder));
    output.append(",\"channels\":[");
    for (std::size_t index = 0; index < status.channels.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(channel_status_json(status.channels[index]));
    }
    output.append("],\"inputs\":[],\"routes\":[],\"processes\":[],\"resources\":[");
    for (std::size_t index = 0; index < status.resources.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(resource_status_json(status.resources[index]));
    }
    output.append("],\"io_boundaries\":[");
    for (std::size_t index = 0; index < status.io_boundaries.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(boundary_status_json(status.io_boundaries[index]));
    }
    output.append("],\"params\":[");
    for (std::size_t index = 0; index < status.params.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(param_status_json(status.params[index]));
    }
    output.append("],\"services\":[");
    for (std::size_t index = 0; index < status.services.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(service_status_json(status.services[index]));
    }
    output.append("],\"operations\":[");
    for (std::size_t index = 0; index < status.operations.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(operation_status_json(status.operations[index]));
    }
    output.append("],\"tasks\":[");
    for (std::size_t index = 0; index < status.tasks.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(task_health_json(status.tasks[index]));
    }
    output.append("],\"lanes\":[");
    for (std::size_t index = 0; index < status.lanes.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(lane_health_json(status.lanes[index]));
    }
    output.append("],\"diagnostics\":[");
    for (std::size_t index = 0; index < status.diagnostics.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(diagnostic_status_json(status.diagnostics[index]));
    }
    output.append("]}");
    return output;
}

inline std::string param_list_response_json(const IntrospectionHandshake &handshake,
                                            const std::vector<IntrospectionParamStatus> &params) {
    std::string output;
    output.append("{\"response\":\"param_list\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"params\":[");
    for (std::size_t index = 0; index < params.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(param_status_json(params[index]));
    }
    output.append("]}");
    return output;
}

inline std::string param_value_response_json(const IntrospectionHandshake &handshake,
                                             const IntrospectionParamStatus &param) {
    std::string output;
    output.append("{\"response\":\"param_value\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"param\":");
    output.append(param_status_json(param));
    output.push_back('}');
    return output;
}

inline std::string boundary_publish_response_json(
    const IntrospectionHandshake &handshake, const IntrospectionBoundaryPublishStatus &boundary) {
    std::string output;
    output.append("{\"response\":\"boundary_publish\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"boundary\":");
    output.append(boundary_publish_status_json(boundary));
    output.push_back('}');
    return output;
}

inline std::string operation_value_response_json(const IntrospectionHandshake &handshake,
                                                 const IntrospectionOperationStatus &operation) {
    std::string output;
    output.append("{\"response\":\"operation_value\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"operation\":");
    output.append(operation_status_json(operation));
    output.push_back('}');
    return output;
}

inline std::string self_description_response_json(const IntrospectionHandshake &handshake,
                                                  std::string_view json) {
    std::string output;
    output.append("{\"response\":\"self_description\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"json\":");
    output.append(json_string(json));
    output.push_back('}');
    return output;
}

inline std::string payload_json(const std::optional<std::vector<std::uint8_t>> &payload) {
    if (!payload) {
        return "null";
    }
    std::string output;
    output.push_back('[');
    for (std::size_t index = 0; index < payload->size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(std::to_string(static_cast<unsigned int>((*payload)[index])));
    }
    output.push_back(']');
    return output;
}

inline std::string recorder_entity_json(const IntrospectionRecorderEvent &event) {
    std::string output;
    output.append("{\"kind\":");
    output.append(json_string(event.entity_kind));
    output.append(",\"name\":");
    output.append(json_string(event.entity_name));
    if (event.entity_instance) {
        output.append(",\"instance\":");
        output.append(json_string(*event.entity_instance));
    }
    if (event.entity_task) {
        output.append(",\"task\":");
        output.append(json_string(*event.entity_task));
    }
    if (event.entity_type_name) {
        output.append(",\"type_name\":");
        output.append(json_string(*event.entity_type_name));
    }
    output.push_back('}');
    return output;
}

inline std::string recorder_event_json(const IntrospectionRecorderEvent &event) {
    std::string output;
    output.append("{\"schema_version\":");
    output.append(std::to_string(event.schema_version));
    output.append(",\"event_kind\":");
    output.append(json_string(event.event_kind));
    output.append(",\"package\":");
    output.append(json_string(event.package));
    output.append(",\"process\":");
    output.append(json_string(event.process));
    output.append(",\"runtime_pid\":");
    output.append(std::to_string(event.runtime_pid));
    output.append(",\"selfdesc_hash\":");
    output.append(json_string(event.selfdesc_hash));
    output.append(",\"monotonic_ns\":");
    output.append(std::to_string(event.monotonic_ns));
    output.append(",\"wall_unix_ns\":");
    output.append(std::to_string(event.wall_unix_ns));
    output.append(",\"sequence\":");
    output.append(std::to_string(event.sequence));
    output.append(",\"entity\":");
    output.append(recorder_entity_json(event));
    output.append(",\"payload_encoding\":");
    output.append(json_string(event.payload_encoding));
    output.append(",\"payload_schema\":");
    output.append(json_string(event.payload_schema));
    output.append(",\"payload\":");
    output.append(payload_json(std::optional<std::vector<std::uint8_t>>{event.payload}));
    output.push_back('}');
    return output;
}

inline std::string channel_snapshot_json(const IntrospectionChannelSnapshot &channel) {
    std::string output;
    output.append("{\"published_count\":");
    output.append(std::to_string(channel.published_count));
    output.append(",\"payload\":");
    output.append(payload_json(channel.payload));
    output.append(",\"published_at_ms\":");
    output.append(channel.published_at_ms ? std::to_string(*channel.published_at_ms) : "null");
    output.push_back('}');
    return output;
}

inline std::string status_response_json(const IntrospectionHandshake &handshake,
                                        const IntrospectionStatus &status) {
    std::string output;
    output.append("{\"response\":\"status\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"status\":");
    output.append(status_json(status));
    output.push_back('}');
    return output;
}

inline std::string recorder_value_response_json(const IntrospectionHandshake &handshake,
                                                const IntrospectionRecorderStatus &recorder) {
    std::string output;
    output.append("{\"response\":\"recorder_value\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"recorder\":");
    output.append(recorder_status_json(recorder));
    output.push_back('}');
    return output;
}

inline std::string recorder_events_response_json(
    const IntrospectionHandshake &handshake, const IntrospectionRecorderStatus &recorder,
    const std::vector<IntrospectionRecorderEvent> &events) {
    std::string output;
    output.append("{\"response\":\"recorder_events\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"recorder\":");
    output.append(recorder_status_json(recorder));
    output.append(",\"events\":[");
    for (std::size_t index = 0; index < events.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(recorder_event_json(events[index]));
    }
    output.append("]}");
    return output;
}

inline std::string channel_snapshot_response_json(const IntrospectionHandshake &handshake,
                                                  const IntrospectionChannelSnapshot &channel) {
    std::string output;
    output.append("{\"response\":\"channel_snapshot\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"channel\":");
    output.append(channel_snapshot_json(channel));
    output.push_back('}');
    return output;
}

inline std::string observe_ready_response_json(const IntrospectionHandshake &handshake,
                                               const IntrospectionChannelStatus &channel) {
    std::string output;
    output.append("{\"response\":\"observe_ready\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"channel\":");
    output.append(channel_status_json(channel));
    output.push_back('}');
    return output;
}

inline std::string error_response_json(const IntrospectionHandshake &handshake,
                                       std::string_view message) {
    std::string output;
    output.append("{\"response\":\"error\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"message\":");
    output.append(json_string(message));
    output.push_back('}');
    return output;
}

inline bool json_whitespace(char byte) noexcept {
    return byte == ' ' || byte == '\t' || byte == '\n' || byte == '\r';
}

}  // namespace detail

}  // namespace flowrt
