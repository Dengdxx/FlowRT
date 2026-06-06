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
fn validates_ros2_bridge_string_field_shape() {
    let ir = ros2_bridge_contract(r#""zenoh""#, "payload", "data");
    let report = validate_contract(&ir).expect_err("unknown bridge field should fail");

    assert!(report.errors.iter().any(|error| {
        error.message.contains(
            "ROS2 bridge `ros2_bridge_0` maps field `data`, but type `TextFrame` has no such field",
        )
    }));
}
