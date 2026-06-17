// FlowRT 管理产物。不要手工修改。

#include <array>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>

#include "flowrt_app/messages.hpp"

namespace {

template <typename T>
std::array<std::uint8_t, sizeof(T)> bytes_of(const T& value) {
    std::array<std::uint8_t, sizeof(T)> bytes{};
    std::memcpy(bytes.data(), &value, bytes.size());
    return bytes;
}

template <typename T>
void assert_default_bytes_zero() {
    T value{};
    std::array<std::uint8_t, sizeof(T)> expected{};
    assert(bytes_of(value) == expected);
}

template <typename T>
void assert_byte_roundtrip(const T& value) {
    const auto bytes = bytes_of(value);
    T roundtrip{};
    std::memset(&roundtrip, 0, sizeof(roundtrip));
    std::memcpy(&roundtrip, bytes.data(), bytes.size());
    assert(std::memcmp(&roundtrip, &value, sizeof(T)) == 0);
}

template <typename T, std::size_t N>
void assert_sample_bytes(const T& value, const std::array<std::uint8_t, N>& expected) {
    static_assert(sizeof(T) == N);
    assert(bytes_of(value) == expected);
}

template <std::size_t N>
void write_fixture(std::string_view name, const std::array<std::uint8_t, N>& bytes) {
#ifdef FLOWRT_ABI_FIXTURE_DIR
    std::filesystem::create_directories(FLOWRT_ABI_FIXTURE_DIR);
    auto path = std::filesystem::path(FLOWRT_ABI_FIXTURE_DIR) / std::string(name);
    std::ofstream output(path, std::ios::binary);
    if (!output) {
        throw std::runtime_error("failed to open ABI fixture output");
    }
    output.write(reinterpret_cast<const char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));
    if (!output) {
        throw std::runtime_error("failed to write ABI fixture output");
    }
#else
    (void)name;
    (void)bytes;
#endif
}

constexpr std::array<std::uint8_t, 8> EXPECTED_CMD_BYTES{{0, 0, 0, 0, 0, 0, 2, 64}};
constexpr std::array<std::uint8_t, 8> EXPECTED_STATE_BYTES{{0, 0, 0, 0, 0, 0, 2, 64}};

flowrt_app::Cmd sample_cmd() {
    flowrt_app::Cmd value{};
    std::memset(&value, 0, sizeof(value));
    value.u = 2.25;
    return value;
}

flowrt_app::State sample_state() {
    flowrt_app::State value{};
    std::memset(&value, 0, sizeof(value));
    value.x = 2.25;
    return value;
}

void test_cmd_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::Cmd>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::Cmd>);
    static_assert(sizeof(flowrt_app::Cmd) == 8);
    static_assert(alignof(flowrt_app::Cmd) == 8);
    assert_default_bytes_zero<flowrt_app::Cmd>();
    static_assert(offsetof(flowrt_app::Cmd, u) == 0);
    assert_byte_roundtrip(sample_cmd());
    assert_sample_bytes(sample_cmd(), EXPECTED_CMD_BYTES);
    write_fixture("cmd.bin", bytes_of(sample_cmd()));
}

void test_cmd_wire_codec_omits_native_padding() {
    const auto value = sample_cmd();
    std::array<std::uint8_t, flowrt_app::Cmd::wire_size()> wire{};
    value.encode_wire(wire);
    const std::array<std::uint8_t, flowrt_app::Cmd::wire_size()> expected_wire{0, 0, 0, 0, 0, 0, 2, 64};
    assert(wire == expected_wire);
    const auto decoded = flowrt_app::Cmd::decode_wire(wire);
    assert(bytes_of(decoded) == bytes_of(value));
}

void test_state_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::State>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::State>);
    static_assert(sizeof(flowrt_app::State) == 8);
    static_assert(alignof(flowrt_app::State) == 8);
    assert_default_bytes_zero<flowrt_app::State>();
    static_assert(offsetof(flowrt_app::State, x) == 0);
    assert_byte_roundtrip(sample_state());
    assert_sample_bytes(sample_state(), EXPECTED_STATE_BYTES);
    write_fixture("state.bin", bytes_of(sample_state()));
}

void test_state_wire_codec_omits_native_padding() {
    const auto value = sample_state();
    std::array<std::uint8_t, flowrt_app::State::wire_size()> wire{};
    value.encode_wire(wire);
    const std::array<std::uint8_t, flowrt_app::State::wire_size()> expected_wire{0, 0, 0, 0, 0, 0, 2, 64};
    assert(wire == expected_wire);
    const auto decoded = flowrt_app::State::decode_wire(wire);
    assert(bytes_of(decoded) == bytes_of(value));
}

}  // namespace

int main() {
    test_cmd_message_abi();
    test_cmd_wire_codec_omits_native_padding();
    test_state_message_abi();
    test_state_wire_codec_omits_native_padding();
    return 0;
}
