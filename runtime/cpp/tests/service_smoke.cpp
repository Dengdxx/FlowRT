/// Service core primitives C++ smoke 测试。
///
/// 覆盖 ServiceError ABI 稳定性、ServiceFrameHeader roundtrip、
/// 非法 magic/version 报错、deadline 语义、ServiceResult 语义。

#include <array>
#include <cassert>
#include <cstddef>
#include <cstdint>
#include <flowrt/abi.h>
#include <flowrt/service.hpp>
#include <flowrt/zenoh.hpp>
#include <limits>
#include <string_view>
#include <vector>

int main() {
    // ── ABI 常量稳定性 ──────────────────────────────────────────────────────

    static_assert(FLOWRT_SERVICE_OK == 0U);
    static_assert(FLOWRT_SERVICE_TIMEOUT == 1U);
    static_assert(FLOWRT_SERVICE_UNAVAILABLE == 2U);
    static_assert(FLOWRT_SERVICE_BUSY == 3U);
    static_assert(FLOWRT_SERVICE_REJECTED == 4U);
    static_assert(FLOWRT_SERVICE_CANCELLED == 5U);
    static_assert(FLOWRT_SERVICE_DEADLINE_EXCEEDED == 6U);
    static_assert(FLOWRT_SERVICE_PROTOCOL == 7U);
    static_assert(FLOWRT_SERVICE_BACKEND == 8U);
    static_assert(FLOWRT_SERVICE_WOULD_DEADLOCK == 9U);
    static_assert(FLOWRT_SERVICE_HANDLER_ERROR == 10U);

    static_assert(FLOWRT_SERVICE_FRAME_MAGIC == 0x53525646U);
    static_assert(FLOWRT_SERVICE_FRAME_VERSION == 1U);
    static_assert(FLOWRT_SERVICE_FRAME_HEADER_SIZE == 80U);

    // ── C ABI frame header 布局 ─────────────────────────────────────────────

    static_assert(sizeof(flowrt_service_frame_header_t) == 80U);
    static_assert(offsetof(flowrt_service_frame_header_t, magic) == 0U);
    static_assert(offsetof(flowrt_service_frame_header_t, version) == 4U);
    static_assert(offsetof(flowrt_service_frame_header_t, error_code) == 6U);
    static_assert(offsetof(flowrt_service_frame_header_t, service_id) == 8U);
    static_assert(offsetof(flowrt_service_frame_header_t, session_id) == 16U);
    static_assert(offsetof(flowrt_service_frame_header_t, sequence) == 24U);
    static_assert(offsetof(flowrt_service_frame_header_t, correlation_id) == 32U);
    static_assert(offsetof(flowrt_service_frame_header_t, timeout_ms) == 40U);
    static_assert(offsetof(flowrt_service_frame_header_t, absolute_deadline_ms) == 48U);
    static_assert(offsetof(flowrt_service_frame_header_t, schema_hash) == 56U);
    static_assert(offsetof(flowrt_service_frame_header_t, payload_offset) == 64U);
    static_assert(offsetof(flowrt_service_frame_header_t, payload_len) == 68U);
    static_assert(offsetof(flowrt_service_frame_header_t, error_msg_offset) == 72U);
    static_assert(offsetof(flowrt_service_frame_header_t, error_msg_len) == 76U);

    // ── ServiceError ABI 转换 ───────────────────────────────────────────────

    assert(flowrt::service_error_from_abi(0) == flowrt::ServiceError::Ok);
    assert(flowrt::service_error_from_abi(10) == flowrt::ServiceError::HandlerError);
    assert(!flowrt::service_error_from_abi(11).has_value());
    assert(!flowrt::service_error_from_abi(0xFFFF).has_value());

    assert(flowrt::is_ok(flowrt::ServiceError::Ok));
    assert(!flowrt::is_ok(flowrt::ServiceError::Timeout));
    assert(flowrt::ServiceResult<int>::err(flowrt::ServiceError::Ok).error_code() ==
           flowrt::ServiceError::Protocol);
    assert(flowrt::ServiceResult<int>::err_with_message(flowrt::ServiceError::Ok, "bad")
               .error_code() == flowrt::ServiceError::Protocol);

    assert(flowrt::to_string(flowrt::ServiceError::Ok) == "Ok");
    assert(flowrt::to_string(flowrt::ServiceError::Timeout) == "Timeout");
    assert(flowrt::to_string(flowrt::ServiceError::HandlerError) == "HandlerError");

    // ── FNV-1a 一致性 ───────────────────────────────────────────────────────

    assert(flowrt::fnv1a64("") == 0xcbf29ce484222325ULL);
    assert(flowrt::fnv1a64("a") != flowrt::fnv1a64("b"));
    assert(flowrt::fnv1a64("service_a") != flowrt::fnv1a64("service_b"));
    assert(flowrt::zenoh::service_key_expr("flowrt/test service") ==
           "flowrt/service/flowrt_x2F_test_x20_service/request");
    assert(flowrt::zenoh::service_key_expr("a/b") != flowrt::zenoh::service_key_expr("a_x2F_b"));
    assert(flowrt::zenoh::service_key_expr("a_x2F_b") == "flowrt/service/a_x5F_x2F_x5F_b/request");
    auto future_error =
        flowrt::zenoh::service_result_from_response_error_code<int>(99U, "future service error");
    assert(future_error.error_code() == flowrt::ServiceError::Protocol);
    assert(future_error.error_message().has_value());
    assert(future_error.error_message().value() ==
           "unknown service error code 99: future service error");
    auto zenoh_service_config = flowrt::zenoh::ZenohServiceConfig::defaults();
    assert(zenoh_service_config.max_in_flight() == 64U);
    zenoh_service_config.set_max_in_flight(2U);
    assert(zenoh_service_config.max_in_flight() == 2U);
    zenoh_service_config.set_max_in_flight(0U);
    assert(zenoh_service_config.max_in_flight() == 1U);

    // ── Deadline 语义 ───────────────────────────────────────────────────────

    assert(!flowrt::Deadline::make(0, 1000).has_value());
    auto deadline = flowrt::Deadline::make(500, 1000);
    assert(deadline.has_value());
    assert(deadline->timeout_ms == 500U);
    assert(deadline->absolute_deadline_ms == 1500U);
    assert(!deadline->expired(1499));
    assert(deadline->expired(1500));
    assert(deadline->expired(2000));
    auto saturated = flowrt::Deadline::make(10, std::numeric_limits<std::uint64_t>::max() - 5U);
    assert(saturated.has_value());
    assert(saturated->absolute_deadline_ms == std::numeric_limits<std::uint64_t>::max());

    // ── RequestId 构造 ──────────────────────────────────────────────────────

    flowrt::RequestId rid{0xAAAA, 1, 0xBBBB};
    assert(rid.session_id == 0xAAAA);
    assert(rid.sequence == 1);
    assert(rid.service_id == 0xBBBB);

    // ── ServiceFrameHeader 请求帧 roundtrip ─────────────────────────────────

    {
        auto d = flowrt::Deadline::make(2000, 500).value();
        auto header = flowrt::ServiceFrameHeader::make_request(flowrt::RequestId{100, 1, 0xABCD}, d,
                                                               0x9999, 0);

        std::array<std::uint8_t, 80> buf{};
        header.encode(buf);

        assert(buf[0] == 0x46U);  // magic LE first byte
        assert(buf[1] == 0x56U);
        assert(buf[2] == 0x52U);
        assert(buf[3] == 0x53U);

        auto decoded = flowrt::ServiceFrameHeader::decode(buf);
        assert(decoded == header);
        assert(decoded.error_code == static_cast<std::uint16_t>(flowrt::ServiceError::Ok));
        assert(decoded.session_id == 100U);
        assert(decoded.sequence == 1U);
        assert(decoded.service_id == 0xABCDU);
        assert(decoded.correlation_id == 0x9999U);
        assert(decoded.timeout_ms == 2000U);
        assert(decoded.absolute_deadline_ms == 2500U);
    }

    // ── ServiceFrameHeader 响应帧 roundtrip ─────────────────────────────────

    {
        auto d = flowrt::Deadline::make(3000, 1000).value();
        auto header = flowrt::ServiceFrameHeader::make_response(
            flowrt::RequestId{200, 5, 0x1234}, d, 0, 0, flowrt::ServiceError::HandlerError);

        std::array<std::uint8_t, 80> buf{};
        header.encode(buf);
        auto decoded = flowrt::ServiceFrameHeader::decode(buf);

        assert(decoded.error_code ==
               static_cast<std::uint16_t>(flowrt::ServiceError::HandlerError));
        assert(decoded.session_id == 200U);
        assert(decoded.sequence == 5U);
    }

    // ── 非法 magic 报错 ────────────────────────────────────────────────────

    {
        std::array<std::uint8_t, 80> bad_magic{};
        bad_magic[0] = 0xBAU;
        bad_magic[1] = 0xDBU;
        bad_magic[2] = 0xADU;
        bad_magic[3] = 0xB0U;
        bad_magic[4] = 0x01U;
        bad_magic[5] = 0x00U;
        bool caught = false;
        try {
            flowrt::ServiceFrameHeader::decode(bad_magic);
        } catch (const flowrt::WireCodecError &error) {
            caught = true;
            const auto msg = std::string_view{error.what()};
            assert(msg.find("magic") != std::string_view::npos);
        }
        assert(caught);
    }

    // ── 旧版 version 报错，未来兼容 version 保留 ────────────────────────────

    {
        std::array<std::uint8_t, 80> bad_version{};
        bad_version[0] = 0x46U;
        bad_version[1] = 0x56U;
        bad_version[2] = 0x52U;
        bad_version[3] = 0x53U;
        bad_version[4] = 0x00U;
        bad_version[5] = 0x00U;
        bool caught = false;
        try {
            flowrt::ServiceFrameHeader::decode(bad_version);
        } catch (const flowrt::WireCodecError &error) {
            caught = true;
            const auto msg = std::string_view{error.what()};
            assert(msg.find("version") != std::string_view::npos);
        }
        assert(caught);

        auto d = flowrt::Deadline::make(1000, 500).value();
        auto header = flowrt::ServiceFrameHeader::make_request(flowrt::RequestId{1, 1, 1}, d, 0, 0);
        std::array<std::uint8_t, 80> future_version{};
        header.encode(future_version);
        future_version[4] = 0x02U;
        future_version[5] = 0x00U;
        const auto decoded = flowrt::ServiceFrameHeader::decode(future_version);
        assert(decoded.version == flowrt::SERVICE_FRAME_VERSION + 1U);
    }

    // ── 未知 error code 保留 raw code ─────────────────────────────────────

    {
        auto d = flowrt::Deadline::make(1000, 500).value();
        auto header = flowrt::ServiceFrameHeader::make_request(flowrt::RequestId{1, 1, 1}, d, 0, 0);
        std::array<std::uint8_t, 80> bad_error_code{};
        header.encode(bad_error_code);
        bad_error_code[6] = 99U;
        bad_error_code[7] = 0U;
        const auto decoded = flowrt::ServiceFrameHeader::decode(bad_error_code);
        assert(decoded.error_code == 99U);
        assert(!flowrt::service_error_from_abi(decoded.error_code).has_value());
    }

    // ── zero timeout 报错 ──────────────────────────────────────────────────

    {
        auto d = flowrt::Deadline::make(1000, 500).value();
        auto header = flowrt::ServiceFrameHeader::make_request(flowrt::RequestId{1, 1, 1}, d, 0, 0);
        std::array<std::uint8_t, 80> zero_timeout{};
        header.encode(zero_timeout);
        for (std::size_t index = 40; index < 48; ++index) {
            zero_timeout[index] = 0U;
        }
        bool caught = false;
        try {
            flowrt::ServiceFrameHeader::decode(zero_timeout);
        } catch (const flowrt::WireCodecError &error) {
            caught = true;
            const auto msg = std::string_view{error.what()};
            assert(msg.find("timeout_ms") != std::string_view::npos);
        }
        assert(caught);
    }

    // ── 完整 frame roundtrip（请求 + payload）──────────────────────────────

    {
        const std::vector<std::uint8_t> payload{0x48U, 0x65U, 0x6CU, 0x6CU, 0x6FU};
        auto d = flowrt::Deadline::make(1000, 500).value();
        auto header = flowrt::ServiceFrameHeader::make_request(
            flowrt::RequestId{42, 7, flowrt::fnv1a64("my_service")}, d, 0xDEAD, 0xBEEF);

        auto frame = flowrt::encode_service_frame(header, payload, {});
        assert(frame.size() > flowrt::SERVICE_FRAME_HEADER_SIZE);

        auto decoded = flowrt::decode_service_frame(frame);
        assert(decoded.header.magic == flowrt::SERVICE_FRAME_MAGIC);
        assert(decoded.header.version == flowrt::SERVICE_FRAME_VERSION);
        assert(decoded.header.session_id == 42U);
        assert(decoded.header.sequence == 7U);
        assert(decoded.header.service_id == flowrt::fnv1a64("my_service"));
        assert(decoded.header.correlation_id == 0xDEADU);
        assert(decoded.header.timeout_ms == 1000U);
        assert(decoded.header.absolute_deadline_ms == 1500U);
        assert(decoded.header.schema_hash == 0xBEEFU);
        assert(decoded.payload == payload);
        assert(decoded.error_msg.empty());
    }

    // ── 完整 frame roundtrip（错误响应 + error message）────────────────────

    {
        const std::vector<std::uint8_t> error_msg_bytes{0x64U, 0x69U, 0x76U, 0x69U, 0x73U, 0x69U,
                                                        0x6FU, 0x6EU, 0x20U, 0x62U, 0x79U, 0x20U,
                                                        0x7AU, 0x65U, 0x72U, 0x6FU};
        auto d = flowrt::Deadline::make(3000, 1000).value();
        auto header = flowrt::ServiceFrameHeader::make_response(
            flowrt::RequestId{200, 5, 0x1234}, d, 0, 0, flowrt::ServiceError::HandlerError);

        auto frame = flowrt::encode_service_frame(header, {}, error_msg_bytes);
        auto decoded = flowrt::decode_service_frame(frame);

        assert(decoded.header.error_code ==
               static_cast<std::uint16_t>(flowrt::ServiceError::HandlerError));
        assert(decoded.payload.empty());
        assert(decoded.error_msg == error_msg_bytes);
    }

    // ── 空 frame roundtrip ─────────────────────────────────────────────────

    {
        auto d = flowrt::Deadline::make(100, 0).value();
        auto header = flowrt::ServiceFrameHeader::make_request(flowrt::RequestId{1, 1, 1}, d, 0, 0);

        auto frame = flowrt::encode_service_frame(header, {}, {});
        auto decoded = flowrt::decode_service_frame(frame);

        assert(decoded.header == header);
        assert(decoded.payload.empty());
        assert(decoded.error_msg.empty());
    }

    // ── ServiceResult 语义 ─────────────────────────────────────────────────

    {
        auto ok = flowrt::ServiceResult<int>::ok(42);
        assert(ok.is_ok());
        assert(!ok.is_err());
        assert(ok.error_code() == flowrt::ServiceError::Ok);
        assert(!ok.error_message().has_value());
        assert(ok.value() != nullptr);
        assert(*ok.value() == 42);

        auto err = flowrt::ServiceResult<int>::err_with_message(flowrt::ServiceError::Timeout,
                                                                "timed out");
        assert(!err.is_ok());
        assert(err.is_err());
        assert(err.error_code() == flowrt::ServiceError::Timeout);
        assert(err.error_message() == "timed out");
        assert(err.value() == nullptr);
    }

    return 0;
}
