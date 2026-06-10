use super::*;

fn ros2_bridge_contract(
    target_backends: &str,
    message_field: &str,
    bridge_field: &str,
) -> ContractIr {
    let source = format!(
        r#"
[package]
name = "ros2_bridge_demo"
rsdl_version = "0.1"

[type.TextFrame]
{message_field} = "string"

[component.source]
language = "rust"
output = ["text:TextFrame"]

[instance.source]
component = "source"
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
field = "{bridge_field}"

[profile.default]
backend = "inproc"

[target.linux]
runtime = ["rust"]
backends = [{target_backends}]
"#
    );
    let raw = parse_str(&source).unwrap();
    normalize_document(&raw, hash_source(&source)).unwrap()
}

fn ros2_pose_bridge_contract(ros2_type: &str) -> ContractIr {
    let source = format!(
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
target = "linux"

[instance.source.task]
trigger = "periodic"
period_ms = 10
output = ["text", "pose"]

[instance.sink]
component = "sink"
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
ros2_type = "{ros2_type}"
direction = "flowrt_to_ros2"

[[bridge.ros2]]
flowrt = "sink.pose"
ros2_topic = "/ros2/pose"
ros2_type = "{ros2_type}"
direction = "ros2_to_flowrt"

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["rust"]
backends = ["zenoh"]
"#
    );
    let raw = parse_str(&source).unwrap();
    normalize_document(&raw, hash_source(&source)).unwrap()
}

#[test]
fn validates_ros2_bridge_requires_zenoh_backend_on_source_target() {
    let ir = ros2_bridge_contract(r#""inproc", "zenoh""#, "data", "data");
    validate_contract(&ir).unwrap();

    let ir = ros2_bridge_contract(r#""inproc""#, "data", "data");
    let report = validate_contract(&ir).expect_err("ROS2 bridge must require zenoh");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "ROS2 bridge `ros2_bridge_0` requires target `linux` to support backend `zenoh`",
        )
    }));
}

#[test]
fn validates_ros2_bridge_pose_bidirectional_subset() {
    let ir = ros2_pose_bridge_contract("geometry_msgs/msg/Pose");
    validate_contract(&ir).unwrap();
}

#[test]
fn rejects_unsupported_ros2_bridge_type() {
    let ir = ros2_pose_bridge_contract("sensor_msgs/msg/Image");
    let report = validate_contract(&ir).expect_err("unsupported ROS2 type should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "ROS2 bridge `ros2_bridge_1` uses unsupported ROS2 type `sensor_msgs/msg/Image`",
        )
    }));
}

#[test]
fn validates_ros2_bridge_string_field_shape() {
    let ir = ros2_bridge_contract(r#""zenoh""#, "payload", "data");
    let report = validate_contract(&ir).expect_err("unknown bridge field should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "ROS2 bridge `ros2_bridge_0` maps field `data`, but type `TextFrame` has no such field",
        )
    }));
}
