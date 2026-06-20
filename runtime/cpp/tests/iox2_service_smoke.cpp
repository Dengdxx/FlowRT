#include <algorithm>
#include <atomic>
#include <cassert>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <flowrt/iox2.hpp>
#include <flowrt/service.hpp>
#include <future>
#include <span>
#include <string>
#include <thread>
#include <unistd.h>
#include <vector>

struct Req {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTCppIox2ServiceReq";

    std::uint32_t goal{};
};

struct Resp {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTCppIox2ServiceResp";

    std::uint32_t accepted{};
};

struct FrameMsg {
    std::vector<std::uint8_t> payload;

    [[nodiscard]] std::size_t encoded_frame_size() const noexcept { return 1U + payload.size(); }

    void encode_frame(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(encoded_frame_size(), output.size());
        output[0] = static_cast<std::uint8_t>(payload.size());
        std::copy(payload.begin(), payload.end(), output.begin() + 1U);
    }

    static FrameMsg decode_frame(std::span<const std::uint8_t> input) {
        if (input.empty()) {
            throw flowrt::WireCodecError(1U, 0U);
        }
        const auto len = static_cast<std::size_t>(input[0]);
        flowrt::ensure_wire_size(len, input.size() - 1U);
        return FrameMsg{.payload = std::vector<std::uint8_t>{input.begin() + 1, input.end()}};
    }
};

int main() {
    const auto slot = flowrt::iox2::Iox2FrameSlot<4>::try_from_message(
        FrameMsg{.payload = std::vector<std::uint8_t>{1U, 2U, 3U}});
    assert(slot.has_value());
    assert(slot->len == 4U);
    const auto decoded = slot->decode_message<FrameMsg>();
    assert(decoded.has_value());
    assert(decoded->payload == (std::vector<std::uint8_t>{1U, 2U, 3U}));
    assert(!flowrt::iox2::Iox2FrameSlot<3>::try_from_message(
                FrameMsg{.payload = std::vector<std::uint8_t>{1U, 2U, 3U}})
                .has_value());

#ifdef FLOWRT_HAS_ICEORYX2_CXX
    const auto name = std::string{"FlowRT/Cpp/Iox2/Service/Smoke/"} + std::to_string(::getpid());
    std::atomic_bool server_done{false};
    std::promise<void> ready_promise;
    auto ready = ready_promise.get_future();
    auto server_thread = std::thread{[&]() {
        auto server = flowrt::iox2::Iox2ServiceServer<Req, Resp>::open(name, 8);
        assert(server.health().state == flowrt::BackendHealthState::Ready);
        ready_promise.set_value();
        for (std::uint8_t attempt = 0; attempt < 50 && !server_done.load(); ++attempt) {
            auto handled = server.poll_requests([](Req req) {
                return flowrt::ServiceResult<Resp>::ok(
                    Resp{.accepted = static_cast<std::uint32_t>(req.goal % 2U == 0U)});
            });
            assert(handled.has_value());
            std::this_thread::sleep_for(std::chrono::milliseconds{5});
        }
    }};

    ready.wait();
    auto client = flowrt::iox2::Iox2ServiceClient<Req, Resp>::open(name);
    assert(client.health().state == flowrt::BackendHealthState::Ready);
    auto response = client.call(Req{.goal = 4U}, 1000U);
    assert(response.is_ok());
    assert(response.value() != nullptr);
    assert(response.value()->accepted == 1U);
    server_done.store(true);
    server_thread.join();
#else
    auto client = flowrt::iox2::Iox2ServiceClient<Req, Resp>::unavailable("svc", "no sdk");
    assert(client.health().state == flowrt::BackendHealthState::Unsupported);
    const auto response = client.call(Req{.goal = 1U}, 10U);
    assert(response.error_code() == flowrt::ServiceError::Unavailable);
    auto frame_client =
        flowrt::iox2::Iox2FrameServiceClient<FrameMsg, FrameMsg, 8, 8>::unavailable("svc", "no sdk");
    assert(frame_client.health().state == flowrt::BackendHealthState::Unsupported);
    const auto frame_response =
        frame_client.call(FrameMsg{.payload = std::vector<std::uint8_t>{1U}}, 10U);
    assert(frame_response.error_code() == flowrt::ServiceError::Unavailable);
#endif
    return 0;
}
