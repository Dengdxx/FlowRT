use super::*;

#[test]
fn inserts_implicit_default_profile_when_source_omits_profiles() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(ir.profiles.len(), 1);
    assert_eq!(ir.profiles[0].name, "default");
    assert_eq!(ir.profiles[0].backend.0, "inproc");
    assert_eq!(ir.deployments.len(), 1);
    assert_eq!(ir.deployments[0].profile.name, "default");
    assert_eq!(ir.deployments[0].backend.0, "inproc");
    assert!(ir.deployments[0].satisfied);
}

#[test]
fn normalizes_global_tick_determinism_profile() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[instance.controller]
component = "controller"
process = "controller_proc"

[[process]]
name = "controller_proc"

[profile.test]
backend = "inproc"

[profile.test.determinism]
mode = "global_tick"
timeout_ms = 1000
on_timeout = "fault_graph"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let profile = &ir.profiles[0];

    assert_eq!(profile.determinism.mode, DeterminismMode::GlobalTick);
    assert_eq!(profile.determinism.timeout_ms, Some(1000));
    assert_eq!(
        profile.determinism.on_timeout,
        DeterminismTimeoutPolicy::FaultGraph
    );
    assert_eq!(profile.determinism.processes, vec!["controller_proc"]);
}

#[test]
fn omits_default_process_local_determinism_from_canonical_json() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let json = ir.to_canonical_json().unwrap();

    assert_eq!(
        ir.profiles[0].determinism.mode,
        DeterminismMode::ProcessLocal
    );
    assert!(
        !json.contains("determinism"),
        "default process_local determinism should not be serialized: {json}"
    );
}

#[test]
fn rejects_instance_param_overrides_with_incompatible_types() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = 1.0
enabled = true
gains = [1.0, 2.0]

[component.controller.params.limits]
max = 10

[instance.controller]
component = "controller"

[instance.controller.params]
kp = "fast"
enabled = 1
gains = [true]

[instance.controller.params.limits]
max = false
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("incompatible parameter overrides should fail");

    assert!(matches!(
        error,
        IrError::IncompatibleParamOverride {
            instance,
            component,
            ..
        } if instance == "controller" && component == "controller"
    ));
}

#[test]
fn allows_non_empty_array_override_for_empty_default_array() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
gains = []

[instance.controller]
component = "controller"

[instance.controller.params]
gains = [true]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let instance = &ir.graphs[0].instances[0];

    assert_eq!(
        instance.params[0].value,
        ParamValue::Array(vec![ParamValue::Bool(true)])
    );
}

#[test]
fn rejects_non_finite_parameter_values() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = nan
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("non-finite parameter default should fail");

    assert!(matches!(
        error,
        IrError::InvalidParamSchema {
            component,
            param,
            message,
        } if component == "controller" && param == "kp" && message.contains("finite")
    ));
}

#[test]
fn rejects_wide_integer_default_above_float_max_without_rounding() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
limit = { type = "f64", default = 9007199254740993, max = 9007199254740992.0 }
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("wide integer should be compared to float max without f64 rounding");

    assert!(matches!(
        error,
        IrError::InvalidParamSchema {
            component,
            param,
            message,
        } if component == "controller" && param == "limit" && message.contains("above")
    ));
}

#[test]
fn rejects_parameter_enum_choice_outside_schema_range() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
gain = { type = "u8", default = 1, min = 0, max = 10, enum = [1, 20] }
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("enum choices must obey declared min/max constraints");

    assert!(matches!(
        error,
        IrError::InvalidParamSchema {
            component,
            param,
            message,
        } if component == "controller" && param == "gain" && message.contains("above")
    ));
}

#[test]
fn normalizes_params_schema_and_legacy_defaults() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }
mode = { type = "string", default = "normal", enum = ["normal", "safe"], update = "on_tick" }
legacy_gain = 2.0

[instance.controller]
component = "controller"

[instance.controller.params]
kp = 2.5
mode = "safe"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let component = &ir.components[0];

    assert_eq!(component.params[0].name, "kp");
    assert_eq!(component.params[0].ty, ParamType::F32);
    assert_eq!(component.params[0].default, ParamValue::Float(1.0));
    assert_eq!(component.params[0].update, ParamUpdatePolicy::OnTick);
    assert_eq!(component.params[0].min, Some(ParamValue::Float(0.0)));
    assert_eq!(component.params[0].max, Some(ParamValue::Float(10.0)));

    assert_eq!(component.params[1].name, "legacy_gain");
    assert_eq!(component.params[1].ty, ParamType::F64);
    assert_eq!(component.params[1].update, ParamUpdatePolicy::Startup);

    assert_eq!(component.params[2].name, "mode");
    assert_eq!(component.params[2].ty, ParamType::String);
    assert_eq!(component.params[2].choices.len(), 2);
    assert_eq!(component.params[2].update, ParamUpdatePolicy::OnTick);

    let instance = &ir.graphs[0].instances[0];
    assert_eq!(instance.params[0].value, ParamValue::Float(2.5));
    assert_eq!(instance.params[1].value, ParamValue::Float(2.0));
    assert_eq!(
        instance.params[2].value,
        ParamValue::String("safe".to_string())
    );
}

#[test]
fn rejects_parameter_override_outside_schema_range() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.params]
kp = 12.0
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("out-of-range parameter override should fail");

    assert!(matches!(
        error,
        IrError::InvalidParamSchema {
            component,
            param,
            ..
        } if component == "controller" && param == "kp"
    ));
}

#[test]
fn rejects_integer_param_default_outside_declared_type_range() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
gain = { type = "u8", default = 256, update = "on_tick" }

[instance.controller]
component = "controller"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("out-of-range integer parameter default should fail");

    assert!(matches!(
        error,
        IrError::InvalidParamSchema {
            component,
            param,
            ..
        } if component == "controller" && param == "gain"
    ));
}

#[test]
fn rejects_integer_param_override_outside_declared_type_range() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
gain = { type = "u8", default = 1, update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.params]
gain = -1
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("out-of-range integer parameter override should fail");

    assert!(matches!(
        error,
        IrError::InvalidParamSchema {
            component,
            param,
            ..
        } if component == "controller" && param == "gain"
    ));
}

#[test]
fn rejects_f32_param_default_outside_declared_type_range() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
gain = { type = "f32", default = 1e40, update = "on_tick" }

[instance.controller]
component = "controller"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("out-of-range f32 parameter default should fail");

    assert!(matches!(
        error,
        IrError::InvalidParamSchema {
            component,
            param,
            ..
        } if component == "controller" && param == "gain"
    ));
}

#[test]
fn rejects_unknown_parameter_update_policy() {
    let source = r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
kp = { type = "f32", default = 1.0, update = "immediate" }

[instance.controller]
component = "controller"
"#;
    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source))
        .expect_err("unknown parameter update policy should fail");

    assert!(matches!(
        error,
        IrError::InvalidEnum {
            context,
            kind: "parameter update policy",
            value
        } if context == "component.controller.params.kp.update" && value == "immediate"
    ));
}

#[test]
fn projects_selected_profile_without_touching_other_profiles() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let projected = project_contract_to_profile(&ir, Some("iox2")).unwrap();

    assert_eq!(ir.profiles.len(), 2);
    assert_eq!(projected.profiles.len(), 1);
    assert_eq!(projected.profiles[0].name, "iox2");
    assert_eq!(projected.profiles[0].backend.0, "iox2");
    assert_eq!(projected.deployments.len(), 1);
    assert_eq!(projected.deployments[0].profile.name, "iox2");
    assert_eq!(projected.deployments[0].backend.0, "iox2");
}

#[test]
fn projects_selected_profile_channel_policy_defaults() {
    let source = r#"
[package]
name = "profile_policy_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["defaulted:Sample", "explicit:Sample"]

[component.consumer]
language = "rust"
input = ["defaulted:Sample", "explicit:Sample"]

[instance.producer]
component = "producer"
target = "linux"

[instance.consumer]
component = "consumer"
target = "linux"

[[bind.dataflow]]
from = "producer.defaulted"
to = "consumer.defaulted"
channel = "fifo"
depth = 2

[[bind.dataflow]]
from = "producer.explicit"
to = "consumer.explicit"
channel = "latest"
overflow = "drop_newest"
stale_policy = "hold_last"
max_age_ms = 7

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[profile.safety]
backend = "inproc"
default_overflow = "error"
default_stale_policy = "drop"
max_age_ms = 25

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let projected = project_contract_to_profile(&ir, Some("safety")).unwrap();
    let defaulted = projected.graphs[0]
        .binds
        .iter()
        .find(|bind| bind.to.port == "defaulted")
        .unwrap();
    let explicit = projected.graphs[0]
        .binds
        .iter()
        .find(|bind| bind.to.port == "explicit")
        .unwrap();

    assert_eq!(defaulted.overflow, OverflowPolicy::Error);
    assert_eq!(defaulted.stale, StalePolicy::Drop);
    assert_eq!(defaulted.max_age_ms, Some(25));
    assert_eq!(
        defaulted.capability_requirements,
        channel_route_capabilities(
            &projected.types,
            &TypeExpr::Primitive {
                name: PrimitiveType::U32
            },
            defaulted.channel,
            defaulted.overflow,
            defaulted.stale,
            RouteTopology::local()
        )
    );

    assert_eq!(explicit.overflow, OverflowPolicy::DropNewest);
    assert_eq!(explicit.stale, StalePolicy::HoldLast);
    assert_eq!(explicit.max_age_ms, Some(7));
}

#[test]
fn projects_auto_route_backend_and_falls_back_for_variable_frames() {
    let source = r#"
[package]
name = "route_backend_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"

[type.Counter]
value = "u32"

[component.producer]
language = "rust"
output = ["packet:Packet", "counter:Counter"]

[component.consumer]
language = "rust"
input = ["packet:Packet", "counter:Counter"]

[instance.producer]
component = "producer"
target = "linux"

[instance.consumer]
component = "consumer"
target = "linux"

[[bind.dataflow]]
from = "producer.packet"
to = "consumer.packet"
channel = "latest"
backend = "auto"

[[bind.dataflow]]
from = "producer.counter"
to = "consumer.counter"
channel = "latest"

[profile.default]
backend = "inproc"

[profile.ipc]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2", "zenoh"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let projected = project_contract_to_profile(&ir, Some("ipc")).unwrap();
    let packet = projected.graphs[0]
        .binds
        .iter()
        .find(|bind| bind.from.port == "packet")
        .unwrap();
    let counter = projected.graphs[0]
        .binds
        .iter()
        .find(|bind| bind.from.port == "counter")
        .unwrap();

    assert_eq!(packet.backend.0, "zenoh");
    assert_eq!(packet.backend_source, ChannelBackendSource::AutoFallback);
    assert_eq!(counter.backend.0, "iox2");
    assert_eq!(counter.backend_source, ChannelBackendSource::ProfileDefault);
}

#[test]
fn rejects_explicit_iox2_route_backend_for_variable_frames() {
    let source = r#"
[package]
name = "explicit_iox2_variable_route_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"

[component.producer]
language = "rust"
output = ["packet:Packet"]

[component.consumer]
language = "rust"
input = ["packet:Packet"]

[instance.producer]
component = "producer"

[instance.consumer]
component = "consumer"

[[bind.dataflow]]
from = "producer.packet"
to = "consumer.packet"
channel = "latest"
backend = "iox2"

[target.default]
runtime = ["rust"]
backends = ["inproc", "iox2", "zenoh"]
"#;

    let raw = parse_str(source).unwrap();
    let error = normalize_document(&raw, hash_source(source)).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("explicit `iox2` dataflow backend cannot carry variable-frame messages"),
        "unexpected error: {error}"
    );
}

#[test]
fn explicit_route_backend_survives_profile_projection() {
    let source = r#"
[package]
name = "explicit_route_backend_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"

[component.producer]
language = "rust"
output = ["packet:Packet"]

[component.consumer]
language = "rust"
input = ["packet:Packet"]

[instance.producer]
component = "producer"
target = "linux"

[instance.consumer]
component = "consumer"
target = "linux"

[[bind.dataflow]]
from = "producer.packet"
to = "consumer.packet"
channel = "latest"
backend = "zenoh"

[profile.default]
backend = "inproc"

[profile.ipc]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2", "zenoh"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let projected = project_contract_to_profile(&ir, Some("ipc")).unwrap();
    let bind = &projected.graphs[0].binds[0];

    assert_eq!(bind.backend.0, "zenoh");
    assert_eq!(bind.backend_source, ChannelBackendSource::Explicit);
}

#[test]
fn projects_default_profile_when_selection_is_omitted() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let projected = project_contract_to_profile(&ir, None).unwrap();

    assert_eq!(projected.profiles.len(), 1);
    assert_eq!(projected.profiles[0].name, "default");
    assert_eq!(projected.deployments.len(), 1);
    assert_eq!(projected.deployments[0].profile.name, "default");
    assert_eq!(projected.deployments[0].backend.0, "inproc");
}

#[test]
fn projects_first_profile_when_selection_is_omitted_and_default_is_absent() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.alpha]
backend = "inproc"

[profile.beta]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "iox2"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let projected = project_contract_to_profile(&ir, None).unwrap();

    assert_eq!(projected.profiles.len(), 1);
    assert_eq!(projected.profiles[0].name, "alpha");
    assert_eq!(projected.deployments.len(), 1);
    assert_eq!(projected.deployments[0].profile.name, "alpha");
    assert_eq!(projected.deployments[0].backend.0, "inproc");
}

#[test]
fn rejects_unknown_profile_selection() {
    let source = r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let error = project_contract_to_profile(&ir, Some("iox2"))
        .expect_err("unknown profile selection should fail");

    assert!(matches!(
        error,
        IrError::UnknownProfile { profile } if profile == "iox2"
    ));
}

/// 回归测试：覆盖 workspace import + module name resolver + service bind normalization
/// + profile iox2 variable route auto-zenoh fallback 四个 seam。

#[test]
fn split_seams_regression_workspace_service_profile_fallback() {
    let root = unique_temp_dir();
    std::fs::create_dir_all(root.join("modules")).unwrap();
    std::fs::create_dir_all(root.join("composition")).unwrap();

    // workspace root
    std::fs::write(
        root.join("robot.rsdl"),
        r#"
[package]
name = "seam_regression"
rsdl_version = "0.1"

[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]
"#,
    )
    .unwrap();

    // perception module: outputs a variable-frame type (bytes)
    std::fs::write(
        root.join("modules/perception.rsdl"),
        r#"
[module]
name = "perception"

[type.SensorFrame]
payload = "bytes"

[component.sensor]
language = "rust"
output = ["frame:SensorFrame"]

[component.display]
language = "rust"
service_server = ["status:u32->bool"]
"#,
    )
    .unwrap();

    // control module: references perception::SensorFrame via qualified name
    std::fs::write(
        root.join("modules/control.rsdl"),
        r#"
[module]
name = "control"

[component.controller]
language = "rust"
input = ["frame:perception::SensorFrame"]
service_client = ["status:u32->bool"]
"#,
    )
    .unwrap();

    // composition: instances, dataflow bind, service bind
    std::fs::write(
        root.join("composition/default.rsdl"),
        r#"
[instance.sensor]
component = "perception::sensor"
process = "main"
target = "linux"

[instance.sensor.task]
trigger = "periodic"
period_ms = 10
output = ["frame"]

[instance.controller]
component = "control::controller"
process = "main"
target = "linux"

[instance.controller.task]
trigger = "on_message"
input = ["frame"]

[[bind.dataflow]]
from = "sensor.frame"
to = "controller.frame"
channel = "latest"

[[bind.service]]
client = "controller.status"
server = "display.status"

[instance.display]
component = "perception::display"
process = "main"
target = "linux"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2", "zenoh"]
"#,
    )
    .unwrap();

    let loaded = load_file(root.join("robot.rsdl")).unwrap();
    let ir = normalize_loaded_document(&loaded, hash_source(&loaded.source_bundle_text())).unwrap();

    // seam 1: workspace import resolved 2 modules
    assert_eq!(ir.modules.len(), 2);
    assert_eq!(ir.modules[0].name, "control");
    assert_eq!(ir.modules[1].name, "perception");

    // seam 2: module name resolver — qualified names work
    assert_eq!(ir.types[0].qualified_name, "perception::SensorFrame");
    assert_eq!(ir.components[0].qualified_name, "control::controller");
    assert_eq!(ir.components[1].qualified_name, "perception::display");
    assert_eq!(ir.components[2].qualified_name, "perception::sensor");
    // controller input references perception::SensorFrame
    assert_eq!(
        ir.components[0].inputs[0].ty.canonical_syntax(),
        "perception::SensorFrame"
    );

    // seam 3: service bind normalization
    assert_eq!(ir.graphs[0].services.len(), 1);
    let service = &ir.graphs[0].services[0];
    assert_eq!(service.client.instance.name, "controller");
    assert_eq!(service.client.port, "status");
    assert_eq!(service.server.instance.name, "display");
    assert_eq!(service.server.port, "status");

    // seam 4: iox2 profile + variable frame (bytes) → auto-zenoh fallback
    let bind = &ir.graphs[0].binds[0];
    assert_eq!(bind.backend.0, "zenoh");
    assert_eq!(bind.backend_source, ChannelBackendSource::AutoFallback);

    // profile projection preserves the fallback
    let projected = project_contract_to_profile(&ir, None).unwrap();
    let projected_bind = &projected.graphs[0].binds[0];
    assert_eq!(projected_bind.backend.0, "zenoh");
    assert_eq!(
        projected_bind.backend_source,
        ChannelBackendSource::AutoFallback
    );

    std::fs::remove_dir_all(root).unwrap();
}
