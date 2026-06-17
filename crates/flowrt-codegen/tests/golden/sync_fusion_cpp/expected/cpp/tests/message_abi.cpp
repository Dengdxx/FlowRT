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

constexpr std::array<std::uint8_t, 8> EXPECTED_ESTIMATE_BYTES{{0, 0, 0, 0, 0, 0, 2, 64}};
constexpr std::array<std::uint8_t, 16> EXPECTED_IMU_BYTES{{0, 0, 0, 0, 0, 0, 2, 64, 3, 0, 0, 0, 0, 0, 0, 0}};
constexpr std::array<std::uint8_t, 16> EXPECTED_ODOM_BYTES{{0, 0, 0, 0, 0, 0, 2, 64, 3, 0, 0, 0, 0, 0, 0, 0}};

flowrt_app::Estimate sample_estimate() {
    flowrt_app::Estimate value{};
    std::memset(&value, 0, sizeof(value));
    value.x = 2.25;
    return value;
}

flowrt_app::Imu sample_imu() {
    flowrt_app::Imu value{};
    std::memset(&value, 0, sizeof(value));
    value.ax = 2.25;
    value.stamp_ns = std::uint64_t{3};
    return value;
}

flowrt_app::Odom sample_odom() {
    flowrt_app::Odom value{};
    std::memset(&value, 0, sizeof(value));
    value.vx = 2.25;
    value.stamp_ns = std::uint64_t{3};
    return value;
}

void test_estimate_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::Estimate>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::Estimate>);
    static_assert(sizeof(flowrt_app::Estimate) == 8);
    static_assert(alignof(flowrt_app::Estimate) == 8);
    assert_default_bytes_zero<flowrt_app::Estimate>();
    static_assert(offsetof(flowrt_app::Estimate, x) == 0);
    assert_byte_roundtrip(sample_estimate());
    assert_sample_bytes(sample_estimate(), EXPECTED_ESTIMATE_BYTES);
    write_fixture("estimate.bin", bytes_of(sample_estimate()));
}

void test_imu_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::Imu>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::Imu>);
    static_assert(sizeof(flowrt_app::Imu) == 16);
    static_assert(alignof(flowrt_app::Imu) == 8);
    assert_default_bytes_zero<flowrt_app::Imu>();
    static_assert(offsetof(flowrt_app::Imu, ax) == 0);
    static_assert(offsetof(flowrt_app::Imu, stamp_ns) == 8);
    assert_byte_roundtrip(sample_imu());
    assert_sample_bytes(sample_imu(), EXPECTED_IMU_BYTES);
    write_fixture("imu.bin", bytes_of(sample_imu()));
}

void test_odom_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::Odom>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::Odom>);
    static_assert(sizeof(flowrt_app::Odom) == 16);
    static_assert(alignof(flowrt_app::Odom) == 8);
    assert_default_bytes_zero<flowrt_app::Odom>();
    static_assert(offsetof(flowrt_app::Odom, vx) == 0);
    static_assert(offsetof(flowrt_app::Odom, stamp_ns) == 8);
    assert_byte_roundtrip(sample_odom());
    assert_sample_bytes(sample_odom(), EXPECTED_ODOM_BYTES);
    write_fixture("odom.bin", bytes_of(sample_odom()));
}

}  // namespace

int main() {
    test_estimate_message_abi();
    test_imu_message_abi();
    test_odom_message_abi();
    return 0;
}
