use flowrt_ir::{ContractIr, InstanceIr};

use crate::runtime_plan::{
    BindRuntimePlan, active_binds_for_instances, runtime_channel_message_type,
    runtime_channel_name, runtime_channel_probe_capacity,
};

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
            state.try_record_channel_sample_bytes(
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
    order: &[&InstanceIr],
    binds: &[BindRuntimePlan],
) -> String {
    let mut output = String::new();
    for bind in active_binds_for_instances(binds, order) {
        output.push_str(&format!(
            "        self.{probe} = register_introspection_channel(&introspection_state, {}, {}, {});\n",
            crate::rust_string_literal(&runtime_channel_name(bind)),
            crate::rust_string_literal(&runtime_channel_message_type(bind)),
            rust_optional_usize_literal(runtime_channel_probe_capacity(contract, bind)),
            probe = bind.probe_field_name
        ));
    }
    output
}

fn rust_optional_usize_literal(value: Option<usize>) -> String {
    value.map_or_else(|| "None".to_string(), |value| format!("Some({value})"))
}
