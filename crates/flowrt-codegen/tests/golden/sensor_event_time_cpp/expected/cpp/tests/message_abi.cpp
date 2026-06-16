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

constexpr std::array<std::uint8_t, 8> EXPECTED_IMU_SAMPLE_BYTES{{2, 0, 0, 0, 0, 0, 80, 64}};

flowrt_app::ImuSample sample_imu_sample() {
    flowrt_app::ImuSample value{};
    std::memset(&value, 0, sizeof(value));
    value.stamp_us = std::uint32_t{2};
    value.ax = 3.25F;
    return value;
}

void test_imu_sample_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::ImuSample>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::ImuSample>);
    static_assert(sizeof(flowrt_app::ImuSample) == 8);
    static_assert(alignof(flowrt_app::ImuSample) == 4);
    assert_default_bytes_zero<flowrt_app::ImuSample>();
    static_assert(offsetof(flowrt_app::ImuSample, stamp_us) == 0);
    static_assert(offsetof(flowrt_app::ImuSample, ax) == 4);
    assert_byte_roundtrip(sample_imu_sample());
    assert_sample_bytes(sample_imu_sample(), EXPECTED_IMU_SAMPLE_BYTES);
    write_fixture("imu_sample.bin", bytes_of(sample_imu_sample()));
}

}  // namespace

int main() {
    test_imu_sample_message_abi();
    return 0;
}
