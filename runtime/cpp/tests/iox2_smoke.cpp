#include <cassert>
#include <chrono>
#include <cstdint>
#include <flowrt/runtime.hpp>
#include <thread>

struct Iox2SmokeSample {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTCppIox2SmokeSample";

    std::uint64_t value{};
};

int main() {
    auto endpoint = flowrt::iox2::Iox2PubSub<Iox2SmokeSample>::open_with_config(
        "FlowRT/Cpp/Iox2/Smoke",
        flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config(
            flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Error}));

    assert(endpoint.ready());

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
            return 0;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{10});
    }

    return 1;
}
