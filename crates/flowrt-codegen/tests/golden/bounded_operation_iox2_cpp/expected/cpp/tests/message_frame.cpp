// FlowRT 管理产物。不要手工修改。

#include <algorithm>
#include <array>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <fstream>
#include <limits>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#include "flowrt_app/messages.hpp"

namespace {

template <typename T>
std::vector<std::uint8_t> frame_of(const T& value) {
    std::vector<std::uint8_t> frame(value.encoded_frame_size());
    value.encode_frame(frame);
    return frame;
}

template <std::size_t N>
void assert_frame_bytes(const std::vector<std::uint8_t>& frame, const std::array<std::uint8_t, N>& expected) {
    assert(frame.size() == expected.size());
    assert(std::equal(frame.begin(), frame.end(), expected.begin(), expected.end()));
}

void write_fixture(std::string_view name, const std::vector<std::uint8_t>& bytes) {
#ifdef FLOWRT_ABI_FIXTURE_DIR
    std::filesystem::create_directories(FLOWRT_ABI_FIXTURE_DIR);
    auto path = std::filesystem::path(FLOWRT_ABI_FIXTURE_DIR) / std::string(name);
    std::ofstream output(path, std::ios::binary);
    if (!output) {
        throw std::runtime_error("failed to open frame fixture output");
    }
    output.write(reinterpret_cast<const char*>(bytes.data()), static_cast<std::streamsize>(bytes.size()));
    if (!output) {
        throw std::runtime_error("failed to write frame fixture output");
    }
#else
    (void)name;
    (void)bytes;
#endif
}

void write_var_span(std::vector<std::uint8_t>& frame, std::size_t header_offset, std::uint32_t offset, std::uint32_t len) {
    flowrt::write_wire_le(std::span<std::uint8_t>{frame.data(), frame.size()}, header_offset, offset);
    flowrt::write_wire_le(std::span<std::uint8_t>{frame.data(), frame.size()}, header_offset + 4U, len);
}

constexpr std::array<std::uint8_t, 14> EXPECTED_PLAN_GOAL_FRAME{{0, 0, 0, 0, 6, 0, 0, 0, 117, 116, 102, 56, 45, 50}};
constexpr std::array<std::uint8_t, 8> EXPECTED_PLAN_GOAL_EMPTY_FRAME{{0, 0, 0, 0, 0, 0, 0, 0}};

flowrt_app::PlanGoal sample_plan_goal() {
    flowrt_app::PlanGoal value{};
    value.target = "utf8-2";
    return value;
}

flowrt_app::PlanGoal sample_plan_goal_empty() {
    flowrt_app::PlanGoal value{};
    value.target = std::string{};
    return value;
}

void test_plan_goal_canonical_frame_codec() {
    const auto value = sample_plan_goal();
    const auto frame = frame_of(value);
    assert_frame_bytes(frame, EXPECTED_PLAN_GOAL_FRAME);
    const auto decoded = flowrt_app::PlanGoal::decode_frame(frame);
    assert(decoded == value);
    assert_frame_bytes(frame_of(decoded), EXPECTED_PLAN_GOAL_FRAME);
    write_fixture("plan_goal.frame", frame);
}

void test_plan_goal_empty_variable_fields_frame_codec() {
    const auto value = sample_plan_goal_empty();
    const auto frame = frame_of(value);
    assert_frame_bytes(frame, EXPECTED_PLAN_GOAL_EMPTY_FRAME);
    const auto decoded = flowrt_app::PlanGoal::decode_frame(frame);
    assert(decoded == value);
    assert_frame_bytes(frame_of(decoded), EXPECTED_PLAN_GOAL_EMPTY_FRAME);
}

void test_plan_goal_rejects_malformed_frame_decode() {
    bool saw_truncation = false;
    try {
        flowrt_app::PlanGoal::decode_frame(std::span<const std::uint8_t>{EXPECTED_PLAN_GOAL_FRAME.data(), 7});
    } catch (const flowrt::WireCodecError&) {
        saw_truncation = true;
    }
    assert(saw_truncation);
    auto offset_overflow = std::vector<std::uint8_t>(EXPECTED_PLAN_GOAL_FRAME.begin(), EXPECTED_PLAN_GOAL_FRAME.end());
    write_var_span(offset_overflow, 0, std::numeric_limits<std::uint32_t>::max(), 1U);
    bool saw_offset = false;
    try {
        flowrt_app::PlanGoal::decode_frame(offset_overflow);
    } catch (const flowrt::WireCodecError&) {
        saw_offset = true;
    }
    assert(saw_offset);
    auto length_overflow = std::vector<std::uint8_t>(EXPECTED_PLAN_GOAL_FRAME.begin(), EXPECTED_PLAN_GOAL_FRAME.end());
    write_var_span(length_overflow, 0, 0U, std::numeric_limits<std::uint32_t>::max());
    bool saw_length = false;
    try {
        flowrt_app::PlanGoal::decode_frame(length_overflow);
    } catch (const flowrt::WireCodecError&) {
        saw_length = true;
    }
    assert(saw_length);
    auto invalid_utf8 = std::vector<std::uint8_t>(EXPECTED_PLAN_GOAL_FRAME.begin(), EXPECTED_PLAN_GOAL_FRAME.end());
    invalid_utf8[8] = 0xffU;
    bool saw_utf8 = false;
    try {
        flowrt_app::PlanGoal::decode_frame(invalid_utf8);
    } catch (const flowrt::WireCodecError&) {
        saw_utf8 = true;
    }
    assert(saw_utf8);
}

}  // namespace

int main() {
    test_plan_goal_canonical_frame_codec();
    test_plan_goal_empty_variable_fields_frame_codec();
    test_plan_goal_rejects_malformed_frame_decode();
    return 0;
}
