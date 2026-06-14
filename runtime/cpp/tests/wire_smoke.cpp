#include <algorithm>
#include <cassert>
#include <cstdint>
#include <flowrt/wire.hpp>
#include <limits>
#include <span>
#include <string>
#include <vector>

namespace {

struct Point {
    float x{};
    float y{};

    static constexpr std::size_t wire_size() noexcept { return 8U; }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0U, x);
        flowrt::write_wire_le(output, 4U, y);
    }

    static Point decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        return Point{
            .x = flowrt::read_wire_le<float>(input, 0U),
            .y = flowrt::read_wire_le<float>(input, 4U),
        };
    }
};

struct VariableFrame {
    std::string label;
    std::vector<std::uint8_t> payload;
    std::vector<Point> points;

    [[nodiscard]] std::size_t encoded_frame_size() const noexcept {
        return flowrt::VAR_SPAN_WIRE_SIZE * 3U + label.size() + payload.size() +
               points.size() * Point::wire_size();
    }

    void encode_frame(std::span<std::uint8_t> output) const {
        std::vector<std::uint8_t> tail;
        const auto label_span = flowrt::append_tail_block(
            tail, std::span<const std::uint8_t>{
                      reinterpret_cast<const std::uint8_t *>(label.data()), label.size()});
        const auto payload_span = flowrt::append_tail_block(
            tail, std::span<const std::uint8_t>{payload.data(), payload.size()});
        std::vector<std::uint8_t> points_tail(points.size() * Point::wire_size());
        std::size_t cursor = 0U;
        for (const auto &point : points) {
            point.encode_wire(
                std::span<std::uint8_t>{points_tail.data(), points_tail.size()}.subspan(
                    cursor, Point::wire_size()));
            cursor += Point::wire_size();
        }
        const auto points_span = flowrt::append_tail_block(
            tail, std::span<const std::uint8_t>{points_tail.data(), points_tail.size()});

        flowrt::ensure_wire_size(encoded_frame_size(), output.size());
        flowrt::write_var_span(output.subspan(0U, flowrt::VAR_SPAN_WIRE_SIZE), label_span);
        flowrt::write_var_span(output.subspan(8U, flowrt::VAR_SPAN_WIRE_SIZE), payload_span);
        flowrt::write_var_span(output.subspan(16U, flowrt::VAR_SPAN_WIRE_SIZE), points_span);
        std::copy(tail.begin(), tail.end(), output.begin() + 24U);
    }

    static VariableFrame decode_frame(std::span<const std::uint8_t> input) {
        if (input.size() < 24U) {
            throw flowrt::WireCodecError(24U, input.size());
        }
        const auto label_span = flowrt::read_var_span(input.subspan(0U, 8U));
        const auto payload_span = flowrt::read_var_span(input.subspan(8U, 8U));
        const auto points_span = flowrt::read_var_span(input.subspan(16U, 8U));
        flowrt::FrameDecoder decoder(input.subspan(24U));
        VariableFrame value{};
        const auto label_block = decoder.read_block(label_span);
        if (!flowrt::valid_utf8(label_block)) {
            throw flowrt::WireCodecError("string field is not valid UTF-8");
        }
        value.label.assign(reinterpret_cast<const char *>(label_block.data()), label_block.size());
        const auto payload_block = decoder.read_block(payload_span);
        value.payload.assign(payload_block.begin(), payload_block.end());
        const auto points_block = decoder.read_block(points_span);
        if (points_block.size() % Point::wire_size() != 0U) {
            throw flowrt::WireCodecError(
                "sequence byte length is not divisible by element wire size");
        }
        for (std::size_t index = 0U; index < points_block.size(); index += Point::wire_size()) {
            value.points.push_back(
                Point::decode_wire(points_block.subspan(index, Point::wire_size())));
        }
        decoder.finish();
        return value;
    }
};

const std::vector<std::uint8_t> EXPECTED_VARIABLE_FRAME_BYTES{
    0, 0, 0,   0,  9, 0, 0,   0,   9,   0,  0,   0,   3,   0,  0,   0,  12, 0,
    0, 0, 16,  0,  0, 0, 117, 116, 102, 56, 45,  206, 188, 45, 50,  3,  4,  5,
    0, 0, 128, 64, 0, 0, 144, 64,  0,   0,  160, 64,  0,   0,  176, 64,
};

VariableFrame sample_variable_frame() {
    return VariableFrame{
        .label = "utf8-\xCE\xBC-2",
        .payload = std::vector<std::uint8_t>{3U, 4U, 5U},
        .points = std::vector<Point>{Point{.x = 4.0F, .y = 4.5F}, Point{.x = 5.0F, .y = 5.5F}},
    };
}

std::vector<std::uint8_t> encode(const VariableFrame &value) {
    std::vector<std::uint8_t> output(value.encoded_frame_size());
    value.encode_frame(output);
    return output;
}

void write_span(std::vector<std::uint8_t> &frame, std::size_t header_offset, std::uint32_t offset,
                std::uint32_t len) {
    flowrt::write_wire_le(std::span<std::uint8_t>{frame.data(), frame.size()}, header_offset,
                          offset);
    flowrt::write_wire_le(std::span<std::uint8_t>{frame.data(), frame.size()}, header_offset + 4U,
                          len);
}

}  // namespace

int main() {
    const auto encoded = encode(sample_variable_frame());
    assert(encoded == EXPECTED_VARIABLE_FRAME_BYTES);
    const auto decoded = VariableFrame::decode_frame(encoded);
    assert(decoded.label == "utf8-\xCE\xBC-2");
    assert(decoded.payload == std::vector<std::uint8_t>({3U, 4U, 5U}));
    assert(decoded.points.size() == 2U);
    assert(decoded.points[0].x == 4.0F);
    assert(decoded.points[1].y == 5.5F);

    const VariableFrame empty{};
    assert(encode(empty) == std::vector<std::uint8_t>(flowrt::VAR_SPAN_WIRE_SIZE * 3U, 0U));

    bool saw_truncation = false;
    try {
        VariableFrame::decode_frame(
            std::span<const std::uint8_t>{EXPECTED_VARIABLE_FRAME_BYTES.data(), 23U});
    } catch (const flowrt::WireCodecError &) {
        saw_truncation = true;
    }
    assert(saw_truncation);

    auto offset_overflow = EXPECTED_VARIABLE_FRAME_BYTES;
    write_span(offset_overflow, 0U, std::numeric_limits<std::uint32_t>::max(), 1U);
    bool saw_offset = false;
    try {
        VariableFrame::decode_frame(offset_overflow);
    } catch (const flowrt::WireCodecError &) {
        saw_offset = true;
    }
    assert(saw_offset);

    auto length_overflow = EXPECTED_VARIABLE_FRAME_BYTES;
    write_span(length_overflow, 0U, 0U, std::numeric_limits<std::uint32_t>::max());
    bool saw_length = false;
    try {
        VariableFrame::decode_frame(length_overflow);
    } catch (const flowrt::WireCodecError &) {
        saw_length = true;
    }
    assert(saw_length);

    auto invalid_utf8 = EXPECTED_VARIABLE_FRAME_BYTES;
    invalid_utf8[24] = 0xffU;
    bool saw_utf8 = false;
    try {
        VariableFrame::decode_frame(invalid_utf8);
    } catch (const flowrt::WireCodecError &) {
        saw_utf8 = true;
    }
    assert(saw_utf8);

    return 0;
}
