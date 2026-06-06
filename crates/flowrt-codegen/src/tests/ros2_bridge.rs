use super::*;

#[test]
fn emits_ros2_bridge_adapter_over_zenoh() {
    let ir = contract_from_source(
        r#"
[package]
name = "ros2_bridge_demo"
rsdl_version = "0.1"

[type.TextFrame]
data = "string"

[component.source]
language = "rust"
output = ["text:TextFrame"]

[instance.source]
component = "source"
process = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 10
output = ["text"]

[[bridge.ros2]]
flowrt = "source.text"
ros2_topic = "/flowrt/text"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"
field = "data"

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let adapter = artifact_content(&bundle, "cpp/src/ros2_bridge.cpp");
    let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
    let launch = artifact_content(&bundle, "launch/launch.json");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(adapter.contains("#include <rclcpp/rclcpp.hpp>"));
    assert!(adapter.contains("#include <std_msgs/msg/string.hpp>"));
    assert!(adapter.contains("#include <zenoh.hxx>"));
    assert!(adapter.contains("setenv(\"RMW_IMPLEMENTATION\", \"rmw_zenoh_cpp\", 0);"));
    assert!(adapter.contains("BridgeZenohLatest<TextFrame>"));
    assert!(!adapter.contains("flowrt::zenoh::ZenohPubSub<TextFrame>"));
    assert!(adapter.contains(
        "\"flowrt/ros2_bridge_demo/default/default/ros2_bridge_0/source_text_to__flowrt_text\""
    ));
    assert!(adapter.contains("message.data = value.data;"));

    assert!(cmake.contains("find_package(rclcpp REQUIRED)"));
    assert!(cmake.contains("FLOWRT_AMENT_PREFIX_PATH"));
    assert!(cmake.contains("list(PREPEND CMAKE_PREFIX_PATH ${FLOWRT_AMENT_PREFIX_PATH})"));
    assert!(cmake.contains("find_package(std_msgs REQUIRED)"));
    assert!(
        cmake.contains("add_executable(ros2_bridge_demo_ros2_bridge ../cpp/src/ros2_bridge.cpp)")
    );
    assert!(cmake.contains("find_package(rmw_zenoh_cpp REQUIRED)"));
    assert!(cmake.contains("find_package(zenoh_cpp_vendor REQUIRED)"));
    assert!(cmake.contains("FLOWRT_ROS2_ZENOH_VENDOR_PREFIX"));
    assert!(!cmake.contains("find_package(zenohc 1.9.0 QUIET)"));
    assert!(!cmake.contains("FLOWRT_ZENOH_CXX_TARGET"));
    assert!(cmake.contains(
        "target_include_directories(ros2_bridge_demo_ros2_bridge BEFORE PRIVATE ${FLOWRT_ROS2_ZENOH_INCLUDE})"
    ));
    assert!(
        cmake.contains("target_link_libraries(ros2_bridge_demo_ros2_bridge PRIVATE rclcpp::rclcpp")
    );
    assert!(!cmake.contains(
        "target_link_libraries(ros2_bridge_demo_ros2_bridge PRIVATE ros2_bridge_demo_flowrt_app"
    ));
    assert!(cmake.contains("rosidl_typesupport_cpp::rosidl_typesupport_cpp"));

    assert!(launch.contains("\"name\": \"ros2_bridge\""));
    assert!(launch.contains("\"runtime_kind\": \"ros2_bridge\""));
    assert!(launch.contains("\"backend\": \"zenoh\""));

    assert!(rust_shell.contains("flowrt::zenoh::ZenohPubSub<TextFrame>"));
    assert!(rust_shell.contains("ros2_bridge_0"));
}
