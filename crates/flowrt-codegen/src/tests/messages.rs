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
