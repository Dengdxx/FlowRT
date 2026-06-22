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

struct PlanRequest {
    static constexpr const char* IOX2_TYPE_NAME = "PlanRequest";
    std::uint32_t goal{};

    bool operator==(const PlanRequest&) const = default;

    static constexpr std::size_t wire_size() noexcept { return 4; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_wire_le(output, cursor, goal);
        cursor += 4;
    }

    static PlanRequest decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        std::size_t cursor = 0;
        PlanRequest value{};
        value.goal = flowrt::read_wire_le<std::uint32_t>(input, cursor);
        cursor += 4;
        return value;
    }
};

struct PlanResponse {
    static constexpr const char* IOX2_TYPE_NAME = "PlanResponse";
    bool accepted{};

    bool operator==(const PlanResponse&) const = default;

    static constexpr std::size_t wire_size() noexcept { return 1; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_wire_le(output, cursor, accepted);
        cursor += 1;
    }

    static PlanResponse decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        std::size_t cursor = 0;
        PlanResponse value{};
        value.accepted = flowrt::read_wire_le<bool>(input, cursor);
        cursor += 1;
        return value;
    }
};

}  // namespace flowrt_app
