// FlowRT 管理产物。不要手工修改。
#pragma once

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <span>
#include <string>
#include <vector>

#include <flowrt/runtime.hpp>

namespace flowrt_app {

struct PlanFeedback {
    float progress{};

    bool operator==(const PlanFeedback&) const = default;

    static constexpr std::size_t wire_size() noexcept { return 4; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_wire_le(output, cursor, progress);
        cursor += 4;
    }

    static PlanFeedback decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        std::size_t cursor = 0;
        PlanFeedback value{};
        value.progress = flowrt::read_wire_le<float>(input, cursor);
        cursor += 4;
        return value;
    }
};

struct PlanGoal {
    std::uint32_t target{};

    bool operator==(const PlanGoal&) const = default;

    static constexpr std::size_t wire_size() noexcept { return 4; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_wire_le(output, cursor, target);
        cursor += 4;
    }

    static PlanGoal decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        std::size_t cursor = 0;
        PlanGoal value{};
        value.target = flowrt::read_wire_le<std::uint32_t>(input, cursor);
        cursor += 4;
        return value;
    }
};

struct PlanResult {
    bool accepted{};

    bool operator==(const PlanResult&) const = default;

    static constexpr std::size_t wire_size() noexcept { return 1; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_wire_le(output, cursor, accepted);
        cursor += 1;
    }

    static PlanResult decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        std::size_t cursor = 0;
        PlanResult value{};
        value.accepted = flowrt::read_wire_le<bool>(input, cursor);
        cursor += 1;
        return value;
    }
};

}  // namespace flowrt_app
