#include <cassert>
#include <chrono>
#include <cstdint>
#include <flowrt/runtime.hpp>
#include <thread>
#include <variant>

struct Iox2SmokeSample {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTCppIox2SmokeSample";

    std::uint64_t value{};
};

int main() {
    static_assert(flowrt::Iox2Backend::compiled_with_transport(),
                  "iox2 smoke requires iox2 transport");
    static_assert(!flowrt::ZenohBackend::compiled_with_transport(),
                  "iox2 smoke should not have zenoh transport");

    auto endpoint = flowrt::iox2::Iox2PubSub<Iox2SmokeSample>::open_with_config(
        "FlowRT/Cpp/Iox2/Smoke",
        flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config(
            flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Error}));

    assert(endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Ready);

    const auto published = endpoint.publish_at(Iox2SmokeSample{42U}, 100U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(published));
    assert(std::get<flowrt::ChannelWriteOutcome>(published) ==
           flowrt::ChannelWriteOutcome::Accepted);

    for (std::uint8_t attempt = 0; attempt < 10; ++attempt) {
        auto received = endpoint.receive_latest_at(105U);
        assert(!std::holds_alternative<flowrt::ChannelError>(received));
        auto latest = std::get<flowrt::Latest<Iox2SmokeSample>>(received);
        if (latest.present()) {
            assert(!latest.stale());
            assert(latest.as_ref()->value == 42U);
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{10});
    }

#ifdef FLOWRT_ENABLE_TEST_HOOKS
    endpoint.reset_transport_for_test();
    assert(!endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Degraded);
    const auto recovered = endpoint.publish_at(Iox2SmokeSample{99U}, 120U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(recovered));
    assert(endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Ready);
#endif

    return 0;
}
