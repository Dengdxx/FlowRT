use flowrt_ir::{
    ChannelEdgeIr, ChannelKind, ContractIr, GraphIr, OverflowPolicy as IrOverflowPolicy,
    StalePolicy as IrStalePolicy,
};

use crate::messages::rust_type;
use crate::runtime_plan::{
    BindRuntimePlan, BridgeRuntimePlan, bind_backend, runtime_channel_message_type,
    runtime_channel_name,
};
use crate::{flowrt_path_part, flowrt_topic_path_part};

pub(super) fn runtime_channel_type(bind: &BindRuntimePlan) -> String {
    let ty = rust_type(&bind.source_type);
    if bind_backend(bind) == "iox2" {
        return format!("flowrt::iox2::Iox2PubSub<{ty}>");
    }
    if bind_backend(bind) == "zenoh" {
        return format!("flowrt::zenoh::ZenohPubSub<{ty}>");
    }

    match bind.channel {
        ChannelKind::Latest => format!("flowrt::LatestChannel<{ty}>"),
        ChannelKind::Fifo => format!("flowrt::FifoChannel<{ty}>"),
    }
}

pub(super) fn bridge_runtime_channel_type(bridge: &BridgeRuntimePlan) -> String {
    format!(
        "flowrt::zenoh::ZenohPubSub<{}>",
        rust_type(&bridge.source_type)
    )
}

pub(super) fn runtime_channel_initializer(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
) -> String {
    if bind_backend(bind) == "iox2" {
        let service_name = crate::rust_string_literal(&iox2_service_name(contract, graph, bind));
        let config = iox2_channel_config_expr(bind);
        return format!(
            "match flowrt::iox2::Iox2PubSub::open_with_config({service_name}, {config}) {{\n                Ok(channel) => channel,\n                Err(error) => {{\n                    eprintln!(\"FlowRT: failed to open iox2 channel {{}}: {{error}}\", {service_name});\n                    startup_status = flowrt::Status::Error;\n                    flowrt::iox2::Iox2PubSub::unavailable({service_name}, {config}, error.to_string())\n                }}\n            }}",
        );
    }
    if bind_backend(bind) == "zenoh" {
        let key_expr = crate::rust_string_literal(&zenoh_key_expr(contract, graph, bind));
        let config = zenoh_channel_config_expr(bind);
        return format!(
            "match flowrt::zenoh::ZenohPubSub::open_with_config({key_expr}, {config}) {{\n                Ok(channel) => channel,\n                Err(error) => {{\n                    eprintln!(\"FlowRT: failed to open zenoh channel {{}}: {{error}}\", {key_expr});\n                    startup_status = flowrt::Status::Error;\n                    flowrt::zenoh::ZenohPubSub::unavailable({key_expr}, {config}, error.to_string())\n                }}\n            }}",
        );
    }

    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::LatestChannel::with_stale_config({})",
            runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => runtime_fifo_channel_initializer(bind),
    }
}

pub(super) fn bridge_runtime_channel_initializer(
    contract: &ContractIr,
    graph: &GraphIr,
    bridge: &BridgeRuntimePlan,
) -> String {
    let key_expr = crate::rust_string_literal(&ros2_bridge_key_expr(contract, graph, bridge));
    let config = "flowrt::zenoh::ZenohChannelConfig::latest()";
    format!(
        "match flowrt::zenoh::ZenohPubSub::open_with_config({key_expr}, {config}) {{\n            Ok(channel) => channel,\n            Err(error) => {{\n                eprintln!(\"FlowRT: failed to open ROS2 bridge zenoh channel {{}}: {{error}}\", {key_expr});\n                startup_status = flowrt::Status::Error;\n                flowrt::zenoh::ZenohPubSub::unavailable({key_expr}, {config}, error.to_string())\n            }}\n        }}",
    )
}

pub(super) fn runtime_channel_read(
    input: &flowrt_ir::PortIr,
    bind: &BindRuntimePlan,
    use_cached_transport: bool,
) -> String {
    let revision = input_revision_local(input);
    let guard = format!("__flowrt_{}_guard", bind.field_name);
    if matches!(bind_backend(bind), "iox2" | "zenoh") {
        if use_cached_transport {
            return format!(
                "        let {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n        let {input} = {guard}.cached_latest_at(tick_time_ms);\n        let {revision} = {guard}.revision();\n",
                input = input.name,
                field = bind.field_name,
                revision = revision,
                guard = guard,
            );
        }
        return format!(
            "        let mut {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n        let ({input}, {revision}) = match {guard}.receive_latest_with_revision_at(tick_time_ms) {{\n            Ok(value) => value,\n            Err(_) => return flowrt::Status::Error,\n        }};\n",
            input = input.name,
            field = bind.field_name,
            revision = revision,
            guard = guard,
        );
    }

    match bind.channel {
        ChannelKind::Latest => {
            format!(
                "        let {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n        let {input} = {guard}.view_at(tick_time_ms);\n        let {revision} = {guard}.revision();\n",
                input = input.name,
                field = bind.field_name,
                revision = revision,
                guard = guard,
            )
        }
        ChannelKind::Fifo => {
            format!(
                "        let mut {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n        let {input}_read = {guard}.pop_at(tick_time_ms);\n        let {revision} = {guard}.revision();\n        let {input} = {input}_read.view();\n",
                input = input.name,
                field = bind.field_name,
                revision = revision,
                guard = guard,
            )
        }
    }
}

/// 生成 channel 写入代码，带健康计数器记录。
///
/// 当 `task_health_name` 为 `Some` 时，FIFO 的 backpressure 和 overflow 事件
/// 会被记录到 `health_map`。
pub(super) fn runtime_channel_write_with_health(
    bind: &BindRuntimePlan,
    task_health_name: &str,
) -> String {
    runtime_channel_write_inner(bind, Some(task_health_name))
}

pub(super) fn runtime_channel_commit_with_health(
    bind: &BindRuntimePlan,
    task_health_name: &str,
) -> String {
    let body = runtime_channel_write_inner(bind, Some(task_health_name)).replace("self.", "app.");
    let health_arg = if body.contains("health_map") {
        "health_map"
    } else {
        "_health_map"
    };
    format!(
        "            let value = value.clone();\n            __flowrt_output_commits.push(Box::new(move |app, introspection_state, scheduler_events, {health_arg}| {{\n{body}                flowrt::Status::Ok\n            }}));\n"
    )
}

fn runtime_channel_write_inner(bind: &BindRuntimePlan, task_health_name: Option<&str>) -> String {
    let introspection_record = runtime_introspection_publish_record(bind);
    let route_name = crate::rust_string_literal(&runtime_channel_name(bind));
    if matches!(bind_backend(bind), "iox2" | "zenoh") {
        return format!(
            "            if let Err(error) = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_at(value.clone(), tick_time_ms) {{\n                introspection_state.record_route_error({route_name}, error.to_string());\n                return flowrt::Status::Error;\n            }}\n            introspection_state.record_route_publish({route_name}, Some(tick_time_ms));\n            scheduler_events.notify_data();\n{introspection_record}",
            field = bind.field_name,
            route_name = route_name,
        );
    }

    match bind.channel {
        ChannelKind::Latest => {
            format!(
                "            self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_at(value.clone(), tick_time_ms);\n            introspection_state.record_route_publish({route_name}, Some(tick_time_ms));\n            scheduler_events.notify_data();\n{introspection_record}",
                field = bind.field_name,
                route_name = route_name,
            )
        }
        ChannelKind::Fifo => {
            if let Some(task_health) = task_health_name {
                format!(
                    "            match self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {{\n                Ok(flowrt::ChannelWriteOutcome::Accepted) => {{\n                    introspection_state.record_route_publish({route_name}, Some(tick_time_ms));\n                    scheduler_events.notify_data();\n{introspection_record}                }}\n                Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {{\n                    introspection_state.record_route_publish({route_name}, Some(tick_time_ms));\n                    introspection_state.record_route_drop({route_name});\n                    scheduler_events.notify_data();\n{introspection_record}                }}\n                Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {{\n                    introspection_state.record_route_drop({route_name});\n                }}\n                Ok(flowrt::ChannelWriteOutcome::Backpressured) => {{\n                    introspection_state.record_route_backpressure({route_name});\n                    health_map.entry({task_health:?}.to_string()).or_default().backpressure += 1;\n                    return flowrt::Status::Retry;\n                }}\n                Err(flowrt::ChannelError::Overflow) => {{\n                    introspection_state.record_route_overflow({route_name});\n                    health_map.entry({task_health:?}.to_string()).or_default().overflow += 1;\n                    return flowrt::Status::Error;\n                }}\n            }}\n",
                    field = bind.field_name,
                    task_health = task_health,
                    route_name = route_name,
                )
            } else {
                format!(
                    "            match self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push_at(value.clone(), tick_time_ms) {{\n                Ok(flowrt::ChannelWriteOutcome::Accepted) => {{\n                    introspection_state.record_route_publish({route_name}, Some(tick_time_ms));\n                    scheduler_events.notify_data();\n{introspection_record}                }}\n                Ok(flowrt::ChannelWriteOutcome::DroppedOldest) => {{\n                    introspection_state.record_route_publish({route_name}, Some(tick_time_ms));\n                    introspection_state.record_route_drop({route_name});\n                    scheduler_events.notify_data();\n{introspection_record}                }}\n                Ok(flowrt::ChannelWriteOutcome::DroppedNewest) => {{\n                    introspection_state.record_route_drop({route_name});\n                }}\n                Ok(flowrt::ChannelWriteOutcome::Backpressured) => {{\n                    introspection_state.record_route_backpressure({route_name});\n                    return flowrt::Status::Retry;\n                }}\n                Err(flowrt::ChannelError::Overflow) => {{\n                    introspection_state.record_route_overflow({route_name});\n                    return flowrt::Status::Error;\n                }}\n            }}\n",
                    field = bind.field_name,
                    route_name = route_name,
                )
            }
        }
    }
}

pub(super) fn bridge_runtime_channel_write(bridge: &BridgeRuntimePlan) -> String {
    let route_name = crate::rust_string_literal(&bridge.name);
    format!(
        "            if let Err(error) = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_at(value.clone(), tick_time_ms) {{\n                introspection_state.record_route_error({route_name}, error.to_string());\n                return flowrt::Status::Error;\n            }}\n            introspection_state.record_route_publish({route_name}, Some(tick_time_ms));\n",
        field = bridge.field_name,
        route_name = route_name,
    )
}

pub(super) fn bridge_runtime_channel_commit(bridge: &BridgeRuntimePlan) -> String {
    let route_name = crate::rust_string_literal(&bridge.name);
    format!(
        "            let value = value.clone();\n            __flowrt_output_commits.push(Box::new(move |app, introspection_state, _scheduler_events, _health_map| {{\n                if let Err(error) = app.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).publish_at(value.clone(), tick_time_ms) {{\n                    introspection_state.record_route_error({route_name}, error.to_string());\n                    return flowrt::Status::Error;\n                }}\n                introspection_state.record_route_publish({route_name}, Some(tick_time_ms));\n                flowrt::Status::Ok\n            }}));\n",
        field = bridge.field_name,
        route_name = route_name,
    )
}

pub(super) fn bridge_runtime_channel_read(
    input: &flowrt_ir::PortIr,
    bridge: &BridgeRuntimePlan,
    use_cached_transport: bool,
) -> String {
    let revision = input_revision_local(input);
    let guard = format!("__flowrt_{}_guard", bridge.field_name);
    if use_cached_transport {
        return format!(
            "        let {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n        let {input} = {guard}.cached_latest_at(tick_time_ms);\n        let {revision} = {guard}.revision();\n",
            input = input.name,
            field = bridge.field_name,
            revision = revision,
            guard = guard,
        );
    }
    format!(
        "        let mut {guard} = self.{field}.lock().unwrap_or_else(|poisoned| poisoned.into_inner());\n        let ({input}, {revision}) = match {guard}.receive_latest_with_revision_at(tick_time_ms) {{\n            Ok(value) => value,\n            Err(_) => return flowrt::Status::Error,\n        }};\n",
        input = input.name,
        field = bridge.field_name,
        revision = revision,
        guard = guard,
    )
}

pub(super) fn input_revision_local(input: &flowrt_ir::PortIr) -> String {
    format!("__flowrt_{}_revision", input.name)
}

pub(crate) fn iox2_service_name(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
) -> String {
    iox2_service_name_from_parts(
        &contract.package.name,
        &graph.name,
        bind.index,
        &bind.source_instance,
        &bind.source_port,
        &bind.target_instance,
        &bind.target_port,
    )
}

pub(crate) fn zenoh_key_expr(
    contract: &ContractIr,
    graph: &GraphIr,
    bind: &BindRuntimePlan,
) -> String {
    zenoh_key_expr_from_parts(
        "flowrt",
        &contract.package.name,
        &selected_profile_name(contract),
        &graph.name,
        bind.index,
        &bind.source_instance,
        &bind.source_port,
        &bind.target_instance,
        &bind.target_port,
    )
}

pub(crate) fn zenoh_key_expr_for_edge(
    contract: &ContractIr,
    graph: &GraphIr,
    index: usize,
    bind: &ChannelEdgeIr,
) -> String {
    zenoh_key_expr_from_parts(
        "flowrt",
        &contract.package.name,
        &selected_profile_name(contract),
        &graph.name,
        index,
        &bind.from.instance.name,
        &bind.from.port,
        &bind.to.instance.name,
        &bind.to.port,
    )
}

pub(crate) fn ros2_bridge_key_expr(
    contract: &ContractIr,
    graph: &GraphIr,
    bridge: &BridgeRuntimePlan,
) -> String {
    let (source_instance, source_port) = ros2_bridge_flowrt_key_parts(bridge);
    ros2_bridge_key_expr_from_parts(
        &contract.package.name,
        &selected_profile_name(contract),
        &graph.name,
        bridge.index,
        &source_instance,
        &source_port,
        &bridge.ros2_topic,
    )
}

fn ros2_bridge_flowrt_key_parts(bridge: &BridgeRuntimePlan) -> (String, String) {
    bridge.boundary_endpoint.as_ref().map_or_else(
        || (bridge.source_instance.clone(), bridge.source_port.clone()),
        |endpoint| ("boundary".to_string(), endpoint.clone()),
    )
}

pub(crate) fn iox2_service_name_for_edge(
    contract: &ContractIr,
    graph: &GraphIr,
    index: usize,
    bind: &ChannelEdgeIr,
) -> String {
    iox2_service_name_from_parts(
        &contract.package.name,
        &graph.name,
        index,
        &bind.from.instance.name,
        &bind.from.port,
        &bind.to.instance.name,
        &bind.to.port,
    )
}

pub(crate) fn selected_backend_name(contract: &ContractIr) -> String {
    contract
        .profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| contract.profiles.first())
        .map(|profile| profile.backend.0.clone())
        .unwrap_or_else(|| "inproc".to_string())
}

fn selected_profile_name(contract: &ContractIr) -> String {
    contract
        .profiles
        .iter()
        .find(|profile| profile.name == "default")
        .or_else(|| contract.profiles.first())
        .map(|profile| profile.name.clone())
        .unwrap_or_else(|| "default".to_string())
}

fn runtime_introspection_publish_record(bind: &BindRuntimePlan) -> String {
    let helper = if bind.source_uses_variable_frame || bind_backend(bind) == "zenoh" {
        "record_introspection_publish_frame"
    } else {
        "record_introspection_publish_copy"
    };
    format!(
        "            if let Some({probe}_probe) = self.{probe}.get() {{\n                {helper}(&introspection_state, {channel}, {message_type}, {probe}_probe, &value, tick_time_ms);\n            }}\n",
        channel = crate::rust_string_literal(&runtime_channel_name(bind)),
        message_type = crate::rust_string_literal(&runtime_channel_message_type(bind)),
        probe = bind.probe_field_name
    )
}

fn zenoh_channel_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::zenoh::ZenohChannelConfig::latest().with_stale_config({})",
            runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => format!(
            "flowrt::zenoh::ZenohChannelConfig::fifo({}, {}).with_stale_config({})",
            bind.depth.unwrap_or(1),
            runtime_overflow_policy(bind.overflow),
            runtime_stale_config_expr(bind)
        ),
    }
}

fn runtime_fifo_channel_initializer(bind: &BindRuntimePlan) -> String {
    let depth = bind.depth.unwrap_or(1);
    let overflow = runtime_overflow_policy(bind.overflow);
    if bind.max_age_ms.is_none() && bind.stale == IrStalePolicy::Warn {
        return format!("flowrt::FifoChannel::new({depth}, {overflow})");
    }

    format!(
        "flowrt::FifoChannel::with_stale_config({}, {}, {})",
        depth,
        overflow,
        runtime_stale_config_expr(bind)
    )
}

fn iox2_channel_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.channel {
        ChannelKind::Latest => format!(
            "flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config({})",
            runtime_stale_config_expr(bind)
        ),
        ChannelKind::Fifo => format!(
            "flowrt::iox2::Iox2ChannelConfig::fifo({}, {}).with_stale_config({})",
            bind.depth.unwrap_or(1),
            runtime_overflow_policy(bind.overflow),
            runtime_stale_config_expr(bind)
        ),
    }
}

fn runtime_stale_config_expr(bind: &BindRuntimePlan) -> String {
    match bind.max_age_ms {
        Some(max_age_ms) => format!(
            "flowrt::StaleConfig::new(Some({max_age_ms}), {})",
            runtime_stale_policy(bind.stale)
        ),
        None => format!(
            "flowrt::StaleConfig::new(None, {})",
            runtime_stale_policy(bind.stale)
        ),
    }
}

fn runtime_overflow_policy(policy: IrOverflowPolicy) -> &'static str {
    match policy {
        IrOverflowPolicy::DropOldest => "flowrt::OverflowPolicy::DropOldest",
        IrOverflowPolicy::DropNewest => "flowrt::OverflowPolicy::DropNewest",
        IrOverflowPolicy::Error => "flowrt::OverflowPolicy::Error",
        IrOverflowPolicy::Block => "flowrt::OverflowPolicy::Block",
    }
}

fn runtime_stale_policy(policy: IrStalePolicy) -> &'static str {
    match policy {
        IrStalePolicy::Warn => "flowrt::StalePolicy::Warn",
        IrStalePolicy::Drop => "flowrt::StalePolicy::Drop",
        IrStalePolicy::HoldLast => "flowrt::StalePolicy::HoldLast",
        IrStalePolicy::Error => "flowrt::StalePolicy::Error",
    }
}

fn ros2_bridge_key_expr_from_parts(
    package: &str,
    profile: &str,
    graph: &str,
    index: usize,
    source_instance: &str,
    source_port: &str,
    ros2_topic: &str,
) -> String {
    format!(
        "flowrt/{}/{}/{}/ros2_bridge_{}/{}_{}_to_{}",
        flowrt_path_part(package),
        flowrt_path_part(profile),
        flowrt_path_part(graph),
        index,
        flowrt_path_part(source_instance),
        flowrt_path_part(source_port),
        flowrt_topic_path_part(ros2_topic),
    )
}

#[allow(clippy::too_many_arguments)]
fn zenoh_key_expr_from_parts(
    namespace: &str,
    package: &str,
    profile: &str,
    graph: &str,
    index: usize,
    source_instance: &str,
    source_port: &str,
    target_instance: &str,
    target_port: &str,
) -> String {
    format!(
        "{}/{}/{}/{}/bind_{}/{}_{}_to_{}_{}",
        flowrt_path_part(namespace),
        flowrt_path_part(package),
        flowrt_path_part(profile),
        flowrt_path_part(graph),
        index,
        flowrt_path_part(source_instance),
        flowrt_path_part(source_port),
        flowrt_path_part(target_instance),
        flowrt_path_part(target_port),
    )
}

fn iox2_service_name_from_parts(
    package: &str,
    graph: &str,
    index: usize,
    source_instance: &str,
    source_port: &str,
    target_instance: &str,
    target_port: &str,
) -> String {
    format!(
        "FlowRT/{}/{}/bind_{}/{}_{}_to_{}_{}",
        flowrt_path_part(package),
        flowrt_path_part(graph),
        index,
        flowrt_path_part(source_instance),
        flowrt_path_part(source_port),
        flowrt_path_part(target_instance),
        flowrt_path_part(target_port),
    )
}
