use super::*;

#[test]
fn rust_shell_registers_active_channels_and_records_publish_snapshots() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.sensor_source]
language = "rust"
output = ["sample:Sample"]

[component.sensor_sink]
language = "rust"
input = ["sample:Sample"]

[component.aux_source]
language = "rust"
output = ["sample:Sample"]

[component.aux_sink]
language = "rust"
input = ["sample:Sample"]

[instance.sensor_source]
component = "sensor_source"
process = "sensors"
target = "linux"

[instance.sensor_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sensor_sink]
component = "sensor_sink"
process = "control"
target = "linux"

[instance.sensor_sink.task]
trigger = "on_message"
input = ["sample"]

[instance.aux_source]
component = "aux_source"
process = "aux"
target = "linux"

[instance.aux_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.aux_sink]
component = "aux_sink"
process = "aux"
target = "linux"

[instance.aux_sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "sensor_source.sample"
to = "sensor_sink.sample"
channel = "latest"

[[bind.dataflow]]
from = "aux_source.sample"
to = "aux_sink.sample"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let sensor_channel = "sensor_source.sample_to_sensor_sink.sample";
    let aux_channel = "aux_source.sample_to_aux_sink.sample";

    let sensors_run = generated_function_block(rust_shell, "fn run_process_sensors");
    let sensor_register_marker = format!(
        " = register_introspection_channel(&introspection_state, {}, \"Sample\", Some(4));",
        rust_string_literal(sensor_channel)
    );
    let sensor_probe = extract_probe_field_for_registration(sensors_run, &sensor_register_marker)
        .expect("sensors process should register sensor channel");
    assert!(
        sensors_run
            .contains("introspection_state.register_route(flowrt::IntrospectionRouteStatus {")
    );
    assert!(sensors_run.contains(&format!(
        "name: {}.to_string(),",
        rust_string_literal(sensor_channel)
    )));
    assert!(sensors_run.contains("backend: \"iox2\".to_string(),"));
    assert!(sensors_run.contains("selected_reason: \"profile_default\".to_string(),"));
    assert!(
        !sensors_run
            .contains("introspection_state.record_input_status(flowrt::IntrospectionInputStatus {")
    );
    let control_run = generated_function_block(rust_shell, "fn run_process_control");
    assert!(
        control_run
            .contains("introspection_state.record_input_status(flowrt::IntrospectionInputStatus {")
    );
    assert!(control_run.contains("task: \"sensor_sink.main\".to_string(),"));
    assert!(control_run.contains("input: \"sample\".to_string(),"));
    let aux_register_marker = format!(
        "register_introspection_channel(&introspection_state, {}, \"Sample\", Some(4));",
        rust_string_literal(aux_channel)
    );
    assert!(
        !sensors_run.contains(&aux_register_marker),
        "sensors process should not register aux channel:\n{sensors_run}"
    );
    let sensor_record = format!(
        "record_introspection_publish_copy(&introspection_state, {channel}, \"Sample\", &self.{sensor_probe}, &value, tick_time_ms);",
        channel = rust_string_literal(sensor_channel)
    );
    assert!(rust_shell.contains(&sensor_record));
    assert!(rust_shell.contains(
        "if !probe.enabled() && !state.recorder_enabled_for_channel(name) {\n        return;\n    }"
    ));
    assert!(rust_shell.contains(
        "state.try_record_channel_sample_bytes(name, message_type, payload, Some(published_at_ms));"
    ));
    assert!(rust_shell.contains(&format!(
        "record_introspection_input_read(&introspection_state, \"sensor_sink.main.sample\", \"sensor_sink.main\", \"sample\", {}, \"Sample\", &sample, __flowrt_sample_revision, tick_time_ms);",
        rust_string_literal(sensor_channel)
    )));
    assert!(rust_shell.contains(&format!(
        "introspection_state.record_route_publish({}, Some(tick_time_ms));",
        rust_string_literal(sensor_channel)
    )));
    let sensor_record_at = rust_shell.find(&sensor_record).unwrap();
    let sensor_before_record = &rust_shell[..sensor_record_at];
    assert!(sensor_before_record.contains(".publish_at(value.clone(), tick_time_ms)"));
}

#[test]
fn probe_capacity_uses_message_abi_size_including_padding() {
    let ir = contract_from_source(
        r#"
[package]
name = "padding_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"
ay = "f32"
az = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains(
        "register_introspection_channel(&introspection_state, \"source.imu_to_sink.imu\", \"Imu\", Some(24));"
    ));
}

#[test]
fn frame_descriptor_resource_schema_enters_self_description_without_payload_opt_in_by_default() {
    let ir = contract_from_source(
        r#"
[package]
name = "descriptor_demo"
rsdl_version = "0.1"

[type.FrameHandle]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"
timestamp_unix_ns = "u64"
width = "u32"
height = "u32"
stride_bytes = "u32"
format_id = "u32"
encoding_id = "u32"
flags = "u32"

[component.camera]
language = "rust"
kind = "io_boundary"
io_side_effect = ["device", "read"]
output = ["frame:FrameHandle"]

[component.camera.resource.frames]
kind = "shm"

[component.camera.resource.frames.descriptor]
kind = "frame"
port = "frame"
format = "rgb8"
encoding = "row_major"
metadata = { width = "640", height = "480" }

[instance.camera]
component = "camera"

[instance.camera.task]
trigger = "periodic"
period_ms = 33
output = ["frame"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    let descriptor = &selfdesc["component_types"][0]["resources"][0]["descriptor"];

    assert_eq!(descriptor["kind"], "frame");
    assert_eq!(descriptor["port"], "frame");
    assert_eq!(descriptor["format"], "rgb8");
    assert_eq!(descriptor["encoding"], "row_major");
    assert_eq!(descriptor["metadata"]["width"], "640");
    assert_eq!(descriptor["record_payload"], false);
}

#[test]
fn rust_shell_omits_channel_helpers_when_process_has_no_channels() {
    let ir = contract_from_source(
        r#"
[package]
name = "no_channel_demo"
rsdl_version = "0.1"

[type.Counter]
value = "u32"

[component.worker]
language = "rust"
output = ["count:Counter"]

[instance.worker]
component = "worker"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
output = ["count"]

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("flowrt::spawn_status_server("));
    assert!(!rust_shell.contains("fn register_introspection_channel("));
    assert!(!rust_shell.contains("fn record_introspection_publish_copy"));
    assert!(!rust_shell.contains("fn record_introspection_publish_frame"));
}

#[test]
fn cpp_shell_registers_active_channels_and_records_publish_snapshots() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.sensor_source]
language = "cpp"
output = ["sample:Sample"]

[component.sensor_sink]
language = "cpp"
input = ["sample:Sample"]

[component.aux_source]
language = "cpp"
output = ["sample:Sample"]

[component.aux_sink]
language = "cpp"
input = ["sample:Sample"]

[instance.sensor_source]
component = "sensor_source"
process = "sensors"
target = "linux"

[instance.sensor_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sensor_sink]
component = "sensor_sink"
process = "sensors"
target = "linux"

[instance.sensor_sink.task]
trigger = "on_message"
input = ["sample"]

[instance.aux_source]
component = "aux_source"
process = "aux"
target = "linux"

[instance.aux_source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.aux_sink]
component = "aux_sink"
process = "aux"
target = "linux"

[instance.aux_sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "sensor_source.sample"
to = "sensor_sink.sample"
channel = "latest"

[[bind.dataflow]]
from = "aux_source.sample"
to = "aux_sink.sample"
channel = "fifo"
depth = 2
overflow = "drop_oldest"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let cpp_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    let sensor_channel = "sensor_source.sample_to_sensor_sink.sample";
    let aux_channel = "aux_source.sample_to_aux_sink.sample";

    assert!(cpp_shell.contains("flowrt::IntrospectionState introspection_state;"));
    assert!(cpp_shell.contains(
            "introspection_state.set_self_description_json(std::string{flowrt_app::self_description_json()});"
        ));
    assert!(cpp_header.contains("flowrt::IntrospectionChannelProbe introspection_probe_bind_"));
    assert!(cpp_shell.contains("flowrt::spawn_status_server("));
    assert!(cpp_shell.contains("flowrt_app::self_description_hash()"));
    assert!(cpp_shell.contains("runtime = \"cpp\""));
    assert!(cpp_shell.contains("introspection_state.record_tick();"));

    let sensors_run = generated_function_block(cpp_shell, "App::run_process_sensors");
    let sensor_register_marker = format!(
        " = register_introspection_channel(introspection_state, {}, \"Sample\", std::optional<std::size_t>{{4}});",
        cpp_string_literal(sensor_channel)
    );
    let sensor_probe = extract_probe_field_for_registration(sensors_run, &sensor_register_marker)
        .expect("sensors process should register sensor channel");
    let aux_register_marker = format!(
        "register_introspection_channel(introspection_state, {}, \"Sample\", std::optional<std::size_t>{{4}});",
        cpp_string_literal(aux_channel)
    );
    assert!(
        !sensors_run.contains(&aux_register_marker),
        "sensors process should not register aux channel:\n{sensors_run}"
    );
    let sensor_record = format!(
        "record_introspection_publish_copy(introspection_state, {channel}, \"Sample\", this->{sensor_probe}, *value, tick_time_ms);",
        channel = cpp_string_literal(sensor_channel)
    );
    assert!(cpp_shell.contains(&sensor_record));
    assert!(cpp_shell.contains(
        "if (!probe.enabled() && !state.recorder_enabled_for_channel(name)) {\n        return;\n    }"
    ));
    assert!(cpp_shell.contains("state.try_record_channel_sample_bytes("));
    assert!(cpp_shell.contains("message_type,\n            payload,"));
    let sensor_record_at = cpp_shell.find(&sensor_record).unwrap();
    let sensor_before_record = &cpp_shell[..sensor_record_at];
    assert!(sensor_before_record.contains("publish_at(*value, tick_time_ms)"));

    let aux_run = generated_function_block(cpp_shell, "App::run_process_aux");
    let aux_register_marker = format!(
        " = register_introspection_channel(introspection_state, {}, \"Sample\", std::optional<std::size_t>{{4}});",
        cpp_string_literal(aux_channel)
    );
    let aux_probe = extract_probe_field_for_registration(aux_run, &aux_register_marker)
        .expect("aux process should register aux channel");
    let aux_record = format!(
        "record_introspection_publish_copy(introspection_state, {channel}, \"Sample\", this->{aux_probe}, *value, tick_time_ms);",
        channel = cpp_string_literal(aux_channel)
    );
    assert!(cpp_shell.contains(&aux_record));
    let aux_record_at = cpp_shell.find(&aux_record).unwrap();
    let aux_before_record = &cpp_shell[..aux_record_at];
    assert!(aux_before_record.contains("push_at(*value, tick_time_ms)"));
    assert!(aux_before_record.contains("ChannelWriteOutcome::DroppedOldest"));
}
