use super::*;

#[test]
fn emits_variable_frame_message_artifacts() {
    let ir = contract_from_source(
        r#"
[package]
name = "variable_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"
label = "string"
samples = "sequence<u32>"

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
    assert!(rust_messages.contains("pub payload: Vec<u8>"));
    assert!(rust_messages.contains("pub label: String"));
    assert!(rust_messages.contains("pub samples: Vec<u32>"));
    assert!(rust_messages.contains("impl flowrt::FrameCodec for Packet"));
    assert!(rust_messages.contains("flowrt::VarSpan::decode"));
    assert!(rust_messages.contains("String::from_utf8"));
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
fn iox2_profile_routes_variable_messages_over_zenoh_without_frame_slots() {
    let ir = contract_from_source(
        r#"
[package]
name = "variable_route_demo"
rsdl_version = "0.1"

[type.Packet]
payload = "bytes"
label = "string"
samples = "sequence<u32>"

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
backends = ["iox2", "zenoh"]
"#,
    );

    let bundle = emit_artifacts(&ir).unwrap();
    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
    let cpp_shell_header = artifact_content(&bundle, "cpp/include/flowrt_app/runtime_shell.hpp");

    assert!(!rust_messages.contains("PacketIox2"));
    assert_eq!(
        rust_shell
            .matches("flowrt::zenoh::ZenohPubSub<Packet>")
            .count(),
        1
    );
    assert!(rust_shell.contains("flowrt::zenoh::ZenohPubSub<Packet>"));

    assert!(!cpp_messages.contains("PacketIox2"));
    assert!(cpp_messages.contains("std::size_t samples_cursor = 0;"));
    assert!(cpp_messages.contains("samples_cursor += 4;"));
    assert!(!cpp_messages.contains("IOX2_TYPE_NAME"));
    assert!(cpp_shell_header.contains("flowrt::zenoh::ZenohPubSub<Packet> bind_0_;"));

    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    assert_eq!(launch["graphs"][0]["channels"][0]["backend"], "zenoh");

    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
    assert!(cargo_manifest.contains("features = [\"iox2\", \"zenoh\"]"));

    let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
    assert!(cmake.contains("find_package(iceoryx2-cxx 0.9.1 QUIET)"));
    assert!(cmake.contains("find_package(zenohc 1.9.0 QUIET)"));
}

#[test]
fn iox2_profile_keeps_frame_descriptor_routes_on_iox2() {
    let ir = contract_from_source(
        r#"
[package]
name = "descriptor_route_demo"
rsdl_version = "0.1"

[type.FrameHandle]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"
timestamp_unix_ns = "u64"
width = "u32"
height = "u32"
stride_bytes = "u32"
format_id = "u32"
encoding_id = "u32"
flags = "u32"

[component.camera]
language = "rust"
kind = "io_boundary"
io_side_effect = ["device", "read"]
output = ["frame:FrameHandle"]

[component.camera.resource.frames]
capability = "payload.frame_buffer"
required = false

[component.camera.resource.frames.descriptor]
kind = "frame"
port = "frame"
format = "rgb8"
encoding = "row_major"
metadata = { width = "640", height = "480" }

[component.processor]
language = "rust"
input = ["frame:FrameHandle"]

[instance.camera]
component = "camera"
target = "linux"

[instance.camera.task]
trigger = "periodic"
period_ms = 33
output = ["frame"]

[instance.processor]
component = "processor"
target = "linux"

[instance.processor.task]
trigger = "on_message"
input = ["frame"]

[[bind.dataflow]]
from = "camera.frame"
to = "processor.frame"
channel = "latest"

[profile.default]
backend = "iox2"

[target.linux]
runtime = ["rust"]
backends = ["iox2", "zenoh"]
"#,
    );

    let bundle = emit_artifacts(&ir).unwrap();
    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
    assert!(rust_messages.contains("impl From<flowrt::FrameDescriptorFields> for FrameHandle"));
    assert!(rust_messages.contains(
        "pub fn from_frame_descriptor_fields(fields: flowrt::FrameDescriptorFields) -> Self"
    ));
    assert!(
        rust_messages
            .contains("pub fn frame_descriptor_fields(&self) -> flowrt::FrameDescriptorFields")
    );

    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    assert!(rust_shell.contains("flowrt::iox2::Iox2PubSub<FrameHandle>"));
    assert!(!rust_shell.contains("flowrt::zenoh::ZenohPubSub<FrameHandle>"));

    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let channel = &launch["graphs"][0]["channels"][0];
    assert_eq!(channel["backend"], "iox2");
    assert!(
        channel["service"]
            .as_str()
            .unwrap()
            .contains("camera_frame_to_processor_frame")
    );
    assert_eq!(channel["key_expr"], serde_json::Value::Null);

    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    let selfdesc_channel = &selfdesc["graphs"][0]["channels"][0];
    assert_eq!(selfdesc_channel["backend"], "iox2");
    assert_eq!(selfdesc_channel["key_expr"], serde_json::Value::Null);

    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
    assert!(cargo_manifest.contains("features = [\"iox2\"]"));
    assert!(!cargo_manifest.contains("features = [\"iox2\", \"zenoh\"]"));
}

#[test]
fn frame_descriptor_demo_example_codegen_smoke() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crate should live under repo root");
    let rsdl = repo_root.join("examples/frame_descriptor_demo/rsdl/robot.rsdl");
    let ir = contract_from_file(&rsdl);

    let bundle = emit_artifacts(&ir).unwrap();
    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
    let rust_shell = artifact_content(&bundle, "rust/src/runtime_shell.rs");
    let launch: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "launch/launch.json")).unwrap();
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();

    assert!(rust_messages.contains("impl From<flowrt::FrameDescriptorFields> for FrameHandle"));
    assert!(
        rust_messages
            .contains("pub fn frame_descriptor_fields(&self) -> flowrt::FrameDescriptorFields")
    );
    assert!(rust_shell.contains("flowrt::iox2::Iox2PubSub<FrameHandle>"));
    assert_eq!(launch["graphs"][0]["channels"][0]["backend"], "iox2");
    assert_eq!(
        selfdesc["component_types"][0]["resources"][0]["descriptor"]["port"],
        "frame"
    );
    assert_eq!(
        selfdesc["component_types"][0]["resources"][0]["descriptor"]["record_payload"],
        false
    );
}

#[test]
fn cpp_frame_descriptor_message_emits_helper_methods() {
    let ir = contract_from_source(
        r#"
[package]
name = "cpp_descriptor_helper_demo"
rsdl_version = "0.1"

[type.FrameHandle]
resource_id_hash = "u64"
slot = "u32"
generation = "u64"
size_bytes = "u64"
timestamp_unix_ns = "u64"
width = "u32"
height = "u32"
stride_bytes = "u32"
format_id = "u32"
encoding_id = "u32"
flags = "u32"

[component.camera]
language = "cpp"
kind = "io_boundary"
io_side_effect = ["device", "read"]
output = ["frame:FrameHandle"]

[component.camera.resource.frames]
capability = "payload.frame_buffer"
required = false

[component.camera.resource.frames.descriptor]
kind = "frame"
port = "frame"
format = "rgb8"
encoding = "row_major"

[instance.camera]
component = "camera"

[instance.camera.task]
trigger = "periodic"
period_ms = 33
output = ["frame"]
"#,
    );

    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
    assert!(cpp_messages.contains(
        "static FrameHandle from_frame_descriptor_fields(const flowrt::FrameDescriptorFields& fields)"
    ));
    assert!(cpp_messages.contains(
        "[[nodiscard]] flowrt::FrameDescriptorFields frame_descriptor_fields() const noexcept"
    ));
    assert!(cpp_messages.contains(".resource_id_hash = resource_id_hash"));
    assert!(cpp_messages.contains(".flags = flags"));
}

#[test]
fn variable_frame_supports_sequence_of_fixed_structs_in_rust_and_cpp() {
    let ir = contract_from_source(
        r#"
[package]
name = "path_frame_demo"
rsdl_version = "0.1"

[type.Point]
x = "f32"
y = "f32"

[type.PathFrame]
label = "string"
points = "sequence<Point>"

[component.source]
language = "rust"
output = ["path:PathFrame"]

[component.sink]
language = "cpp"
input = ["path:PathFrame"]

[instance.source]
component = "source"
process = "source_proc"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["path"]

[instance.sink]
component = "sink"
process = "sink_proc"

[instance.sink.task]
trigger = "on_message"
input = ["path"]

[[bind.dataflow]]
from = "source.path"
to = "sink.path"
channel = "latest"

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();

    assert!(rust_messages.contains("pub points: Vec<Point>"));
    assert!(
        rust_messages
            .contains("let mut points_tail = Vec::<u8>::with_capacity(self.points.len() * 8);")
    );
    assert!(
        rust_messages
            .contains("element.encode_wire(&mut points_tail[cursor..cursor + Point::WIRE_SIZE])?;")
    );
    assert!(
        rust_messages.contains("points.push(<Point as flowrt::WireCodec>::decode_wire(chunk)?);")
    );

    assert!(cpp_messages.contains("std::vector<Point> points"));
    assert!(cpp_messages.contains("points_tail.resize(points.size() * 8);"));
    assert!(cpp_messages.contains("element.encode_wire(std::span<std::uint8_t>{points_tail.data(), points_tail.size()}.subspan(cursor, Point::wire_size()));"));
    assert!(
        cpp_messages.contains(
            "value.points.push_back(Point::decode_wire(points_block.subspan(index, 8)));"
        )
    );

    assert_eq!(selfdesc["message_abi"][0]["type_name"], "Point");
    assert_eq!(selfdesc["message_frames"][0]["type_name"], "PathFrame");
    assert_eq!(
        selfdesc["message_frames"][0]["encoding"],
        "canonical_frame_v1"
    );
    assert_eq!(selfdesc["message_frames"][0]["header_size_bytes"], 16);
}

#[test]
fn variable_frame_tests_embed_cross_language_byte_fixtures_and_malformed_decode() {
    let ir = contract_from_source(
        r#"
[package]
name = "variable_fixture_demo"
rsdl_version = "0.1"

[type.Point]
x = "f32"
y = "f32"

[type.PathFrame]
label = "string"
payload = "bytes"
points = "sequence<Point>"

[component.source]
language = "rust"
output = ["path:PathFrame"]

[component.sink]
language = "cpp"
input = ["path:PathFrame"]

[instance.source]
component = "source"
process = "source_proc"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["path"]

[instance.sink]
component = "sink"
process = "sink_proc"

[instance.sink.task]
trigger = "on_message"
input = ["path"]

[[bind.dataflow]]
from = "source.path"
to = "sink.path"
channel = "latest"

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();

    let rust_frame = artifact_content(&bundle, "rust/tests/message_frame.rs");
    assert!(rust_frame.contains("const EXPECTED_PATH_FRAME_FRAME: &[u8] = &["));
    assert!(rust_frame.contains("fn assert_cpp_frame_fixture(name: &str, expected: &[u8])"));
    assert!(
        rust_frame
            .contains("assert_cpp_frame_fixture(\"path_frame.frame\", EXPECTED_PATH_FRAME_FRAME);")
    );
    assert!(rust_frame.contains("label: \"utf8-\\u{03bc}-2\".to_string()"));
    assert!(rust_frame.contains("payload: vec![3u8, 4u8, 5u8]"));
    assert!(rust_frame.contains("points: vec!["));
    assert!(rust_frame.contains("fn path_frame_empty_variable_fields_frame_codec()"));
    assert!(rust_frame.contains("fn path_frame_rejects_malformed_frame_decode()"));
    assert!(rust_frame.contains("offset.to_le_bytes()"));
    assert!(rust_frame.contains("len.to_le_bytes()"));

    let cpp_frame = artifact_content(&bundle, "cpp/tests/message_frame.cpp");
    assert!(cpp_frame.contains("constexpr std::array<std::uint8_t, 52> EXPECTED_PATH_FRAME_FRAME"));
    assert!(cpp_frame.contains("#include <span>"));
    assert!(cpp_frame.contains("write_fixture(\"path_frame.frame\", frame);"));
    assert!(cpp_frame.contains("value.label = \"utf8-\\xCE\\xBC-2\";"));
    assert!(cpp_frame.contains("value.payload = std::vector<std::uint8_t>{std::uint8_t{3}, std::uint8_t{4}, std::uint8_t{5}};"));
    assert!(cpp_frame.contains("void test_path_frame_rejects_malformed_frame_decode()"));

    let cargo_manifest = artifact_content(&bundle, "build/Cargo.toml");
    assert!(
        cargo_manifest.contains(
            "[[test]]\nname = \"message_frame\"\npath = \"../rust/tests/message_frame.rs\""
        )
    );

    let cmake = artifact_content(&bundle, "build/CMakeLists.txt");
    assert!(cmake.contains(
        "add_executable(variable_fixture_demo_message_frame ../cpp/tests/message_frame.cpp)"
    ));
    assert!(
        cmake.contains("add_test(NAME message_frame COMMAND variable_fixture_demo_message_frame)")
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
    assert!(cmake.contains("if(NOT CMAKE_CROSSCOMPILING)"));
    assert!(cmake.contains("add_custom_command(TARGET abi_demo_message_abi POST_BUILD"));
    assert!(cmake.contains("add_test(NAME message_abi COMMAND abi_demo_message_abi)"));
    assert!(cmake.contains("Skipping C++ Message ABI fixture execution while cross compiling"));
}

#[test]
fn cpp_messages_use_standard_128_bit_pod_types() {
    let ir = contract_from_source(
        r#"
[package]
name = "wide_demo"
rsdl_version = "0.1"

[type.Wide]
unsigned_value = "u128"
signed_value = "i128"

[component.producer]
language = "cpp"
output = ["wide:Wide"]

[instance.producer]
component = "producer"

[instance.producer.task]
trigger = "periodic"
period_ms = 5
output = ["wide"]

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["cpp"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
    let cpp_abi = artifact_content(&bundle, "cpp/tests/message_abi.cpp");

    assert!(cpp_messages.contains("flowrt::UInt128 unsigned_value"));
    assert!(cpp_messages.contains("flowrt::Int128 signed_value"));
    assert!(cpp_messages.contains("flowrt::read_wire_le<flowrt::UInt128>"));
    assert!(cpp_messages.contains("flowrt::read_wire_le<flowrt::Int128>"));
    assert!(!cpp_messages.contains("__int128"));
    assert!(cpp_abi.contains("flowrt::UInt128{"));
    assert!(cpp_abi.contains("flowrt::Int128{"));
    assert!(!cpp_abi.contains("__int128"));
}

#[test]
fn cpp_message_abi_fixture_qualifies_nested_fixed_struct_array_elements() {
    let ir = contract_from_source(
        r#"
[package]
name = "path_demo"
rsdl_version = "0.1"

[type.PathPoint]
x = "f32"
y = "f32"

[type.PathFrame]
points = "[PathPoint; 2]"
count = "u32"

[component.consumer]
language = "cpp"
input = ["frame:PathFrame"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();
    let cpp_abi = artifact_content(&bundle, "cpp/tests/message_abi.cpp");

    assert!(cpp_abi.contains("std::array<flowrt_app::PathPoint, 2>"));
    assert!(!cpp_abi.contains("std::array<PathPoint, 2>"));
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
    assert_eq!(
        rust_messages
            .matches("debug_assert_eq!(cursor, Self::WIRE_SIZE);")
            .count(),
        4,
        "fixed Rust WireCodec encode/decode must read the final cursor value for each message"
    );

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
fn emits_explicit_empty_message_artifacts() {
    let ir = contract_from_source(
        r#"
[package]
name = "empty_demo"
rsdl_version = "0.1"

[type.Empty]
empty = true

[component.source]
language = "rust"
output = ["packet:Empty"]

[component.sink]
language = "cpp"
input = ["packet:Empty"]

[instance.source]
component = "source"

[instance.source.task]
trigger = "periodic"
period_ms = 5
output = ["packet"]

[profile.default]
backend = "zenoh"

[target.linux]
runtime = ["rust", "cpp"]
backends = ["zenoh"]
"#,
    );
    let bundle = emit_artifacts(&ir).unwrap();

    let rust_messages = artifact_content(&bundle, "rust/src/messages.rs");
    assert!(rust_messages.contains("pub struct Empty {"));
    assert!(rust_messages.contains("const WIRE_SIZE: usize = 0;"));

    let cpp_messages = artifact_content(&bundle, "cpp/include/flowrt_app/messages.hpp");
    assert!(cpp_messages.contains("struct Empty {"));
    assert!(
        cpp_messages.contains("static constexpr std::size_t wire_size() noexcept { return 0; }")
    );

    let rust_abi = artifact_content(&bundle, "rust/tests/message_abi.rs");
    assert!(rust_abi.contains("const EXPECTED_EMPTY_BYTES: &[u8] = &[];"));
    assert!(rust_abi.contains("fn empty_message_abi()"));
    assert!(rust_abi.contains("fn empty_wire_codec_omits_native_padding()"));

    let cpp_abi = artifact_content(&bundle, "cpp/tests/message_abi.cpp");
    assert!(cpp_abi.contains("constexpr std::array<std::uint8_t, 0> EXPECTED_EMPTY_BYTES"));
    assert!(cpp_abi.contains("void test_empty_message_abi()"));
    assert!(cpp_abi.contains("void test_empty_wire_codec_omits_native_padding()"));

    let selfdesc: serde_json::Value =
        serde_json::from_str(artifact_content(&bundle, "selfdesc/selfdesc.json")).unwrap();
    assert_eq!(selfdesc["message_abi"][0]["type_name"], "Empty");
    assert_eq!(selfdesc["message_abi"][0]["size_bytes"], 0);
    assert_eq!(selfdesc["message_abi"][0]["empty"], true);
    assert_eq!(selfdesc["message_abi"][0]["fields"], serde_json::json!([]));
}
