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

struct Sample {
    std::uint32_t value{};

    bool operator==(const Sample&) const = default;

    static constexpr std::size_t wire_size() noexcept { return 4; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_wire_le(output, cursor, value);
        cursor += 4;
    }

    static Sample decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        std::size_t cursor = 0;
        Sample value{};
        value.value = flowrt::read_wire_le<std::uint32_t>(input, cursor);
        cursor += 4;
        return value;
    }
};

}  // namespace flowrt_app
