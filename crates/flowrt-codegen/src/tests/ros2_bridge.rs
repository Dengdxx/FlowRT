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
    assert!(!adapter.contains("#include <geometry_msgs/msg/pose.hpp>"));
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
    assert!(!cmake.contains("find_package(geometry_msgs REQUIRED)"));
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
    assert!(cmake.contains(
        "target_link_options(ros2_bridge_demo_ros2_bridge PRIVATE \"-Wl,--disable-new-dtags\")"
    ));
    assert!(cmake.contains(
        "set_property(TARGET ros2_bridge_demo_ros2_bridge PROPERTY BUILD_RPATH \"${FLOWRT_ROS2_ZENOH_VENDOR_PREFIX}/lib;${FLOWRT_ROS2_BRIDGE_BUILD_RPATH}\")"
    ));
    assert!(!cmake.contains("geometry_msgs::geometry_msgs__rosidl_typesupport_cpp"));
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

#[test]
fn emits_ros2_bridge_bidirectional_pose_slice() {
    let ir = contract_from_source(
        r#"
[package]
name = "ros2_pose_bridge_demo"
rsdl_version = "0.1"

[type.TextFrame]
data = "string"

[type.Pose]
position = "Point3"
orientation = "Quaternion"

[type.Point3]
x = "f64"
y = "f64"
z = "f64"

[type.Quaternion]
x = "f64"
y = "f64"
z = "f64"
w = "f64"

[component.source]
language = "rust"
output = ["text:TextFrame", "pose:Pose"]

[component.sink]
language = "rust"
input = ["pose:Pose"]

[instance.source]
component = "source"
process = "source"
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 10
output = ["text", "pose"]

[instance.sink]
component = "sink"
process = "sink"
target = "linux"

[instance.sink.task]
trigger = "on_message"
input = ["pose"]

[[bridge.ros2]]
flowrt = "source.text"
ros2_topic = "/flowrt/text"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"
field = "data"

[[bridge.ros2]]
flowrt = "source.pose"
ros2_topic = "/flowrt/pose"
ros2_type = "geometry_msgs/msg/Pose"
direction = "flowrt_to_ros2"

[[bridge.ros2]]
flowrt = "sink.pose"
ros2_topic = "/ros2/pose"
ros2_type = "geometry_msgs/msg/Pose"
direction = "ros2_to_flowrt"

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

    assert!(adapter.contains("#include <geometry_msgs/msg/pose.hpp>"));
    assert!(adapter.contains("BridgeZenohLatest<Pose>"));
    assert!(adapter.contains("BridgeZenohPublisher<Pose>"));
    assert!(
        adapter.contains("rclcpp::Subscription<geometry_msgs::msg::Pose>::SharedPtr subscriber;")
    );
    assert!(adapter.contains("message.position.x = value.position.x;"));
    assert!(adapter.contains("message.orientation.w = value.orientation.w;"));
    assert!(adapter.contains("value.position.x = message.position.x;"));
    assert!(adapter.contains("value.orientation.w = message.orientation.w;"));
    assert!(adapter.contains("endpoint->publish(value, now_ms());"));

    assert!(cmake.contains("find_package(geometry_msgs REQUIRED)"));
    assert!(
        cmake.contains("geometry_msgs::geometry_msgs__rosidl_typesupport_cpp"),
        "{cmake}"
    );

    assert!(launch.contains("\"direction\": \"flowrt_to_ros2\""));
    assert!(launch.contains("\"direction\": \"ros2_to_flowrt\""));
    assert!(launch.contains("\"ros2_type\": \"geometry_msgs/msg/Pose\""));

    assert!(rust_shell.contains("flowrt::zenoh::ZenohPubSub<Pose>"));
    assert!(rust_shell.contains(
        "let _ = app.ros2_bridge_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).receive_latest_at(tick_time_ms);"
    ));
    assert!(rust_shell.contains("let __flowrt_ros2_bridge_2_guard = self.ros2_bridge_2.lock().unwrap_or_else(|poisoned| poisoned.into_inner());"));
    assert!(
        rust_shell
            .contains("let pose = __flowrt_ros2_bridge_2_guard.cached_latest_at(tick_time_ms);")
    );
    assert!(
        rust_shell
            .contains("let __flowrt_pose_revision = __flowrt_ros2_bridge_2_guard.revision();")
    );
    assert!(rust_shell.contains("name: \"ros2_bridge_1\".to_string(),"));
    assert!(rust_shell.contains("from: \"source.pose\".to_string(),"));
    assert!(rust_shell.contains("to: \"ros2:/flowrt/pose\".to_string(),"));
    assert!(rust_shell.contains("name: \"ros2_bridge_2\".to_string(),"));
    assert!(rust_shell.contains("from: \"ros2:/ros2/pose\".to_string(),"));
    assert!(rust_shell.contains("to: \"sink.pose\".to_string(),"));
    assert!(rust_shell.contains("selected_reason: \"ros2_bridge\".to_string(),"));
    assert!(rust_shell.contains("task: \"sink.main\".to_string(),"));
    assert!(rust_shell.contains("input: \"pose\".to_string(),"));
    assert!(rust_shell.contains("channel: \"ros2_bridge_2\".to_string(),"));
    assert!(rust_shell.contains(
        "introspection_state.record_route_publish(\"ros2_bridge_1\", Some(tick_time_ms));"
    ));
    assert!(
        rust_shell.contains(
            "introspection_state.record_route_error(\"ros2_bridge_1\", error.to_string());"
        )
    );
    assert!(rust_shell.contains(
        "record_introspection_input_read(&introspection_state, \"sink.main.pose\", \"sink.main\", \"pose\", \"ros2_bridge_2\", \"Pose\", &pose, __flowrt_pose_revision, tick_time_ms);"
    ));
    assert!(!rust_shell.contains("task input `sink.pose` has no incoming bind"));
}

#[test]
fn emits_ros2_bridge_for_island_boundary_endpoints() {
    let ir = contract_from_source(
        r#"
[package]
name = "ros2_boundary_demo"
rsdl_version = "0.1"

[type.TextFrame]
data = "string"

[component.echo]
language = "rust"
input = ["request:TextFrame"]
output = ["reply:TextFrame"]

[instance.echo]
component = "echo"
process = "echo"
target = "linux"

[instance.echo.task]
trigger = "on_message"
input = ["request"]
output = ["reply"]

[profile.default]
backend = "zenoh"
mode = "island"

[[boundary.input]]
name = "request_in"
port = "echo.request"
type = "TextFrame"

[[boundary.output]]
name = "reply_out"
port = "echo.reply"
type = "TextFrame"

[[bridge.ros2]]
flowrt = "request_in"
ros2_topic = "/ros2/request"
ros2_type = "std_msgs/msg/String"
direction = "ros2_to_flowrt"
field = "data"

[[bridge.ros2]]
flowrt = "reply_out"
ros2_topic = "/flowrt/reply"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"
field = "data"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let adapter = artifact_content(&bundle, "cpp/src/ros2_bridge.cpp");
    let launch = artifact_content(&bundle, "launch/launch.json");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");

    assert!(adapter.contains("BridgeZenohPublisher<TextFrame>"));
    assert!(adapter.contains("BridgeZenohLatest<TextFrame>"));
    assert!(adapter.contains(
        "\"flowrt/ros2_boundary_demo/default/default/ros2_bridge_0/boundary_request_in_to__ros2_request\""
    ));
    assert!(adapter.contains(
        "\"flowrt/ros2_boundary_demo/default/default/ros2_bridge_1/boundary_reply_out_to__flowrt_reply\""
    ));
    assert!(!adapter.contains("iox2"));

    assert!(launch.contains("\"mode\": \"island\""));
    assert!(launch.contains("\"boundary_endpoint\": \"request_in\""));
    assert!(launch.contains("\"boundary_endpoint\": \"reply_out\""));
    assert!(launch.contains("\"flowrt\": \"boundary:request_in\""));
    assert!(launch.contains("\"flowrt\": \"boundary:reply_out\""));

    assert!(rust_shell.contains("boundary_input_request_in: flowrt::BoundaryInput<TextFrame>"));
    assert!(rust_shell.contains("boundary_output_reply_out: flowrt::BoundaryOutput<TextFrame>"));
    assert!(
        rust_shell.contains("app.ros2_bridge_0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).set_schedule_waiter(scheduler_events.clone());")
    );
    assert!(rust_shell.contains("Ok(value) => value.as_ref().cloned(),"));
    assert!(rust_shell.contains("self.boundary_input_request_in.inject_at(value, tick_time_ms);"));
    assert!(
        rust_shell
            .contains("let request_read = self.boundary_input_request_in.read_at(tick_time_ms);")
    );
    assert!(
        rust_shell.contains(
            "let mut __flowrt_route = self.ros2_bridge_1.lock().unwrap_or_else(|poisoned| poisoned.into_inner());"
        )
    );
    assert!(rust_shell.contains("let __flowrt_route_health = __flowrt_route.health();"));
    assert!(rust_shell.contains(
        "introspection_state.record_route_backend_health(\"ros2_bridge_1\", __flowrt_route_health);"
    ));
    assert!(rust_shell.contains("from: \"ros2:/ros2/request\".to_string()"));
    assert!(rust_shell.contains("to: \"boundary:request_in\".to_string()"));
    assert!(rust_shell.contains("from: \"boundary:reply_out\".to_string()"));
    assert!(rust_shell.contains("to: \"ros2:/flowrt/reply\".to_string()"));
    assert!(!rust_shell.contains("task input `echo.request` has no incoming bind"));

    let cpp_ir = contract_from_source(
        r#"
[package]
name = "ros2_boundary_cpp_demo"
rsdl_version = "0.1"

[type.TextFrame]
data = "string"

[component.echo]
language = "cpp"
input = ["request:TextFrame"]
output = ["reply:TextFrame"]

[instance.echo]
component = "echo"
process = "echo"
target = "linux"

[instance.echo.task]
trigger = "on_message"
input = ["request"]
output = ["reply"]

[profile.default]
backend = "zenoh"
mode = "island"

[[boundary.input]]
name = "request_in"
port = "echo.request"
type = "TextFrame"

[[boundary.output]]
name = "reply_out"
port = "echo.reply"
type = "TextFrame"

[[bridge.ros2]]
flowrt = "request_in"
ros2_topic = "/ros2/request"
ros2_type = "std_msgs/msg/String"
direction = "ros2_to_flowrt"
field = "data"

[[bridge.ros2]]
flowrt = "reply_out"
ros2_topic = "/flowrt/reply"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"
field = "data"

[target.linux]
runtime = ["cpp"]
backends = ["zenoh"]
"#,
    );
    let cpp_bundle = emit_artifacts(&cpp_ir).unwrap();
    let cpp_shell = artifact_content(&cpp_bundle, "cpp/src/runtime_shell.cpp");

    assert!(cpp_shell.contains("ros2_bridge_0_.set_schedule_waiter(scheduler_events);"));
    assert!(cpp_shell.contains("boundary_input_request_in_.inject_at(*value, tick_time_ms);"));
    assert!(cpp_shell.contains("boundary_output_reply_out_.publish_at(*value, tick_time_ms);"));
    assert!(cpp_shell.contains("ros2_bridge_1_.publish_at(*value, tick_time_ms)"));
}
