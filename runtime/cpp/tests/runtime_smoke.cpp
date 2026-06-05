#include <cassert>
#include <array>
#include <chrono>
#include <cstdint>
#include <flowrt/runtime.hpp>
#include <string_view>
#include <vector>

struct Sample {
    std::uint32_t value;
};

struct TinyWireMessage {
    std::uint16_t value{};

    static constexpr std::size_t wire_size() noexcept { return sizeof(std::uint16_t); }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0, value);
    }

    static TinyWireMessage decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        TinyWireMessage value{};
        value.value = flowrt::read_wire_le<std::uint16_t>(input, 0);
        return value;
    }
};

template <std::size_t N>
void assert_capabilities_equal(flowrt::BackendCapabilities capabilities,
                               const std::array<std::string_view, N> &expected) {
    const auto actual = capabilities.items();
    assert(actual.size() == expected.size());
    for (std::size_t index = 0; index < expected.size(); ++index) {
        assert(actual[index] == expected[index]);
    }
}

int main() {
    static_assert(flowrt::ok() == flowrt::Status::Ok);

    flowrt::Context context;
    (void)context;

    std::array<std::uint8_t, TinyWireMessage::wire_size()> tiny_wire{};
    TinyWireMessage{0x1234U}.encode_wire(tiny_wire);
    assert((tiny_wire == std::array<std::uint8_t, 2>{0x34U, 0x12U}));
    assert(TinyWireMessage::decode_wire(tiny_wire).value == 0x1234U);
    bool saw_wire_size_error = false;
    try {
        TinyWireMessage{7U}.encode_wire(std::span<std::uint8_t>{tiny_wire.data(), 1});
    } catch (const flowrt::WireCodecError &error) {
        saw_wire_size_error = true;
        assert(error.expected() == 2U);
        assert(error.actual() == 1U);
    }
    assert(saw_wire_size_error);

    flowrt::InprocBackend inproc_backend;
    assert(inproc_backend.kind() == flowrt::BackendKind::Inproc);
    assert(inproc_backend.capabilities().contains("channel:latest"));
    assert(inproc_backend.capabilities().contains("graph:static_graph"));
    assert(inproc_backend.capabilities().contains("timing:deadline_aware"));
    assert_capabilities_equal(
        inproc_backend.capabilities(),
        std::array<std::string_view, 22>{
            "abi:fixed_size_plain_data",
            "layout:native_layout",
            "allocation:bounded",
            "graph:static_graph",
            "trigger:periodic",
            "trigger:on_message",
            "trigger:startup",
            "trigger:shutdown",
            "timing:deadline_aware",
            "channel:latest",
            "channel:fifo",
            "overflow:drop_oldest",
            "overflow:drop_newest",
            "overflow:error",
            "overflow:block",
            "stale:warn",
            "stale:drop",
            "stale:hold_last",
            "stale:error",
            "topology:single_process",
            "transfer:copy",
            "observability:health",
        });

    flowrt::Iox2Backend iox2_backend;
    assert(iox2_backend.kind() == flowrt::BackendKind::Iox2);
    assert(iox2_backend.capabilities().contains("topology:multi_process"));
    assert_capabilities_equal(
        iox2_backend.capabilities(),
        std::array<std::string_view, 24>{
            "abi:fixed_size_plain_data",
            "layout:native_layout",
            "allocation:bounded",
            "graph:static_graph",
            "trigger:periodic",
            "trigger:on_message",
            "trigger:startup",
            "trigger:shutdown",
            "timing:deadline_aware",
            "channel:latest",
            "channel:fifo",
            "overflow:drop_oldest",
            "overflow:drop_newest",
            "overflow:error",
            "overflow:block",
            "stale:warn",
            "stale:drop",
            "stale:hold_last",
            "stale:error",
            "topology:multi_process",
            "topology:single_host",
            "transfer:zero_copy",
            "transfer:loaned",
            "observability:health",
        });

    flowrt::ZenohBackend zenoh_backend;
    assert(zenoh_backend.kind() == flowrt::BackendKind::Zenoh);
    assert(zenoh_backend.capabilities().contains("topology:multi_process"));
    assert(zenoh_backend.capabilities().contains("topology:multi_host"));
    assert(zenoh_backend.capabilities().contains("transfer:copy"));
    assert_capabilities_equal(
        zenoh_backend.capabilities(),
        std::array<std::string_view, 20>{
            "abi:fixed_size_plain_data",
            "layout:native_layout",
            "allocation:bounded",
            "graph:static_graph",
            "trigger:periodic",
            "trigger:on_message",
            "trigger:startup",
            "trigger:shutdown",
            "timing:deadline_aware",
            "channel:latest",
            "channel:fifo",
            "overflow:drop_oldest",
            "stale:warn",
            "stale:drop",
            "stale:hold_last",
            "stale:error",
            "topology:multi_process",
            "topology:multi_host",
            "transfer:copy",
            "observability:health",
        });

    std::size_t seen = 0;
    const auto scheduler_status = inproc_backend.scheduler().run_ticks(
        5, [&seen](std::size_t tick, flowrt::Context &) -> flowrt::Status {
            ++seen;
            if (tick == 2) {
                return flowrt::Status::Error;
            }
            return flowrt::Status::Ok;
        });
    assert(seen == 3);
    assert(scheduler_status == flowrt::Status::Error);

    Sample sample{42U};
    flowrt::Latest<Sample> latest(&sample, true);
    assert(latest.present());
    assert(latest.stale());
    assert(latest.get()->value == 42U);
    assert(latest.as_ref()->value == 42U);

    flowrt::Output<Sample> output;
    assert(!output.present());
    output.write(Sample{7U});
    assert(output.present());
    assert(output.as_ref()->value == 7U);
    assert(output.take()->value == 7U);
    assert(!output.present());

    flowrt::LatestChannel<Sample> latest_channel;
    latest_channel.publish(Sample{11U});
    assert(latest_channel.view().present());
    assert(latest_channel.view().get()->value == 11U);
    assert(latest_channel.take()->value == 11U);

    auto warn_channel = flowrt::LatestChannel<Sample>::with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Warn});
    warn_channel.publish_at(Sample{13U}, 100);
    assert(warn_channel.view_at(109).present());
    assert(!warn_channel.view_at(109).stale());
    assert(warn_channel.view_at(111).present());
    assert(warn_channel.view_at(111).stale());
    assert(warn_channel.view_at(111).get()->value == 13U);

    auto drop_channel = flowrt::LatestChannel<Sample>::with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Drop});
    drop_channel.publish_at(Sample{17U}, 100);
    assert(!drop_channel.view_at(111).present());
    assert(drop_channel.view_at(111).stale());

    auto hold_last_channel = flowrt::LatestChannel<Sample>::with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::HoldLast});
    hold_last_channel.publish_at(Sample{19U}, 100);
    assert(hold_last_channel.view_at(111).present());
    assert(hold_last_channel.view_at(111).stale());
    assert(hold_last_channel.view_at(111).get()->value == 19U);

    auto error_channel = flowrt::LatestChannel<Sample>::with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Error});
    error_channel.publish_at(Sample{23U}, 100);
    assert(error_channel.view_at(111).present());
    assert(error_channel.view_at(111).stale());
    assert(error_channel.view_at(111).get()->value == 23U);

    auto fifo_warn_channel = flowrt::FifoChannel<Sample>::with_stale_config(
        2, flowrt::OverflowPolicy::DropOldest,
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Warn});
    const auto fifo_warn_first = fifo_warn_channel.push_at(Sample{29U}, 100);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(fifo_warn_first));
    assert(std::get<flowrt::ChannelWriteOutcome>(fifo_warn_first) ==
           flowrt::ChannelWriteOutcome::Accepted);
    const auto fifo_warn_second = fifo_warn_channel.push_at(Sample{31U}, 100);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(fifo_warn_second));
    assert(std::get<flowrt::ChannelWriteOutcome>(fifo_warn_second) ==
           flowrt::ChannelWriteOutcome::Accepted);
    const auto fifo_fresh_read = fifo_warn_channel.pop_at(109);
    const auto fifo_fresh = fifo_fresh_read.view();
    assert(fifo_fresh.present());
    assert(!fifo_fresh.stale());
    assert(fifo_fresh.get()->value == 29U);
    const auto fifo_stale_read = fifo_warn_channel.pop_at(111);
    const auto fifo_stale = fifo_stale_read.view();
    assert(fifo_stale.present());
    assert(fifo_stale.stale());
    assert(fifo_stale.get()->value == 31U);

    auto fifo_drop_channel = flowrt::FifoChannel<Sample>::with_stale_config(
        1, flowrt::OverflowPolicy::DropOldest,
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Drop});
    const auto fifo_drop_write = fifo_drop_channel.push_at(Sample{37U}, 100);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(fifo_drop_write));
    const auto fifo_drop_read = fifo_drop_channel.pop_at(111);
    const auto fifo_drop = fifo_drop_read.view();
    assert(!fifo_drop.present());
    assert(fifo_drop.stale());
    assert(fifo_drop_channel.empty());

    auto fifo_error_channel = flowrt::FifoChannel<Sample>::with_stale_config(
        1, flowrt::OverflowPolicy::DropOldest,
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::Error});
    const auto fifo_error_write = fifo_error_channel.push_at(Sample{41U}, 100);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(fifo_error_write));
    const auto fifo_error_read = fifo_error_channel.pop_at(111);
    const auto fifo_error = fifo_error_read.view();
    assert(fifo_error.present());
    assert(fifo_error.stale());
    assert(fifo_error.get()->value == 41U);

    flowrt::FifoChannel<Sample> fifo_channel(1, flowrt::OverflowPolicy::DropOldest);
    const auto first = fifo_channel.push(Sample{1U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(first));
    assert(std::get<flowrt::ChannelWriteOutcome>(first) == flowrt::ChannelWriteOutcome::Accepted);
    const auto second = fifo_channel.push(Sample{2U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(second));
    assert(std::get<flowrt::ChannelWriteOutcome>(second) ==
           flowrt::ChannelWriteOutcome::DroppedOldest);
    assert(fifo_channel.pop()->value == 2U);

    flowrt::FifoChannel<Sample> block_channel(1, flowrt::OverflowPolicy::Block);
    const auto block_first = block_channel.push(Sample{3U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(block_first));
    assert(std::get<flowrt::ChannelWriteOutcome>(block_first) ==
           flowrt::ChannelWriteOutcome::Accepted);
    const auto block_second = block_channel.push(Sample{4U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(block_second));
    assert(std::get<flowrt::ChannelWriteOutcome>(block_second) ==
           flowrt::ChannelWriteOutcome::Backpressured);
    assert(block_channel.pop()->value == 3U);

    auto iox2_config = flowrt::iox2::Iox2ChannelConfig::fifo(0, flowrt::OverflowPolicy::DropOldest)
                           .with_stale_config(flowrt::StaleConfig{std::chrono::milliseconds{5},
                                                                  flowrt::StalePolicy::Error});
    auto iox2_hold_last_config = flowrt::iox2::Iox2ChannelConfig::latest().with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{10}, flowrt::StalePolicy::HoldLast});
    auto iox2_block_config =
        flowrt::iox2::Iox2ChannelConfig::fifo(0, flowrt::OverflowPolicy::Block);
    static_assert(std::string_view{flowrt::iox2::FlowrtIox2Header::IOX2_TYPE_NAME} ==
                  "FlowRTIox2Header");
    static_assert(sizeof(flowrt::iox2::FlowrtIox2Header) == sizeof(std::uint64_t));
    flowrt::iox2::FlowrtIox2Header iox2_header{10U};
    assert(iox2_header.published_at_ms == 10U);
    assert(iox2_config.depth() == 1U);
    assert(iox2_config.overflow() == flowrt::OverflowPolicy::DropOldest);
    assert(iox2_config.stale().policy() == flowrt::StalePolicy::Error);
    assert(iox2_hold_last_config.stale().policy() == flowrt::StalePolicy::HoldLast);
    assert(iox2_hold_last_config.stale().max_age() ==
           std::optional<flowrt::StaleConfig::Duration>{std::chrono::milliseconds{10}});
    assert(iox2_block_config.depth() == 1U);
    assert(iox2_block_config.overflow() == flowrt::OverflowPolicy::Block);

    auto iox2_endpoint =
        flowrt::iox2::Iox2PubSub<Sample>::open_with_config("FlowRT/Cpp/Smoke", iox2_config);
    assert(iox2_endpoint.service_name() == "FlowRT/Cpp/Smoke");
    assert(iox2_endpoint.config().depth() == 1U);
    assert(iox2_endpoint.config().overflow() == flowrt::OverflowPolicy::DropOldest);
    assert(!iox2_endpoint.ready());
    const auto transport_write = iox2_endpoint.publish_at(Sample{23U}, 10U);
    assert(std::holds_alternative<flowrt::ChannelError>(transport_write));
    assert(std::get<flowrt::ChannelError>(transport_write) == flowrt::ChannelError::Transport);
    const auto transport_read = iox2_endpoint.receive_latest_at(10U);
    assert(std::holds_alternative<flowrt::ChannelError>(transport_read));
    assert(std::get<flowrt::ChannelError>(transport_read) == flowrt::ChannelError::Transport);

    auto zenoh_config =
        flowrt::zenoh::ZenohChannelConfig::fifo(0, flowrt::OverflowPolicy::DropNewest)
            .with_stale_config(
                flowrt::StaleConfig{std::chrono::milliseconds{5}, flowrt::StalePolicy::Drop});
    auto zenoh_latest_config = flowrt::zenoh::ZenohChannelConfig::latest();
    assert(zenoh_config.depth() == 1U);
    assert(zenoh_config.overflow() == flowrt::OverflowPolicy::DropNewest);
    assert(!zenoh_config.is_latest());
    assert(zenoh_latest_config.is_latest());
    assert(zenoh_config.stale().policy() == flowrt::StalePolicy::Drop);
    assert(zenoh_config.stale().max_age() ==
           std::optional<flowrt::StaleConfig::Duration>{std::chrono::milliseconds{5}});
    assert(zenoh_latest_config.depth() == 1U);
    assert(zenoh_latest_config.overflow() == flowrt::OverflowPolicy::DropOldest);

    auto zenoh_endpoint = flowrt::zenoh::ZenohPubSub<TinyWireMessage>::open_with_config(
        "flowrt/cpp/smoke", zenoh_config);
    assert(zenoh_endpoint.key_expr() == "flowrt/cpp/smoke");
    assert(zenoh_endpoint.config().depth() == 1U);
    assert(zenoh_endpoint.config().overflow() == flowrt::OverflowPolicy::DropNewest);
    assert(!zenoh_endpoint.ready());
    const auto zenoh_transport_write = zenoh_endpoint.publish_at(TinyWireMessage{23U}, 10U);
    assert(std::holds_alternative<flowrt::ChannelError>(zenoh_transport_write));
    assert(std::get<flowrt::ChannelError>(zenoh_transport_write) ==
           flowrt::ChannelError::Transport);
    const auto zenoh_transport_read = zenoh_endpoint.receive_latest_at(10U);
    assert(std::holds_alternative<flowrt::ChannelError>(zenoh_transport_read));
    assert(std::get<flowrt::ChannelError>(zenoh_transport_read) == flowrt::ChannelError::Transport);

    return 0;
}
