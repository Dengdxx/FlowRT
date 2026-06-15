use super::*;

#[test]
fn normalizes_ros2_bridge_bidirectional_typed_slice() {
    let source = r#"
[package]
name = "ros2_bridge_demo"
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

[instance.sink]
component = "sink"
target = "linux"

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
runtime = ["rust"]
backends = ["zenoh"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let bridges = &ir.graphs[0].ros2_bridges;

    assert_eq!(bridges.len(), 3);
    assert_eq!(bridges[0].direction, Ros2BridgeDirection::FlowrtToRos2);
    assert_eq!(bridges[1].direction, Ros2BridgeDirection::FlowrtToRos2);
    assert_eq!(bridges[2].direction, Ros2BridgeDirection::Ros2ToFlowrt);
    assert_eq!(bridges[2].flowrt.instance.name, "sink");
    assert_eq!(bridges[2].flowrt.port, "pose");
    assert_eq!(bridges[2].backend.0, "zenoh");
}

#[test]
fn normalizes_ros2_bridge_boundary_endpoint_refs() {
    let source = r#"
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

[[bridge.ros2]]
flowrt = "reply_out"
ros2_topic = "/flowrt/reply"
ros2_type = "std_msgs/msg/String"
direction = "flowrt_to_ros2"

[target.linux]
runtime = ["rust"]
backends = ["zenoh"]
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let bridges = &ir.graphs[0].ros2_bridges;

    assert_eq!(bridges.len(), 2);
    assert_eq!(bridges[0].flowrt.instance.name, "echo");
    assert_eq!(bridges[0].flowrt.port, "request");
    assert_eq!(
        bridges[0]
            .boundary_endpoint
            .as_ref()
            .map(|endpoint| endpoint.name.as_str()),
        Some("request_in")
    );
    assert_eq!(bridges[1].flowrt.instance.name, "echo");
    assert_eq!(bridges[1].flowrt.port, "reply");
    assert_eq!(
        bridges[1]
            .boundary_endpoint
            .as_ref()
            .map(|endpoint| endpoint.name.as_str()),
        Some("reply_out")
    );
}

#[test]
fn normalizes_workspace_module_qualified_names() {
    let root = unique_temp_dir();
    std::fs::create_dir_all(root.join("modules")).unwrap();
    std::fs::create_dir_all(root.join("composition")).unwrap();

    std::fs::write(
        root.join("robot.rsdl"),
        r#"
[package]
name = "workspace_demo"
rsdl_version = "0.1"

[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("modules/perception.rsdl"),
        r#"
[module]
name = "perception"

[type.Imu]
timestamp = "u64"

[component.imu_sim]
language = "rust"
output = ["imu:Imu"]
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("modules/control.rsdl"),
        r#"
[module]
name = "control"

[type.Odom]
timestamp = "u64"

[component.estimator]
language = "rust"
input = ["imu:perception::Imu"]
output = ["odom:Odom"]
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("composition/default.rsdl"),
        r#"
[instance.imu_sim]
component = "perception::imu_sim"

[instance.imu_sim.task]
trigger = "periodic"
period_ms = 5
output = ["imu"]

[instance.estimator]
component = "control::estimator"

[instance.estimator.task]
trigger = "on_message"
input = ["imu"]
output = ["odom"]

[[bind.dataflow]]
from = "imu_sim.imu"
to = "estimator.imu"
channel = "latest"
"#,
    )
    .unwrap();

    let loaded = load_file(root.join("robot.rsdl")).unwrap();
    let ir = normalize_loaded_document(&loaded, hash_source(&loaded.source_bundle_text())).unwrap();

    assert_eq!(ir.modules.len(), 2);
    assert_eq!(ir.modules[0].name, "control");
    assert_eq!(ir.types[0].qualified_name, "control::Odom");
    assert_eq!(ir.types[1].qualified_name, "perception::Imu");
    assert_eq!(ir.components[0].qualified_name, "control::estimator");
    assert_eq!(ir.components[1].qualified_name, "perception::imu_sim");
    assert_eq!(
        ir.graphs[0].instances[0].component.name,
        "control::estimator"
    );
    assert_eq!(
        ir.components[0].inputs[0].ty.canonical_syntax(),
        "perception::Imu"
    );

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejects_ambiguous_short_type_references_across_modules() {
    let root = unique_temp_dir();
    std::fs::create_dir_all(root.join("modules")).unwrap();
    std::fs::create_dir_all(root.join("composition")).unwrap();

    std::fs::write(
        root.join("robot.rsdl"),
        r#"
[package]
name = "workspace_demo"
rsdl_version = "0.1"

[workspace]
modules = ["modules/*.rsdl"]
compositions = ["composition/default.rsdl"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("modules/a.rsdl"),
        r#"
[module]
name = "perception"

[type.Sample]
value = "u32"
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("modules/b.rsdl"),
        r#"
[module]
name = "control"

[type.Sample]
value = "u64"
"#,
    )
    .unwrap();
    std::fs::write(root.join("composition/default.rsdl"), "").unwrap();

    let loaded = load_file(root.join("robot.rsdl")).unwrap();
    let error = normalize_loaded_document(&loaded, hash_source(&loaded.source_bundle_text()))
        .expect_err("ambiguous short type reference should fail");

    assert!(matches!(
        error,
        IrError::AmbiguousName { kind: "type", name, .. } if name == "Sample"
    ));

    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn normalizes_island_profile_and_boundary_endpoints() {
    let source = r#"
[package]
name = "island_demo"
rsdl_version = "0.1"

[type.Command]
speed = "f32"

[type.Scan]
range = "f32"

[component.planner]
language = "rust"
input = ["scan:Scan"]
output = ["cmd:Command"]

[instance.planner]
component = "planner"

[instance.planner.task]
trigger = "on_message"
input = ["scan"]
output = ["cmd"]

[profile.dev]
mode = "island"
backend = "inproc"

[[boundary.output]]
name = "cmd_out"
port = "planner.cmd"
type = "Command"

[[boundary.input]]
name = "scan_in"
port = "planner.scan"
type = "Scan"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();

    assert_eq!(ir.profiles[0].mode, GraphMode::Island);
    assert_eq!(ir.graphs[0].boundary_endpoints.len(), 2);

    let input = &ir.graphs[0].boundary_endpoints[0];
    assert!(input.id.0.starts_with("boundary_"));
    assert_eq!(input.name, "scan_in");
    assert_eq!(input.direction, BoundaryDirection::Input);
    assert_eq!(input.port.instance.name, "planner");
    assert_eq!(input.port.port, "scan");
    assert_eq!(
        input.ty,
        TypeExpr::Named {
            name: "Scan".to_string()
        }
    );

    let output = &ir.graphs[0].boundary_endpoints[1];
    assert_eq!(output.name, "cmd_out");
    assert_eq!(output.direction, BoundaryDirection::Output);
    assert_eq!(output.port.instance.name, "planner");
    assert_eq!(output.port.port, "cmd");
    assert_eq!(
        output.ty,
        TypeExpr::Named {
            name: "Command".to_string()
        }
    );

    let json = ir.to_canonical_json().unwrap();
    assert!(json.contains("\"mode\": \"island\""));
    assert!(json.contains("\"boundary_endpoints\""));
    assert!(json.contains("\"direction\": \"input\""));
}

#[test]
fn temporary_island_overlay_projects_strict_contract_to_test_only_island() {
    let source = r#"
[package]
name = "temporary_island_demo"
rsdl_version = "0.1"

[type.Command]
speed = "f32"

[type.Scan]
range = "f32"

[component.planner]
language = "rust"
input = ["scan:Scan"]
output = ["cmd:Command"]

[instance.planner]
component = "planner"

[instance.planner.task]
trigger = "on_message"
input = ["scan"]
output = ["cmd"]

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let projected = project_contract_to_profile(&ir, None).unwrap();
    let overlay = TemporaryIslandOverlay {
        boundary_inputs: vec![TemporaryBoundaryMapping {
            name: "scan_in".to_string(),
            endpoint: "planner.scan".to_string(),
        }],
        boundary_outputs: vec![TemporaryBoundaryMapping {
            name: "cmd_out".to_string(),
            endpoint: "planner.cmd".to_string(),
        }],
        generated_by: Default::default(),
    };

    let island = apply_temporary_island_overlay(&projected, &overlay).unwrap();

    assert_eq!(island.profiles[0].mode, GraphMode::Island);
    assert_eq!(island.artifact.mode, GraphMode::Island);
    assert!(island.artifact.temporary_island);
    assert!(island.artifact.test_only);
    let overlay_metadata = island
        .artifact
        .temporary_overlay
        .as_ref()
        .expect("temporary island artifact must record overlay metadata");
    assert_eq!(overlay_metadata.kind, "temporary_island");
    assert_eq!(overlay_metadata.original_profile_mode, GraphMode::Strict);
    assert_eq!(overlay_metadata.generated_by.command, "flowrt prepare");
    assert_eq!(overlay_metadata.generated_by.source, "cli");
    assert_eq!(overlay_metadata.boundary_mappings.len(), 2);
    assert_eq!(
        overlay_metadata.boundary_mappings[0].source,
        "--boundary-input"
    );
    assert_eq!(overlay_metadata.boundary_mappings[0].name, "scan_in");
    assert_eq!(
        overlay_metadata.boundary_mappings[0].endpoint,
        "planner.scan"
    );
    assert_eq!(
        overlay_metadata.boundary_mappings[1].source,
        "--boundary-output"
    );
    assert_eq!(island.graphs[0].boundary_endpoints.len(), 2);
    assert_eq!(island.graphs[0].boundary_endpoints[0].name, "scan_in");
    assert_eq!(
        island.graphs[0].boundary_endpoints[0].ty,
        TypeExpr::Named {
            name: "Scan".to_string()
        }
    );
    assert_eq!(island.graphs[0].boundary_endpoints[1].name, "cmd_out");
    assert_eq!(
        island.graphs[0].boundary_endpoints[1].ty,
        TypeExpr::Named {
            name: "Command".to_string()
        }
    );
}

#[test]
fn temporary_island_overlay_rejects_duplicate_boundary_names() {
    let source = r#"
[package]
name = "temporary_duplicate_demo"
rsdl_version = "0.1"

[component.consumer]
language = "rust"
input = ["sample:u32"]
output = ["sample:u32"]

[instance.consumer]
component = "consumer"

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let projected = project_contract_to_profile(&ir, None).unwrap();
    let overlay = TemporaryIslandOverlay {
        boundary_inputs: vec![TemporaryBoundaryMapping {
            name: "sample".to_string(),
            endpoint: "consumer.sample".to_string(),
        }],
        boundary_outputs: vec![TemporaryBoundaryMapping {
            name: "sample".to_string(),
            endpoint: "consumer.sample".to_string(),
        }],
        generated_by: Default::default(),
    };

    let error = apply_temporary_island_overlay(&projected, &overlay)
        .expect_err("duplicate boundary names should fail");

    assert!(
        error
            .to_string()
            .contains("duplicate temporary boundary name")
    );
}

#[test]
fn temporary_island_overlay_rejects_input_with_existing_dataflow_bind() {
    let source = r#"
[package]
name = "temporary_overlap_demo"
rsdl_version = "0.1"

[type.Sample]
value = "u32"

[component.producer]
language = "rust"
output = ["sample:Sample"]

[component.consumer]
language = "rust"
input = ["sample:Sample"]

[instance.producer]
component = "producer"

[instance.producer.task]
trigger = "periodic"
period_ms = 10
output = ["sample"]

[instance.consumer]
component = "consumer"

[instance.consumer.task]
trigger = "on_message"
input = ["sample"]

[[bind.dataflow]]
from = "producer.sample"
to = "consumer.sample"
channel = "latest"

[profile.default]
backend = "inproc"
"#;
    let raw = parse_str(source).unwrap();
    let ir = normalize_document(&raw, hash_source(source)).unwrap();
    let projected = project_contract_to_profile(&ir, None).unwrap();
    let overlay = TemporaryIslandOverlay {
        boundary_inputs: vec![TemporaryBoundaryMapping {
            name: "sample_in".to_string(),
            endpoint: "consumer.sample".to_string(),
        }],
        boundary_outputs: vec![],
        generated_by: Default::default(),
    };

    let error = apply_temporary_island_overlay(&projected, &overlay)
        .expect_err("dataflow plus temporary boundary input should fail");

    assert!(
        error
            .to_string()
            .contains("already has an incoming dataflow bind")
    );
}

#[test]
fn canonicalizes_boundary_endpoint_order_independent_of_source_order() {
    let source_a = r#"
[package]
name = "island_demo"
rsdl_version = "0.1"

[type.Command]
speed = "f32"

[type.Scan]
range = "f32"

[component.planner]
language = "rust"
input = ["scan:Scan"]
output = ["cmd:Command"]

[instance.planner]
component = "planner"

[profile.dev]
mode = "island"

[[boundary.input]]
name = "z_scan"
port = "planner.scan"
type = "Scan"

[[boundary.input]]
name = "a_scan"
port = "planner.scan"
type = "Scan"

[[boundary.output]]
name = "cmd"
port = "planner.cmd"
type = "Command"
"#;
    let source_b = source_a.replace(
        r#"[[boundary.input]]
name = "z_scan"
port = "planner.scan"
type = "Scan"

[[boundary.input]]
name = "a_scan"
port = "planner.scan"
type = "Scan""#,
        r#"[[boundary.input]]
name = "a_scan"
port = "planner.scan"
type = "Scan"

[[boundary.input]]
name = "z_scan"
port = "planner.scan"
type = "Scan""#,
    );
    let raw_a = parse_str(source_a).unwrap();
    let raw_b = parse_str(&source_b).unwrap();

    let mut ir_a = normalize_document(&raw_a, hash_source("same logical source")).unwrap();
    let mut ir_b = normalize_document(&raw_b, hash_source("same logical source")).unwrap();

    ir_a.source_hash = "stable".to_string();
    ir_b.source_hash = "stable".to_string();
    assert_eq!(
        ir_a.to_canonical_json().unwrap(),
        ir_b.to_canonical_json().unwrap()
    );
    assert_eq!(ir_a.graphs[0].boundary_endpoints[0].name, "a_scan");
    assert_eq!(ir_a.graphs[0].boundary_endpoints[1].name, "z_scan");
    assert_eq!(
        ir_a.graphs[0].boundary_endpoints[2].direction,
        BoundaryDirection::Output
    );
}
