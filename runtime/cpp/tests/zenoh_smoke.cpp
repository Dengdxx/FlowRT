#include <cassert>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <flowrt/runtime.hpp>
#include <span>
#include <stdexcept>
#include <thread>
#include <variant>

struct WireProbe {
    std::uint8_t tag{};
    std::uint32_t value{};

    static inline std::size_t encode_calls = 0;
    static inline std::size_t decode_calls = 0;

    static constexpr std::size_t wire_size() noexcept {
        return sizeof(std::uint8_t) + sizeof(std::uint32_t);
    }

    void encode_wire(std::span<std::uint8_t> output) const {
        ++encode_calls;
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0, tag);
        flowrt::write_wire_le(output, sizeof(std::uint8_t), value);
    }

    static WireProbe decode_wire(std::span<const std::uint8_t> input) {
        ++decode_calls;
        flowrt::ensure_wire_size(wire_size(), input.size());
        const auto tag = flowrt::read_wire_le<std::uint8_t>(input, 0);
        if (tag == 0xFFU) {
            throw std::runtime_error("intentional decode failure");
        }
        return WireProbe{tag, flowrt::read_wire_le<std::uint32_t>(input, sizeof(std::uint8_t))};
    }
};

int main() {
    static_assert(flowrt::CanonicalTransportMessage<WireProbe>);
    static_assert(sizeof(WireProbe) > WireProbe::wire_size());

    auto config = flowrt::zenoh::ZenohChannelConfig::latest().with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{5}, flowrt::StalePolicy::Warn});
    auto endpoint = flowrt::zenoh::ZenohPubSub<WireProbe>::open_with_config(
        "flowrt/runtime/cpp/zenoh_smoke", config);
    assert(endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Ready);

    std::this_thread::sleep_for(std::chrono::milliseconds{200});

    const auto first_write = endpoint.publish_at(WireProbe{1U, 11U}, 100U);
    const auto second_write = endpoint.publish_at(WireProbe{2U, 22U}, 102U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(first_write));
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(second_write));
    assert(WireProbe::encode_calls == 2U);

    for (std::size_t attempt = 0; attempt < 100U; ++attempt) {
        const auto read = endpoint.receive_latest_at(103U);
        if (std::holds_alternative<flowrt::Latest<WireProbe>>(read)) {
            const auto latest = std::get<flowrt::Latest<WireProbe>>(read);
            if (latest.present()) {
                assert(!latest.stale());
                assert(latest.get()->tag == 2U);
                assert(latest.get()->value == 22U);
                break;
            }
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{20});
    }

    assert(WireProbe::decode_calls == 1U);
    const auto stale_read = endpoint.receive_latest_at(108U);
    assert(std::holds_alternative<flowrt::Latest<WireProbe>>(stale_read));
    const auto stale = std::get<flowrt::Latest<WireProbe>>(stale_read);
    assert(stale.present());
    assert(stale.stale());
    assert(stale.get()->value == 22U);

#ifdef FLOWRT_ENABLE_TEST_HOOKS
    endpoint.close_session_for_test();
    assert(!endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Degraded);
    const auto recovered_write = endpoint.publish_at(WireProbe{3U, 33U}, 120U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(recovered_write));
    assert(endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Ready);
#endif

    const auto invalid_write = endpoint.publish_at(WireProbe{0xFFU, 99U}, 500U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(invalid_write));
    bool saw_decode_error = false;
    for (std::size_t attempt = 0; attempt < 100U; ++attempt) {
        const auto read = endpoint.receive_latest_at(108U);
        if (std::holds_alternative<flowrt::ChannelError>(read)) {
            saw_decode_error = true;
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{20});
    }
    assert(saw_decode_error);
    assert(endpoint.health().state == flowrt::BackendHealthState::Ready);

    const auto preserved_read = endpoint.receive_latest_at(108U);
    assert(std::holds_alternative<flowrt::Latest<WireProbe>>(preserved_read));
    const auto preserved = std::get<flowrt::Latest<WireProbe>>(preserved_read);
    assert(preserved.present());
    assert(preserved.stale());
    assert(preserved.get()->value == 22U);

    return 0;
}
