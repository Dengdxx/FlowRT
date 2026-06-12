use super::*;

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
    assert!(!rust_shell.contains("SELECTED_BACKEND"));
    assert!(rust_shell.contains("Box::new(flowrt::iox2_backend())"));
    assert!(!rust_shell.contains("flowrt::inproc_backend()"));
    assert!(!rust_shell.contains("unsupported generated FlowRT backend"));
    assert!(!rust_shell.contains("panic!("));
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
    assert!(!rust_shell.contains("SELECTED_BACKEND"));
    assert!(rust_shell.contains("Box::new(flowrt::iox2_backend())"));
    assert!(!rust_shell.contains("flowrt::inproc_backend()"));
    assert!(!rust_shell.contains("unsupported generated FlowRT backend"));
    assert!(!rust_shell.contains("panic!("));
}

#[test]
fn rejects_native_zenoh_service_before_placeholder_codegen() {
    let ir = contract_from_source(
        r#"
[package]
name = "service_backend_demo"
rsdl_version = "0.1"

[component.client]
language = "rust"
service_client = ["plan:u32->bool"]

[component.server]
language = "rust"
service_server = ["plan:u32->bool"]

[instance.client]
component = "client"
process = "client_proc"
target = "linux"

[instance.server]
component = "server"
process = "server_proc"
target = "linux"

[[bind.service]]
client = "client.plan"
server = "server.plan"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = ["inproc", "zenoh"]
"#,
    );
    let error = emit_artifacts(&ir).expect_err("native zenoh service codegen must fail fast");

    assert!(
        error.to_string().contains("generated Service codegen"),
        "unexpected error: {error}"
    );
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
    assert!(cmake.contains("list(PREPEND CMAKE_PREFIX_PATH \"${FLOWRT_CPP_RUNTIME_DIR}\")"));
    assert!(cmake.contains("list(PREPEND CMAKE_BUILD_RPATH \"${FLOWRT_CPP_RUNTIME_DIR}/lib\")"));
    assert!(cmake.contains("find_package(iceoryx2-cxx 0.9.1 QUIET)"));
    assert!(cmake.contains("if(NOT TARGET iceoryx2-cxx::static-lib-cxx)"));
    assert!(!cmake.contains("if(NOT iceoryx2-cxx_FOUND AND NOT TARGET"));
    assert!(!cmake.contains("FetchContent"));
    assert!(!cmake.contains("GIT_REPOSITORY"));
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
fn rust_transport_channel_init_records_startup_error_without_panicking() {
    let ir = contract_from_source(
        r#"
[package]
name = "startup_test"
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

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["rust"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(!rust_shell.contains(".expect(\"failed to open FlowRT"));
    assert!(rust_shell.contains("startup_status: flowrt::Status"));
    assert!(rust_shell.contains("let mut startup_status = flowrt::Status::Ok;"));
    assert!(rust_shell.contains("startup_status = flowrt::Status::Error;"));
    assert!(rust_shell.contains("ZenohPubSub::unavailable("));
    assert!(
        rust_shell.contains("if self.startup_status != flowrt::Status::Ok {\n            return self.startup_status;\n        }")
    );
}

#[test]
fn rust_user_factory_still_returns_app_after_transport_startup_hardening() {
    let ir = contract_from_source(
        r#"
[package]
name = "factory_test"
rsdl_version = "0.1"

[component.worker]
language = "rust"

[instance.worker]
component = "worker"
target = "linux"

[instance.worker.task]
trigger = "periodic"
period_ms = 1

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
        rust_shell.contains("pub fn new(\n        worker: Box<dyn Worker + Send>,\n    ) -> Self")
    );
    assert!(rust_shell.contains("user::build_app().run(backend.as_ref(), run_ticks)"));
    assert!(!rust_shell.contains("match user::build_app()"));
    assert!(!rust_shell.contains("-> Result<Self"));
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
    assert!(!rust_shell.contains("SELECTED_BACKEND"));
    assert!(rust_shell.contains("Box::new(flowrt::zenoh_backend())"));
    assert!(!rust_shell.contains("flowrt::inproc_backend()"));
    assert!(!rust_shell.contains("flowrt::iox2_backend()"));
    assert!(!rust_shell.contains("unsupported generated FlowRT backend"));
    assert!(!rust_shell.contains("panic!("));
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
