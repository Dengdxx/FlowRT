#pragma once

#include <algorithm>
#include <array>
#include <atomic>
#include <cerrno>
#include <chrono>
#include <concepts>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <fcntl.h>
#include <filesystem>
#include <functional>
#include <limits>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <stdexcept>
#include <string>
#include <string_view>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <system_error>
#include <thread>
#include <type_traits>
#include <unistd.h>
#include <utility>
#include <variant>
#include <vector>

#ifdef FLOWRT_HAS_ICEORYX2_CXX
#include <iox2/iceoryx2.hpp>
#endif

#ifdef FLOWRT_HAS_ZENOH_CXX
#include <zenoh.hxx>
#endif

namespace flowrt {

/**
 * @brief 组件回调和调度步骤的统一返回状态。
 *
 * 生成的 runtime shell 通过该状态决定是否继续当前调度循环。算法代码不应抛出异常来表达
 * FlowRT 语义错误；需要重试或停止时返回对应状态。
 */
enum class Status : std::uint8_t {
    Ok = 0,     ///< 本次步骤完成，调度器可以继续执行后续 tick。
    Retry = 1,  ///< 本次步骤未完成，调用方可按调度策略稍后重试。
    Error = 2,  ///< 本次步骤失败，调度器应停止当前运行序列并向上报告。
};

/**
 * @brief 返回成功状态的便捷函数。
 *
 * @return `Status::Ok`。
 */
constexpr Status ok() noexcept { return Status::Ok; }

/**
 * @brief runtime 传递给生命周期钩子和调度步骤的上下文。
 *
 * v0.1 暂不暴露资源句柄；保留该类型是为了后续承载 clock、logger、参数快照和 backend
 * 能力视图，同时保持用户接口稳定。
 */
struct Context {};

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
        : std::runtime_error("wire payload size mismatch"),
          expected_(expected),
          actual_(actual) {}

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
    using Type = std::
        conditional_t<sizeof(T) == sizeof(std::uint32_t), std::uint32_t, std::uint64_t>;
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
        return BoundedString(std::string_view{reinterpret_cast<const char *>(bytes.data()),
                                             bytes.size()});
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

/**
 * @brief 有界 channel 写满时的处理策略。
 */
enum class OverflowPolicy : std::uint8_t {
    DropOldest = 0,  ///< 丢弃最旧样本，接收新样本。
    DropNewest = 1,  ///< 丢弃当前写入样本，保留已有队列。
    Error = 2,       ///< 返回溢出错误，由 runtime shell 或用户代码处理。
    Block = 3,       ///< 表达背压意图；实时路径不应默认使用无界阻塞。
};

/**
 * @brief channel 写入成功后的结果。
 */
enum class ChannelWriteOutcome : std::uint8_t {
    Accepted = 0,       ///< 样本已进入 channel。
    DroppedOldest = 1,  ///< 为接收新样本丢弃了最旧样本。
    DroppedNewest = 2,  ///< 当前样本被丢弃。
    Backpressured = 3,  ///< 写入方遇到背压，样本未进入 channel。
};

/**
 * @brief channel 严格写入失败时的错误。
 */
enum class ChannelError : std::uint8_t {
    Overflow = 0,   ///< 有界队列已满且策略要求报告错误。
    Transport = 1,  ///< backend transport 无法完成本次读写。
};

/**
 * @brief 有界 channel 写入结果。
 *
 * variant 左侧表示已按策略处理，右侧表示需要调用方显式处理的错误。
 */
using ChannelPushResult = std::variant<ChannelWriteOutcome, ChannelError>;

/**
 * @brief 输入样本过期时的处理策略。
 */
enum class StalePolicy : std::uint8_t {
    Warn = 0,      ///< 保留样本并暴露 stale 标记。
    Drop = 1,      ///< 过期后隐藏样本。
    HoldLast = 2,  ///< 保留最后一个样本并暴露 stale 标记。
    Error = 3,     ///< 由 generated shell 将过期输入提升为错误状态。
};

/**
 * @brief 带时间戳 channel 读取时的 freshness 配置。
 *
 * C++ runtime 使用 `std::chrono::milliseconds` 表达时间窗口，避免在公共 API 中传递没有单位的
 * 裸整数。generated shell 仍可把调度 tick 归一化为毫秒计数，再交给 channel 计算 stale 状态。
 */
class StaleConfig {
   public:
    /**
     * @brief freshness 时间窗口类型。
     */
    using Duration = std::chrono::milliseconds;

    /**
     * @brief 构造不检查过期时间的默认配置。
     */
    constexpr StaleConfig() noexcept = default;

    /**
     * @brief 构造不检查过期时间、但保留指定 stale policy 的配置。
     *
     * @param policy 样本过期时的处理策略。
     */
    constexpr explicit StaleConfig(StalePolicy policy) noexcept : policy_(policy) {}

    /**
     * @brief 构造带最大样本年龄的 freshness 配置。
     *
     * @param max_age 最大允许样本年龄。
     * @param policy 样本过期时的处理策略。
     */
    constexpr StaleConfig(Duration max_age, StalePolicy policy) noexcept
        : max_age_(max_age), policy_(policy) {}

    /**
     * @brief 返回不检查过期时间的默认配置。
     *
     * @return 默认 freshness 配置。
     */
    static constexpr StaleConfig none() noexcept { return StaleConfig{}; }

    /**
     * @brief 返回最大允许样本年龄。
     *
     * @return 配置了年龄窗口时返回该窗口，否则返回空值。
     */
    constexpr std::optional<Duration> max_age() const noexcept { return max_age_; }

    /**
     * @brief 返回样本过期时的处理策略。
     *
     * @return stale policy。
     */
    constexpr StalePolicy policy() const noexcept { return policy_; }

    /**
     * @brief 判断指定发布时间的样本在当前时间是否过期。
     *
     * @param published_at_ms 样本发布时间，单位为 runtime 毫秒。
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 超过 `max_age` 时返回 true。
     */
    constexpr bool stale_at(std::optional<std::uint64_t> published_at_ms,
                            std::uint64_t now_ms) const noexcept {
        if (!max_age_ || !published_at_ms || now_ms <= *published_at_ms) {
            return false;
        }
        if (max_age_->count() < 0) {
            return true;
        }
        const auto max_age_ms = static_cast<std::uint64_t>(max_age_->count());
        return now_ms - *published_at_ms > max_age_ms;
    }

   private:
    std::optional<Duration> max_age_;
    StalePolicy policy_ = StalePolicy::Warn;
};

/**
 * @brief latest snapshot 输入视图。
 *
 * @tparam T 消息类型，必须由 Contract IR 和 Message ABI 约束保证布局稳定。
 *
 * `Latest<T>` 不拥有消息对象，只在一次用户回调期间借用 runtime shell 中的最新样本。
 * `present()` 表示当前是否有可读样本，`stale()` 表示样本是否超过 RSDL 声明的 freshness
 * 约束。用户代码不得保存内部指针到回调之外。
 */
template <typename T>
class Latest {
   public:
    /**
     * @brief 构造一个空输入视图。
     */
    constexpr Latest() noexcept = default;

    /**
     * @brief 从借用指针和 stale 标记构造输入视图。
     *
     * @param value 当前可读样本；为 `nullptr` 时表示没有样本。
     * @param stale 当前样本是否过期。
     */
    constexpr Latest(const T *value, bool stale = false) noexcept : value_(value), stale_(stale) {}

    /**
     * @brief 判断当前输入是否有样本。
     *
     * @return 有样本时返回 true。
     */
    constexpr bool present() const noexcept { return value_ != nullptr; }

    /**
     * @brief 判断当前样本是否已过期。
     *
     * @return 样本超过 freshness 约束时返回 true。
     */
    constexpr bool stale() const noexcept { return stale_; }

    /**
     * @brief 返回当前样本指针。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    constexpr const T *get() const noexcept { return value_; }

    /**
     * @brief 返回当前样本指针。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    constexpr const T *as_ref() const noexcept { return value_; }

   private:
    const T *value_ = nullptr;
    bool stale_ = false;
};

/**
 * @brief 组件输出端口的单样本写入句柄。
 *
 * @tparam T 输出消息类型。
 *
 * 用户回调通过 `write()` 设置本次输出。runtime shell 在回调返回后取走该值并发布到对应
 * channel；如果用户没有写入，则该端口本次 tick 不产生输出。
 */
template <typename T>
class Output {
   public:
    /**
     * @brief 构造空输出句柄。
     */
    Output() = default;

    /**
     * @brief 写入本次回调的输出样本。
     *
     * @param value 要发布的消息。若重复调用，后一次写入覆盖前一次值。
     */
    void write(T value) { value_ = std::move(value); }

    /**
     * @brief 判断本次回调是否已经写入输出。
     *
     * @return 已写入时返回 true。
     */
    bool present() const noexcept { return value_.has_value(); }

    /**
     * @brief 借用当前输出样本。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    const T *as_ref() const noexcept { return value_ ? std::addressof(*value_) : nullptr; }

    /**
     * @brief 可变借用当前输出样本。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    T *as_mut() noexcept { return value_ ? std::addressof(*value_) : nullptr; }

    /**
     * @brief 取走当前输出样本并清空句柄。
     *
     * @return 样本存在时返回该样本，否则返回空值。
     */
    std::optional<T> take() {
        std::optional<T> value = std::move(value_);
        value_.reset();
        return value;
    }

   private:
    std::optional<T> value_;
};

/**
 * @brief latest channel 的最小内存态实现。
 *
 * @tparam T channel 承载的消息类型。
 *
 * 该类型服务于 C++ inproc demo 和生成 shell 的语义验证。真实跨进程 backend 需要保持同样的
 * `Latest<T>` 用户视图语义，但可以使用不同存储和传输机制。
 */
template <typename T>
class LatestChannel {
   public:
    /**
     * @brief 构造空 latest channel。
     */
    LatestChannel() = default;

    /**
     * @brief 使用 freshness 配置构造空 latest channel。
     *
     * @param stale_config 读取时使用的 freshness 配置。
     */
    explicit LatestChannel(StaleConfig stale_config) noexcept : stale_config_(stale_config) {}

    /**
     * @brief 使用 freshness 配置构造空 latest channel。
     *
     * 该工厂函数与 Rust runtime 的 `with_stale_config` 保持命名一致，方便 codegen
     * 在跨语言 shell 中使用同一套语义表达。
     *
     * @param stale_config 读取时使用的 freshness 配置。
     * @return 配置后的空 latest channel。
     */
    static LatestChannel with_stale_config(StaleConfig stale_config) noexcept {
        return LatestChannel(stale_config);
    }

    /**
     * @brief 发布一个新样本并清除 stale 标记。
     *
     * @param value 新样本。
     */
    void publish(T value) {
        value_ = std::move(value);
        stale_ = false;
        published_at_ms_.reset();
    }

    /**
     * @brief 带 runtime 时间戳发布一个新样本。
     *
     * @param value 新样本。
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     */
    void publish_at(T value, std::uint64_t now_ms) {
        value_ = std::move(value);
        stale_ = false;
        published_at_ms_ = now_ms;
    }

    /**
     * @brief 设置当前样本的 stale 标记。
     *
     * @param stale 为 true 时，后续 `view()` 会暴露过期状态。
     */
    void mark_stale(bool stale) noexcept { stale_ = stale; }

    /**
     * @brief 借用当前 latest snapshot。
     *
     * @return 只在 channel 当前状态有效期间可用的输入视图。
     */
    Latest<T> view() const noexcept {
        return Latest<T>{value_ ? std::addressof(*value_) : nullptr, stale_};
    }

    /**
     * @brief 以指定 runtime 时间读取 latest snapshot，并按 freshness 配置计算 stale 状态。
     *
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 只在 channel 当前状态有效期间可用的输入视图。
     */
    Latest<T> view_at(std::uint64_t now_ms) const noexcept {
        const bool stale = stale_ || stale_config_.stale_at(published_at_ms_, now_ms);
        const bool drop_stale = stale && stale_config_.policy() == StalePolicy::Drop;
        return Latest<T>{value_ && !drop_stale ? std::addressof(*value_) : nullptr, stale};
    }

    /**
     * @brief 取走当前样本并清空 channel。
     *
     * @return 样本存在时返回该样本，否则返回空值。
     */
    std::optional<T> take() {
        std::optional<T> value = std::move(value_);
        value_.reset();
        return value;
    }

   private:
    std::optional<T> value_;
    bool stale_ = false;
    std::optional<std::uint64_t> published_at_ms_;
    StaleConfig stale_config_;
};

/**
 * @brief FIFO channel 的单次读取结果。
 *
 * @tparam T channel 承载的消息类型。
 *
 * 该类型拥有从 FIFO 队列取出的样本，并在一次调度步骤内借出 `Latest<T>` 用户视图。
 */
template <typename T>
class FifoRead {
   public:
    /**
     * @brief 构造空读取结果。
     */
    FifoRead() = default;

    /**
     * @brief 从样本和 stale 标记构造读取结果。
     *
     * @param value 本次读取到的样本；为空表示没有样本或 stale drop 已隐藏样本。
     * @param stale 本次读取是否发现样本过期。
     */
    FifoRead(std::optional<T> value, bool stale) : value_(std::move(value)), stale_(stale) {}

    /**
     * @brief 借用本次读取结果，形成组件输入使用的 latest-style 视图。
     *
     * @return 只在本读取结果对象存活期间可用的输入视图。
     */
    Latest<T> view() const noexcept {
        return Latest<T>{value_ ? std::addressof(*value_) : nullptr, stale_};
    }

    /**
     * @brief 判断本次读取是否有可见样本。
     *
     * @return 有可见样本时返回 true。
     */
    bool present() const noexcept { return value_.has_value(); }

    /**
     * @brief 判断本次读取是否发现样本过期。
     *
     * @return 样本超过 freshness 约束时返回 true。
     */
    bool stale() const noexcept { return stale_; }

    /**
     * @brief 借用本次读取的样本。
     *
     * @return 样本存在时返回非空指针，否则返回 `nullptr`。
     */
    const T *as_ref() const noexcept { return value_ ? std::addressof(*value_) : nullptr; }

   private:
    std::optional<T> value_;
    bool stale_ = false;
};

/**
 * @brief 有界 FIFO channel 的最小内存态实现。
 *
 * @tparam T channel 承载的消息类型。
 *
 * `FifoChannel` 用于表达 RSDL 中 `fifo(depth = N)` 的基础行为。它不提供线程同步；
 * 多线程或跨进程 backend 应在自己的实现中保证并发安全，并保持相同的 overflow 语义。
 */
template <typename T>
class FifoChannel {
   public:
    /**
     * @brief 构造有界 FIFO channel。
     *
     * @param depth 队列深度；传入 0 时按 1 处理。
     * @param overflow 队列满时的处理策略。
     */
    explicit FifoChannel(std::size_t depth, OverflowPolicy overflow)
        : depth_(depth == 0 ? 1 : depth), overflow_(overflow) {}

    /**
     * @brief 使用 freshness 配置构造有界 FIFO channel。
     *
     * @param depth 队列深度；传入 0 时按 1 处理。
     * @param overflow 队列满时的处理策略。
     * @param stale_config 读取时使用的 freshness 配置。
     * @return 配置后的空 FIFO channel。
     */
    static FifoChannel with_stale_config(std::size_t depth, OverflowPolicy overflow,
                                         StaleConfig stale_config) noexcept {
        FifoChannel channel(depth, overflow);
        channel.stale_config_ = stale_config;
        return channel;
    }

    /**
     * @brief 写入一个样本。
     *
     * @param value 要写入的消息。
     * @return 成功处理结果或严格错误。
     */
    ChannelPushResult push(T value) { return push_entry(Entry{std::move(value), std::nullopt}); }

    /**
     * @brief 带 runtime 时间戳写入一个样本。
     *
     * @param value 要写入的消息。
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 成功处理结果或严格错误。
     */
    ChannelPushResult push_at(T value, std::uint64_t now_ms) {
        return push_entry(Entry{std::move(value), now_ms});
    }

    /**
     * @brief 弹出最旧样本。
     *
     * @return 队列非空时返回样本，否则返回空值。
     */
    std::optional<T> pop() {
        if (queue_.empty()) {
            return std::nullopt;
        }
        Entry entry = std::move(queue_.front());
        queue_.pop_front();
        return std::move(entry.value);
    }

    /**
     * @brief 以指定 runtime 时间弹出最旧样本，并按 freshness 配置计算 stale 状态。
     *
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 拥有样本的读取结果。
     */
    FifoRead<T> pop_at(std::uint64_t now_ms) {
        if (queue_.empty()) {
            return FifoRead<T>{};
        }
        Entry entry = std::move(queue_.front());
        queue_.pop_front();
        const bool stale = stale_config_.stale_at(entry.published_at_ms, now_ms);
        if (stale && stale_config_.policy() == StalePolicy::Drop) {
            return FifoRead<T>{std::nullopt, true};
        }
        return FifoRead<T>{std::move(entry.value), stale};
    }

    /**
     * @brief 返回当前队列长度。
     *
     * @return 当前样本数量。
     */
    std::size_t len() const noexcept { return queue_.size(); }

    /**
     * @brief 判断队列是否为空。
     *
     * @return 队列为空时返回 true。
     */
    bool empty() const noexcept { return queue_.empty(); }

    /**
     * @brief 返回归一化后的队列深度。
     *
     * @return 至少为 1 的队列深度。
     */
    std::size_t depth() const noexcept { return depth_; }

   private:
    struct Entry {
        T value;
        std::optional<std::uint64_t> published_at_ms;
    };

    ChannelPushResult push_entry(Entry entry) {
        if (queue_.size() < depth_) {
            queue_.push_back(std::move(entry));
            return ChannelWriteOutcome::Accepted;
        }

        switch (overflow_) {
            case OverflowPolicy::DropOldest:
                queue_.pop_front();
                queue_.push_back(std::move(entry));
                return ChannelWriteOutcome::DroppedOldest;
            case OverflowPolicy::DropNewest:
                return ChannelWriteOutcome::DroppedNewest;
            case OverflowPolicy::Error:
                return ChannelError::Overflow;
            case OverflowPolicy::Block:
                return ChannelWriteOutcome::Backpressured;
        }

        return ChannelWriteOutcome::Backpressured;
    }

    std::deque<Entry> queue_;
    std::size_t depth_ = 1;
    OverflowPolicy overflow_ = OverflowPolicy::DropOldest;
    StaleConfig stale_config_;
};

namespace iox2 {

/**
 * @brief FlowRT iox2 transport user header。
 *
 * 该 header 保存 transport 层 runtime timestamp，使 iceoryx2 payload 仍保持业务消息类型 `T`。这样
 * C++ `IOX2_TYPE_NAME` 与 Rust `#[type_name(...)]` 都作用在同一个 payload 类型上。
 */
struct FlowrtIox2Header {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTIox2Header";

    std::uint64_t published_at_ms{};
};

/**
 * @brief 打开 iceoryx2 publish-subscribe endpoint 时使用的 C++ QoS 配置。
 *
 * 该类型承载 Contract IR channel policy 归一化后的 depth、overflow 和 freshness intent。它不暴露
 * iceoryx2 底层 publisher/subscriber API；生成 shell 用它把 FlowRT 语义传给后续真实 transport
 * binding。
 */
class Iox2ChannelConfig {
   public:
    /**
     * @brief 构造 latest channel 的默认 QoS 配置。
     *
     * @return depth 为 1、overflow 为 DropOldest 的配置。
     */
    static constexpr Iox2ChannelConfig latest() noexcept { return Iox2ChannelConfig{}; }

    /**
     * @brief 构造 FIFO channel 的 QoS 配置。
     *
     * @param depth 队列深度；传入 0 时按 1 处理。
     * @param overflow 队列满时的 FlowRT 语义。
     * @return 归一化后的配置。
     */
    static constexpr Iox2ChannelConfig fifo(std::size_t depth, OverflowPolicy overflow) noexcept {
        return Iox2ChannelConfig(depth == 0 ? 1 : depth, overflow, StaleConfig{});
    }

    /**
     * @brief 设置 freshness 配置。
     *
     * @param stale stale-data policy 和时间窗口。
     * @return 更新后的配置副本。
     */
    constexpr Iox2ChannelConfig with_stale_config(StaleConfig stale) const noexcept {
        return Iox2ChannelConfig(depth_, overflow_, stale);
    }

    /**
     * @brief 返回归一化后的 channel depth。
     */
    constexpr std::size_t depth() const noexcept { return depth_; }

    /**
     * @brief 返回 overflow policy。
     */
    constexpr OverflowPolicy overflow() const noexcept { return overflow_; }

    /**
     * @brief 返回 stale-data 配置。
     */
    constexpr StaleConfig stale() const noexcept { return stale_; }

   private:
    constexpr Iox2ChannelConfig() noexcept = default;

    constexpr Iox2ChannelConfig(std::size_t depth, OverflowPolicy overflow,
                                StaleConfig stale) noexcept
        : depth_(depth), overflow_(overflow), stale_(stale) {}

    std::size_t depth_ = 1;
    OverflowPolicy overflow_ = OverflowPolicy::DropOldest;
    StaleConfig stale_;
};

/**
 * @brief typed iceoryx2 publish-subscribe endpoint 的 C++ API 边界。
 *
 * @tparam T FlowRT Message ABI v0.1 plain-data payload 类型。
 *
 * 开启 `FLOWRT_HAS_ICEORYX2_CXX` 时，该类绑定真实 `iceoryx2-cxx` typed pub/sub endpoint；
 * 默认构建不依赖 iceoryx2，并保持安全失败语义。业务组件接口不应暴露该类型。
 */
template <typename T>
class Iox2PubSub {
   public:
    Iox2PubSub(Iox2PubSub &&) noexcept = default;
    Iox2PubSub(const Iox2PubSub &) = delete;
    auto operator=(Iox2PubSub &&) noexcept -> Iox2PubSub & = default;
    auto operator=(const Iox2PubSub &) -> Iox2PubSub & = delete;
    ~Iox2PubSub() = default;

    /**
     * @brief 打开或创建一个 FlowRT iox2 service endpoint。
     *
     * @param service_name canonical iox2 service name。
     * @param config 从 Contract IR channel policy 生成的 QoS 配置。
     * @return endpoint 对象；底层资源打开失败或未开启 iox2 支持时 `ready()` 返回 false。
     */
    static Iox2PubSub open_with_config(std::string_view service_name, Iox2ChannelConfig config) {
        return Iox2PubSub(service_name, config);
    }

    /**
     * @brief 返回 canonical service name。
     */
    std::string_view service_name() const noexcept { return service_name_; }

    /**
     * @brief 返回 channel QoS 配置。
     */
    constexpr Iox2ChannelConfig config() const noexcept { return config_; }

    /**
     * @brief 判断 transport endpoint 是否已经绑定到底层 iceoryx2 资源。
     */
    bool ready() const noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        return publisher_.has_value() && subscriber_.has_value();
#else
        return false;
#endif
    }

    /**
     * @brief 带 FlowRT runtime 时间戳发布一个值。
     *
     * @return 写入成功时返回 `Accepted`；transport 无法完成时返回 `ChannelError::Transport`。
     */
    ChannelPushResult publish_at(T value, std::uint64_t published_at_ms) noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        if (!publisher_) {
            return ChannelError::Transport;
        }

        auto sample = publisher_->loan_uninit();
        if (!sample.has_value()) {
            return ChannelError::Transport;
        }

        auto loaned_sample = std::move(sample).value();
        loaned_sample.user_header_mut().published_at_ms = published_at_ms;
        auto initialized_sample = loaned_sample.write_payload(std::move(value));
        auto sent = ::iox2::send(std::move(initialized_sample));
        if (!sent.has_value()) {
            return ChannelError::Transport;
        }

        return ChannelWriteOutcome::Accepted;
#else
        (void)value;
        (void)published_at_ms;
        return ChannelError::Transport;
#endif
    }

    /**
     * @brief 读取 latest snapshot，并保留 transport 错误通道。
     *
     * @return 读取成功时返回 `Latest<T>`；transport 无法完成时返回 `ChannelError::Transport`。
     */
    std::variant<Latest<T>, ChannelError> receive_latest_at(std::uint64_t now_ms) noexcept {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        if (!subscriber_) {
            return ChannelError::Transport;
        }

        while (true) {
            auto received = subscriber_->receive();
            if (!received.has_value()) {
                return ChannelError::Transport;
            }

            auto sample = std::move(received).value();
            if (!sample.has_value()) {
                break;
            }

            received_ = sample->payload();
            published_at_ms_ = sample->user_header().published_at_ms;
        }

        const bool stale = config_.stale().stale_at(published_at_ms_, now_ms);
        const bool drop_stale = stale && config_.stale().policy() == StalePolicy::Drop;
        return Latest<T>{received_ && !drop_stale ? std::addressof(*received_) : nullptr, stale};
#else
        (void)now_ms;
        return ChannelError::Transport;
#endif
    }

   private:
    Iox2PubSub(std::string_view service_name, Iox2ChannelConfig config)
        : service_name_(service_name), config_(config) {
#ifdef FLOWRT_HAS_ICEORYX2_CXX
        static_assert(std::is_trivially_copyable_v<T>,
                      "FlowRT iox2 C++ payload must be trivially copyable");
        open_iox2_endpoint();
#endif
    }

#ifdef FLOWRT_HAS_ICEORYX2_CXX
    using Iox2Node = ::iox2::Node<::iox2::ServiceType::Ipc>;
    using Iox2Service =
        ::iox2::PortFactoryPublishSubscribe<::iox2::ServiceType::Ipc, T, FlowrtIox2Header>;
    using Iox2Publisher = ::iox2::Publisher<::iox2::ServiceType::Ipc, T, FlowrtIox2Header>;
    using Iox2Subscriber = ::iox2::Subscriber<::iox2::ServiceType::Ipc, T, FlowrtIox2Header>;

    static constexpr bool safe_overflow(Iox2ChannelConfig config) noexcept {
        return config.overflow() != OverflowPolicy::Block;
    }

    static constexpr ::iox2::BackpressureStrategy backpressure_strategy(
        Iox2ChannelConfig config) noexcept {
        return config.overflow() == OverflowPolicy::Block
                   ? ::iox2::BackpressureStrategy::RetryUntilDelivered
                   : ::iox2::BackpressureStrategy::DiscardData;
    }

    void open_iox2_endpoint() {
        auto name = ::iox2::ServiceName::create(service_name_.c_str());
        if (!name.has_value()) {
            return;
        }

        auto node = ::iox2::NodeBuilder().create<::iox2::ServiceType::Ipc>();
        if (!node.has_value()) {
            return;
        }
        node_.emplace(std::move(node).value());

        const auto depth = static_cast<std::uint64_t>(config_.depth());
        auto service = node_->service_builder(std::move(name).value())
                           .publish_subscribe<T>()
                           .template user_header<FlowrtIox2Header>()
                           .enable_safe_overflow(safe_overflow(config_))
                           .history_size(depth)
                           .subscriber_max_buffer_size(depth)
                           .open_or_create();
        if (!service.has_value()) {
            node_.reset();
            return;
        }
        service_.emplace(std::move(service).value());

        auto subscriber = service_->subscriber_builder().buffer_size(depth).create();
        if (!subscriber.has_value()) {
            service_.reset();
            node_.reset();
            return;
        }
        subscriber_.emplace(std::move(subscriber).value());

        auto publisher = service_->publisher_builder()
                             .backpressure_strategy(backpressure_strategy(config_))
                             .max_loaned_samples(depth)
                             .create();
        if (!publisher.has_value()) {
            subscriber_.reset();
            service_.reset();
            node_.reset();
            return;
        }
        publisher_.emplace(std::move(publisher).value());
    }
#endif

    std::string service_name_;
    Iox2ChannelConfig config_;
#ifdef FLOWRT_HAS_ICEORYX2_CXX
    std::optional<Iox2Node> node_;
    std::optional<Iox2Service> service_;
    std::optional<Iox2Publisher> publisher_;
    std::optional<Iox2Subscriber> subscriber_;
    std::optional<T> received_;
    std::optional<std::uint64_t> published_at_ms_;
#endif
};

}  // namespace iox2

namespace zenoh {

/**
 * @brief 打开 zenoh publish-subscribe endpoint 时使用的 FlowRT channel 配置。
 *
 * 该类型承载 Contract IR channel policy 归一化后的 depth、overflow 和 freshness intent。
 * 它不暴露 zenoh-cpp API；generated shell 只通过该配置和 `ZenohPubSub<T>` 绑定 transport。
 */
class ZenohChannelConfig {
   public:
    /**
     * @brief channel buffering kind。
     */
    enum class Kind : std::uint8_t {
        Latest = 0,
        Fifo = 1,
    };

    /**
     * @brief 构造 latest channel 的默认配置。
     *
     * @return depth 为 1、overflow 为 DropOldest 的配置。
     */
    static constexpr ZenohChannelConfig latest() noexcept { return ZenohChannelConfig{}; }

    /**
     * @brief 构造 FIFO channel 配置。
     *
     * @param depth 队列深度；传入 0 时按 1 处理。
     * @param overflow 队列满时的 FlowRT 语义。
     * @return 归一化后的配置。
     */
    static constexpr ZenohChannelConfig fifo(std::size_t depth, OverflowPolicy overflow) noexcept {
        return ZenohChannelConfig(Kind::Fifo, depth == 0 ? 1 : depth, overflow, StaleConfig{});
    }

    /**
     * @brief 设置 freshness 配置。
     *
     * @param stale stale-data policy 和时间窗口。
     * @return 更新后的配置副本。
     */
    constexpr ZenohChannelConfig with_stale_config(StaleConfig stale) const noexcept {
        return ZenohChannelConfig(kind_, depth_, overflow_, stale);
    }

    /**
     * @brief 返回归一化后的 channel depth。
     */
    constexpr std::size_t depth() const noexcept { return depth_; }

    /**
     * @brief 返回 overflow policy。
     */
    constexpr OverflowPolicy overflow() const noexcept { return overflow_; }

    /**
     * @brief 返回 stale-data 配置。
     */
    constexpr StaleConfig stale() const noexcept { return stale_; }

    /**
     * @brief 判断是否为 latest channel。
     */
    constexpr bool is_latest() const noexcept { return kind_ == Kind::Latest; }

   private:
    constexpr ZenohChannelConfig() noexcept = default;

    constexpr ZenohChannelConfig(Kind kind, std::size_t depth, OverflowPolicy overflow,
                                 StaleConfig stale) noexcept
        : kind_(kind), depth_(depth), overflow_(overflow), stale_(stale) {}

    Kind kind_ = Kind::Latest;
    std::size_t depth_ = 1;
    OverflowPolicy overflow_ = OverflowPolicy::DropOldest;
    StaleConfig stale_;
};

/**
 * @brief canonical wire message 的 zenoh publish-subscribe endpoint。
 *
 * @tparam T 满足 `CanonicalTransportMessage` 的 generated message 类型。
 *
 * 开启 `FLOWRT_HAS_ZENOH_CXX` 时，该类绑定 zenoh-cpp 1.9 publish-subscribe endpoint，并把
 * runtime timestamp 与 generated message canonical payload 组合成 wire frame。默认构建不包含或
 * 依赖 zenoh-cpp，并保持安全失败语义。业务组件接口不应暴露该类型。
 */
template <CanonicalTransportMessage T>
class ZenohPubSub {
   public:
    ZenohPubSub(ZenohPubSub &&) noexcept = default;
    ZenohPubSub(const ZenohPubSub &) = delete;
    auto operator=(ZenohPubSub &&) noexcept -> ZenohPubSub & = default;
    auto operator=(const ZenohPubSub &) -> ZenohPubSub & = delete;
    ~ZenohPubSub() = default;

    /**
     * @brief 打开一个 canonical zenoh key expression 对应的 endpoint。
     *
     * @param key_expr generated shell 提供的 canonical key expression。
     * @param config 从 Contract IR channel policy 生成的配置。
     * @return endpoint 对象；未开启 zenoh-cpp 支持时 `ready()` 返回 false。
     */
    static ZenohPubSub open_with_config(std::string_view key_expr, ZenohChannelConfig config) {
        return ZenohPubSub(key_expr, config);
    }

    /**
     * @brief 返回 canonical zenoh key expression。
     */
    std::string_view key_expr() const noexcept { return key_expr_; }

    /**
     * @brief 返回 channel 配置。
     */
    constexpr ZenohChannelConfig config() const noexcept { return config_; }

    /**
     * @brief 判断 endpoint 是否已经绑定到底层 zenoh transport 资源。
     */
    bool ready() const noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        return session_.has_value() && publisher_.has_value() && subscriber_.has_value();
#else
        return false;
#endif
    }

    /**
     * @brief 带 FlowRT runtime 时间戳发布一个 canonical wire message。
     *
     * @param value 要发布的 generated message。
     * @param published_at_ms 样本发布时间，单位为 runtime 毫秒。
     * @return 未开启 zenoh-cpp 支持时返回 `ChannelError::Transport`。
     */
    ChannelPushResult publish_at(T value, std::uint64_t published_at_ms) noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        if (!publisher_) {
            return ChannelError::Transport;
        }

        try {
            std::vector<std::uint8_t> frame(timestamp_wire_size() +
                                            detail::encoded_frame_size(value));
            auto output = std::span<std::uint8_t>{frame};
            write_wire_le(output, 0, published_at_ms);
            detail::encode_frame(value, output.subspan(timestamp_wire_size()));
            publisher_->put(::zenoh::Bytes(std::move(frame)));
            return ChannelWriteOutcome::Accepted;
        } catch (...) {
            return ChannelError::Transport;
        }
#else
        (void)value;
        (void)published_at_ms;
        return ChannelError::Transport;
#endif
    }

    /**
     * @brief 非阻塞读取 latest snapshot。
     *
     * @param now_ms 当前 runtime 时间，单位为毫秒。
     * @return 读取成功时返回 latest view；未开启 zenoh-cpp 支持或 transport/codec 失败时返回
     * `ChannelError::Transport`。
     *
     * zenoh subscriber 使用有界 `RingChannel`，callback 在容量满时覆盖旧样本而不阻塞。latest
     * channel 会排空当前 ring 后暴露最新值；FIFO channel 每次只消费一个最旧可用样本。
     */
    std::variant<Latest<T>, ChannelError> receive_latest_at(std::uint64_t now_ms) noexcept {
#ifdef FLOWRT_HAS_ZENOH_CXX
        if (!subscriber_) {
            return ChannelError::Transport;
        }

        try {
            while (true) {
                auto result = subscriber_->handler().try_recv();
                if (std::holds_alternative<::zenoh::Sample>(result)) {
                    auto sample = std::get<::zenoh::Sample>(std::move(result));
                    if (!decode_frame(sample.get_payload().as_vector())) {
                        return ChannelError::Transport;
                    }
                    if (!config_.is_latest()) {
                        break;
                    }
                    continue;
                }

                const auto error = std::get<::zenoh::channels::RecvError>(result);
                if (error == ::zenoh::channels::RecvError::Z_NODATA) {
                    break;
                }
                return ChannelError::Transport;
            }
        } catch (...) {
            return ChannelError::Transport;
        }

        const bool stale = config_.stale().stale_at(published_at_ms_, now_ms);
        const bool drop_stale = stale && config_.stale().policy() == StalePolicy::Drop;
        return Latest<T>{received_ && !drop_stale ? std::addressof(*received_) : nullptr, stale};
#else
        (void)now_ms;
        return ChannelError::Transport;
#endif
    }

   private:
    ZenohPubSub(std::string_view key_expr, ZenohChannelConfig config)
        : key_expr_(key_expr), config_(config) {
#ifdef FLOWRT_HAS_ZENOH_CXX
        open_zenoh_endpoint();
#endif
    }

#ifdef FLOWRT_HAS_ZENOH_CXX
    using ZenohSubscriber =
        ::zenoh::Subscriber<::zenoh::channels::RingChannel::HandlerType<::zenoh::Sample>>;

    static constexpr std::size_t timestamp_wire_size() noexcept { return sizeof(std::uint64_t); }

    void open_zenoh_endpoint() noexcept {
        if (config_.overflow() != OverflowPolicy::DropOldest) {
            return;
        }

        try {
            session_.emplace(::zenoh::Session::open(config_from_environment()));
            publisher_.emplace(session_->declare_publisher(::zenoh::KeyExpr(key_expr_)));
            subscriber_.emplace(session_->declare_subscriber(
                ::zenoh::KeyExpr(key_expr_), ::zenoh::channels::RingChannel(config_.depth())));
        } catch (...) {
            subscriber_.reset();
            publisher_.reset();
            session_.reset();
        }
    }

    bool decode_frame(const std::vector<std::uint8_t> &frame) {
        if (frame.size() < timestamp_wire_size()) {
            return false;
        }

        const auto input = std::span<const std::uint8_t>{frame};
        const auto published_at_ms = read_wire_le<std::uint64_t>(input, 0);
        auto decoded = detail::decode_frame<T>(input.subspan(timestamp_wire_size()));
        published_at_ms_ = published_at_ms;
        received_ = std::move(decoded);
        return true;
    }

    static ::zenoh::Config config_from_environment() {
        auto config = ::zenoh::Config::create_default();
        if (const auto *mode = std::getenv("FLOWRT_ZENOH_MODE")) {
            config.insert_json5(Z_CONFIG_MODE_KEY, json_string(std::string_view{mode}));
        }
        if (const auto *listen = std::getenv("FLOWRT_ZENOH_LISTEN")) {
            if (const auto json = endpoint_list_json(std::string_view{listen}); !json.empty()) {
                config.insert_json5(Z_CONFIG_LISTEN_KEY, json);
            }
        }
        if (const auto *connect = std::getenv("FLOWRT_ZENOH_CONNECT")) {
            if (const auto json = endpoint_list_json(std::string_view{connect}); !json.empty()) {
                config.insert_json5(Z_CONFIG_CONNECT_KEY, json);
            }
        }
        if (const auto *no_multicast = std::getenv("FLOWRT_ZENOH_NO_MULTICAST");
            env_flag_enabled(no_multicast)) {
            config.insert_json5(Z_CONFIG_MULTICAST_SCOUTING_KEY, "false");
        }
        return config;
    }

    static bool env_flag_enabled(const char *value) noexcept {
        if (value == nullptr) {
            return false;
        }
        const auto flag = std::string_view{value};
        return flag == "1" || flag == "true" || flag == "TRUE" || flag == "yes" ||
               flag == "on";
    }

    static std::string endpoint_list_json(std::string_view raw) {
        std::vector<std::string_view> endpoints;
        std::size_t start = 0;
        while (start <= raw.size()) {
            const auto comma = raw.find(',', start);
            const auto end = comma == std::string_view::npos ? raw.size() : comma;
            auto item = raw.substr(start, end - start);
            while (!item.empty() && (item.front() == ' ' || item.front() == '\t')) {
                item.remove_prefix(1);
            }
            while (!item.empty() && (item.back() == ' ' || item.back() == '\t')) {
                item.remove_suffix(1);
            }
            if (!item.empty()) {
                endpoints.push_back(item);
            }
            if (comma == std::string_view::npos) {
                break;
            }
            start = comma + 1;
        }

        if (endpoints.empty()) {
            return {};
        }

        std::string json = "[";
        for (std::size_t index = 0; index < endpoints.size(); ++index) {
            if (index != 0U) {
                json += ",";
            }
            json += json_string(endpoints[index]);
        }
        json += "]";
        return json;
    }

    static std::string json_string(std::string_view value) {
        std::string output = "\"";
        for (const char ch : value) {
            switch (ch) {
                case '\\':
                    output += "\\\\";
                    break;
                case '"':
                    output += "\\\"";
                    break;
                case '\n':
                    output += "\\n";
                    break;
                case '\r':
                    output += "\\r";
                    break;
                case '\t':
                    output += "\\t";
                    break;
                default:
                    output += ch;
                    break;
            }
        }
        output += "\"";
        return output;
    }
#endif

    std::string key_expr_;
    ZenohChannelConfig config_;
#ifdef FLOWRT_HAS_ZENOH_CXX
    std::optional<::zenoh::Session> session_;
    std::optional<::zenoh::Publisher> publisher_;
    std::optional<ZenohSubscriber> subscriber_;
    std::optional<T> received_;
    std::optional<std::uint64_t> published_at_ms_;
#endif
};

}  // namespace zenoh

/// 当前 runtime introspection JSON-line 协议版本。
inline constexpr const char *INTROSPECTION_PROTOCOL_VERSION = "0.1";

/**
 * @brief CLI 连接 socket 后首先验证的进程身份。
 */
struct IntrospectionHandshake {
    std::string protocol_version;
    std::uint32_t pid = 0;
    std::uint64_t started_at_unix_ms = 0;
    std::string self_description_hash;
    std::string package;
    std::string process;
    std::string runtime;
};

/**
 * @brief 单个 channel 的运行态摘要。
 */
struct IntrospectionChannelStatus {
    std::string name;
    std::string message_type;
    std::uint64_t published_count = 0;
    std::optional<std::size_t> last_payload_len;
};

/**
 * @brief 单个 channel 的 latest raw ABI snapshot。
 */
struct IntrospectionChannelSnapshot {
    std::uint64_t published_count = 0;
    std::optional<std::vector<std::uint8_t>> payload;
    std::optional<std::uint64_t> published_at_ms;
};

/**
 * @brief 运行态 status 快照。
 */
struct IntrospectionStatus {
    std::uint64_t tick_count = 0;
    std::vector<IntrospectionChannelStatus> channels;
};

namespace detail {

inline std::uint64_t unix_time_ms() {
    const auto now = std::chrono::system_clock::now().time_since_epoch();
    const auto millis = std::chrono::duration_cast<std::chrono::milliseconds>(now).count();
    return millis < 0 ? 0U : static_cast<std::uint64_t>(millis);
}

inline std::string json_string(std::string_view value) {
    static constexpr char kHex[] = "0123456789abcdef";
    std::string output;
    output.reserve(value.size() + 2);
    output.push_back('"');
    for (const unsigned char byte : value) {
        switch (byte) {
            case '"':
                output.append("\\\"");
                break;
            case '\\':
                output.append("\\\\");
                break;
            case '\b':
                output.append("\\b");
                break;
            case '\f':
                output.append("\\f");
                break;
            case '\n':
                output.append("\\n");
                break;
            case '\r':
                output.append("\\r");
                break;
            case '\t':
                output.append("\\t");
                break;
            default:
                if (byte < 0x20U) {
                    output.append("\\u00");
                    output.push_back(kHex[(byte >> 4U) & 0x0FU]);
                    output.push_back(kHex[byte & 0x0FU]);
                } else {
                    output.push_back(static_cast<char>(byte));
                }
                break;
        }
    }
    output.push_back('"');
    return output;
}

inline std::string handshake_json(const IntrospectionHandshake &handshake) {
    std::string output;
    output.append("{\"protocol_version\":");
    output.append(json_string(handshake.protocol_version));
    output.append(",\"pid\":");
    output.append(std::to_string(handshake.pid));
    output.append(",\"started_at_unix_ms\":");
    output.append(std::to_string(handshake.started_at_unix_ms));
    output.append(",\"self_description_hash\":");
    output.append(json_string(handshake.self_description_hash));
    output.append(",\"package\":");
    output.append(json_string(handshake.package));
    output.append(",\"process\":");
    output.append(json_string(handshake.process));
    output.append(",\"runtime\":");
    output.append(json_string(handshake.runtime));
    output.push_back('}');
    return output;
}

inline std::string channel_status_json(const IntrospectionChannelStatus &channel) {
    std::string output;
    output.append("{\"name\":");
    output.append(json_string(channel.name));
    output.append(",\"message_type\":");
    output.append(json_string(channel.message_type));
    output.append(",\"published_count\":");
    output.append(std::to_string(channel.published_count));
    output.append(",\"last_payload_len\":");
    output.append(channel.last_payload_len ? std::to_string(*channel.last_payload_len) : "null");
    output.push_back('}');
    return output;
}

inline std::string status_json(const IntrospectionStatus &status) {
    std::string output;
    output.append("{\"tick_count\":");
    output.append(std::to_string(status.tick_count));
    output.append(",\"channels\":[");
    for (std::size_t index = 0; index < status.channels.size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(channel_status_json(status.channels[index]));
    }
    output.append("]}");
    return output;
}

inline std::string payload_json(const std::optional<std::vector<std::uint8_t>> &payload) {
    if (!payload) {
        return "null";
    }
    std::string output;
    output.push_back('[');
    for (std::size_t index = 0; index < payload->size(); ++index) {
        if (index != 0) {
            output.push_back(',');
        }
        output.append(std::to_string(static_cast<unsigned int>((*payload)[index])));
    }
    output.push_back(']');
    return output;
}

inline std::string channel_snapshot_json(const IntrospectionChannelSnapshot &channel) {
    std::string output;
    output.append("{\"published_count\":");
    output.append(std::to_string(channel.published_count));
    output.append(",\"payload\":");
    output.append(payload_json(channel.payload));
    output.append(",\"published_at_ms\":");
    output.append(channel.published_at_ms ? std::to_string(*channel.published_at_ms) : "null");
    output.push_back('}');
    return output;
}

inline std::string status_response_json(const IntrospectionHandshake &handshake,
                                        const IntrospectionStatus &status) {
    std::string output;
    output.append("{\"response\":\"status\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"status\":");
    output.append(status_json(status));
    output.push_back('}');
    return output;
}

inline std::string channel_snapshot_response_json(const IntrospectionHandshake &handshake,
                                                  const IntrospectionChannelSnapshot &channel) {
    std::string output;
    output.append("{\"response\":\"channel_snapshot\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"channel\":");
    output.append(channel_snapshot_json(channel));
    output.push_back('}');
    return output;
}

inline std::string error_response_json(const IntrospectionHandshake &handshake,
                                       std::string_view message) {
    std::string output;
    output.append("{\"response\":\"error\",\"handshake\":");
    output.append(handshake_json(handshake));
    output.append(",\"message\":");
    output.append(json_string(message));
    output.push_back('}');
    return output;
}

inline bool json_whitespace(char byte) noexcept {
    return byte == ' ' || byte == '\t' || byte == '\n' || byte == '\r';
}

inline std::optional<std::size_t> find_json_string_value(std::string_view input,
                                                         std::string_view key, std::string &value) {
    const std::string needle = "\"" + std::string(key) + "\"";
    const auto key_pos = input.find(needle);
    if (key_pos == std::string_view::npos) {
        return std::nullopt;
    }
    std::size_t index = key_pos + needle.size();
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size() || input[index] != ':') {
        return std::nullopt;
    }
    ++index;
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size() || input[index] != '"') {
        return std::nullopt;
    }
    ++index;

    value.clear();
    while (index < input.size()) {
        const char byte = input[index++];
        if (byte == '"') {
            return index;
        }
        if (byte != '\\') {
            value.push_back(byte);
            continue;
        }
        if (index >= input.size()) {
            return std::nullopt;
        }
        const char escape = input[index++];
        switch (escape) {
            case '"':
            case '\\':
            case '/':
                value.push_back(escape);
                break;
            case 'b':
                value.push_back('\b');
                break;
            case 'f':
                value.push_back('\f');
                break;
            case 'n':
                value.push_back('\n');
                break;
            case 'r':
                value.push_back('\r');
                break;
            case 't':
                value.push_back('\t');
                break;
            default:
                return std::nullopt;
        }
    }
    return std::nullopt;
}

enum class IntrospectionRequestKind : std::uint8_t {
    Status = 0,
    ChannelSnapshot = 1,
};

struct ParsedIntrospectionRequest {
    IntrospectionRequestKind kind = IntrospectionRequestKind::Status;
    std::string channel;
};

inline std::optional<ParsedIntrospectionRequest> parse_introspection_request(
    std::string_view line) {
    std::string command;
    if (!find_json_string_value(line, "command", command)) {
        return std::nullopt;
    }
    if (command == "status") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::Status, {}};
    }
    if (command == "channel_snapshot") {
        std::string channel;
        if (!find_json_string_value(line, "channel", channel)) {
            return std::nullopt;
        }
        return ParsedIntrospectionRequest{IntrospectionRequestKind::ChannelSnapshot,
                                          std::move(channel)};
    }
    return std::nullopt;
}

inline bool write_all(int fd, std::string_view data) {
#if defined(MSG_NOSIGNAL)
    constexpr int send_flags = MSG_NOSIGNAL;
#else
    constexpr int send_flags = 0;
#endif
    std::size_t offset = 0;
    while (offset < data.size()) {
        const auto written = ::send(fd, data.data() + offset, data.size() - offset, send_flags);
        if (written < 0) {
            if (errno == EINTR) {
                continue;
            }
            return false;
        }
        if (written == 0) {
            return false;
        }
        offset += static_cast<std::size_t>(written);
    }
    return true;
}

inline void set_socket_timeout(int fd) {
    timeval timeout{};
    timeout.tv_sec = 1;
    timeout.tv_usec = 0;
    (void)::setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout));
    (void)::setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout));
}

inline std::optional<std::string> read_line(int fd) {
    std::string line;
    char byte = '\0';
    while (line.size() < 65536U) {
        const auto received = ::read(fd, &byte, 1);
        if (received == 0) {
            break;
        }
        if (received < 0) {
            if (errno == EINTR) {
                continue;
            }
            return std::nullopt;
        }
        if (byte == '\n') {
            return line;
        }
        line.push_back(byte);
    }
    return line.empty() ? std::nullopt : std::optional<std::string>{std::move(line)};
}

}  // namespace detail

/**
 * @brief 生成 handshake 的输入元数据。
 */
struct IntrospectionIdentity {
    std::string self_description_hash;
    std::string package;
    std::string process;
    std::string runtime;

    /**
     * @brief 构造当前进程的 handshake。
     */
    IntrospectionHandshake handshake() const {
        return IntrospectionHandshake{
            .protocol_version = std::string{INTROSPECTION_PROTOCOL_VERSION},
            .pid = static_cast<std::uint32_t>(::getpid()),
            .started_at_unix_ms = detail::unix_time_ms(),
            .self_description_hash = self_description_hash,
            .package = package,
            .process = process,
            .runtime = runtime,
        };
    }
};

/**
 * @brief runtime shell 可共享更新的 introspection live 状态。
 */
class IntrospectionState {
   public:
    /**
     * @brief 构造空 live 状态。
     */
    IntrospectionState() : inner_(std::make_shared<Inner>()) {}

    /**
     * @brief 预注册 channel，使其在尚未发布样本时也出现在 status 中。
     */
    void register_channel(std::string name, std::string message_type) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        inner_->channels.try_emplace(std::move(name), ChannelState{std::move(message_type)});
    }

    /**
     * @brief 增加 scheduler tick 计数。
     */
    void record_tick() const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        if (inner_->tick_count != UINT64_MAX) {
            ++inner_->tick_count;
        }
    }

    /**
     * @brief 记录 channel 发布的 raw ABI bytes。
     */
    void record_channel_publish_bytes(std::string name, std::string message_type,
                                      std::vector<std::uint8_t> payload,
                                      std::optional<std::uint64_t> published_at_ms) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        auto [iterator, _inserted] =
            inner_->channels.try_emplace(std::move(name), ChannelState{message_type});
        auto &channel = iterator->second;
        channel.message_type = std::move(message_type);
        if (channel.published_count != UINT64_MAX) {
            ++channel.published_count;
        }
        channel.payload = std::move(payload);
        channel.published_at_ms = published_at_ms;
    }

    /**
     * @brief 记录 channel 发布的 Message ABI 对象表示。
     */
    template <typename T>
    void record_channel_publish(std::string name, std::string message_type, const T &value,
                                std::optional<std::uint64_t> published_at_ms) const {
        static_assert(std::is_trivially_copyable_v<T>,
                      "FlowRT introspection payload snapshot requires trivially copyable values");
        std::vector<std::uint8_t> payload(sizeof(T));
        if (!payload.empty()) {
            std::memcpy(payload.data(), std::addressof(value), payload.size());
        }
        record_channel_publish_bytes(std::move(name), std::move(message_type), std::move(payload),
                                     published_at_ms);
    }

    /**
     * @brief 返回当前 status 快照。
     */
    IntrospectionStatus status() const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        IntrospectionStatus snapshot;
        snapshot.tick_count = inner_->tick_count;
        snapshot.channels.reserve(inner_->channels.size());
        for (const auto &[name, channel] : inner_->channels) {
            snapshot.channels.push_back(IntrospectionChannelStatus{
                .name = name,
                .message_type = channel.message_type,
                .published_count = channel.published_count,
                .last_payload_len = channel.payload
                                        ? std::optional<std::size_t>{channel.payload->size()}
                                        : std::nullopt,
            });
        }
        return snapshot;
    }

    /**
     * @brief 返回指定 channel 的 raw ABI snapshot。
     */
    std::optional<IntrospectionChannelSnapshot> channel_snapshot(std::string_view name) const {
        std::lock_guard<std::mutex> lock(inner_->mutex);
        const auto channel = inner_->channels.find(std::string{name});
        if (channel == inner_->channels.end()) {
            return std::nullopt;
        }
        return IntrospectionChannelSnapshot{
            .published_count = channel->second.published_count,
            .payload = channel->second.payload,
            .published_at_ms = channel->second.published_at_ms,
        };
    }

   private:
    struct ChannelState {
        std::string message_type;
        std::uint64_t published_count = 0;
        std::optional<std::vector<std::uint8_t>> payload;
        std::optional<std::uint64_t> published_at_ms;
    };

    struct Inner {
        std::mutex mutex;
        std::uint64_t tick_count = 0;
        std::map<std::string, ChannelState> channels;
    };

    std::shared_ptr<Inner> inner_;
};

/**
 * @brief 返回当前用户 runtime socket 目录。
 *
 * 优先使用 `$XDG_RUNTIME_DIR/flowrt`；没有时 fallback 到 `/tmp/flowrt.<uid>`，避免不同用户
 * 的同名 PID socket 互相污染。
 */
inline std::filesystem::path runtime_socket_dir() {
    if (const char *runtime_dir = std::getenv("XDG_RUNTIME_DIR"); runtime_dir != nullptr) {
        return std::filesystem::path(runtime_dir) / "flowrt";
    }
    return std::filesystem::path("/tmp") /
           ("flowrt." + std::to_string(static_cast<unsigned int>(::getuid())));
}

/**
 * @brief 返回指定 PID 的默认 runtime socket 路径。
 */
inline std::filesystem::path runtime_socket_path_for_pid(std::uint32_t pid) {
    return runtime_socket_dir() / (std::to_string(pid) + ".sock");
}

class IntrospectionServer;

namespace detail {

inline void handle_introspection_connection(int client_fd, const IntrospectionHandshake &handshake,
                                            const IntrospectionState &state) {
    const auto line = read_line(client_fd);
    std::string response;
    if (!line) {
        response = error_response_json(handshake, "invalid FlowRT introspection request");
    } else if (const auto request = parse_introspection_request(*line)) {
        switch (request->kind) {
            case IntrospectionRequestKind::Status:
                response = status_response_json(handshake, state.status());
                break;
            case IntrospectionRequestKind::ChannelSnapshot: {
                const auto channel = state.channel_snapshot(request->channel);
                response = channel ? channel_snapshot_response_json(handshake, *channel)
                                   : error_response_json(handshake, "unknown FlowRT channel");
                break;
            }
        }
    } else {
        response = error_response_json(handshake, "invalid FlowRT introspection request");
    }
    response.push_back('\n');
    (void)write_all(client_fd, response);
}

}  // namespace detail

/**
 * @brief 已启动的 introspection 服务。
 *
 * 该对象拥有 Unix socket listener 线程，并在析构时停止 listener、删除 socket 文件。
 */
class IntrospectionServer {
   public:
    IntrospectionServer() = default;
    IntrospectionServer(const IntrospectionServer &) = delete;
    auto operator=(const IntrospectionServer &) -> IntrospectionServer & = delete;

    IntrospectionServer(IntrospectionServer &&other) noexcept
        : path_(std::move(other.path_)),
          handle_(std::move(other.handle_)),
          stop_(std::move(other.stop_)) {
        other.path_.clear();
    }

    auto operator=(IntrospectionServer &&other) noexcept -> IntrospectionServer & {
        if (this != std::addressof(other)) {
            stop();
            path_ = std::move(other.path_);
            handle_ = std::move(other.handle_);
            stop_ = std::move(other.stop_);
            other.path_.clear();
        }
        return *this;
    }

    ~IntrospectionServer() { stop(); }

    /**
     * @brief 返回服务 socket 路径。
     */
    const std::filesystem::path &path() const noexcept { return path_; }

   private:
    friend std::optional<IntrospectionServer> spawn_status_server_at(
        std::filesystem::path path, IntrospectionHandshake handshake, IntrospectionState state);

    IntrospectionServer(std::filesystem::path path, std::thread handle,
                        std::shared_ptr<std::atomic_bool> stop)
        : path_(std::move(path)), handle_(std::move(handle)), stop_(std::move(stop)) {}

    void stop() noexcept {
        if (stop_) {
            stop_->store(true, std::memory_order_relaxed);
        }
        if (!path_.empty()) {
            std::error_code ignored;
            std::filesystem::remove(path_, ignored);
        }
        if (handle_.joinable()) {
            handle_.join();
        }
        stop_.reset();
        path_.clear();
    }

    std::filesystem::path path_;
    std::thread handle_;
    std::shared_ptr<std::atomic_bool> stop_;
};

/**
 * @brief 在指定路径启动最小 introspection status 服务，主要用于测试和后续 generated shell 接入。
 */
inline std::optional<IntrospectionServer> spawn_status_server_at(std::filesystem::path path,
                                                                 IntrospectionHandshake handshake,
                                                                 IntrospectionState state) {
    std::error_code filesystem_error;
    if (const auto parent = path.parent_path(); !parent.empty()) {
        std::filesystem::create_directories(parent, filesystem_error);
        if (filesystem_error) {
            return std::nullopt;
        }
    }
    std::filesystem::remove(path, filesystem_error);

    const int listener_fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
    if (listener_fd < 0) {
        return std::nullopt;
    }

    auto close_listener = [listener_fd]() { ::close(listener_fd); };
    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    const auto path_string = path.string();
    if (path_string.size() >= sizeof(address.sun_path)) {
        close_listener();
        return std::nullopt;
    }
    std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", path_string.c_str());

    if (::bind(listener_fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) != 0) {
        close_listener();
        return std::nullopt;
    }
    if (::listen(listener_fd, 16) != 0) {
        close_listener();
        std::filesystem::remove(path, filesystem_error);
        return std::nullopt;
    }
    const int flags = ::fcntl(listener_fd, F_GETFL, 0);
    if (flags < 0 || ::fcntl(listener_fd, F_SETFL, flags | O_NONBLOCK) != 0) {
        close_listener();
        std::filesystem::remove(path, filesystem_error);
        return std::nullopt;
    }

    auto stop = std::make_shared<std::atomic_bool>(false);
    auto thread_stop = stop;
    std::thread handle;
    try {
        handle = std::thread([listener_fd, thread_stop, handshake = std::move(handshake),
                              state = std::move(state)]() mutable {
            while (!thread_stop->load(std::memory_order_relaxed)) {
                const int client_fd = ::accept(listener_fd, nullptr, nullptr);
                if (client_fd >= 0) {
                    detail::set_socket_timeout(client_fd);
                    detail::handle_introspection_connection(client_fd, handshake, state);
                    ::close(client_fd);
                    continue;
                }
                if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) {
                    std::this_thread::sleep_for(std::chrono::milliseconds{10});
                    continue;
                }
                break;
            }
            ::close(listener_fd);
        });
    } catch (...) {
        close_listener();
        std::filesystem::remove(path, filesystem_error);
        return std::nullopt;
    }

    return IntrospectionServer{std::move(path), std::move(handle), std::move(stop)};
}

/**
 * @brief 用当前进程 PID 命名 socket 并启动最小 introspection status 服务。
 */
inline std::optional<IntrospectionServer> spawn_status_server(IntrospectionIdentity identity,
                                                              IntrospectionState state) {
    auto handshake = identity.handshake();
    auto path = runtime_socket_path_for_pid(handshake.pid);
    return spawn_status_server_at(std::move(path), std::move(handshake), std::move(state));
}

/**
 * @brief backend capability 的只读视图。
 *
 * capability 字符串来自 Contract IR/backend contract，例如 `channel:latest` 或
 * `topology:multi_process`。validator 使用同一套 capability 语义判断部署是否可满足。
 */
class BackendCapabilities {
   public:
    /**
     * @brief 从静态 capability 列表构造视图。
     *
     * @param items capability 字符串切片；调用方必须保证其生命周期覆盖本对象。
     */
    constexpr explicit BackendCapabilities(std::span<const std::string_view> items) noexcept
        : items_(items) {}

    /**
     * @brief 查询 backend 是否声明某项能力。
     *
     * @param capability capability 字符串。
     * @return 存在时返回 true。
     */
    bool contains(std::string_view capability) const noexcept {
        return std::find(items_.begin(), items_.end(), capability) != items_.end();
    }

    /**
     * @brief 返回完整 capability 列表。
     *
     * @return capability 字符串切片。
     */
    std::span<const std::string_view> items() const noexcept { return items_; }

   private:
    std::span<const std::string_view> items_;
};

/**
 * @brief runtime 当前认识的 backend 类型。
 */
enum class BackendKind : std::uint8_t {
    Inproc = 0,  ///< 单进程内存 backend，主要用于测试、CI 和最小 demo。
    Iox2 = 1,    ///< iceoryx2 backend，用于本机多进程高性能 dataflow。
    Zenoh = 2,   ///< zenoh backend，用于跨主机 copy transport dataflow。
};

/**
 * @brief 调度器抽象边界。
 *
 * 调度器负责驱动 generated runtime shell 的 tick，不负责用户算法逻辑。v0.1 使用同步 tick
 * 接口表达最小语义，后续可以在不改变组件接口的前提下替换为更完整的实时调度实现。
 */
class Scheduler {
   public:
    /**
     * @brief 单次调度步骤函数。
     *
     * 第一个参数是 tick 序号，第二个参数是本轮共享 runtime context。
     */
    using StepFn = std::function<Status(std::size_t, Context &)>;

    virtual ~Scheduler() = default;

    /**
     * @brief 连续运行固定数量的 tick。
     *
     * @param ticks 要运行的 tick 数量。
     * @param step 每个 tick 调用一次的步骤函数。
     * @return 全部 tick 成功时返回 `Status::Ok`；否则返回第一个非 OK 状态。
     */
    virtual Status run_ticks(std::size_t ticks, StepFn step) const = 0;
};

/**
 * @brief runtime backend 抽象边界。
 *
 * Backend 暴露能力集合和调度器，用于 generated shell 在不依赖具体通信库 API 的情况下绑定运行时。
 */
class Backend {
   public:
    virtual ~Backend() = default;

    /**
     * @brief 返回 backend 类型。
     */
    virtual BackendKind kind() const noexcept = 0;

    /**
     * @brief 返回 backend capability 视图。
     */
    virtual BackendCapabilities capabilities() const noexcept = 0;

    /**
     * @brief 返回 backend 提供的调度器。
     */
    virtual const Scheduler &scheduler() const noexcept = 0;
};

/**
 * @brief 单进程同步调度器。
 *
 * 该调度器按 tick 顺序直接调用步骤函数。它用于 v0.1 的 inproc demo 和测试，不承诺实时线程、
 * 优先级继承或跨进程同步。
 */
class InprocScheduler final : public Scheduler {
   public:
    /**
     * @copydoc Scheduler::run_ticks
     */
    Status run_ticks(std::size_t ticks, StepFn step) const override {
        Context context;
        const auto tick_sleep = configured_tick_sleep();
        for (std::size_t tick = 0; tick < ticks; ++tick) {
            const auto status = step(tick, context);
            if (status != Status::Ok) {
                return status;
            }
            if (tick_sleep.count() > 0) {
                std::this_thread::sleep_for(tick_sleep);
            }
        }
        return Status::Ok;
    }

   private:
    static std::chrono::milliseconds configured_tick_sleep() noexcept {
        const auto *raw = std::getenv("FLOWRT_TICK_SLEEP_MS");
        if (raw == nullptr) {
            return std::chrono::milliseconds{0};
        }
        char *end = nullptr;
        errno = 0;
        const auto value = std::strtoull(raw, &end, 10);
        if (errno != 0 || end == raw || *end != '\0' || value == 0U) {
            return std::chrono::milliseconds{0};
        }
        return std::chrono::milliseconds{static_cast<std::chrono::milliseconds::rep>(value)};
    }
};

/**
 * @brief 单进程 backend 实现。
 *
 * InprocBackend 使用进程内 channel 和同步调度器，适合测试、CI 和最小端到端 demo。
 */
class InprocBackend final : public Backend {
   public:
    /**
     * @copydoc Backend::kind
     */
    BackendKind kind() const noexcept override { return BackendKind::Inproc; }

    /**
     * @copydoc Backend::capabilities
     */
    BackendCapabilities capabilities() const noexcept override {
        return BackendCapabilities{std::span<const std::string_view>(kCapabilities)};
    }

    /**
     * @copydoc Backend::scheduler
     */
    const Scheduler &scheduler() const noexcept override { return scheduler_; }

   private:
    static inline constexpr std::array<std::string_view, 22> kCapabilities = {
        "abi:fixed_size_plain_data",
        "layout:native_layout",
        "allocation:bounded",
        "graph:static_graph",
        "trigger:periodic",
        "trigger:on_message",
        "trigger:startup",
        "trigger:shutdown",
        "timing:deadline_aware",
        "channel:latest",
        "channel:fifo",
        "overflow:drop_oldest",
        "overflow:drop_newest",
        "overflow:error",
        "overflow:block",
        "stale:warn",
        "stale:drop",
        "stale:hold_last",
        "stale:error",
        "topology:single_process",
        "transfer:copy",
        "observability:health",
    };

    InprocScheduler scheduler_;
};

/**
 * @brief iceoryx2 backend 的 C++ capability 骨架。
 *
 * 该 backend 报告 iox2 capability，并继续复用同步调度器驱动 generated shell。具体 channel
 * transport 由 `flowrt::iox2::Iox2PubSub<T>` 在 shell 内部绑定；业务组件仍只应依赖 FlowRT
 * runtime API，不直接依赖 iox2 publisher/subscriber。
 */
class Iox2Backend final : public Backend {
   public:
    /**
     * @copydoc Backend::kind
     */
    BackendKind kind() const noexcept override { return BackendKind::Iox2; }

    /**
     * @copydoc Backend::capabilities
     */
    BackendCapabilities capabilities() const noexcept override {
        return BackendCapabilities{std::span<const std::string_view>(kCapabilities)};
    }

    /**
     * @copydoc Backend::scheduler
     */
    const Scheduler &scheduler() const noexcept override { return scheduler_; }

   private:
    static inline constexpr std::array<std::string_view, 24> kCapabilities = {
        "abi:fixed_size_plain_data",
        "layout:native_layout",
        "allocation:bounded",
        "graph:static_graph",
        "trigger:periodic",
        "trigger:on_message",
        "trigger:startup",
        "trigger:shutdown",
        "timing:deadline_aware",
        "channel:latest",
        "channel:fifo",
        "overflow:drop_oldest",
        "overflow:drop_newest",
        "overflow:error",
        "overflow:block",
        "stale:warn",
        "stale:drop",
        "stale:hold_last",
        "stale:error",
        "topology:multi_process",
        "topology:single_host",
        "transfer:zero_copy",
        "transfer:loaned",
        "observability:health",
    };

    InprocScheduler scheduler_;
};

/**
 * @brief zenoh backend 的 C++ capability 骨架。
 *
 * 该 backend 报告跨主机 copy transport capability，并继续复用同步调度器驱动 generated shell。
 * 具体 channel transport 由后续 `flowrt::zenoh` endpoint 在 shell 内部绑定；业务组件仍只应依赖
 * FlowRT runtime API，不直接依赖 zenoh publisher/subscriber。
 */
class ZenohBackend final : public Backend {
   public:
    /**
     * @copydoc Backend::kind
     */
    BackendKind kind() const noexcept override { return BackendKind::Zenoh; }

    /**
     * @copydoc Backend::capabilities
     */
    BackendCapabilities capabilities() const noexcept override {
        return BackendCapabilities{std::span<const std::string_view>(kCapabilities)};
    }

    /**
     * @copydoc Backend::scheduler
     */
    const Scheduler &scheduler() const noexcept override { return scheduler_; }

   private:
    static inline constexpr std::array<std::string_view, 20> kCapabilities = {
        "abi:fixed_size_plain_data",
        "layout:native_layout",
        "allocation:bounded",
        "graph:static_graph",
        "trigger:periodic",
        "trigger:on_message",
        "trigger:startup",
        "trigger:shutdown",
        "timing:deadline_aware",
        "channel:latest",
        "channel:fifo",
        "overflow:drop_oldest",
        "stale:warn",
        "stale:drop",
        "stale:hold_last",
        "stale:error",
        "topology:multi_process",
        "topology:multi_host",
        "transfer:copy",
        "observability:health",
    };

    InprocScheduler scheduler_;
};

/**
 * @brief 构造默认 inproc backend。
 *
 * @return 可直接传给 generated shell 的单进程 backend。
 */
inline InprocBackend inproc_backend() { return InprocBackend{}; }

/**
 * @brief 构造 iox2 backend capability 骨架。
 *
 * @return 可用于 capability 选择和后续 iox2 shell 绑定的 backend 对象。
 */
inline Iox2Backend iox2_backend() { return Iox2Backend{}; }

/**
 * @brief 构造 zenoh backend capability 骨架。
 *
 * @return 可用于 capability 选择和后续 zenoh shell 绑定的 backend 对象。
 */
inline ZenohBackend zenoh_backend() { return ZenohBackend{}; }

}  // namespace flowrt
