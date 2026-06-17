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

struct Cmd {
    double u{};

    bool operator==(const Cmd&) const = default;

    static constexpr std::size_t wire_size() noexcept { return 8; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_wire_le(output, cursor, u);
        cursor += 8;
    }

    static Cmd decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        std::size_t cursor = 0;
        Cmd value{};
        value.u = flowrt::read_wire_le<double>(input, cursor);
        cursor += 8;
        return value;
    }
};

struct State {
    double x{};

    bool operator==(const State&) const = default;

    static constexpr std::size_t wire_size() noexcept { return 8; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_wire_le(output, cursor, x);
        cursor += 8;
    }

    static State decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        std::size_t cursor = 0;
        State value{};
        value.x = flowrt::read_wire_le<double>(input, cursor);
        cursor += 8;
        return value;
    }
};

}  // namespace flowrt_app
