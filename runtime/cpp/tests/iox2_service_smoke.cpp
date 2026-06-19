#include <atomic>
#include <cassert>
#include <chrono>
#include <cstdint>
#include <flowrt/iox2.hpp>
#include <flowrt/service.hpp>
#include <future>
#include <string>
#include <thread>
#include <unistd.h>

struct Req {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTCppIox2ServiceReq";

    std::uint32_t goal{};
};

struct Resp {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTCppIox2ServiceResp";

    std::uint32_t accepted{};
};

int main() {
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
#endif
    return 0;
}
