#include <algorithm>
#include <atomic>
#include <cassert>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <flowrt/iox2.hpp>
#include <flowrt/operation.hpp>
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

struct BoundedFrameMsg {
    std::vector<std::uint8_t> payload;

    [[nodiscard]] std::size_t encoded_frame_size() const noexcept { return 1U + payload.size(); }

    void encode_frame(std::span<std::uint8_t> output) const {
        if (payload.size() > 3U) {
            throw flowrt::WireCodecError("field BoundedFrameMsg.payload exceeds max 3");
        }
        flowrt::ensure_wire_size(encoded_frame_size(), output.size());
        output[0] = static_cast<std::uint8_t>(payload.size());
        std::copy(payload.begin(), payload.end(), output.begin() + 1U);
    }

    static BoundedFrameMsg decode_frame(std::span<const std::uint8_t> input) {
        if (input.empty()) {
            throw flowrt::WireCodecError(1U, 0U);
        }
        const auto len = static_cast<std::size_t>(input[0]);
        flowrt::ensure_wire_size(len, input.size() - 1U);
        return BoundedFrameMsg{.payload =
                                   std::vector<std::uint8_t>{input.begin() + 1, input.end()}};
    }
};

struct OperationFrameGoal {
    std::uint32_t target{};
    std::vector<std::uint8_t> label;

    [[nodiscard]] std::size_t encoded_frame_size() const noexcept { return 5U + label.size(); }

    void encode_frame(std::span<std::uint8_t> output) const {
        if (label.size() > 6U) {
            throw flowrt::WireCodecError("field OperationFrameGoal.label exceeds max 6");
        }
        flowrt::ensure_wire_size(encoded_frame_size(), output.size());
        flowrt::write_wire_le(output, 0U, target);
        output[4] = static_cast<std::uint8_t>(label.size());
        std::copy(label.begin(), label.end(), output.begin() + 5U);
    }

    static OperationFrameGoal decode_frame(std::span<const std::uint8_t> input) {
        if (input.size() < 5U) {
            throw flowrt::WireCodecError(5U, input.size());
        }
        const auto len = static_cast<std::size_t>(input[4]);
        flowrt::ensure_wire_size(len, input.size() - 5U);
        return OperationFrameGoal{
            .target = flowrt::read_wire_le<std::uint32_t>(input, 0U),
            .label = std::vector<std::uint8_t>{input.begin() + 5, input.end()},
        };
    }
};

using OperationFrameStart = flowrt::OperationStartRequest<OperationFrameGoal>;

static_assert(flowrt::CanonicalTransportMessage<OperationFrameStart>);

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
    const auto bounded_error = flowrt::iox2::Iox2FrameSlot<8>::try_from_message_result(
        BoundedFrameMsg{.payload = std::vector<std::uint8_t>{1U, 2U, 3U, 4U}});
    assert(std::holds_alternative<std::string>(bounded_error));
    assert(std::get<std::string>(bounded_error) == "field BoundedFrameMsg.payload exceeds max 3");

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

    const auto frame_name =
        std::string{"FlowRT/Cpp/Iox2/FrameService/Smoke/"} + std::to_string(::getpid());
    std::atomic_bool frame_server_done{false};
    std::promise<void> frame_ready_promise;
    auto frame_ready = frame_ready_promise.get_future();
    auto frame_server_thread = std::thread{[&]() {
        auto server =
            flowrt::iox2::Iox2FrameServiceServer<FrameMsg, FrameMsg, 8, 8>::open(frame_name, 8);
        assert(server.health().state == flowrt::BackendHealthState::Ready);
        frame_ready_promise.set_value();
        for (std::uint8_t attempt = 0; attempt < 50 && !frame_server_done.load(); ++attempt) {
            auto handled = server.poll_requests([](FrameMsg req) {
                auto payload = std::move(req.payload);
                payload.push_back(55U);
                return flowrt::ServiceResult<FrameMsg>::ok(FrameMsg{.payload = payload});
            });
            assert(handled.has_value());
            std::this_thread::sleep_for(std::chrono::milliseconds{5});
        }
    }};

    frame_ready.wait();
    auto frame_client =
        flowrt::iox2::Iox2FrameServiceClient<FrameMsg, FrameMsg, 8, 8>::open(frame_name);
    assert(frame_client.health().state == flowrt::BackendHealthState::Ready);
    auto frame_response =
        frame_client.call(FrameMsg{.payload = std::vector<std::uint8_t>{1U, 2U, 3U}}, 1000U);
    assert(frame_response.is_ok());
    assert(frame_response.value() != nullptr);
    assert(frame_response.value()->payload == (std::vector<std::uint8_t>{1U, 2U, 3U, 55U}));
    frame_server_done.store(true);
    frame_server_thread.join();

    const auto operation_name =
        std::string{"FlowRT/Cpp/Iox2/OperationFrame/Smoke/"} + std::to_string(::getpid());
    std::atomic_bool operation_server_done{false};
    std::promise<void> operation_ready_promise;
    auto operation_ready = operation_ready_promise.get_future();
    auto operation_server_thread = std::thread{[&]() {
        auto server =
            flowrt::iox2::Iox2FrameServiceServer<OperationFrameStart, flowrt::OperationStartAck, 40,
                                                 49>::open(operation_name, 8);
        assert(server.health().state == flowrt::BackendHealthState::Ready);
        operation_ready_promise.set_value();
        for (std::uint8_t attempt = 0; attempt < 50 && !operation_server_done.load(); ++attempt) {
            auto handled = server.poll_requests([](OperationFrameStart req) {
                assert(req.goal.target == 7U);
                assert(req.goal.label == (std::vector<std::uint8_t>{10U, 20U, 30U}));
                assert(req.owner.scope_key == 101U);
                assert(req.owner.owner_key == 202U);
                assert(req.timeout == std::chrono::milliseconds{333});
                return flowrt::ServiceResult<flowrt::OperationStartAck>::ok(
                    flowrt::OperationStartAck::accepted_with_authority(
                        flowrt::OperationId{.operation_key = 303U,
                                            .client_id = req.owner.owner_key,
                                            .sequence = 1U},
                        req.owner, 1234U));
            });
            assert(handled.has_value());
            std::this_thread::sleep_for(std::chrono::milliseconds{5});
        }
    }};

    operation_ready.wait();
    auto operation_client =
        flowrt::iox2::Iox2FrameServiceClient<OperationFrameStart, flowrt::OperationStartAck, 40,
                                             49>::open(operation_name);
    assert(operation_client.health().state == flowrt::BackendHealthState::Ready);
    auto operation_response = operation_client.call(
        OperationFrameStart{
            .goal =
                OperationFrameGoal{
                    .target = 7U,
                    .label = std::vector<std::uint8_t>{10U, 20U, 30U},
                },
            .owner = flowrt::OperationOwner{.scope_key = 101U, .owner_key = 202U},
            .timeout = std::chrono::milliseconds{333},
        },
        1000U);
    assert(operation_response.is_ok());
    assert(operation_response.value() != nullptr);
    assert(operation_response.value()->accepted);
    assert(operation_response.value()->id.operation_key == 303U);
    assert(operation_response.value()->owner.owner_key == 202U);
    assert(operation_response.value()->deadline_ms == 1234U);
    const auto oversized_operation_response = operation_client.call(
        OperationFrameStart{
            .goal =
                OperationFrameGoal{
                    .target = 8U,
                    .label = std::vector<std::uint8_t>{1U, 2U, 3U, 4U, 5U, 6U, 7U},
                },
            .owner = flowrt::OperationOwner{.scope_key = 101U, .owner_key = 202U},
            .timeout = std::chrono::milliseconds{333},
        },
        1000U);
    assert(oversized_operation_response.error_code() == flowrt::ServiceError::Backend);
    operation_server_done.store(true);
    operation_server_thread.join();
#else
    auto client = flowrt::iox2::Iox2ServiceClient<Req, Resp>::unavailable("svc", "no sdk");
    assert(client.health().state == flowrt::BackendHealthState::Unsupported);
    const auto response = client.call(Req{.goal = 1U}, 10U);
    assert(response.error_code() == flowrt::ServiceError::Unavailable);
    auto frame_client = flowrt::iox2::Iox2FrameServiceClient<FrameMsg, FrameMsg, 8, 8>::unavailable(
        "svc", "no sdk");
    assert(frame_client.health().state == flowrt::BackendHealthState::Unsupported);
    const auto frame_response =
        frame_client.call(FrameMsg{.payload = std::vector<std::uint8_t>{1U}}, 10U);
    assert(frame_response.error_code() == flowrt::ServiceError::Unavailable);
    auto operation_client =
        flowrt::iox2::Iox2FrameServiceClient<OperationFrameStart, flowrt::OperationStartAck, 40,
                                             49>::unavailable("op", "no sdk");
    assert(operation_client.health().state == flowrt::BackendHealthState::Unsupported);
    const auto operation_response = operation_client.call(OperationFrameStart{}, 10U);
    assert(operation_response.error_code() == flowrt::ServiceError::Unavailable);
#endif
    return 0;
}
