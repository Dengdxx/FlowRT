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
