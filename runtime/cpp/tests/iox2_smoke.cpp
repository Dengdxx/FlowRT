#include <algorithm>
#include <cassert>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <flowrt/runtime.hpp>
#include <span>
#include <string>
#include <thread>
#include <variant>
#include <vector>

struct Iox2SmokeSample {
    static constexpr const char *IOX2_TYPE_NAME = "FlowRTCppIox2SmokeSample";

    std::uint64_t value{};
};

struct Iox2SmokeFrame {
    std::vector<std::uint8_t> payload;

    [[nodiscard]] std::size_t encoded_frame_size() const noexcept { return 1U + payload.size(); }

    void encode_frame(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(encoded_frame_size(), output.size());
        output[0] = static_cast<std::uint8_t>(payload.size());
        std::copy(payload.begin(), payload.end(), output.begin() + 1U);
    }

    static Iox2SmokeFrame decode_frame(std::span<const std::uint8_t> input) {
        if (input.empty()) {
            throw flowrt::WireCodecError(1U, 0U);
        }
        const auto len = static_cast<std::size_t>(input[0]);
        flowrt::ensure_wire_size(len, input.size() - 1U);
        return Iox2SmokeFrame{.payload = std::vector<std::uint8_t>{input.begin() + 1, input.end()}};
    }
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

    bool received_sample = false;
    for (std::uint8_t attempt = 0; attempt < 10; ++attempt) {
        auto received = endpoint.receive_latest_at(105U);
        assert(!std::holds_alternative<flowrt::ChannelError>(received));
        auto latest = std::get<flowrt::Latest<Iox2SmokeSample>>(received);
        if (latest.present()) {
            assert(!latest.stale());
            assert(latest.as_ref()->value == 42U);
            received_sample = true;
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{10});
    }
    assert(received_sample);

    auto frame_endpoint = flowrt::iox2::Iox2FramePubSub<Iox2SmokeFrame, 8>::open_with_config(
        std::string{"FlowRT/Cpp/Iox2/FrameSmoke/"} + std::to_string(::getpid()),
        flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config(
            flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Error}));
    assert(frame_endpoint.ready());
    assert(frame_endpoint.health().state == flowrt::BackendHealthState::Ready);

    const auto frame_message = Iox2SmokeFrame{.payload = std::vector<std::uint8_t>{3U, 5U, 8U}};
    const auto frame_published = frame_endpoint.publish_at(frame_message, 200U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(frame_published));
    assert(std::get<flowrt::ChannelWriteOutcome>(frame_published) ==
           flowrt::ChannelWriteOutcome::Accepted);

    bool received_frame_sample = false;
    for (std::uint8_t attempt = 0; attempt < 10; ++attempt) {
        auto received = frame_endpoint.receive_latest_at(205U);
        assert(!std::holds_alternative<flowrt::ChannelError>(received));
        auto latest = std::get<flowrt::Latest<Iox2SmokeFrame>>(received);
        if (latest.present()) {
            assert(!latest.stale());
            assert(latest.as_ref()->payload == frame_message.payload);
            received_frame_sample = true;
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{10});
    }
    assert(received_frame_sample);

#ifdef FLOWRT_ENABLE_TEST_HOOKS
    const auto revision_before_wake_failure = endpoint.revision();
    endpoint.reset_wake_notifier_for_test();
    const auto wake_failed = endpoint.publish_at(Iox2SmokeSample{77U}, 110U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(wake_failed));
    assert(endpoint.health().state == flowrt::BackendHealthState::Degraded);
    auto after_wake_failure = endpoint.receive_latest_at(115U);
    assert(!std::holds_alternative<flowrt::ChannelError>(after_wake_failure));
    assert(endpoint.revision() == revision_before_wake_failure + 1U);
    auto latest_after_wake_failure = std::get<flowrt::Latest<Iox2SmokeSample>>(after_wake_failure);
    assert(latest_after_wake_failure.present());
    assert(latest_after_wake_failure.as_ref()->value == 77U);

    endpoint.reset_transport_for_test();
    assert(!endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Degraded);
    const auto recovered = endpoint.publish_at(Iox2SmokeSample{99U}, 120U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(recovered));
    assert(endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Ready);
    auto after_recovery = endpoint.receive_latest_at(121U);
    assert(!std::holds_alternative<flowrt::ChannelError>(after_recovery));
    auto latest_after_recovery = std::get<flowrt::Latest<Iox2SmokeSample>>(after_recovery);
    assert(latest_after_recovery.present());
    assert(latest_after_recovery.as_ref()->value == 99U);
#endif

    return 0;
}
