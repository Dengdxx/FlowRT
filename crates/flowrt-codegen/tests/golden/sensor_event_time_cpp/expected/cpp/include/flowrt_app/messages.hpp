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

struct ImuSample {
    std::uint32_t stamp_us{};
    float ax{};

    bool operator==(const ImuSample&) const = default;

    static constexpr std::size_t wire_size() noexcept { return 8; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_wire_le(output, cursor, stamp_us);
        cursor += 4;
        flowrt::write_wire_le(output, cursor, ax);
        cursor += 4;
    }

    static ImuSample decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        std::size_t cursor = 0;
        ImuSample value{};
        value.stamp_us = flowrt::read_wire_le<std::uint32_t>(input, cursor);
        cursor += 4;
        value.ax = flowrt::read_wire_le<float>(input, cursor);
        cursor += 4;
        return value;
    }
};

}  // namespace flowrt_app
