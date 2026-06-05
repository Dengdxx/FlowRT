use flowrt_ir::{hash_source, normalize_document};
use flowrt_rsdl::parse_str;

use super::*;

#[test]
fn plans_rust_artifacts_for_rust_component() {
    let ir = contract_from_source(
        r#"
[package]
name = "demo"
rsdl_version = "0.1"

[component.monitor]
language = "rust"
"#,
    );
    let plan = plan_codegen(&ir);
    assert_eq!(plan.units.len(), 1);
    assert_eq!(plan.units[0].language, CodegenLanguage::Rust);
}

#[test]
fn rejects_contract_without_exactly_one_graph() {
    let mut ir = contract_from_source(
        r#"
[package]
name = "demo"
rsdl_version = "0.1"

[component.monitor]
language = "rust"
"#,
    );
    ir.graphs.clear();

    let error = emit_artifacts(&ir).expect_err("codegen should reject a graphless contract");
    assert!(
        error
            .to_string()
            .contains("Contract IR v0.1 must contain exactly one graph; found 0"),
        "{error}"
    );

    let mut ir = contract_from_source(
        r#"
[package]
name = "demo"
rsdl_version = "0.1"

[component.monitor]
language = "rust"
"#,
    );
    ir.graphs.push(ir.graphs[0].clone());

    let error = emit_artifacts(&ir).expect_err("codegen should reject multiple graphs");
    assert!(
        error
            .to_string()
            .contains("Contract IR v0.1 must contain exactly one graph; found 2"),
        "{error}"
    );
}

#[test]
fn rejects_invalid_contract_before_emitting_artifacts() {
    let mut ir = contract_from_source(
        r#"
[package]
name = "bad"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#,
    );
    ir.graphs[0].binds[0].from.port = "missing".to_string();

    let result = std::panic::catch_unwind(|| emit_artifacts(&ir));

    assert!(result.is_ok(), "codegen should return an error, not panic");
    let error = result
        .expect("codegen invocation should not panic")
        .expect_err("invalid Contract IR should be rejected before emission");
    assert!(
        error
            .to_string()
            .contains("instance `source` component `source` has no Output port `missing`"),
        "{error}"
    );
}

#[test]
fn emits_variable_frame_message_artifacts() {
    let ir = contract_from_source(
        r#"
[package]
name = "variable_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes<max=262144>"
label = "string<max=64>"
samples = "sequence<u32,max=16>"

[component.source]
language = "rust"
output = ["packet:Packet"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]
"#,
    );

    let bundle = emit_artifacts(&ir).unwrap();
    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
    assert!(rust_messages.contains("#[derive(Clone, Debug, PartialEq)]"));
    assert!(rust_messages.contains("pub payload: flowrt::BoundedBytes<262144>"));
    assert!(rust_messages.contains("pub label: flowrt::BoundedString<64>"));
    assert!(rust_messages.contains("pub samples: flowrt::BoundedSequence<u32, 16>"));
    assert!(rust_messages.contains("impl flowrt::FrameCodec for Packet"));
    assert!(rust_messages.contains("flowrt::VarSpan::decode"));
    assert!(rust_messages.contains("BoundedString::<64>::try_from_utf8"));
    assert!(!bundle.artifacts.iter().any(|artifact| {
        artifact.relative_path == std::path::Path::new("rust/tests/message_abi.rs")
    }));

    let selfdesc = artifact_content(&bundle, "selfdesc/selfdesc.json");
    let json: serde_json::Value = serde_json::from_str(selfdesc).unwrap();
    assert_eq!(json["message_abi"], serde_json::json!([]));
    assert_eq!(json["message_frames"][0]["type_name"], "Packet");
    assert_eq!(json["message_frames"][0]["encoding"], "canonical_frame_v1");
    assert_eq!(json["message_frames"][0]["variable"], true);
    assert_eq!(json["message_frames"][0]["header_size_bytes"], 24);
}

#[test]
fn emits_iox2_frame_slots_for_variable_messages() {
    let ir = contract_from_source(
        r#"
[package]
name = "variable_iox2_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes<max=32>"
label = "string<max=16>"
samples = "sequence<u32,max=4>"

[component.source]
language = "rust"
output = ["packet:Packet"]

[component.sink]
language = "cpp"
input = ["packet:Packet"]

[instance.source]
component = "source"
target = "linux"
process = "source_proc"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]

[instance.sink]
component = "sink"
target = "linux"
process = "sink_proc"

[instance.sink.task]
trigger = "on_message"
input = ["packet"]

[[bind.dataflow]]
from = "source.packet"
to = "sink.packet"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["iox2"]
"#,
    );

    let bundle = emit_artifacts(&ir).unwrap();
    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
    let cpp_shell_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");

    assert!(rust_messages.contains("pub struct PacketIox2Frame"));
    assert!(rust_messages.contains("#[type_name(\"Packet\")]"));
    assert!(rust_messages.contains("impl flowrt::iox2::Iox2FrameSlot<Packet> for PacketIox2Frame"));
    assert!(rust_shell.contains("flowrt::iox2::Iox2FramePubSub<Packet, PacketIox2Frame>"));

    assert!(cpp_messages.contains("struct PacketIox2Frame"));
    assert!(cpp_messages.contains("std::size_t samples_cursor = 0;"));
    assert!(cpp_messages.contains("samples_cursor += 4;"));
    assert!(cpp_messages.contains("static constexpr const char* IOX2_TYPE_NAME = \"Packet\";"));
    assert!(cpp_messages.contains("static PacketIox2Frame from_message(const Packet& value)"));
    assert!(
        cpp_shell_header
            .contains("flowrt::iox2::Iox2FramePubSub<Packet, PacketIox2Frame> bind_0_;")
    );
}

#[test]
fn emits_cpp_and_rust_application_artifacts() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[type.Cmd]
left = "f32"
right = "f32"

[component.controller]
language = "cpp"
input = ["imu:Imu"]
output = ["cmd:Cmd"]

[component.monitor]
language = "rust"
input = ["imu:Imu"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();

    let paths = bundle
        .artifacts
        .iter()
        .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"cpp/include/flowrt_app/messages.hpp".to_string()));
    assert!(paths.contains(&"cpp/include/flowrt_app/selfdesc.hpp".to_string()));
    assert!(paths.contains(&"cpp/src/selfdesc.cpp".to_string()));
    assert!(paths.contains(&"rust/src/selfdesc.rs".to_string()));
    assert!(paths.contains(&"rust/src/components.rs".to_string()));
    assert!(paths.contains(&"cpp/tests/message_abi.cpp".to_string()));
    assert!(paths.contains(&"rust/tests/message_abi.rs".to_string()));
    assert!(paths.contains(&"selfdesc/selfdesc.json".to_string()));
    assert!(paths.contains(&"launch/launch.json".to_string()));

    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
    assert!(cpp_messages.contains("struct Imu"));
    assert!(cpp_messages.contains("std::uint64_t timestamp{};"));

    let rust_components = artifact_content(&bundle, "rust/src/components.rs");
    assert!(rust_components.contains("pub trait Monitor"));
    assert!(!rust_components.contains("pub trait Controller"));
    assert!(rust_components.contains("imu: flowrt::Latest<'_, Imu>"));

    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
    assert!(rust_messages.contains("impl Default for Imu"));
    assert!(rust_messages.contains("std::mem::zeroed()"));

    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("const SELECTED_BACKEND: &str = \"inproc\";"));
    assert!(rust_shell.contains("const PACKAGE_NAME: &str = \"robot_demo\";"));
    assert!(rust_shell.contains("flowrt::spawn_status_server("));
    assert!(rust_shell.contains("let introspection_state = flowrt::IntrospectionState::new();"));
    assert!(rust_shell.contains(
        "introspection_state.set_self_description_json(selfdesc::self_description_json());"
    ));
    assert!(rust_shell.contains("introspection_state.record_tick();"));
    assert!(!rust_shell.contains("flowrt::IntrospectionStatus {"));
    assert!(rust_shell.contains("selfdesc::self_description_hash().to_string()"));

    let sidecar: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    assert_eq!(sidecar["self_description_version"], "0.1");
    assert_eq!(sidecar["package"]["name"], "robot_demo");
    assert_eq!(sidecar["graphs"][0]["name"], "default");
    assert!(
        sidecar["graphs"][0]["instances"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(sidecar["message_abi"][0]["type_name"], "Cmd");
    assert_eq!(sidecar["message_abi"][0]["fields"][0]["type"], "f32");

    let rust_selfdesc = artifact_content(&bundle, "rust/src/selfdesc.rs");
    assert!(rust_selfdesc.contains("#[unsafe(link_section = \".flowrt.selfdesc\")]"));
    assert!(rust_selfdesc.contains("static FLOWRT_SELF_DESCRIPTION"));
    assert!(rust_selfdesc.contains("= *br#"));
    assert!(!rust_selfdesc.contains("*bbr#"));

    let cpp_selfdesc = artifact_content(&bundle, "cpp/src/selfdesc.cpp");
    assert!(cpp_selfdesc.contains("[[gnu::used, gnu::section(\".flowrt.selfdesc\")]]"));
    assert!(cpp_selfdesc.contains("const char kFlowrtSelfDescription[]"));
    assert!(rust_shell.contains("flowrt::iox2_backend()"));
}

#[test]
fn generated_shells_cleanup_entered_lifecycle_stages_in_reverse_order() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.cpp_alpha]
language = "cpp"

[component.cpp_beta]
language = "cpp"

[component.rust_alpha]
language = "rust"

[component.rust_beta]
language = "rust"

[instance.cpp_alpha]
component = "cpp_alpha"

[instance.cpp_alpha.task]
trigger = "periodic"
period_ms = 5

[instance.cpp_beta]
component = "cpp_beta"

[instance.cpp_beta.task]
trigger = "periodic"
period_ms = 5

[instance.rust_alpha]
component = "rust_alpha"

[instance.rust_alpha.task]
trigger = "periodic"
period_ms = 5

[instance.rust_beta]
component = "rust_beta"

[instance.rust_beta.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(cpp_shell.contains("auto status = flowrt::Status::Ok;"));
    assert!(cpp_shell.contains("bool cpp_alpha_initialized = false;"));
    assert!(cpp_shell.contains("bool cpp_alpha_started = false;"));
    assert!(cpp_shell.contains("if (status == flowrt::Status::Ok) {"));
    assert!(
        cpp_shell
            .contains("if (status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok)")
    );
    assert!(
        cpp_shell
            .contains("if (status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok)")
    );
    assert!(cpp_shell.contains("return status;"));
    assert!(!cpp_shell.contains("if (status != flowrt::Status::Ok) {\n        return status;"));
    assert!(
        cpp_shell
            .find("if (cpp_beta_started && cpp_beta_)")
            .unwrap()
            < cpp_shell
                .find("if (cpp_alpha_started && cpp_alpha_)")
                .unwrap()
    );
    assert!(
        cpp_shell
            .find("if (cpp_beta_initialized && cpp_beta_)")
            .unwrap()
            < cpp_shell
                .find("if (cpp_alpha_initialized && cpp_alpha_)")
                .unwrap()
    );

    assert!(rust_shell.contains("let mut status = flowrt::Status::Ok;"));
    assert!(rust_shell.contains("let mut rust_alpha_initialized = false;"));
    assert!(rust_shell.contains("let mut rust_alpha_started = false;"));
    assert!(rust_shell.contains("if status == flowrt::Status::Ok {"));
    assert!(
        rust_shell.contains("if status == flowrt::Status::Ok && stop_status != flowrt::Status::Ok")
    );
    assert!(
        rust_shell
            .contains("if status == flowrt::Status::Ok && shutdown_status != flowrt::Status::Ok")
    );
    assert!(rust_shell.contains("        status\n    }\n"));
    assert!(
        !rust_shell
            .contains("if status != flowrt::Status::Ok {\n            return status;\n        }")
    );
    assert!(
        rust_shell.find("if rust_beta_started {").unwrap()
            < rust_shell.find("if rust_alpha_started {").unwrap()
    );
    assert!(
        rust_shell.find("if rust_beta_initialized {").unwrap()
            < rust_shell.find("if rust_alpha_initialized {").unwrap()
    );
}

#[test]
fn message_abi_tests_embed_cross_language_byte_fixtures() {
    let ir = contract_from_source(
        r#"
[package]
name = "abi_demo"
rsdl_version = "0.1"

[type.Packet]
tag = "u8"
count = "u32"
temperature = "f32"

[component.producer]
language = "rust"
output = ["packet:Packet"]

[component.consumer]
language = "cpp"
input = ["packet:Packet"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();

    let rust_abi = artifact_content(&bundle, "rust/tests/message_abi.rs");
    assert!(rust_abi.contains("const EXPECTED_PACKET_BYTES: &[u8] = &["));
    assert!(rust_abi.contains("2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 136, 64"));
    assert!(rust_abi.contains("assert_sample_bytes(sample_packet(), EXPECTED_PACKET_BYTES);"));
    assert!(rust_abi.contains("fn assert_cpp_fixture_roundtrip<T: Copy + Default>"));
    assert!(rust_abi.contains(
            "assert_cpp_fixture_roundtrip::<flowrt_app::messages::Packet>(\"packet.bin\", EXPECTED_PACKET_BYTES);"
        ));

    let cpp_abi = artifact_content(&bundle, "cpp/tests/message_abi.cpp");
    assert!(cpp_abi.contains("constexpr std::array<std::uint8_t, 12> EXPECTED_PACKET_BYTES"));
    assert!(cpp_abi.contains("2, 0, 0, 0, 3, 0, 0, 0, 0, 0, 136, 64"));
    assert!(cpp_abi.contains("assert_sample_bytes(sample_packet(), EXPECTED_PACKET_BYTES);"));
    assert!(cpp_abi.contains("write_fixture(\"packet.bin\", bytes_of(sample_packet()));"));

    let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
    assert!(cmake.contains(
        "target_compile_definitions(abi_demo_message_abi PRIVATE FLOWRT_ABI_FIXTURE_DIR="
    ));
    assert!(cmake.contains("add_custom_command(TARGET abi_demo_message_abi POST_BUILD"));
}

#[test]
fn message_abi_tests_assert_default_initialization_zeroes_padding_bytes() {
    let ir = contract_from_source(
        r#"
[package]
name = "abi_demo"
rsdl_version = "0.1"

[type.Padded]
flag = "bool"
count = "u32"

[component.producer]
language = "rust"
output = ["padded:Padded"]

[component.consumer]
language = "cpp"
input = ["padded:Padded"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();

    let rust_abi = artifact_content(&bundle, "rust/tests/message_abi.rs");
    assert!(rust_abi.contains("fn assert_default_bytes_zero<T: Copy + Default>()"));
    assert!(rust_abi.contains("assert_default_bytes_zero::<flowrt_app::messages::Padded>();"));

    let cpp_abi = artifact_content(&bundle, "cpp/tests/message_abi.cpp");
    assert!(cpp_abi.contains("void assert_default_bytes_zero()"));
    assert!(cpp_abi.contains("std::array<std::uint8_t, sizeof(T)> expected{};"));
    assert!(cpp_abi.contains("assert(bytes_of(value) == expected);"));
    assert!(cpp_abi.contains("assert_default_bytes_zero<flowrt_app::Padded>();"));
}

#[test]
fn profile_selection_projects_selected_backend_into_generated_artifacts() {
    let ir = contract_from_source(
        r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[profile.default]
backend = "inproc"
default_overflow = "drop_oldest"
default_stale_policy = "warn"

[profile.iox2]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );
    let projected = flowrt_ir::project_contract_to_profile(&ir, Some("iox2")).unwrap();
    let bundle = emit_artifacts(&projected).unwrap();
    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
    assert!(cargo_manifest.contains("features = [\"iox2\"]"));
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("const SELECTED_BACKEND: &str = \"iox2\";"));
}

#[test]
fn iox2_runtime_shell_omits_tick_timestamp_for_empty_bind_graphs() {
    let ir = contract_from_source(
        r#"
[package]
name = "profile_demo"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
process = "main"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 1

[profile.iox2]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(!rust_shell.contains("let tick_time_ms = tick as u64;"));
}

#[test]
fn mixed_rust_shell_does_not_invent_traits_for_cpp_components() {
    let ir = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "cpp_source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "rust_sink"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let rust_components = artifact_content(&bundle, "rust/src/components.rs");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(!rust_components.contains("pub trait Source"));
    assert!(rust_components.contains("pub trait Sink"));
    assert!(!rust_shell.contains("source: Box<dyn Source>"));
    assert!(rust_shell.contains("sink: Box<dyn Sink>"));
    assert!(!rust_shell.contains("mixed-language runtime shell is not implemented"));
    assert!(rust_shell.contains("flowrt::iox2::Iox2PubSub<u32>"));
    assert!(rust_shell.contains("receive_latest_at(tick_time_ms)"));
    assert!(!cpp_header.contains("std::unique_ptr<SinkInterface>"));
    assert!(cpp_header.contains("std::unique_ptr<SourceInterface> source"));
    assert!(!cpp_shell.contains("return flowrt::ok();"));
    assert!(cpp_shell.contains("flowrt::iox2::Iox2PubSub<std::uint32_t>"));
    assert!(cpp_shell.contains("bind_0_.publish_at(*value, tick_time_ms)"));
}

#[test]
fn emits_cpp_managed_app_targets() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Odom]
timestamp = "u64"
x = "f32"

[type.Cmd]
left = "f32"
right = "f32"

[component.source]
language = "cpp"
output = ["odom:Odom"]

[component.controller]
language = "cpp"
input = ["odom:Odom"]
output = ["cmd:Cmd"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["odom"]

[instance.controller]
component = "controller"

[instance.controller.task]
trigger = "on_message"
input = ["odom"]
output = ["cmd"]

[[bind.dataflow]]
from = "source.odom"
to = "controller.odom"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let paths = bundle
        .artifacts
        .iter()
        .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(paths.contains(&"cpp/include/flowrt_app/runtime_shell.hpp".to_string()));
    assert!(paths.contains(&"cpp/src/runtime_shell.cpp".to_string()));
    assert!(paths.contains(&"cpp/src/main.cpp".to_string()));

    let runtime_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    assert!(runtime_header.contains("#include <memory>"));
    assert!(runtime_header.contains("class App"));
    assert!(runtime_header.contains("std::unique_ptr<SourceInterface> source"));
    assert!(runtime_header.contains(
        "flowrt::Status run(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks);"
    ));
    assert!(runtime_header.contains("namespace flowrt_user"));
    assert!(runtime_header.contains("flowrt_app::App build_app();"));

    let runtime_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    assert!(runtime_shell.contains("#include \"flowrt_app/runtime_shell.hpp\""));
    assert!(runtime_shell.contains("App::App("));
    assert!(runtime_shell.contains("bind_0_"));
    assert!(runtime_shell.contains("flowrt::Output<Odom> source_odom;"));
    assert!(runtime_shell.contains("const auto controller_odom = bind_0_.view_at(tick_time_ms);"));
    assert!(runtime_shell.contains("source_->on_tick(source_odom)"));
    assert!(runtime_shell.contains("controller_->on_tick(controller_odom, controller_cmd)"));
    assert!(runtime_shell.contains("flowrt_user::build_app().run(backend, run_ticks);"));

    let main = artifact_content(&bundle, "cpp/src/main.cpp");
    assert!(main.contains("#include \"flowrt_app/runtime_shell.hpp\""));
    assert!(main.contains("std::string_view process;"));
    assert!(main.contains("--flowrt-run-ticks"));
    assert!(main.contains("flowrt_app::run_process(process, run_ticks)"));

    let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
    assert!(cmake.contains("set(CMAKE_EXPORT_COMPILE_COMMANDS ON)"));
    assert!(cmake.contains("find_package(flowrt_runtime 0.1 QUIET)"));
    assert!(
        cmake.contains("target_link_libraries(robot_demo_flowrt_app INTERFACE flowrt::runtime)")
    );
    assert!(cmake.contains("FLOWRT_CPP_RUNTIME_DIR"));
    assert!(cmake.contains("FlowRT C++ runtime was not found"));
    assert!(cmake.contains(
            "add_library(robot_demo_cpp_shell STATIC ../cpp/src/runtime_shell.cpp ../cpp/src/selfdesc.cpp)"
        ));
    assert!(
        cmake.contains("target_link_libraries(robot_demo_cpp_shell PUBLIC robot_demo_flowrt_app)")
    );
    assert!(cmake.contains("FLOWRT_USER_CPP_SOURCES"));
    assert!(cmake.contains("add_library(robot_demo_cpp_user STATIC"));
    assert!(cmake.contains("add_executable(robot_demo_cpp_app ../cpp/src/main.cpp)"));
}

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
fn emits_supervisor_only_rust_crate_for_cpp_only_launch() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "cpp"
input = ["value:u32"]

[instance.source]
component = "source"
process = "control"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let paths = bundle
        .artifacts
        .iter()
        .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();

    assert!(paths.contains(&"rust/src/supervisor.rs".to_string()));
    assert!(paths.contains(&"rust/src/supervisor_main.rs".to_string()));
    assert!(paths.contains(&"rust/src/lib.rs".to_string()));
    assert!(paths.contains(&"rust/src/selfdesc.rs".to_string()));
    assert!(!paths.contains(&"rust/src/runtime_shell.rs".to_string()));
    assert!(!paths.contains(&"rust/src/main.rs".to_string()));

    let rust_lib = artifact_content(&bundle, "rust/src/lib.rs");
    assert!(rust_lib.contains("pub(crate) mod selfdesc;"));
    assert!(rust_lib.contains("pub mod supervisor;"));
    assert!(!rust_lib.contains("pub mod runtime_shell;"));
    assert!(!rust_lib.contains("pub mod user;"));

    let rust_selfdesc = artifact_content(&bundle, "rust/src/selfdesc.rs");
    assert!(rust_selfdesc.contains("#[allow(dead_code)]\npub fn self_description_hash()"));

    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
    assert!(cargo_manifest.contains("flowrt = { version = \"0.1\" }"));
    assert!(cargo_manifest.contains("[[bin]]\nname = \"robot-demo-flowrt-supervisor\""));
    assert!(cargo_manifest.contains("path = \"../rust/src/supervisor_main.rs\""));
    assert!(!cargo_manifest.contains("path = \"../rust/src/main.rs\""));
}

#[test]
fn emits_documented_component_interfaces() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"

[type.Cmd]
left = "f32"
right = "f32"

[component.controller]
language = "cpp"
input = ["imu:Imu"]
output = ["cmd:Cmd"]

[component.monitor]
language = "rust"
input = ["imu:Imu"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_components = artifact_content(&bundle, "cpp/include/flowrt_app/components.hpp");
    let rust_components = artifact_content(&bundle, "rust/src/components.rs");

    assert!(cpp_components.contains(" * @brief `controller` 组件的 C++ 用户实现接口。"));
    assert!(cpp_components.contains(" * @brief 组件初始化钩子。"));
    assert!(cpp_components.contains(" * @brief 执行一次 `controller` 组件调度回调。"));
    assert!(cpp_components.contains(" * @param imu latest snapshot 输入视图。"));
    assert!(cpp_components.contains(" * @param cmd 输出端口写入句柄。"));
    assert!(cpp_components.contains(" * @return 本次回调的 FlowRT 执行状态。"));

    assert!(rust_components.contains("/// `monitor` 组件的 Rust 用户实现 trait。"));
    assert!(rust_components.contains("/// 组件初始化钩子。"));
    assert!(rust_components.contains("/// 执行一次 `monitor` 组件调度回调。"));
    assert!(rust_components.contains("/// - `imu`: latest snapshot 输入视图。"));
    assert!(rust_components.contains("/// 返回本次回调的 FlowRT 执行状态。"));
}

#[test]
fn enables_flowrt_iox2_feature_when_profile_selects_iox2() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.monitor]
language = "rust"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
    assert!(cargo_manifest.contains("features = [\"iox2\"]"));
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("const SELECTED_BACKEND: &str = \"iox2\";"));
}

#[test]
fn emits_cpp_iox2_transport_contract_when_profile_selects_iox2() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
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
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["cpp"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();

    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
    assert!(cpp_messages.contains("static constexpr const char* IOX2_TYPE_NAME = \"Imu\";"));

    let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
    assert!(cmake.contains("include(FetchContent)"));
    assert!(cmake.contains("find_package(iceoryx2-cxx 0.9.1 QUIET)"));
    assert!(cmake.contains("FetchContent_Declare("));
    assert!(cmake.contains("iceoryx2"));
    assert!(cmake.contains("GIT_TAG v0.9.1"));
    assert!(cmake.contains("FetchContent_MakeAvailable(iceoryx2)"));
    assert!(cmake.contains("if(NOT TARGET iceoryx2-cxx::static-lib-cxx)"));
    assert!(!cmake.contains("if(NOT iceoryx2-cxx_FOUND AND NOT TARGET"));
    assert!(cmake.contains("iceoryx2-cxx::static-lib-cxx"));
    assert!(cmake.contains(
        "target_compile_definitions(robot_demo_flowrt_app INTERFACE FLOWRT_HAS_ICEORYX2_CXX=1)"
    ));

    let runtime_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    assert!(runtime_header.contains("flowrt::iox2::Iox2PubSub<Imu> bind_0_;"));

    let runtime_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    assert!(runtime_shell.contains("flowrt::iox2::Iox2PubSub<Imu>::open_with_config"));
    assert!(runtime_shell.contains("\"FlowRT/robot_demo/default/bind_0/source_imu_to_sink_imu\""));
    assert!(runtime_shell.contains(
            "flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config(flowrt::StaleConfig{std::chrono::milliseconds{20}, flowrt::StalePolicy::Error})"
        ));
    assert!(runtime_shell.contains("bind_0_.receive_latest_at(tick_time_ms)"));
    assert!(runtime_shell.contains("bind_0_.publish_at(*value, tick_time_ms)"));
    assert!(runtime_shell.contains("auto backend = flowrt::iox2_backend();"));
}

#[test]
fn emits_iox2_typed_channels_when_profile_selects_iox2() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[component.fifo_sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[instance.fifo_sink]
component = "fifo_sink"
target = "linux"

[instance.fifo_sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "drop"

[[bind.dataflow]]
from = "source.imu"
to = "fifo_sink.imu"
channel = "fifo"
depth = 8
overflow = "drop_oldest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
    assert!(rust_messages.contains("use flowrt::ZeroCopySend;"));
    assert!(rust_messages.contains("flowrt::ZeroCopySend"));
    assert!(rust_messages.contains("#[type_name(\"Imu\")]"));
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("flowrt::iox2::Iox2PubSub<Imu>"));
    assert!(!rust_shell.contains("flowrt::LatestChannel<Imu>"));
    assert!(rust_shell.contains("flowrt::iox2::Iox2ChannelConfig::latest()"));
    assert!(
        rust_shell.contains(
            "flowrt::iox2::Iox2ChannelConfig::fifo(8, flowrt::OverflowPolicy::DropOldest)"
        )
    );
    assert!(rust_shell.contains("flowrt::StaleConfig::new(Some(20), flowrt::StalePolicy::Drop)"));
    assert!(rust_shell.contains("publish_at(value.clone(), tick_time_ms)"));
    assert!(rust_shell.contains("receive_latest_at(tick_time_ms)"));
}

#[test]
fn emits_zenoh_backend_and_key_expressions_when_profile_selects_zenoh() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
process = "producer"
target = "dev_host"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
process = "consumer"
target = "pi_host"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"

[profile.default]
backend = "zenoh"

[target.dev_host]
runtime = ["rust"]
backends = ["zenoh"]

[target.pi_host]
runtime = ["cpp"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();

    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("const SELECTED_BACKEND: &str = \"zenoh\";"));
    assert!(rust_shell.contains("\"zenoh\" => Box::new(flowrt::zenoh_backend())"));
    assert!(!rust_shell.contains("_ => Box::new(flowrt::inproc_backend())"));
    assert!(rust_shell.contains("flowrt::zenoh::ZenohPubSub<Imu>"));
    assert!(rust_shell.contains(
            "flowrt::zenoh::ZenohPubSub::open_with_config(\"flowrt/robot_demo/default/default/bind_0/source_imu_to_sink_imu\""
        ));
    assert!(rust_shell.contains("flowrt::zenoh::ZenohChannelConfig::latest()"));
    assert!(rust_shell.contains("publish_at(value.clone(), tick_time_ms)"));

    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    assert!(cpp_shell.contains("auto backend = flowrt::zenoh_backend();"));
    assert!(cpp_shell.contains(
            "flowrt::zenoh::ZenohPubSub<Imu>::open_with_config(\"flowrt/robot_demo/default/default/bind_0/source_imu_to_sink_imu\""
        ));
    assert!(cpp_shell.contains("flowrt::zenoh::ZenohChannelConfig::latest()"));
    assert!(cpp_shell.contains("receive_latest_at(tick_time_ms)"));

    let cpp_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");
    assert!(cpp_header.contains("flowrt::zenoh::ZenohPubSub<Imu> bind_0_;"));

    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
    assert!(cargo_manifest.contains("features = [\"zenoh\"]"));
    assert!(!cargo_manifest.contains("features = [\"iox2\"]"));

    let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
    assert!(cmake.contains("find_package(zenohc 1.9.0 QUIET)"));
    assert!(cmake.contains("find_package(zenohcxx 1.9.0 QUIET)"));
    assert!(cmake.contains("zenohcxx::zenohc"));
    assert!(cmake.contains("${FLOWRT_ZENOH_CXX_TARGET}"));
    assert!(!cmake.contains("FetchContent"));
    assert!(!cmake.contains("GIT_REPOSITORY"));
    assert!(cmake.contains("FLOWRT_HAS_ZENOH_CXX=1"));
    assert!(!cmake.contains("find_package(iceoryx2-cxx"));

    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let channel = &launch["graphs"][0]["channels"][0];
    assert_eq!(channel["backend"], "zenoh");
    assert_eq!(channel["service"], serde_json::Value::Null);
    assert_eq!(
        channel["key_expr"],
        "flowrt/robot_demo/default/default/bind_0/source_imu_to_sink_imu"
    );

    let sidecar: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    let selfdesc_channel = &sidecar["graphs"][0]["channels"][0];
    assert_eq!(selfdesc_channel["backend"], "zenoh");
    assert_eq!(selfdesc_channel["service"], serde_json::Value::Null);
    assert_eq!(
        selfdesc_channel["key_expr"],
        "flowrt/robot_demo/default/default/bind_0/source_imu_to_sink_imu"
    );
}

#[test]
fn emits_rust_wire_codec_when_profile_selects_zenoh() {
    let ir = contract_from_source(
        r#"
[package]
name = "wire_demo"
rsdl_version = "0.1"

[type.Inner]
value = "u16"

[type.Packet]
flag = "bool"
count = "u32"
inner = "Inner"
samples = "[i16; 2]"

[component.source]
language = "rust"
output = ["packet:Packet"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["rust"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");

    assert!(rust_messages.contains("impl flowrt::WireCodec for Inner"));
    assert!(rust_messages.contains("const WIRE_SIZE: usize = 2;"));
    assert!(rust_messages.contains("impl flowrt::WireCodec for Packet"));
    assert!(rust_messages.contains("const WIRE_SIZE: usize = 11;"));
    assert!(rust_messages.contains("output[cursor] = self.flag as u8;"));
    assert!(
        rust_messages
            .contains("output[cursor..cursor + 4].copy_from_slice(&(self.count).to_le_bytes());")
    );
    assert!(
        rust_messages
            .contains("self.inner.encode_wire(&mut output[cursor..cursor + Inner::WIRE_SIZE])?;")
    );
    assert!(rust_messages.contains("for element in &self.samples"));
    assert!(rust_messages.contains("i16::from_le_bytes([input[cursor], input[cursor + 1]])"));

    let rust_abi = artifact_content(&bundle, "rust/tests/message_abi.rs");
    assert!(rust_abi.contains("fn packet_wire_codec_omits_native_padding()"));
    assert!(rust_abi.contains("assert_eq!(wire, vec![1, 3, 0, 0, 0, 2, 0, 251, 255, 251, 255]);"));
    assert!(
        rust_abi.contains(
            "assert_eq!(flowrt_app::messages::Packet::decode_wire(&wire).unwrap(), value);"
        )
    );
}

#[test]
fn emits_cpp_wire_codec_when_profile_selects_zenoh() {
    let ir = contract_from_source(
        r#"
[package]
name = "wire_demo"
rsdl_version = "0.1"

[type.Inner]
value = "u16"

[type.Packet]
flag = "bool"
count = "u32"
inner = "Inner"
samples = "[i16; 2]"

[component.source]
language = "cpp"
output = ["packet:Packet"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["cpp"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");

    assert!(
        cpp_messages.contains("static constexpr std::size_t wire_size() noexcept { return 2; }")
    );
    assert!(
        cpp_messages.contains("static constexpr std::size_t wire_size() noexcept { return 11; }")
    );
    assert!(cpp_messages.contains("flowrt::write_wire_le(output, cursor, flag);"));
    assert!(cpp_messages.contains("flowrt::write_wire_le(output, cursor, count);"));
    assert!(
        cpp_messages.contains("inner.encode_wire(output.subspan(cursor, Inner::wire_size()));")
    );
    assert!(cpp_messages.contains("for (const auto& element : samples)"));
    assert!(
        cpp_messages
            .contains("value.samples[index] = flowrt::read_wire_le<std::int16_t>(input, cursor);")
    );

    let cpp_abi = artifact_content(&bundle, "cpp/tests/message_abi.cpp");
    assert!(cpp_abi.contains("void test_packet_wire_codec_omits_native_padding()"));
    assert!(cpp_abi.contains("std::array<std::uint8_t, flowrt_app::Packet::wire_size()> wire{};"));
    assert!(cpp_abi.contains(
            "const std::array<std::uint8_t, flowrt_app::Packet::wire_size()> expected_wire{1, 3, 0, 0, 0, 2, 0, 251, 255, 251, 255};"
        ));
    assert!(cpp_abi.contains("const auto decoded = flowrt_app::Packet::decode_wire(wire);"));
    assert!(cpp_abi.contains("assert(bytes_of(decoded) == bytes_of(value));"));
}

#[test]
fn emits_inproc_stale_channel_reads_from_bind_policy() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "drop"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains(
            "flowrt::LatestChannel::with_stale_config(flowrt::StaleConfig::new(Some(20), flowrt::StalePolicy::Drop))"
        ));
    assert!(rust_shell.contains("publish_at(value.clone(), tick_time_ms)"));
    assert!(rust_shell.contains("view_at(tick_time_ms)"));
}

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
        " = register_introspection_channel(&introspection_state, {}, \"Sample\", 4);",
        rust_string_literal(sensor_channel)
    );
    let sensor_probe = extract_probe_field_for_registration(sensors_run, &sensor_register_marker)
        .expect("sensors process should register sensor channel");
    let aux_register_marker = format!(
        "register_introspection_channel(&introspection_state, {}, \"Sample\", 4);",
        rust_string_literal(aux_channel)
    );
    assert!(
        !sensors_run.contains(&aux_register_marker),
        "sensors process should not register aux channel:\n{sensors_run}"
    );
    let sensor_record =
        format!("record_introspection_publish_copy(&self.{sensor_probe}, &value, tick_time_ms);");
    assert!(rust_shell.contains(&sensor_record));
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
            "register_introspection_channel(&introspection_state, \"source.imu_to_sink.imu\", \"Imu\", 24);"
        ));
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
        " = register_introspection_channel(introspection_state, {}, \"Sample\", 4);",
        cpp_string_literal(sensor_channel)
    );
    let sensor_probe = extract_probe_field_for_registration(sensors_run, &sensor_register_marker)
        .expect("sensors process should register sensor channel");
    let aux_register_marker = format!(
        "register_introspection_channel(introspection_state, {}, \"Sample\", 4);",
        cpp_string_literal(aux_channel)
    );
    assert!(
        !sensors_run.contains(&aux_register_marker),
        "sensors process should not register aux channel:\n{sensors_run}"
    );
    let sensor_record =
        format!("record_introspection_publish_copy(this->{sensor_probe}, *value, tick_time_ms);");
    assert!(cpp_shell.contains(&sensor_record));
    let sensor_record_at = cpp_shell.find(&sensor_record).unwrap();
    let sensor_before_record = &cpp_shell[..sensor_record_at];
    assert!(sensor_before_record.contains("publish_at(*value, tick_time_ms)"));

    let aux_run = generated_function_block(cpp_shell, "App::run_process_aux");
    let aux_register_marker = format!(
        " = register_introspection_channel(introspection_state, {}, \"Sample\", 4);",
        cpp_string_literal(aux_channel)
    );
    let aux_probe = extract_probe_field_for_registration(aux_run, &aux_register_marker)
        .expect("aux process should register aux channel");
    let aux_record =
        format!("record_introspection_publish_copy(this->{aux_probe}, *value, tick_time_ms);");
    assert!(cpp_shell.contains(&aux_record));
    let aux_record_at = cpp_shell.find(&aux_record).unwrap();
    let aux_before_record = &cpp_shell[..aux_record_at];
    assert!(aux_before_record.contains("push_at(*value, tick_time_ms)"));
    assert!(aux_before_record.contains("ChannelWriteOutcome::DroppedOldest"));
}

#[test]
fn emits_inproc_fifo_stale_channel_reads_from_bind_policy() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "fifo"
depth = 4
overflow = "drop_oldest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains(
            "flowrt::FifoChannel::with_stale_config(4, flowrt::OverflowPolicy::DropOldest, flowrt::StaleConfig::new(Some(20), flowrt::StalePolicy::Error))"
        ));
    assert!(rust_shell.contains("let imu_read = self.bind_0.pop_at(tick_time_ms);"));
    assert!(rust_shell.contains("let imu = imu_read.view();"));
    assert!(rust_shell.contains("push_at(value.clone(), tick_time_ms)"));
    assert!(
        rust_shell
            .contains("if imu.stale() {\n            return flowrt::Status::Error;\n        }")
    );
    assert!(rust_shell.find("if imu.stale()").unwrap() < rust_shell.find(".on_tick(imu)").unwrap());
}

#[test]
fn emits_stale_error_guard_before_user_tick() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "rust"
output = ["imu:Imu"]

[component.sink]
language = "rust"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(
        rust_shell
            .contains("if imu.stale() {\n            return flowrt::Status::Error;\n        }")
    );
    assert!(rust_shell.find("if imu.stale()").unwrap() < rust_shell.find(".on_tick(imu)").unwrap());
}

#[test]
fn cpp_shell_emits_stale_channel_reads_from_bind_policy() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "drop"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(cpp_shell.contains(
            "flowrt::LatestChannel<Imu>::with_stale_config(flowrt::StaleConfig{std::chrono::milliseconds{20}, flowrt::StalePolicy::Drop})"
        ));
    assert!(cpp_shell.contains("const auto tick_time_ms = static_cast<std::uint64_t>(tick);"));
    assert!(cpp_shell.contains("publish_at(*value, tick_time_ms)"));
    assert!(cpp_shell.contains("view_at(tick_time_ms)"));
}

#[test]
fn cpp_shell_emits_fifo_stale_channel_reads_from_bind_policy() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "fifo"
depth = 4
overflow = "drop_oldest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(cpp_shell.contains(
            "flowrt::FifoChannel<Imu>::with_stale_config(4, flowrt::OverflowPolicy::DropOldest, flowrt::StaleConfig{std::chrono::milliseconds{20}, flowrt::StalePolicy::Error})"
        ));
    assert!(cpp_shell.contains("auto sink_imu_read = bind_0_.pop_at(tick_time_ms);"));
    assert!(cpp_shell.contains("const auto sink_imu = sink_imu_read.view();"));
    assert!(cpp_shell.contains("push_at(*value, tick_time_ms)"));
    assert!(
        cpp_shell.contains("if (sink_imu.stale()) {\n        return flowrt::Status::Error;\n    }")
    );
    assert!(
        cpp_shell.find("if (sink_imu.stale())").unwrap()
            < cpp_shell.find("sink_->on_tick(sink_imu)").unwrap()
    );
}

#[test]
fn cpp_shell_emits_stale_error_guard_before_user_tick() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Imu]
timestamp = "u64"
ax = "f32"

[component.source]
language = "cpp"
output = ["imu:Imu"]

[component.sink]
language = "cpp"
input = ["imu:Imu"]

[instance.source]
component = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.sink]
component = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["imu"]

[[bind.dataflow]]
from = "source.imu"
to = "sink.imu"
channel = "latest"
max_age_ms = 20
stale_policy = "error"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["cpp"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");

    assert!(
        cpp_shell.contains("if (sink_imu.stale()) {\n        return flowrt::Status::Error;\n    }")
    );
    assert!(
        cpp_shell.find("if (sink_imu.stale())").unwrap()
            < cpp_shell.find("sink_->on_tick(sink_imu)").unwrap()
    );
}

#[test]
fn rust_shell_accepts_multiple_binds_between_same_instance_pair() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["left:Sample", "right:Sample"]

[component.sink]
language = "rust"
input = ["left:Sample", "right:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["left", "right"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["left", "right"]

[[bind.dataflow]]
from = "source.left"
to = "sink.left"
channel = "latest"

[[bind.dataflow]]
from = "source.right"
to = "sink.right"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(rust_shell.contains("source: Box<dyn Source>"));
    assert!(rust_shell.contains("sink: Box<dyn Sink>"));
}

#[test]
fn rust_shell_uses_task_port_subset_for_channel_io() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.used_source]
language = "rust"
output = ["used_out:Sample"]

[component.unused_source]
language = "rust"
output = ["unused_out:Sample"]

[component.sink]
language = "rust"
input = ["used_in:Sample", "unused_in:Sample"]
output = ["used_out:Sample", "unused_out:Sample"]

[component.monitor]
language = "rust"
input = ["used_in:Sample", "unused_in:Sample"]

[instance.used_source]
component = "used_source"

[instance.used_source.task]
trigger = "periodic"
period_ms = 5
output = ["used_out"]

[instance.unused_source]
component = "unused_source"

[instance.unused_source.task]
trigger = "periodic"
period_ms = 5
output = ["unused_out"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["used_in"]
output = ["used_out"]

[instance.monitor]
component = "monitor"

[instance.monitor.task]
trigger = "on_message"
input = ["used_in", "unused_in"]

[[bind.dataflow]]
from = "used_source.used_out"
to = "sink.used_in"
channel = "latest"

[[bind.dataflow]]
from = "unused_source.unused_out"
to = "sink.unused_in"
channel = "latest"

[[bind.dataflow]]
from = "sink.used_out"
to = "monitor.used_in"
channel = "latest"

[[bind.dataflow]]
from = "sink.unused_out"
to = "monitor.unused_in"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let bind_index = |to_instance: &str, to_port: &str| {
        ir.graphs[0]
            .binds
            .iter()
            .position(|bind| {
                bind.to.instance.name == to_instance && bind.to.port.as_str() == to_port
            })
            .unwrap()
    };
    let sink_used_bind = bind_index("sink", "used_in");
    let sink_unused_bind = bind_index("sink", "unused_in");
    let monitor_used_bind = bind_index("monitor", "used_in");
    let sink_used_read =
        format!("        let used_in = self.bind_{sink_used_bind}.view_at(tick_time_ms);");
    let monitor_used_read =
        format!("        let used_in = self.bind_{monitor_used_bind}.view_at(tick_time_ms);");
    let sink_step_start = rust_shell.find(&sink_used_read).unwrap();
    let monitor_step_start = rust_shell.find(&monitor_used_read).unwrap();
    let sink_step = &rust_shell[sink_step_start..monitor_step_start];

    assert!(sink_step.contains(&sink_used_read));
    assert!(sink_step.contains("let unused_in = flowrt::Latest::new(None, false);"));
    assert!(sink_step.contains("let mut used_out = flowrt::Output::<Sample>::new();"));
    assert!(sink_step.contains("let mut unused_out = flowrt::Output::<Sample>::new();"));
    assert!(sink_step.contains("if used_in.present() {"));
    assert!(
        sink_step.contains("self.sink.on_tick(used_in, unused_in, &mut used_out, &mut unused_out)")
    );
    assert!(sink_step.contains("if let Some(value) = used_out.as_ref().cloned()"));
    assert!(!sink_step.contains(&format!(
        "self.bind_{sink_unused_bind}.view_at(tick_time_ms)"
    )));
    assert!(!sink_step.contains("if let Some(value) = unused_out.as_ref().cloned()"));
}

#[test]
fn rust_shell_runs_startup_and_shutdown_tasks_outside_tick_loop() {
    let mut ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.boot]
language = "rust"

[component.cleanup]
language = "rust"

[instance.boot]
component = "boot"

[instance.boot.task]
trigger = "periodic"
period_ms = 5

[instance.cleanup]
component = "cleanup"

[instance.cleanup.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    ir.graphs[0].tasks[0].trigger = TriggerKind::Startup;
    ir.graphs[0].tasks[0].period_ms = None;
    ir.graphs[0].tasks[1].trigger = TriggerKind::Shutdown;
    ir.graphs[0].tasks[1].period_ms = None;

    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let run_start = rust_shell.find("    pub fn run(").unwrap();
    let run = &rust_shell[run_start..];
    let startup_call = run
        .find("self.step_startup(0, &mut lifecycle_context, &introspection_state)")
        .unwrap();
    let scheduler_call = run.find("backend.scheduler().run_ticks").unwrap();
    let shutdown_call = run
        .find("self.step_shutdown(0, &mut lifecycle_context, &introspection_state)")
        .unwrap();
    let startup_step = generated_function_block(rust_shell, "fn step_startup");
    let shutdown_step = generated_function_block(rust_shell, "fn step_shutdown");
    let scheduler_step = generated_function_block(rust_shell, "fn step(");

    assert!(startup_call < scheduler_call);
    assert!(scheduler_call < shutdown_call);
    assert!(run.contains("let shutdown = flowrt::install_signal_shutdown_token();"));
    assert!(run.contains("&& !shutdown.is_requested()"));
    assert!(run.contains("backend.scheduler().run_ticks_until_shutdown("));
    assert!(startup_step.contains("if self.boot.on_tick()"));
    assert!(shutdown_step.contains("if self.cleanup.on_tick()"));
    assert!(!scheduler_step.contains("if self.boot.on_tick()"));
    assert!(!scheduler_step.contains("if self.cleanup.on_tick()"));
}

#[test]
fn cpp_shell_runs_startup_and_shutdown_tasks_outside_tick_loop() {
    let mut ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.boot]
language = "cpp"

[component.cleanup]
language = "cpp"

[instance.boot]
component = "boot"

[instance.boot.task]
trigger = "periodic"
period_ms = 5

[instance.cleanup]
component = "cleanup"

[instance.cleanup.task]
trigger = "periodic"
period_ms = 5
"#,
    );
    ir.graphs[0].tasks[0].trigger = TriggerKind::Startup;
    ir.graphs[0].tasks[0].period_ms = None;
    ir.graphs[0].tasks[1].trigger = TriggerKind::Shutdown;
    ir.graphs[0].tasks[1].period_ms = None;

    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let run_start = cpp_shell.find("flowrt::Status App::run(").unwrap();
    let run = &cpp_shell[run_start..];
    let startup_call = run
        .find("status = step_startup(0, lifecycle_context, introspection_state)")
        .unwrap();
    let scheduler_call = run.find("backend.scheduler().run_ticks").unwrap();
    let shutdown_call = run
        .find("status = step_shutdown(0, lifecycle_context, introspection_state)")
        .unwrap();

    assert!(startup_call < scheduler_call);
    assert!(scheduler_call < shutdown_call);
    assert!(run.contains("auto shutdown = flowrt::install_signal_shutdown_token();"));
    assert!(run.contains("!shutdown.is_requested()"));
    assert!(run.contains("backend.scheduler().run_ticks_until_shutdown("));
    assert!(cpp_shell.contains("flowrt::Status App::step_startup(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (boot_ && boot_->on_tick()"));
    assert!(cpp_shell.contains("flowrt::Status App::step_shutdown(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (cleanup_ && cleanup_->on_tick()"));
    assert!(!cpp_shell.contains("flowrt::Status App::step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (boot_ && boot_->on_tick()"));
    assert!(!cpp_shell.contains("flowrt::Status App::step(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state) {\n    (void)tick;\n    (void)tick_context;\n    (void)introspection_state;\n    if (cleanup_ && cleanup_->on_tick()"));
}

#[test]
fn rust_shell_enforces_task_deadline_before_publishing_outputs() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "rust"
output = ["sample:Sample"]

[component.sink]
language = "rust"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
deadline_ms = 10
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let deadline_start = rust_shell
        .find("let source_deadline_started_at = std::time::Instant::now();")
        .unwrap();
    let source_call = rust_shell
        .find("self.source.on_tick(&mut sample) != flowrt::Status::Ok")
        .unwrap();
    let deadline_guard = rust_shell
        .find("source_deadline_started_at.elapsed() > std::time::Duration::from_millis(10)")
        .unwrap();
    let publish = rust_shell
        .find("if let Some(value) = sample.as_ref().cloned()")
        .unwrap();

    assert!(deadline_start < source_call);
    assert!(source_call < deadline_guard);
    assert!(deadline_guard < publish);
}

#[test]
fn cpp_shell_enforces_task_deadline_before_publishing_outputs() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "cpp"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
deadline_ms = 10
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let deadline_start = cpp_shell
        .find("const auto source_deadline_started_at = std::chrono::steady_clock::now();")
        .unwrap();
    let source_call = cpp_shell
        .find("source_ && source_->on_tick(source_sample) != flowrt::Status::Ok")
        .unwrap();
    let deadline_guard = cpp_shell
            .find("std::chrono::steady_clock::now() - source_deadline_started_at > std::chrono::milliseconds{10}")
            .unwrap();
    let publish = cpp_shell
        .find("if (const auto* value = source_sample.as_ref())")
        .unwrap();

    assert!(deadline_start < source_call);
    assert!(source_call < deadline_guard);
    assert!(deadline_guard < publish);
}

#[test]
fn cpp_shell_gates_on_message_instances_on_present_inputs() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.source]
language = "cpp"
output = ["sample:Sample"]

[component.sink]
language = "cpp"
input = ["sample:Sample"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["sample"]

[instance.sink]
component = "sink"

[instance.sink.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "source.sample"
to = "sink.sample"
channel = "latest"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_shell = artifact_content(&bundle, "cpp/src/runtime_shell.cpp");
    let source_call = cpp_shell.find("source_->on_tick(source_sample)").unwrap();
    let gate = cpp_shell.find("if (sink_sample.present()) {").unwrap();
    let sink_call = cpp_shell.find("sink_->on_tick(sink_sample)").unwrap();

    assert!(source_call < gate);
    assert!(gate < sink_call);
}

#[test]
fn launch_manifest_groups_instances_by_process() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10
priority = 7

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let processes = launch["graphs"][0]["processes"].as_array().unwrap();

    assert_eq!(processes.len(), 2);
    assert_eq!(processes[0]["name"], "control");
    assert_eq!(processes[0]["backend"], "iox2");
    assert_eq!(processes[0]["target"], "linux");
    assert_eq!(processes[0]["runtimes"], serde_json::json!(["rust"]));
    assert_eq!(processes[0]["runtime_kind"], "rust");
    assert_eq!(processes[0]["instances"], serde_json::json!(["sink"]));
    assert_eq!(
        processes[0]["tasks"],
        serde_json::json!([
            {
                "instance": "sink",
                "trigger": "on_message",
                "period_ms": null,
                "deadline_ms": 10,
                "priority": 7,
                "inputs": ["value"],
                "outputs": []
            }
        ])
    );
    let graph_tasks = launch["graphs"][0]["tasks"].as_array().unwrap();
    let source_task = graph_tasks
        .iter()
        .find(|task| task["instance"] == "source")
        .unwrap();
    let sink_task = graph_tasks
        .iter()
        .find(|task| task["instance"] == "sink")
        .unwrap();
    assert_eq!(source_task["priority"], serde_json::json!(null));
    assert_eq!(source_task["inputs"], serde_json::json!([]));
    assert_eq!(source_task["outputs"], serde_json::json!(["value"]));
    assert_eq!(sink_task["priority"], 7);
    assert_eq!(sink_task["inputs"], serde_json::json!(["value"]));
    assert_eq!(sink_task["outputs"], serde_json::json!([]));
    assert_eq!(processes[1]["name"], "sensors");
    assert_eq!(processes[1]["backend"], "iox2");
    assert_eq!(processes[1]["target"], "linux");
    assert_eq!(processes[1]["runtimes"], serde_json::json!(["rust"]));
    assert_eq!(processes[1]["runtime_kind"], "rust");
    assert_eq!(processes[1]["instances"], serde_json::json!(["source"]));
}

#[test]
fn launch_manifest_marks_mixed_process_runtime_kind() {
    let ir = contract_from_source(
        r#"
[package]
name = "mixed_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "main"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "main"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["inproc"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let process = &launch["graphs"][0]["processes"][0];

    assert_eq!(process["name"], "main");
    assert_eq!(process["runtimes"], serde_json::json!(["cpp", "rust"]));
    assert_eq!(process["runtime_kind"], "mixed");
}

#[test]
fn launch_manifest_exposes_iox2_channel_services() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"

[instance.sink.task]
trigger = "on_message"
input = ["value"]

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"
default_overflow = "drop_oldest"
default_stale_policy = "warn"
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let channels = launch["graphs"][0]["channels"].as_array().unwrap();
    let channel = &channels[0];

    assert_eq!(channels.len(), 1);
    assert_eq!(channel["from"], "source.value");
    assert_eq!(channel["to"], "sink.value");
    assert_eq!(channel["backend"], "iox2");
    assert_eq!(
        channel["service"],
        "FlowRT/robot_demo/default/bind_0/source_value_to_sink_value"
    );
    assert_eq!(channel["channel"], "latest");
    assert_eq!(channel["depth"], 1);
    assert_eq!(channel["overflow"], "drop_oldest");
    assert_eq!(channel["stale_policy"], "warn");
    assert!(channel["max_age_ms"].is_null());
}

#[test]
fn rust_shell_exposes_process_run_entrypoint() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
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
    let rust_main = artifact_content(&bundle, "rust/src/main.rs");
    let rust_lib = artifact_content(&bundle, "rust/src/lib.rs");

    assert!(rust_shell.contains("pub fn run_process(self, backend: &dyn flowrt::Backend, process: &str, run_ticks: Option<usize>) -> flowrt::Status"));
    assert!(rust_shell.contains("\"control\" => self.run_process_control(backend, run_ticks)"));
    assert!(rust_shell.contains("\"sensors\" => self.run_process_sensors(backend, run_ticks)"));
    assert!(
        rust_shell.contains(
            "pub fn run_process(process: &str, run_ticks: Option<usize>) -> flowrt::Status"
        )
    );
    assert!(rust_main.contains("--process"));
    assert!(rust_main.contains("--flowrt-run-ticks"));
    assert!(rust_main.contains("flowrt_app::runtime_shell::run_process(process, run_ticks)"));
    assert!(rust_lib.contains("pub use runtime_shell::{run, run_process, App};"));
}

#[test]
fn cpp_shell_exposes_process_run_entrypoint() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "cpp"
input = ["value:u32"]

[instance.source]
component = "source"
process = "control"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

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
    let cpp_main = artifact_content(&bundle, "cpp/src/main.cpp");

    assert!(cpp_header.contains(
            "flowrt::Status step_process_control(std::size_t tick, flowrt::Context& tick_context, flowrt::IntrospectionState& introspection_state);"
        ));
    assert!(
            cpp_header
                .contains("flowrt::Status run_process_control(const flowrt::Backend& backend, std::optional<std::size_t> run_ticks);")
        );
    assert!(cpp_shell.contains("flowrt::Status App::step_process_control"));
    assert!(cpp_shell.contains("flowrt::Status App::run_process_control"));
    assert!(cpp_shell.contains("if (process == \"control\")"));
    assert!(cpp_shell.contains("return run_process_control(backend, run_ticks);"));
    assert!(cpp_main.contains("--process"));
    assert!(cpp_main.contains("--flowrt-run-ticks"));
    assert!(cpp_main.contains("flowrt_app::run_process(process, run_ticks)"));
}

#[test]
fn emits_rust_supervisor_artifacts_for_process_launch() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "rust"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "sensors"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "control"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let paths = bundle
        .artifacts
        .iter()
        .map(|artifact| artifact.relative_path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let supervisor = artifact_content(&bundle, "rust/src/supervisor.rs");
    let supervisor_main = artifact_content(&bundle, "rust/src/supervisor_main.rs");
    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");

    assert!(paths.contains(&"rust/src/supervisor.rs".to_string()));
    assert!(paths.contains(&"rust/src/supervisor_main.rs".to_string()));
    assert!(
        supervisor
            .contains("const LAUNCH_MANIFEST: &str = include_str!(\"../../launch/launch.json\");")
    );
    assert!(supervisor.contains("runtime_kind: String"));
    assert!(supervisor.contains("backend: String"));
    assert!(supervisor.contains("const RUST_APP_STEM: &str = \"robot-demo-flowrt-app\";"));
    assert!(supervisor.contains("Command::new(app_exe)"));
    assert!(supervisor.contains("flowrt::spawn_status_server("));
    assert!(supervisor.contains("record_process_health"));
    assert!(supervisor.contains("tick_stale"));
    assert!(supervisor.contains("try_wait()"));
    assert!(supervisor.contains("flowrt::request_status(&child.socket)"));
    assert!(supervisor.contains("struct RestartPolicy"));
    assert!(supervisor.contains("const DEFAULT_RESTART_POLICY: RestartPolicy"));
    assert!(supervisor.contains("restart_count: u32"));
    assert!(supervisor.contains("child.state = \"restarting\".to_string();"));
    assert!(supervisor.contains("restart_child(supervisor_state, child, run_ticks)?"));
    assert!(supervisor.contains("restart_count: child.restart_count"));
    assert!(supervisor.contains("if status.success()"));
    assert!(supervisor.contains("child.finished = true;"));
    assert!(supervisor.contains("for graph in &manifest.graphs"));
    assert!(supervisor.contains("zenoh_launch_env_for_graph(graph)?"));
    assert!(supervisor.contains("fn should_auto_configure_zenoh()"));
    assert!(supervisor.contains("fn zenoh_launch_env_for_graph("));
    assert!(supervisor.contains("TcpListener::bind(\"127.0.0.1:0\")"));
    assert!(supervisor.contains("command.env(\"FLOWRT_ZENOH_MODE\", \"peer\")"));
    assert!(supervisor.contains("command.env(\"FLOWRT_ZENOH_LISTEN\", &env.listen)"));
    assert!(supervisor.contains("command.env(\"FLOWRT_ZENOH_CONNECT\", &env.connect)"));
    assert!(supervisor.contains("command.env(\"FLOWRT_ZENOH_NO_MULTICAST\", \"1\")"));
    assert!(!supervisor.contains(".graphs\n        .first()"));
    assert!(
        supervisor.contains("app_executable_for_runtime(&current_exe, &process.runtime_kind)?")
    );
    assert!(supervisor.contains(".arg(\"--process\")"));
    assert!(supervisor.contains(".arg(process_name)"));
    assert!(supervisor.contains(".arg(\"--flowrt-run-ticks\")"));
    assert!(supervisor_main.contains("--flowrt-run-ticks"));
    assert!(supervisor_main.contains("flowrt_app::supervisor::launch(run_ticks)"));
    assert!(cargo_manifest.contains("[[bin]]\nname = \"robot-demo-flowrt-supervisor\""));
    assert!(cargo_manifest.contains("path = \"../rust/src/supervisor_main.rs\""));
    assert!(cargo_manifest.contains("serde = { version = \"1\", features = [\"derive\"] }"));
    assert!(cargo_manifest.contains("serde_json = \"1\""));
    assert!(cargo_manifest.find("serde =").unwrap() < cargo_manifest.find("[[bin]]").unwrap());
}

#[test]
fn rust_supervisor_selects_app_executable_from_runtime_kind() {
    let ir = contract_from_source(
        r#"
[package]
name = "robot_demo"
rsdl_version = "0.1"

[component.source]
language = "cpp"
output = ["value:u32"]

[component.sink]
language = "rust"
input = ["value:u32"]

[instance.source]
component = "source"
process = "cpp_source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["value"]

[instance.sink]
component = "sink"
process = "rust_sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["value"]
deadline_ms = 10

[[bind.dataflow]]
from = "source.value"
to = "sink.value"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["cpp", "rust"]
backends = ["iox2"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let supervisor = artifact_content(&bundle, "rust/src/supervisor.rs");

    assert!(supervisor.contains("runtime_kind: String"));
    assert!(supervisor.contains("backend: String"));
    assert!(supervisor.contains("const RUST_APP_STEM: &str = \"robot-demo-flowrt-app\";"));
    assert!(supervisor.contains("const CPP_APP_STEM: &str = \"robot_demo_cpp_app\";"));
    assert!(supervisor.contains("fn app_executable_for_runtime("));
    assert!(supervisor.contains("\"rust\" => rust_app_executable(current_exe),"));
    assert!(supervisor.contains("\"cpp\" => cpp_app_executable(current_exe),"));
    assert!(supervisor.contains("fn cpp_app_executable("));
    assert!(supervisor.contains("let mut path = build_dir.join(\"cmake\");"));
    assert!(supervisor.contains("path.push(binary_name(CPP_APP_STEM));"));
    assert!(supervisor.contains(
        "\"mixed\" => Err(\"FlowRT mixed process groups are not launchable yet\".to_string()),"
    ));
    assert!(
        supervisor.contains("app_executable_for_runtime(&current_exe, &process.runtime_kind)?")
    );
}

fn contract_from_source(source: &str) -> ContractIr {
    let raw = parse_str(source).unwrap();
    normalize_document(&raw, hash_source(source)).unwrap()
}

fn artifact_content<'a>(bundle: &'a ArtifactBundle, path: &str) -> &'a str {
    bundle
        .artifacts
        .iter()
        .find(|artifact| artifact.relative_path.as_path() == std::path::Path::new(path))
        .map(|artifact| artifact.content.as_str())
        .unwrap()
}

fn generated_function_block<'a>(source: &'a str, function: &str) -> &'a str {
    let start = source
        .find(function)
        .expect("generated function must exist");
    let rest = &source[start..];
    let next = rest[function.len()..]
        .find("\n    fn ")
        .map(|offset| function.len() + offset)
        .unwrap_or(rest.len());
    &rest[..next]
}

fn extract_probe_field_for_registration<'a>(
    source: &'a str,
    registration_marker: &str,
) -> Option<&'a str> {
    let marker_at = source.find(registration_marker)?;
    let before = &source[..marker_at];
    before
        .rsplit_once(|ch: char| ch.is_whitespace())
        .map(|(_, probe)| probe.trim())
        .filter(|probe| probe.starts_with("introspection_probe_bind_"))
        .or_else(|| {
            before
                .rsplit_once("self.")
                .map(|(_, probe)| probe.trim())
                .filter(|probe| probe.starts_with("introspection_probe_bind_"))
        })
        .or_else(|| {
            before
                .rsplit_once("this->")
                .map(|(_, probe)| probe.trim())
                .filter(|probe| probe.starts_with("introspection_probe_bind_"))
        })
}
