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
    static constexpr const char* IOX2_TYPE_NAME = "PlanFeedback";
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
    std::string target;

    bool operator==(const PlanGoal&) const = default;

    std::size_t encoded_frame_size() const noexcept { return 8 + target.size(); }

    void encode_frame(std::span<std::uint8_t> output) const {
        std::vector<std::uint8_t> tail;
        if (target.size() > 8U) {
            throw flowrt::WireCodecError("field PlanGoal.target exceeds max 8");
        }
        const auto target_span = flowrt::append_tail_block(tail, std::span<const std::uint8_t>{reinterpret_cast<const std::uint8_t*>(target.data()), target.size()});
        flowrt::ensure_wire_size(encoded_frame_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_var_span(output.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE), target_span);
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        std::copy(tail.begin(), tail.end(), output.begin() + 8);
    }

    static PlanGoal decode_frame(std::span<const std::uint8_t> input) {
        if (input.size() < 8) {
            throw flowrt::WireCodecError(8, input.size());
        }
        std::size_t cursor = 0;
        PlanGoal value{};
        const auto target_span = flowrt::read_var_span(input.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE));
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        flowrt::FrameDecoder decoder(input.subspan(8));
        const auto target_block = decoder.read_block(target_span);
        if (!flowrt::valid_utf8(target_block)) {
            throw flowrt::WireCodecError("string field is not valid UTF-8");
        }
        if (target_block.empty()) {
            value.target.clear();
        } else {
            value.target.assign(reinterpret_cast<const char*>(target_block.data()), target_block.size());
        }
        decoder.finish();
        return value;
    }
};

struct PlanResult {
    static constexpr const char* IOX2_TYPE_NAME = "PlanResult";
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
