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

constexpr std::array<std::uint8_t, 4> EXPECTED_PLAN_FEEDBACK_BYTES{{0, 0, 16, 64}};
constexpr std::array<std::uint8_t, 4> EXPECTED_PLAN_GOAL_BYTES{{2, 0, 0, 0}};
constexpr std::array<std::uint8_t, 1> EXPECTED_PLAN_RESULT_BYTES{{1}};

flowrt_app::PlanFeedback sample_plan_feedback() {
    flowrt_app::PlanFeedback value{};
    std::memset(&value, 0, sizeof(value));
    value.progress = 2.25F;
    return value;
}

flowrt_app::PlanGoal sample_plan_goal() {
    flowrt_app::PlanGoal value{};
    std::memset(&value, 0, sizeof(value));
    value.target = std::uint32_t{2};
    return value;
}

flowrt_app::PlanResult sample_plan_result() {
    flowrt_app::PlanResult value{};
    std::memset(&value, 0, sizeof(value));
    value.accepted = true;
    return value;
}

void test_plan_feedback_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::PlanFeedback>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::PlanFeedback>);
    static_assert(sizeof(flowrt_app::PlanFeedback) == 4);
    static_assert(alignof(flowrt_app::PlanFeedback) == 4);
    assert_default_bytes_zero<flowrt_app::PlanFeedback>();
    static_assert(offsetof(flowrt_app::PlanFeedback, progress) == 0);
    assert_byte_roundtrip(sample_plan_feedback());
    assert_sample_bytes(sample_plan_feedback(), EXPECTED_PLAN_FEEDBACK_BYTES);
    write_fixture("plan_feedback.bin", bytes_of(sample_plan_feedback()));
}

void test_plan_feedback_wire_codec_omits_native_padding() {
    const auto value = sample_plan_feedback();
    std::array<std::uint8_t, flowrt_app::PlanFeedback::wire_size()> wire{};
    value.encode_wire(wire);
    const std::array<std::uint8_t, flowrt_app::PlanFeedback::wire_size()> expected_wire{0, 0, 16, 64};
    assert(wire == expected_wire);
    const auto decoded = flowrt_app::PlanFeedback::decode_wire(wire);
    assert(bytes_of(decoded) == bytes_of(value));
}

void test_plan_goal_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::PlanGoal>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::PlanGoal>);
    static_assert(sizeof(flowrt_app::PlanGoal) == 4);
    static_assert(alignof(flowrt_app::PlanGoal) == 4);
    assert_default_bytes_zero<flowrt_app::PlanGoal>();
    static_assert(offsetof(flowrt_app::PlanGoal, target) == 0);
    assert_byte_roundtrip(sample_plan_goal());
    assert_sample_bytes(sample_plan_goal(), EXPECTED_PLAN_GOAL_BYTES);
    write_fixture("plan_goal.bin", bytes_of(sample_plan_goal()));
}

void test_plan_goal_wire_codec_omits_native_padding() {
    const auto value = sample_plan_goal();
    std::array<std::uint8_t, flowrt_app::PlanGoal::wire_size()> wire{};
    value.encode_wire(wire);
    const std::array<std::uint8_t, flowrt_app::PlanGoal::wire_size()> expected_wire{2, 0, 0, 0};
    assert(wire == expected_wire);
    const auto decoded = flowrt_app::PlanGoal::decode_wire(wire);
    assert(bytes_of(decoded) == bytes_of(value));
}

void test_plan_result_message_abi() {
    static_assert(std::is_standard_layout_v<flowrt_app::PlanResult>);
    static_assert(std::is_trivially_copyable_v<flowrt_app::PlanResult>);
    static_assert(sizeof(flowrt_app::PlanResult) == 1);
    static_assert(alignof(flowrt_app::PlanResult) == 1);
    assert_default_bytes_zero<flowrt_app::PlanResult>();
    static_assert(offsetof(flowrt_app::PlanResult, accepted) == 0);
    assert_byte_roundtrip(sample_plan_result());
    assert_sample_bytes(sample_plan_result(), EXPECTED_PLAN_RESULT_BYTES);
    write_fixture("plan_result.bin", bytes_of(sample_plan_result()));
}

void test_plan_result_wire_codec_omits_native_padding() {
    const auto value = sample_plan_result();
    std::array<std::uint8_t, flowrt_app::PlanResult::wire_size()> wire{};
    value.encode_wire(wire);
    const std::array<std::uint8_t, flowrt_app::PlanResult::wire_size()> expected_wire{1};
    assert(wire == expected_wire);
    const auto decoded = flowrt_app::PlanResult::decode_wire(wire);
    assert(bytes_of(decoded) == bytes_of(value));
}

}  // namespace

int main() {
    test_plan_feedback_message_abi();
    test_plan_feedback_wire_codec_omits_native_padding();
    test_plan_goal_message_abi();
    test_plan_goal_wire_codec_omits_native_padding();
    test_plan_result_message_abi();
    test_plan_result_wire_codec_omits_native_padding();
    return 0;
}
