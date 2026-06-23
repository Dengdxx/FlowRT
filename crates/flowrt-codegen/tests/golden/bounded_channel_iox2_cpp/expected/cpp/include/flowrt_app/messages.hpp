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

struct Packet {
    std::vector<std::uint8_t> payload;
    std::string label;
    std::vector<std::uint32_t> samples;

    bool operator==(const Packet&) const = default;

    std::size_t encoded_frame_size() const noexcept { return 24 + payload.size() + label.size() + samples.size() * 4; }

    void encode_frame(std::span<std::uint8_t> output) const {
        std::vector<std::uint8_t> tail;
        if (payload.size() > 8U) {
            throw flowrt::WireCodecError("field Packet.payload exceeds max 8");
        }
        const auto payload_span = flowrt::append_tail_block(tail, std::span<const std::uint8_t>{payload.data(), payload.size()});
        if (label.size() > 12U) {
            throw flowrt::WireCodecError("field Packet.label exceeds max 12");
        }
        const auto label_span = flowrt::append_tail_block(tail, std::span<const std::uint8_t>{reinterpret_cast<const std::uint8_t*>(label.data()), label.size()});
        if (samples.size() > 4U) {
            throw flowrt::WireCodecError("field Packet.samples exceeds max 4");
        }
        std::vector<std::uint8_t> samples_tail;
        samples_tail.resize(samples.size() * 4);
        std::size_t samples_cursor = 0;
        for (const auto& element : samples) {
            std::size_t cursor = samples_cursor;
            flowrt::write_wire_le(std::span<std::uint8_t>{samples_tail.data(), samples_tail.size()}, cursor, element);
            cursor += 4;
            samples_cursor += 4;
        }
        const auto samples_span = flowrt::append_tail_block(tail, std::span<const std::uint8_t>{samples_tail.data(), samples_tail.size()});
        flowrt::ensure_wire_size(encoded_frame_size(), output.size());
        std::size_t cursor = 0;
        flowrt::write_var_span(output.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE), payload_span);
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        flowrt::write_var_span(output.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE), label_span);
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        flowrt::write_var_span(output.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE), samples_span);
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        std::copy(tail.begin(), tail.end(), output.begin() + 24);
    }

    static Packet decode_frame(std::span<const std::uint8_t> input) {
        if (input.size() < 24) {
            throw flowrt::WireCodecError(24, input.size());
        }
        std::size_t cursor = 0;
        Packet value{};
        const auto payload_span = flowrt::read_var_span(input.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE));
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        const auto label_span = flowrt::read_var_span(input.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE));
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        const auto samples_span = flowrt::read_var_span(input.subspan(cursor, flowrt::VAR_SPAN_WIRE_SIZE));
        cursor += flowrt::VAR_SPAN_WIRE_SIZE;
        flowrt::FrameDecoder decoder(input.subspan(24));
        const auto payload_block = decoder.read_block(payload_span);
        value.payload.assign(payload_block.begin(), payload_block.end());
        const auto label_block = decoder.read_block(label_span);
        if (!flowrt::valid_utf8(label_block)) {
            throw flowrt::WireCodecError("string field is not valid UTF-8");
        }
        if (label_block.empty()) {
            value.label.clear();
        } else {
            value.label.assign(reinterpret_cast<const char*>(label_block.data()), label_block.size());
        }
        const auto samples_block = decoder.read_block(samples_span);
        if (samples_block.size() % 4 != 0) {
            throw flowrt::WireCodecError("sequence byte length is not divisible by element wire size");
        }
        value.samples.reserve(samples_block.size() / 4);
        for (std::size_t index = 0; index < samples_block.size(); index += 4) {
            value.samples.push_back(flowrt::read_wire_le<std::uint32_t>(samples_block, index));
        }
        decoder.finish();
        return value;
    }
};

}  // namespace flowrt_app
