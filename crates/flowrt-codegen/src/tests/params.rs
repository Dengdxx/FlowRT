use super::*;

#[test]
fn generated_rust_components_receive_typed_params_and_register_runtime_params() {
    let ir = contract_from_source(
        r#"
[package]
name = "param_demo"
rsdl_version = "0.1"

[type.Cmd]
value = "f32"

[component.controller]
language = "rust"
output = ["cmd:Cmd"]

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }
mode = { type = "string", default = "normal", enum = ["normal", "safe"], update = "startup" }

[instance.controller]
component = "controller"

[instance.controller.params]
kp = 2.0
mode = "safe"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
output = ["cmd"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let components = artifact_content(&bundle, "rust/src/components.rs");
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();

    assert!(components.contains("pub struct ControllerParams"));
    assert!(components.contains("pub kp: f32"));
    assert!(components.contains("pub mode: String"));
    assert!(components.contains("params: &ControllerParams"));
    assert!(components.contains("fn on_params_update("));
    assert!(shell.contains("controller_params: ControllerParams"));
    assert!(shell.contains("register_param(flowrt::IntrospectionParamSchema"));
    assert!(shell.contains("name: \"controller.kp\".to_string()"));
    assert!(shell.contains("take_pending_param(\"controller.kp\")"));
    assert!(shell.contains("self.controller.on_params_update("));
    assert!(shell.contains("flowrt::params_key_expr(PACKAGE_NAME"));
    assert!(shell.contains("flowrt::ZenohParamsServer::open_from_environment"));
    assert!(shell.contains("let _remote_params_server = match"));
    assert!(cargo_manifest.contains("features = [\"zenoh\"]"));
    assert_eq!(
        selfdesc["graphs"][0]["instances"][0]["params"][0]["name"],
        "kp"
    );
    assert_eq!(
        selfdesc["graphs"][0]["instances"][0]["params"][0]["type"],
        "f32"
    );
    assert!(
        selfdesc["graphs"][0]["instances"][0]["params"][0]
            .get("ty")
            .is_none()
    );
    assert_eq!(
        selfdesc["graphs"][0]["instances"][0]["params"][0]["update"],
        "on_tick"
    );
}

#[test]
fn handler_signature_summary_exposes_param_on_tick_arguments() {
    let rust_ir = contract_from_source(
        r#"
[package]
name = "rust_signature_demo"
rsdl_version = "0.1"

[type.Cmd]
value = "f32"

[component.controller]
language = "rust"
output = ["cmd:Cmd"]

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
output = ["cmd"]
"#,
    );
    let rust_summary = handler_signature_summary(&rust_ir);
    assert!(rust_summary.contains("component controller language=rust"));
    assert!(
        rust_summary
            .contains("fn on_tick(&mut self, params: &ControllerParams, cmd: &mut flowrt::Output<Cmd>) -> flowrt::Status"),
        "{rust_summary}"
    );

    let cpp_ir = contract_from_source(
        r#"
[package]
name = "cpp_signature_demo"
rsdl_version = "0.1"

[type.Cmd]
value = "f32"

[component.controller]
language = "cpp"
output = ["cmd:Cmd"]

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
output = ["cmd"]
"#,
    );
    let cpp_summary = handler_signature_summary(&cpp_ir);
    assert!(cpp_summary.contains("component controller language=cpp"));
    assert!(
        cpp_summary.contains(
            "flowrt::Status on_tick(const ControllerParams& params, flowrt::Output<Cmd>& cmd)"
        ),
        "{cpp_summary}"
    );
}

#[test]
fn generated_rust_shell_omits_param_decoder_for_no_param_contracts() {
    let ir = contract_from_source(
        r#"
[package]
name = "no_param_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"

[instance.worker.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(!shell.contains("fn decode_flowrt_param_value"));
    assert!(!shell.contains("ZenohParamsServer::open_from_environment"));
}

#[test]
fn generated_rust_shell_exposes_startup_only_params_over_remote_control_plane() {
    let ir = contract_from_source(
        r#"
[package]
name = "startup_param_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params.mode]
type = "string"
default = "normal"
update = "startup"

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");

    assert!(shell.contains("register_param(flowrt::IntrospectionParamSchema"));
    assert!(!shell.contains("fn decode_flowrt_param_value"));
    assert!(shell.contains("flowrt::ZenohParamsServer::open_from_environment"));
    assert!(cargo_manifest.contains("features = [\"zenoh\"]"));
}

#[test]
fn generated_rust_shell_includes_param_decoder_for_runtime_params() {
    let ir = contract_from_source(
        r#"
[package]
name = "param_demo"
rsdl_version = "0.1"

[component.estimator]
language = "rust"

[component.estimator.params.gain]
type = "f64"
default = 1.0
update = "on_tick"

[instance.estimator]
component = "estimator"

[instance.estimator.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(shell.contains("fn decode_flowrt_param_value"));
}

#[test]
fn generated_rust_param_apply_checks_runtime_constraints_before_hook() {
    let ir = contract_from_source(
        r#"
[package]
name = "param_demo"
rsdl_version = "0.1"

[component.controller]
language = "rust"

[component.controller.params]
gain = { type = "f64", default = 1.0, min = 0.5, max = 10.0, update = "on_tick" }
mode = { type = "string", default = "normal", enum = ["normal", "safe"], update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    let gain_guard = shell
        .find("flowrt_validate_pending_param_controller_gain")
        .expect(shell);
    let hook = shell
        .find("self.controller.on_params_update(")
        .expect(shell);
    assert!(gain_guard < hook, "{shell}");
    assert!(
        shell.contains("fn flowrt_validate_pending_param_controller_gain(value: &f64) -> bool"),
        "{shell}"
    );
    assert!(shell.contains("*value >= 0.5f64"), "{shell}");
    assert!(shell.contains("*value <= 10.0f64"), "{shell}");
    assert!(
        shell.contains("fn flowrt_validate_pending_param_controller_mode(value: &String) -> bool"),
        "{shell}"
    );
    assert!(
        shell.contains("value == \"normal\" || value == \"safe\""),
        "{shell}"
    );
}

#[test]
fn generated_cpp_components_receive_typed_params_and_register_runtime_params() {
    let ir = contract_from_source(
        r#"
[package]
name = "param_demo"
rsdl_version = "0.1"

[type.Cmd]
value = "f32"

[component.controller]
language = "cpp"
output = ["cmd:Cmd"]

[component.controller.params]
kp = { type = "f32", default = 1.0, min = 0.0, max = 10.0, update = "on_tick" }
mode = { type = "string", default = "normal", enum = ["normal", "safe"], update = "startup" }

[instance.controller]
component = "controller"

[instance.controller.params]
kp = 2.0
mode = "safe"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
output = ["cmd"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let components = artifact_content(&bundle, "cpp/include/flowrt_app/components.hpp");
    let shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(components.contains("struct ControllerParams"));
    assert!(components.contains("float kp"));
    assert!(components.contains("std::string mode"));
    assert!(components.contains("const ControllerParams& params"));
    assert!(components.contains("virtual flowrt::Status on_params_update("));
    assert!(shell.contains("controller_params_(ControllerParams{"));
    assert!(shell.contains("register_param(flowrt::IntrospectionParamSchema"));
    assert!(shell.contains(".name = \"controller.kp\""));
    assert!(shell.contains("take_pending_param(\"controller.kp\")"));
    assert!(shell.contains("controller_->on_params_update("));
}

#[test]
fn generated_cpp_param_decoder_checks_integer_ranges() {
    let ir = contract_from_source(
        r#"
[package]
name = "param_demo"
rsdl_version = "0.1"

[component.controller]
language = "cpp"

[component.controller.params]
small = { type = "u8", default = 1, update = "on_tick" }
wide = { type = "u64", default = 1844674407370955167, update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(shell.contains("#include <limits>"));
    assert!(shell.contains("std::is_signed_v<T>"));
    assert!(shell.contains("std::strtoull"));
    assert!(shell.contains("owned.front() == '-'"));
    assert!(shell.contains("std::numeric_limits<T>::max()"));
    assert!(shell.contains("std::numeric_limits<T>::min()"));
}

#[test]
fn generated_cpp_param_decoder_rejects_non_finite_floats() {
    let ir = contract_from_source(
        r#"
[package]
name = "param_demo"
rsdl_version = "0.1"

[component.controller]
language = "cpp"

[component.controller.params]
gain = { type = "f64", default = 1.0, update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(shell.contains("#include <cmath>"));
    assert!(shell.contains("!std::isfinite(parsed)"));
}

#[test]
fn generated_cpp_param_apply_checks_runtime_constraints_before_hook() {
    let ir = contract_from_source(
        r#"
[package]
name = "param_demo"
rsdl_version = "0.1"

[component.controller]
language = "cpp"

[component.controller.params]
gain = { type = "f64", default = 1.0, min = 0.5, max = 10.0, update = "on_tick" }
mode = { type = "string", default = "normal", enum = ["normal", "safe"], update = "on_tick" }

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    let gain_guard = shell
        .find("flowrt_validate_pending_param_controller_gain")
        .expect(shell);
    let hook = shell.find("controller_->on_params_update(").expect(shell);
    assert!(gain_guard < hook, "{shell}");
    assert!(
        shell.contains("bool flowrt_validate_pending_param_controller_gain(const double& value)"),
        "{shell}"
    );
    assert!(shell.contains("value >= 0.5"), "{shell}");
    assert!(shell.contains("value <= 10.0"), "{shell}");
    assert!(
        shell.contains(
            "bool flowrt_validate_pending_param_controller_mode(const std::string& value)"
        ),
        "{shell}"
    );
    assert!(
        shell.contains("value == \"normal\" || value == \"safe\""),
        "{shell}"
    );
}
