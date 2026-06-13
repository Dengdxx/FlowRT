use std::collections::BTreeSet;

use flowrt_ir::{ChannelBackendSource, ContractIr, GraphIr, InstanceIr, Ros2BridgeDirection};

use crate::runtime_plan::{
    BindRuntimePlan, BridgeRuntimePlan, active_binds_for_instances, runtime_channel_message_type,
    runtime_channel_name, runtime_channel_probe_capacity,
};
use crate::rust_shell::backend_emit::input_revision_local;

pub(super) fn emit_rust_introspection_helpers(
    include_channel_helpers: bool,
    include_param_decode: bool,
) -> String {
    let mut output = String::new();
    if include_channel_helpers {
        output.push_str(
            r#"fn register_introspection_channel(
    state: &flowrt::IntrospectionState,
    name: &'static str,
    message_type: &'static str,
    max_payload_len: Option<usize>,
) -> flowrt::IntrospectionChannelProbe {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.register_channel_with_probe_capacity(name, message_type, max_payload_len);
        state.channel_probe(name).unwrap_or_default()
    }))
    .unwrap_or_default()
}

"#,
        );
    }
    if include_param_decode {
        output.push_str(
            r#"fn decode_flowrt_param_value<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, serde_json::Error> {
    serde_json::from_value(value)
}

"#,
        );
    }
    if include_channel_helpers {
        output.push_str(
            r#"#[allow(dead_code)]
fn record_introspection_input_read<T>(
    state: &flowrt::IntrospectionState,
    key: &'static str,
    task: &'static str,
    input: &'static str,
    channel: &'static str,
    message_type: &'static str,
    value: &flowrt::Latest<'_, T>,
    revision: u64,
    tick_time_ms: u64,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.record_input_read(
            key,
            task,
            input,
            channel,
            message_type,
            value.present(),
            value.stale(),
            Some(revision),
            Some(tick_time_ms),
        );
    }));
}

#[allow(dead_code)]
fn record_introspection_publish_copy<T: Copy>(
    state: &flowrt::IntrospectionState,
    name: &'static str,
    message_type: &'static str,
    probe: &flowrt::IntrospectionChannelProbe,
    value: &T,
    published_at_ms: u64,
) {
    probe.record_publish_event();
    if !probe.enabled() && !state.recorder_enabled_for_channel(name) {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let payload = unsafe {
            std::slice::from_raw_parts(
                (value as *const T).cast::<u8>(),
                std::mem::size_of::<T>(),
            )
        };
        state.try_record_channel_sample_bytes(name, message_type, payload, Some(published_at_ms));
        if probe.enabled() {
            probe.try_record_bytes(payload, Some(published_at_ms));
        }
    }));
}

#[allow(dead_code)]
fn record_introspection_publish_frame<T: flowrt::FrameCodec>(
    state: &flowrt::IntrospectionState,
    name: &'static str,
    message_type: &'static str,
    probe: &flowrt::IntrospectionChannelProbe,
    value: &T,
    published_at_ms: u64,
) {
    probe.record_publish_event();
    if !probe.enabled() && !state.recorder_enabled_for_channel(name) {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Ok(payload) = value.to_frame_vec() {
            state.try_record_channel_sample_frame_bytes(
                name,
                message_type,
                &payload,
                Some(published_at_ms),
            );
            if probe.enabled() {
                probe.try_record_bytes(&payload, Some(published_at_ms));
            }
        }
    }));
}

"#,
        );
    }
    output
}

pub(super) fn emit_rust_introspection_channel_registration(
    contract: &ContractIr,
    graph: &GraphIr,
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    let active_instances = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();
    for bind in active_binds_for_instances(binds, order) {
        let channel_name = runtime_channel_name(bind);
        let message_type = runtime_channel_message_type(bind);
        output.push_str(&format!(
            "        let {probe} = register_introspection_channel(&introspection_state, {}, {}, {});\n        let _ = self.{probe}.set({probe});\n",
            crate::rust_string_literal(&channel_name),
            crate::rust_string_literal(&message_type),
            rust_optional_usize_literal(runtime_channel_probe_capacity(contract, bind)),
            probe = bind.probe_field_name
        ));
        output.push_str(&format!(
            "        introspection_state.register_route(flowrt::IntrospectionRouteStatus {{\n            name: {name}.to_string(),\n            from: {from}.to_string(),\n            to: {to}.to_string(),\n            message_type: {message_type}.to_string(),\n            backend: {backend}.to_string(),\n            selected_reason: {selected_reason}.to_string(),\n            ..Default::default()\n        }});\n",
            name = crate::rust_string_literal(&channel_name),
            from = crate::rust_string_literal(&format!("{}.{}", bind.source_instance, bind.source_port)),
            to = crate::rust_string_literal(&format!("{}.{}", bind.target_instance, bind.target_port)),
            message_type = crate::rust_string_literal(&message_type),
            backend = crate::rust_string_literal(bind.backend.0.as_str()),
            selected_reason = crate::rust_string_literal(route_selected_reason(bind)),
        ));
        if active_instances.contains(bind.target_instance.as_str()) {
            for task in graph.tasks.iter().filter(|task| {
                task.instance.name == bind.target_instance
                    && task.inputs.contains(&bind.target_port)
            }) {
                let task_name = format!("{}.{}", task.instance.name, task.name);
                output.push_str(&format!(
                    "        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {{\n            task: {task}.to_string(),\n            input: {input}.to_string(),\n            channel: {channel}.to_string(),\n            message_type: {message_type}.to_string(),\n            ..Default::default()\n        }});\n",
                    task = crate::rust_string_literal(&task_name),
                    input = crate::rust_string_literal(&bind.target_port),
                    channel = crate::rust_string_literal(&channel_name),
                    message_type = crate::rust_string_literal(&message_type),
                ));
            }
        }
    }
    output
}

pub(super) fn emit_rust_introspection_bridge_registration(
    graph: &GraphIr,
    order: &[&InstanceIr],
    bridges: &[BridgeRuntimePlan],
) -> String {
    let mut output = String::new();
    let active_instances = order
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<BTreeSet<_>>();
    for bridge in bridges
        .iter()
        .filter(|bridge| active_instances.contains(bridge.source_instance.as_str()))
    {
        let (from, to) = bridge_route_endpoints(bridge);
        let message_type = bridge.source_type.canonical_syntax();
        output.push_str(&format!(
            "        introspection_state.register_route(flowrt::IntrospectionRouteStatus {{\n            name: {name}.to_string(),\n            from: {from}.to_string(),\n            to: {to}.to_string(),\n            message_type: {message_type}.to_string(),\n            backend: \"zenoh\".to_string(),\n            selected_reason: \"ros2_bridge\".to_string(),\n            ..Default::default()\n        }});\n",
            name = crate::rust_string_literal(&bridge.name),
            from = crate::rust_string_literal(&from),
            to = crate::rust_string_literal(&to),
            message_type = crate::rust_string_literal(&message_type),
        ));
        if bridge.direction == Ros2BridgeDirection::Ros2ToFlowrt {
            for task in graph.tasks.iter().filter(|task| {
                task.instance.name == bridge.source_instance
                    && task.inputs.contains(&bridge.source_port)
            }) {
                let task_name = format!("{}.{}", task.instance.name, task.name);
                output.push_str(&format!(
                    "        introspection_state.record_input_status(flowrt::IntrospectionInputStatus {{\n            task: {task}.to_string(),\n            input: {input}.to_string(),\n            channel: {channel}.to_string(),\n            message_type: {message_type}.to_string(),\n            ..Default::default()\n        }});\n",
                    task = crate::rust_string_literal(&task_name),
                    input = crate::rust_string_literal(&bridge.source_port),
                    channel = crate::rust_string_literal(&bridge.name),
                    message_type = crate::rust_string_literal(&message_type),
                ));
            }
        }
    }
    output
}

pub(super) fn rust_input_read_record_for_bind(
    task: &flowrt_ir::TaskIr,
    input: &flowrt_ir::PortIr,
    bind: &BindRuntimePlan,
) -> String {
    rust_input_read_record(
        task,
        input,
        &runtime_channel_name(bind),
        &runtime_channel_message_type(bind),
        &input_revision_local(input),
    )
}

pub(super) fn rust_input_read_record_for_bridge(
    task: &flowrt_ir::TaskIr,
    input: &flowrt_ir::PortIr,
    bridge: &crate::runtime_plan::BridgeRuntimePlan,
) -> String {
    rust_input_read_record(
        task,
        input,
        &bridge.name,
        &bridge.source_type.canonical_syntax(),
        &input_revision_local(input),
    )
}

fn rust_input_read_record(
    task: &flowrt_ir::TaskIr,
    input: &flowrt_ir::PortIr,
    channel: &str,
    message_type: &str,
    revision_expr: &str,
) -> String {
    let task_name = format!("{}.{}", task.instance.name, task.name);
    let key = format!("{task_name}.{}", input.name);
    format!(
        "        record_introspection_input_read(&introspection_state, {key}, {task}, {input_name}, {channel}, {message_type}, &{input}, {revision}, tick_time_ms);\n",
        key = crate::rust_string_literal(&key),
        task = crate::rust_string_literal(&task_name),
        input_name = crate::rust_string_literal(&input.name),
        channel = crate::rust_string_literal(channel),
        message_type = crate::rust_string_literal(message_type),
        input = input.name,
        revision = revision_expr,
    )
}

fn rust_optional_usize_literal(value: Option<usize>) -> String {
    value.map_or_else(|| "None".to_string(), |value| format!("Some({value})"))
}

fn route_selected_reason(bind: &BindRuntimePlan) -> &'static str {
    match bind.backend_source {
        ChannelBackendSource::Explicit => "explicit",
        ChannelBackendSource::ProfileDefault => "profile_default",
        ChannelBackendSource::AutoFallback => "variable_frame_auto_fallback",
    }
}

fn bridge_route_endpoints(bridge: &BridgeRuntimePlan) -> (String, String) {
    let flowrt_endpoint = bridge.boundary_endpoint.as_ref().map_or_else(
        || format!("{}.{}", bridge.source_instance, bridge.source_port),
        |endpoint| format!("boundary:{endpoint}"),
    );
    let ros2_endpoint = format!("ros2:{}", bridge.ros2_topic);
    match bridge.direction {
        Ros2BridgeDirection::FlowrtToRos2 => (flowrt_endpoint, ros2_endpoint),
        Ros2BridgeDirection::Ros2ToFlowrt => (ros2_endpoint, flowrt_endpoint),
    }
}
