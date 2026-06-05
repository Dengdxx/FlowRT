#include <cassert>
#include <chrono>
#include <cstdint>
#include <flowrt/runtime.hpp>
#include <stdexcept>
#include <thread>
#include <variant>

struct Iox2SmokeSample {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTCppIox2SmokeSample";

    std::uint64_t value{};
};

struct Iox2FrameSmokeMessage {
    std::uint32_t value{};
};

struct Iox2DecodeFailingSlot {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTCppIox2DecodeFailingSlot";

    std::uint32_t value{};

    static Iox2DecodeFailingSlot from_message(const Iox2FrameSmokeMessage &message) {
        return Iox2DecodeFailingSlot{message.value};
    }

    Iox2FrameSmokeMessage decode_message() const {
        throw std::runtime_error("intentional decode failure");
    }
};

int main() {
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

    auto frame_endpoint = flowrt::iox2::
        Iox2FramePubSub<Iox2FrameSmokeMessage, Iox2DecodeFailingSlot>::open_with_config(
            "FlowRT/Cpp/Iox2/FrameDecodeHealth", flowrt::iox2::Iox2ChannelConfig::latest());
    assert(frame_endpoint.ready());
    const auto frame_write = frame_endpoint.publish_at(Iox2FrameSmokeMessage{7U}, 200U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(frame_write));
    bool saw_decode_error = false;
    for (std::uint8_t attempt = 0; attempt < 10; ++attempt) {
        const auto read = frame_endpoint.receive_latest_at(205U);
        if (std::holds_alternative<flowrt::ChannelError>(read)) {
            saw_decode_error = true;
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{10});
    }
    assert(saw_decode_error);
    assert(frame_endpoint.health().state == flowrt::BackendHealthState::Ready);

    return 0;
}
