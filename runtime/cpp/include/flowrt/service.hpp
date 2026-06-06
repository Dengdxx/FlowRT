#pragma once

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <flowrt/wire.hpp>
#include <limits>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace flowrt {

/**
 * @brief Service 错误分类。
 *
 * 错误码数值稳定，跨 Rust/C++/C ABI 保持一致。该枚举独立于 `Status`（调度状态）
 * 和 `ChannelError`（通道错误），专门描述 request/response 语义中的失败原因。
 *
 * ABI 编码使用 u16 固定宽度整数，不依赖 C++ enum 的实现相关大小。
 */
enum class ServiceError : std::uint16_t {
    Ok = 0,                ///< 请求成功完成。
    Timeout = 1,           ///< 请求超时。
    Unavailable = 2,       ///< 目标服务不可用。
    Busy = 3,              ///< 服务繁忙，请求被限流或排队溢出。
    Rejected = 4,          ///< 请求被服务端主动拒绝。
    Cancelled = 5,         ///< 请求被取消。
    DeadlineExceeded = 6,  ///< 截止时间已过。
    Protocol = 7,          ///< 协议层错误（magic/version 不匹配、帧格式非法等）。
    Backend = 8,           ///< 后端传输错误。
    WouldDeadlock = 9,     ///< 执行会导致死锁。
    HandlerError = 10,     ///< 用户 handler 自身返回的业务错误。
};

/**
 * @brief 从 ABI u16 值解析错误码，未知值返回 nullopt。
 */
constexpr std::optional<ServiceError> service_error_from_abi(std::uint16_t value) noexcept {
    if (value <= 10U) {
        return static_cast<ServiceError>(value);
    }
    return std::nullopt;
}

/**
 * @brief 判断错误码是否表示成功。
 */
constexpr bool is_ok(ServiceError error) noexcept { return error == ServiceError::Ok; }

inline std::string_view to_string(ServiceError error) noexcept {
    switch (error) {
        case ServiceError::Ok:
            return "Ok";
        case ServiceError::Timeout:
            return "Timeout";
        case ServiceError::Unavailable:
            return "Unavailable";
        case ServiceError::Busy:
            return "Busy";
        case ServiceError::Rejected:
            return "Rejected";
        case ServiceError::Cancelled:
            return "Cancelled";
        case ServiceError::DeadlineExceeded:
            return "DeadlineExceeded";
        case ServiceError::Protocol:
            return "Protocol";
        case ServiceError::Backend:
            return "Backend";
        case ServiceError::WouldDeadlock:
            return "WouldDeadlock";
        case ServiceError::HandlerError:
            return "HandlerError";
    }
    return "Unknown";
}

/**
 * @brief Service request/response 结果类型。
 *
 * `ServiceResult<T>` 携带 `ServiceError` 错误码和可选的成功值。它不能把 runtime/service
 * 错误塞进普通 `Status`，是 Service 语义的专用返回类型。
 */
template <typename T>
class ServiceResult {
   public:
    /// 构造成功结果。
    static ServiceResult ok(T value) {
        ServiceResult result;
        result.value_ = std::move(value);
        result.error_ = ServiceError::Ok;
        return result;
    }

    /// 构造失败结果（无消息）。
    static ServiceResult err(ServiceError code) {
        ServiceResult result;
        result.error_ = code;
        return result;
    }

    /// 构造失败结果（带消息）。
    static ServiceResult err_with_message(ServiceError code, std::string message) {
        ServiceResult result;
        result.error_ = code;
        result.error_message_ = std::move(message);
        return result;
    }

    /// 判断是否成功。
    constexpr bool is_ok() const noexcept { return error_ == ServiceError::Ok; }

    /// 判断是否失败。
    constexpr bool is_err() const noexcept { return error_ != ServiceError::Ok; }

    /// 获取错误码，成功时返回 ServiceError::Ok。
    constexpr ServiceError error_code() const noexcept { return error_; }

    /// 借用错误消息。
    const std::optional<std::string> &error_message() const noexcept { return error_message_; }

    /// 借用成功值，失败时返回 nullptr。
    const T *value() const noexcept {
        if (is_ok()) {
            return &value_;
        }
        return nullptr;
    }

    /// 取走成功值，失败时返回 nullopt。
    std::optional<T> take_value() && {
        if (is_ok()) {
            return std::move(value_);
        }
        return std::nullopt;
    }

   private:
    ServiceResult() = default;

    T value_{};
    ServiceError error_ = ServiceError::Ok;
    std::optional<std::string> error_message_;
};

/**
 * @brief FNV-1a 64-bit hash，用于从 canonical service name 生成 service_id。
 *
 * 该算法选择理由：简单、无依赖、确定性、跨语言易实现。不用于安全场景。
 */
inline std::uint64_t fnv1a64(std::string_view data) noexcept {
    std::uint64_t hash = 0xcbf29ce484222325ULL;
    for (const auto byte : data) {
        hash ^= static_cast<std::uint64_t>(static_cast<std::uint8_t>(byte));
        hash *= 0x100000001b3ULL;
    }
    return hash;
}

/// service frame 魔数常量，ASCII "FRVS" little-endian。
inline constexpr std::uint32_t SERVICE_FRAME_MAGIC = 0x53525646U;
/// service frame 协议版本。
inline constexpr std::uint16_t SERVICE_FRAME_VERSION = 1U;
/// service frame 固定 header 字节数。
inline constexpr std::size_t SERVICE_FRAME_HEADER_SIZE = 80U;

/**
 * @brief Service 请求标识。
 *
 * 三元组唯一标识一个 service request：client session + sequence + service。
 * service_id 使用 canonical service name 的 FNV-1a 64-bit hash。
 */
struct RequestId {
    std::uint64_t session_id = 0;  ///< 发起请求的 client session 标识。
    std::uint64_t sequence = 0;    ///< 该 session 内的单调递增序号。
    std::uint64_t service_id = 0;  ///< 目标 service 的 canonical name hash。
};

/**
 * @brief Service 请求/响应截止时间。
 *
 * 使用单调时钟毫秒表示。不允许无界等待：timeout_ms 为 0 表示非法值。
 */
struct Deadline {
    std::uint64_t timeout_ms = 0;            ///< 相对超时毫秒数。0 为非法。
    std::uint64_t absolute_deadline_ms = 0;  ///< 绝对截止时间（单调时钟毫秒）。

    /**
     * @brief 构造截止时间。
     *
     * timeout_ms 必须大于 0。now_monotonic_ms 为当前单调时钟毫秒。
     * @return 成功返回 Deadline，timeout_ms 为 0 时返回 nullopt。
     */
    static std::optional<Deadline> make(std::uint64_t timeout_ms,
                                        std::uint64_t now_monotonic_ms) noexcept {
        if (timeout_ms == 0U) {
            return std::nullopt;
        }
        const auto max = std::numeric_limits<std::uint64_t>::max();
        const auto absolute =
            now_monotonic_ms > max - timeout_ms ? max : now_monotonic_ms + timeout_ms;
        return Deadline{timeout_ms, absolute};
    }

    /** @brief 判断是否已过期。 */
    bool expired(std::uint64_t now_monotonic_ms) const noexcept {
        return now_monotonic_ms >= absolute_deadline_ms;
    }
};

/**
 * @brief canonical service frame header，固定 80 字节。
 *
 * 布局（所有字段 little-endian）：
 * ```text
 * offset  size  field
 * 0       4     magic (u32, 0x53525646)
 * 4       2     version (u16, 当前 1)
 * 6       2     error_code (u16, ServiceError ABI 编码)
 * 8       8     service_id (u64, FNV-1a hash of canonical name)
 * 16      8     session_id (u64, client session)
 * 24      8     sequence (u64, monotonic sequence)
 * 32      8     correlation_id (u64, 跨系统关联，0 表示无)
 * 40      8     timeout_ms (u64, 相对超时)
 * 48      8     absolute_deadline_ms (u64, 单调时钟绝对截止)
 * 56      8     schema_hash (u64, 预留，0 表示未使用)
 * 64      8     payload_span (VarSpan, tail 中的 payload 位置)
 * 72      8     error_msg_span (VarSpan, tail 中的错误消息位置)
 * ```
 *
 * 变长字段（payload、error message）存储在 tail 中，通过 header 中的 VarSpan 描述符寻址。
 */
struct ServiceFrameHeader {
    std::uint32_t magic = SERVICE_FRAME_MAGIC;
    std::uint16_t version = SERVICE_FRAME_VERSION;
    std::uint16_t error_code = 0U;
    std::uint64_t service_id = 0;
    std::uint64_t session_id = 0;
    std::uint64_t sequence = 0;
    std::uint64_t correlation_id = 0;
    std::uint64_t timeout_ms = 0;
    std::uint64_t absolute_deadline_ms = 0;
    std::uint64_t schema_hash = 0;
    VarSpan payload_span{};
    VarSpan error_msg_span{};

    /** @brief 构造请求帧 header。 */
    static ServiceFrameHeader make_request(RequestId request_id, Deadline deadline,
                                           std::uint64_t correlation_id,
                                           std::uint64_t schema_hash) noexcept {
        ServiceFrameHeader h{};
        h.error_code = static_cast<std::uint16_t>(ServiceError::Ok);
        h.service_id = request_id.service_id;
        h.session_id = request_id.session_id;
        h.sequence = request_id.sequence;
        h.correlation_id = correlation_id;
        h.timeout_ms = deadline.timeout_ms;
        h.absolute_deadline_ms = deadline.absolute_deadline_ms;
        h.schema_hash = schema_hash;
        return h;
    }

    /** @brief 构造响应帧 header。 */
    static ServiceFrameHeader make_response(RequestId request_id, Deadline deadline,
                                            std::uint64_t correlation_id, std::uint64_t schema_hash,
                                            ServiceError error_code) noexcept {
        ServiceFrameHeader h{};
        h.error_code = static_cast<std::uint16_t>(error_code);
        h.service_id = request_id.service_id;
        h.session_id = request_id.session_id;
        h.sequence = request_id.sequence;
        h.correlation_id = correlation_id;
        h.timeout_ms = deadline.timeout_ms;
        h.absolute_deadline_ms = deadline.absolute_deadline_ms;
        h.schema_hash = schema_hash;
        return h;
    }

    /** @brief 将 header 编码到 80 字节 buffer。 */
    void encode(std::span<std::uint8_t> output) const {
        ensure_wire_size(SERVICE_FRAME_HEADER_SIZE, output.size());
        write_wire_le(output, 0, magic);
        write_wire_le(output, 4, version);
        write_wire_le(output, 6, error_code);
        write_wire_le(output, 8, service_id);
        write_wire_le(output, 16, session_id);
        write_wire_le(output, 24, sequence);
        write_wire_le(output, 32, correlation_id);
        write_wire_le(output, 40, timeout_ms);
        write_wire_le(output, 48, absolute_deadline_ms);
        write_wire_le(output, 56, schema_hash);
        write_var_span(output.subspan(64, VAR_SPAN_WIRE_SIZE), payload_span);
        write_var_span(output.subspan(72, VAR_SPAN_WIRE_SIZE), error_msg_span);
    }

    /** @brief 从 80 字节 buffer 解码 header。 */
    static ServiceFrameHeader decode(std::span<const std::uint8_t> input) {
        ensure_wire_size(SERVICE_FRAME_HEADER_SIZE, input.size());
        const auto magic_val = read_wire_le<std::uint32_t>(input, 0);
        if (magic_val != SERVICE_FRAME_MAGIC) {
            throw WireCodecError("service frame magic mismatch");
        }
        const auto version_val = read_wire_le<std::uint16_t>(input, 4);
        if (version_val != SERVICE_FRAME_VERSION) {
            throw WireCodecError("service frame version mismatch");
        }
        ServiceFrameHeader h{};
        h.magic = magic_val;
        h.version = version_val;
        h.error_code = read_wire_le<std::uint16_t>(input, 6);
        if (!service_error_from_abi(h.error_code).has_value()) {
            throw WireCodecError("service frame error code is unknown");
        }
        h.service_id = read_wire_le<std::uint64_t>(input, 8);
        h.session_id = read_wire_le<std::uint64_t>(input, 16);
        h.sequence = read_wire_le<std::uint64_t>(input, 24);
        h.correlation_id = read_wire_le<std::uint64_t>(input, 32);
        h.timeout_ms = read_wire_le<std::uint64_t>(input, 40);
        if (h.timeout_ms == 0U) {
            throw WireCodecError("service frame timeout_ms must be greater than zero");
        }
        h.absolute_deadline_ms = read_wire_le<std::uint64_t>(input, 48);
        h.schema_hash = read_wire_le<std::uint64_t>(input, 56);
        h.payload_span = read_var_span(input.subspan(64, VAR_SPAN_WIRE_SIZE));
        h.error_msg_span = read_var_span(input.subspan(72, VAR_SPAN_WIRE_SIZE));
        return h;
    }

    /** @brief 判断两个 header 是否语义等价（忽略 magic/version 默认值差异）。 */
    bool operator==(const ServiceFrameHeader &other) const noexcept {
        return magic == other.magic && version == other.version && error_code == other.error_code &&
               service_id == other.service_id && session_id == other.session_id &&
               sequence == other.sequence && correlation_id == other.correlation_id &&
               timeout_ms == other.timeout_ms &&
               absolute_deadline_ms == other.absolute_deadline_ms &&
               schema_hash == other.schema_hash &&
               payload_span.offset == other.payload_span.offset &&
               payload_span.len == other.payload_span.len &&
               error_msg_span.offset == other.error_msg_span.offset &&
               error_msg_span.len == other.error_msg_span.len;
    }
};

/**
 * @brief 编码完整的 service frame（header + tail）到 byte vector。
 *
 * @param header 帧头。
 * @param payload 请求/响应体字节。
 * @param error_msg 错误消息字节。
 * @return 完整 frame 字节序列。
 */
inline std::vector<std::uint8_t> encode_service_frame(ServiceFrameHeader header,
                                                      std::span<const std::uint8_t> payload,
                                                      std::span<const std::uint8_t> error_msg) {
    std::vector<std::uint8_t> tail;
    header.payload_span = append_tail_block(tail, payload);
    header.error_msg_span = append_tail_block(tail, error_msg);

    std::vector<std::uint8_t> frame(SERVICE_FRAME_HEADER_SIZE + tail.size(), 0U);
    header.encode(std::span<std::uint8_t>{frame.data(), SERVICE_FRAME_HEADER_SIZE});
    if (!tail.empty()) {
        std::memcpy(frame.data() + SERVICE_FRAME_HEADER_SIZE, tail.data(), tail.size());
    }
    return frame;
}

/**
 * @brief 解码完整的 service frame，返回 header、payload 和 error message。
 */
struct DecodedServiceFrame {
    ServiceFrameHeader header;
    std::vector<std::uint8_t> payload;
    std::vector<std::uint8_t> error_msg;
};

inline DecodedServiceFrame decode_service_frame(std::span<const std::uint8_t> frame) {
    if (frame.size() < SERVICE_FRAME_HEADER_SIZE) {
        throw WireCodecError(SERVICE_FRAME_HEADER_SIZE, frame.size());
    }
    auto header = ServiceFrameHeader::decode(frame.subspan(0, SERVICE_FRAME_HEADER_SIZE));
    FrameDecoder decoder{frame.subspan(SERVICE_FRAME_HEADER_SIZE)};
    auto payload_data = decoder.read_block(header.payload_span);
    auto error_msg_data = decoder.read_block(header.error_msg_span);
    decoder.finish();

    DecodedServiceFrame result;
    result.header = header;
    result.payload.assign(payload_data.begin(), payload_data.end());
    result.error_msg.assign(error_msg_data.begin(), error_msg_data.end());
    return result;
}

}  // namespace flowrt
