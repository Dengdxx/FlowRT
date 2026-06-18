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

constexpr std::array<std::uint8_t, 4> EXPECTED_SAMPLE_BYTES{{2, 0, 0, 0}};

flowrt_app::Sample sample_sample() {
    flowrt_app::Sample value{};
    std::memset(&value, 0, sizeof(value));
    value.value = std::uint32_t{2};
    return value;
}

void test_sample_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::Sample>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::Sample>);
    static_assert(sizeof(flowrt_app::Sample) == 4);
    static_assert(alignof(flowrt_app::Sample) == 4);
    assert_default_bytes_zero<flowrt_app::Sample>();
    static_assert(offsetof(flowrt_app::Sample, value) == 0);
    assert_byte_roundtrip(sample_sample());
    assert_sample_bytes(sample_sample(), EXPECTED_SAMPLE_BYTES);
    write_fixture("sample.bin", bytes_of(sample_sample()));
}

}  // namespace

int main() {
    test_sample_message_abi();
    return 0;
}
