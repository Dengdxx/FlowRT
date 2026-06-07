/// Zenoh service request/response smoke 测试。
///
/// 覆盖基本 request/response、timeout、handler error、multiple clients。

#include <cassert>
#include <chrono>
#include <cstdint>
#include <flowrt/runtime.hpp>
#include <string>
#include <thread>
#include <vector>

struct AddRequest {
    std::int32_t a{};
    std::int32_t b{};

    static constexpr std::size_t wire_size() noexcept {
        return sizeof(std::int32_t) + sizeof(std::int32_t);
    }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0, a);
        flowrt::write_wire_le(output, sizeof(std::int32_t), b);
    }

    static AddRequest decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        return AddRequest{
            flowrt::read_wire_le<std::int32_t>(input, 0),
            flowrt::read_wire_le<std::int32_t>(input, sizeof(std::int32_t))};
    }
};

struct AddResponse {
    std::int32_t sum{};

    static constexpr std::size_t wire_size() noexcept { return sizeof(std::int32_t); }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0, sum);
    }

    static AddResponse decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        return AddResponse{flowrt::read_wire_le<std::int32_t>(input, 0)};
    }
};

int main() {
    static_assert(flowrt::ZenohBackend::compiled_with_transport(),
                  "zenoh service smoke requires zenoh transport");
    static_assert(!flowrt::Iox2Backend::compiled_with_transport(),
                  "zenoh service smoke should not have iox2 transport");

#ifdef FLOWRT_HAS_ZENOH_CXX
    // ── Basic request/response ──────────────────────────────────────────────

    {
        auto session = std::make_shared<::zenoh::Session>(::zenoh::Config::create_default());

        auto server = flowrt::zenoh::ZenohServiceServer<AddRequest, AddResponse>::open(
            "flowrt/test/cpp/basic", session,
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            });
        assert(server.ready());

        auto client = flowrt::zenoh::ZenohServiceClient<AddRequest, AddResponse>::open(
            "flowrt/test/cpp/basic", session);
        assert(client.ready());

        auto result = client.call(AddRequest{3, 4}, 5000);
        if (result.is_err()) {
            std::fprintf(stderr, "basic: code=%d msg=%s\n",
                         static_cast<int>(result.error_code()),
                         result.error_message().value_or("").c_str());
        } else {
            std::fprintf(stderr, "basic: OK sum=%d\n", result.value()->sum);
        }
        assert(result.is_ok());
        assert(result.value()->sum == 7);
    }

    // ── Handler error ───────────────────────────────────────────────────────

    {
        auto session = std::make_shared<::zenoh::Session>(::zenoh::Config::create_default());

        auto server = flowrt::zenoh::ZenohServiceServer<AddRequest, AddResponse>::open(
            "flowrt/test/cpp/handler_error", session,
            [](const AddRequest &) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::err_with_message(
                    flowrt::ServiceError::HandlerError, "division by zero");
            });
        assert(server.ready());

        auto client = flowrt::zenoh::ZenohServiceClient<AddRequest, AddResponse>::open(
            "flowrt/test/cpp/handler_error", session);
        assert(client.ready());

        auto result = client.call(AddRequest{1, 2}, 5000);
        assert(result.is_err());
        assert(result.error_code() == flowrt::ServiceError::HandlerError);
        assert(result.error_message().has_value());
        assert(result.error_message().value() == "division by zero");
    }

    // ── Timeout ─────────────────────────────────────────────────────────────

    {
        auto session = std::make_shared<::zenoh::Session>(::zenoh::Config::create_default());

        auto server = flowrt::zenoh::ZenohServiceServer<AddRequest, AddResponse>::open(
            "flowrt/test/cpp/timeout", session,
            [](const AddRequest &) -> flowrt::ServiceResult<AddResponse> {
                std::this_thread::sleep_for(std::chrono::seconds{5});
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{0});
            });
        assert(server.ready());

        auto client = flowrt::zenoh::ZenohServiceClient<AddRequest, AddResponse>::open(
            "flowrt/test/cpp/timeout", session);
        assert(client.ready());

        auto result = client.call(AddRequest{1, 2}, 200);
        assert(result.is_err());
        assert(result.error_code() == flowrt::ServiceError::Timeout);
    }

    // ── Multiple clients ────────────────────────────────────────────────────

    {
        auto session = std::make_shared<::zenoh::Session>(::zenoh::Config::create_default());

        auto server = flowrt::zenoh::ZenohServiceServer<AddRequest, AddResponse>::open(
            "flowrt/test/cpp/multi_client", session,
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            });
        assert(server.ready());

        std::vector<std::thread> threads;
        for (std::int32_t i = 0; i < 3; ++i) {
            threads.emplace_back([i, &session]() {
                auto client = flowrt::zenoh::ZenohServiceClient<AddRequest, AddResponse>::open(
                    "flowrt/test/cpp/multi_client", session);
                assert(client.ready());

                auto result = client.call(AddRequest{i, i * 2}, 5000);
                if (result.is_err()) {
                    std::fprintf(stderr, "multi client %d: code=%d msg=%s\n", i,
                                 static_cast<int>(result.error_code()),
                                 result.error_message().value_or("").c_str());
                }
                assert(result.is_ok());
                assert(result.value()->sum == i + i * 2);
            });
        }

        for (auto &thread : threads) {
            thread.join();
        }
    }
#endif

    return 0;
}
