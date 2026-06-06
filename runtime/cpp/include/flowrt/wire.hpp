#pragma once

#include <concepts>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <limits>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <vector>

namespace flowrt {

/**
 * @brief canonical wire codec 错误。
 *
 * 该错误只描述 FlowRT wire payload 本身的问题，不暴露具体 backend 或 transport API。
 */
class WireCodecError final : public std::runtime_error {
   public:
    /**
     * @brief 构造 payload size mismatch 错误。
     *
     * @param expected codec 期望的 wire payload 字节数。
     * @param actual 调用方提供的字节数。
     */
    WireCodecError(std::size_t expected, std::size_t actual)
        : std::runtime_error("wire payload size mismatch"), expected_(expected), actual_(actual) {}

    /**
     * @brief 构造 canonical frame 内容错误。
     *
     * @param message 稳定错误说明。
     */
    explicit WireCodecError(const char *message) : std::runtime_error(message) {}

    /**
     * @brief 返回期望字节数。
     */
    constexpr std::size_t expected() const noexcept { return expected_; }

    /**
     * @brief 返回实际字节数。
     */
    constexpr std::size_t actual() const noexcept { return actual_; }

   private:
    std::size_t expected_;
    std::size_t actual_;
};

/**
 * @brief 校验 wire payload buffer 大小。
 *
 * @throws WireCodecError 当实际大小与 codec 固定大小不一致。
 */
inline void ensure_wire_size(std::size_t expected, std::size_t actual) {
    if (expected != actual) {
        throw WireCodecError(expected, actual);
    }
}

namespace detail {

template <typename T, typename Enable = void>
struct WireStorageSelector {
    using Type = std::make_unsigned_t<T>;
};

template <typename T>
struct WireStorageSelector<T, std::enable_if_t<std::is_floating_point_v<T>>> {
    using Type =
        std::conditional_t<sizeof(T) == sizeof(std::uint32_t), std::uint32_t, std::uint64_t>;
};

template <>
struct WireStorageSelector<bool, void> {
    using Type = std::uint8_t;
};

template <typename T>
using WireStorageT = typename WireStorageSelector<T>::Type;

template <typename T>
WireStorageT<T> wire_to_storage(T value) noexcept {
    if constexpr (std::is_floating_point_v<T>) {
        WireStorageT<T> storage{};
        std::memcpy(&storage, &value, sizeof(T));
        return storage;
    } else if constexpr (std::is_same_v<T, bool>) {
        return value ? std::uint8_t{1} : std::uint8_t{0};
    } else {
        return static_cast<WireStorageT<T>>(value);
    }
}

template <typename T>
T wire_from_storage(WireStorageT<T> storage) noexcept {
    if constexpr (std::is_floating_point_v<T>) {
        T value{};
        std::memcpy(&value, &storage, sizeof(T));
        return value;
    } else if constexpr (std::is_same_v<T, bool>) {
        return storage != 0U;
    } else {
        return static_cast<T>(storage);
    }
}

}  // namespace detail

/**
 * @brief 按 little-endian 写入一个 fixed-size scalar。
 */
template <typename T>
void write_wire_le(std::span<std::uint8_t> output, std::size_t offset, T value) {
    auto storage = detail::wire_to_storage(value);
    for (std::size_t index = 0; index < sizeof(T); ++index) {
        output[offset + index] = static_cast<std::uint8_t>((storage >> (index * 8U)) & 0xFFU);
    }
}

/**
 * @brief 按 little-endian 读取一个 fixed-size scalar。
 */
template <typename T>
T read_wire_le(std::span<const std::uint8_t> input, std::size_t offset) {
    detail::WireStorageT<T> storage{};
    for (std::size_t index = 0; index < sizeof(T); ++index) {
        storage |= static_cast<detail::WireStorageT<T>>(input[offset + index]) << (index * 8U);
    }
    return detail::wire_from_storage<T>(storage);
}

inline constexpr std::size_t VAR_SPAN_WIRE_SIZE = 8U;

/**
 * @brief canonical frame 中一个变长字段的 tail-relative 描述符。
 */
struct VarSpan {
    std::uint32_t offset = 0;
    std::uint32_t len = 0;

    constexpr bool empty() const noexcept { return len == 0; }
};

inline void write_var_span(std::span<std::uint8_t> output, VarSpan span) {
    ensure_wire_size(VAR_SPAN_WIRE_SIZE, output.size());
    write_wire_le(output, 0, span.offset);
    write_wire_le(output, 4, span.len);
}

inline VarSpan read_var_span(std::span<const std::uint8_t> input) {
    ensure_wire_size(VAR_SPAN_WIRE_SIZE, input.size());
    return VarSpan{read_wire_le<std::uint32_t>(input, 0), read_wire_le<std::uint32_t>(input, 4)};
}

template <typename T>
concept CanonicalFixedWireMessage =
    requires(const T &value, std::span<std::uint8_t> output, std::span<const std::uint8_t> input) {
        { T::wire_size() } -> std::convertible_to<std::size_t>;
        { value.encode_wire(output) } -> std::same_as<void>;
        { T::decode_wire(input) } -> std::same_as<T>;
    };

template <typename T>
concept CanonicalFrameMessage =
    requires(const T &value, std::span<std::uint8_t> output, std::span<const std::uint8_t> input) {
        { value.encoded_frame_size() } -> std::convertible_to<std::size_t>;
        { T::max_frame_size() } -> std::convertible_to<std::size_t>;
        { value.encode_frame(output) } -> std::same_as<void>;
        { T::decode_frame(input) } -> std::same_as<T>;
    };

template <typename T>
concept CanonicalTransportMessage = CanonicalFrameMessage<T> || CanonicalFixedWireMessage<T>;

namespace detail {

template <CanonicalTransportMessage T>
std::size_t encoded_frame_size(const T &value) {
    if constexpr (CanonicalFrameMessage<T>) {
        return value.encoded_frame_size();
    } else {
        (void)value;
        return T::wire_size();
    }
}

template <CanonicalTransportMessage T>
void encode_frame(const T &value, std::span<std::uint8_t> output) {
    if constexpr (CanonicalFrameMessage<T>) {
        value.encode_frame(output);
    } else {
        value.encode_wire(output);
    }
}

template <CanonicalTransportMessage T>
T decode_frame(std::span<const std::uint8_t> input) {
    if constexpr (CanonicalFrameMessage<T>) {
        return T::decode_frame(input);
    } else {
        return T::decode_wire(input);
    }
}

}  // namespace detail

template <std::size_t MAX>
class BoundedBytes {
   public:
    BoundedBytes() = default;

    explicit BoundedBytes(std::span<const std::uint8_t> bytes) { assign(bytes); }

    static BoundedBytes from(std::span<const std::uint8_t> bytes) { return BoundedBytes(bytes); }

    void assign(std::span<const std::uint8_t> bytes) {
        if (bytes.size() > MAX) {
            throw WireCodecError("bounded bytes length exceeds declared maximum");
        }
        bytes_.assign(bytes.begin(), bytes.end());
    }

    std::span<const std::uint8_t> as_span() const noexcept { return bytes_; }
    const std::vector<std::uint8_t> &vector() const noexcept { return bytes_; }
    std::size_t size() const noexcept { return bytes_.size(); }
    bool empty() const noexcept { return bytes_.empty(); }

   private:
    std::vector<std::uint8_t> bytes_;
};

template <std::size_t MAX>
class BoundedString {
   public:
    BoundedString() = default;

    explicit BoundedString(std::string_view value) { assign(value); }

    static BoundedString from(std::string_view value) { return BoundedString(value); }

    static BoundedString from_utf8(std::span<const std::uint8_t> bytes) {
        if (!valid_utf8(bytes)) {
            throw WireCodecError("bounded string is not valid UTF-8");
        }
        if (bytes.empty()) {
            return BoundedString{};
        }
        return BoundedString(
            std::string_view{reinterpret_cast<const char *>(bytes.data()), bytes.size()});
    }

    void assign(std::string_view value) {
        if (value.size() > MAX) {
            throw WireCodecError("bounded string length exceeds declared maximum");
        }
        value_.assign(value);
    }

    std::string_view view() const noexcept { return value_; }
    std::span<const std::uint8_t> bytes() const noexcept {
        return {reinterpret_cast<const std::uint8_t *>(value_.data()), value_.size()};
    }
    std::size_t size() const noexcept { return value_.size(); }
    bool empty() const noexcept { return value_.empty(); }

   private:
    static bool valid_utf8(std::span<const std::uint8_t> bytes) noexcept {
        std::size_t index = 0;
        while (index < bytes.size()) {
            const auto byte = bytes[index];
            std::size_t extra = 0;
            std::uint32_t codepoint = 0;
            if (byte <= 0x7FU) {
                ++index;
                continue;
            }
            if ((byte & 0xE0U) == 0xC0U) {
                extra = 1;
                codepoint = byte & 0x1FU;
                if (codepoint == 0) {
                    return false;
                }
            } else if ((byte & 0xF0U) == 0xE0U) {
                extra = 2;
                codepoint = byte & 0x0FU;
            } else if ((byte & 0xF8U) == 0xF0U) {
                extra = 3;
                codepoint = byte & 0x07U;
            } else {
                return false;
            }
            if (index + extra >= bytes.size()) {
                return false;
            }
            for (std::size_t offset = 1; offset <= extra; ++offset) {
                const auto continuation = bytes[index + offset];
                if ((continuation & 0xC0U) != 0x80U) {
                    return false;
                }
                codepoint = (codepoint << 6U) | (continuation & 0x3FU);
            }
            if ((extra == 2 && codepoint < 0x800U) || (extra == 3 && codepoint < 0x10000U) ||
                codepoint > 0x10FFFFU || (codepoint >= 0xD800U && codepoint <= 0xDFFFU)) {
                return false;
            }
            index += extra + 1;
        }
        return true;
    }

    std::string value_;
};

template <typename T, std::size_t MAX>
class BoundedSequence {
   public:
    BoundedSequence() = default;

    explicit BoundedSequence(std::span<const T> values) { assign(values); }

    static BoundedSequence from(std::span<const T> values) { return BoundedSequence(values); }

    void assign(std::span<const T> values) {
        if (values.size() > MAX) {
            throw WireCodecError("bounded sequence length exceeds declared maximum");
        }
        values_.assign(values.begin(), values.end());
    }

    std::span<const T> as_span() const noexcept { return values_; }
    const std::vector<T> &vector() const noexcept { return values_; }
    std::size_t size() const noexcept { return values_.size(); }
    bool empty() const noexcept { return values_.empty(); }

   private:
    std::vector<T> values_;
};

class FrameDecoder {
   public:
    explicit FrameDecoder(std::span<const std::uint8_t> tail) noexcept : tail_(tail) {}

    std::span<const std::uint8_t> read_block(VarSpan span, std::size_t max_len) {
        const auto len = static_cast<std::size_t>(span.len);
        if (len == 0U) {
            if (span.offset != 0U) {
                throw WireCodecError("empty variable span must use zero offset");
            }
            return {};
        }
        if (len > max_len) {
            throw WireCodecError("variable field length exceeds declared maximum");
        }
        const auto offset = static_cast<std::size_t>(span.offset);
        if (offset != cursor_) {
            throw WireCodecError("variable tail blocks are not canonical");
        }
        if (offset > tail_.size() || len > tail_.size() - offset) {
            throw WireCodecError("variable span exceeds frame tail length");
        }
        cursor_ = offset + len;
        return tail_.subspan(offset, len);
    }

    void finish() const {
        if (cursor_ != tail_.size()) {
            throw WireCodecError("variable frame contains trailing tail bytes");
        }
    }

   private:
    std::span<const std::uint8_t> tail_;
    std::size_t cursor_ = 0;
};

inline VarSpan append_tail_block(std::vector<std::uint8_t> &tail,
                                 std::span<const std::uint8_t> bytes) {
    if (bytes.empty()) {
        return VarSpan{};
    }
    if (tail.size() > static_cast<std::size_t>(std::numeric_limits<std::uint32_t>::max()) ||
        bytes.size() > static_cast<std::size_t>(std::numeric_limits<std::uint32_t>::max())) {
        throw WireCodecError("variable tail span exceeds u32");
    }
    const auto offset = static_cast<std::uint32_t>(tail.size());
    const auto len = static_cast<std::uint32_t>(bytes.size());
    tail.insert(tail.end(), bytes.begin(), bytes.end());
    return VarSpan{offset, len};
}

}  // namespace flowrt
