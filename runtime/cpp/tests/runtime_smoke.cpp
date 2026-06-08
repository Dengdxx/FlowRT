#include <array>
#include <atomic>
#include <cassert>
#include <chrono>
#include <cstddef>
#include <cstdint>
#include <flowrt/abi.h>
#include <flowrt/runtime.hpp>
#include <optional>
#include <string_view>
#include <type_traits>
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

flowrt::DetachedTask mark_after_schedule(flowrt::ManualExecutor &executor, bool &flag) {
    co_await flowrt::schedule_on(executor);
    flag = true;
}

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
    static_assert(FLOWRT_ABI_VERSION_MAJOR == 0U);
    static_assert(FLOWRT_ABI_VERSION_MINOR == 1U);
    static_assert(sizeof(flowrt_status_t) == sizeof(std::uint32_t));
    static_assert(FLOWRT_STATUS_OK == 0U);
    static_assert(FLOWRT_STATUS_RETRY == 1U);
    static_assert(FLOWRT_STATUS_ERROR == 2U);
    static_assert(FLOWRT_BACKEND_INPROC == 0U);
    static_assert(FLOWRT_BACKEND_IOX2 == 1U);
    static_assert(FLOWRT_BACKEND_ZENOH == 2U);
    static_assert(FLOWRT_BACKEND_HEALTH_READY == 0U);
    static_assert(FLOWRT_BACKEND_HEALTH_DEGRADED == 1U);
    static_assert(FLOWRT_BACKEND_HEALTH_RECONNECTING == 2U);
    static_assert(FLOWRT_BACKEND_HEALTH_FAILED == 3U);
    static_assert(FLOWRT_BACKEND_HEALTH_UNSUPPORTED == 4U);
    static_assert(offsetof(flowrt_string_view_t, data) == 0U);
    static_assert(offsetof(flowrt_string_view_t, len) == sizeof(void *));
    static_assert(sizeof(flowrt_string_view_t) == sizeof(void *) * 2U);
    static_assert(offsetof(flowrt_reconnect_policy_t, initial_delay_ms) == 0U);
    static_assert(offsetof(flowrt_reconnect_policy_t, max_delay_ms) == 8U);
    static_assert(offsetof(flowrt_reconnect_policy_t, max_attempts) == 16U);
    static_assert(offsetof(flowrt_reconnect_policy_t, has_max_attempts) == 20U);
    static_assert(offsetof(flowrt_backend_health_snapshot_t, state) == 0U);
    static_assert(offsetof(flowrt_backend_health_snapshot_t, attempt) == 4U);
    static_assert(offsetof(flowrt_backend_health_snapshot_t, next_retry_unix_ms) == 8U);
    static_assert(offsetof(flowrt_backend_health_snapshot_t, last_error) == 16U);
    static_assert(sizeof(flowrt_u128_t) == 16U);
    static_assert(sizeof(flowrt_i128_t) == 16U);
    static_assert(offsetof(flowrt_u128_t, lo) == 0U);
    static_assert(offsetof(flowrt_u128_t, hi) == 8U);
    static_assert(offsetof(flowrt_i128_t, lo) == 0U);
    static_assert(offsetof(flowrt_i128_t, hi) == 8U);
    static_assert(std::is_same_v<flowrt::UInt128, flowrt_u128_t>);
    static_assert(std::is_same_v<flowrt::Int128, flowrt_i128_t>);

    flowrt::Context context;
    (void)context;

    const flowrt_string_view_t label_view{
        .data = "imu",
        .len = 3U,
    };
    assert(label_view.data[0] == 'i');
    assert(label_view.len == 3U);

    const std::array<std::uint8_t, 3> bytes{1U, 2U, 3U};
    const flowrt_bytes_view_t bytes_view{
        .data = bytes.data(),
        .len = bytes.size(),
    };
    assert(bytes_view.data[2] == 3U);
    assert(bytes_view.len == 3U);

    const flowrt::UInt128 wide_unsigned{0x0123456789ABCDEFULL, 0xFEDCBA9876543210ULL};
    std::array<std::uint8_t, 16> wide_wire{};
    flowrt::write_wire_le(wide_wire, 0, wide_unsigned);
    assert((wide_wire == std::array<std::uint8_t, 16>{0xEFU, 0xCDU, 0xABU, 0x89U, 0x67U, 0x45U,
                                                      0x23U, 0x01U, 0x10U, 0x32U, 0x54U, 0x76U,
                                                      0x98U, 0xBAU, 0xDCU, 0xFEU}));
    const auto wide_unsigned_roundtrip = flowrt::read_wire_le<flowrt::UInt128>(wide_wire, 0);
    assert(wide_unsigned_roundtrip.lo == wide_unsigned.lo);
    assert(wide_unsigned_roundtrip.hi == wide_unsigned.hi);
    const flowrt::Int128 wide_signed{0xFFFFFFFFFFFFFFFFULL, 0xFFFFFFFFFFFFFFFFULL};
    flowrt::write_wire_le(wide_wire, 0, wide_signed);
    const auto wide_signed_roundtrip = flowrt::read_wire_le<flowrt::Int128>(wide_wire, 0);
    assert(wide_signed_roundtrip.lo == wide_signed.lo);
    assert(wide_signed_roundtrip.hi == wide_signed.hi);

    const flowrt_reconnect_policy_t abi_policy{
        .initial_delay_ms = 100U,
        .max_delay_ms = 1000U,
        .max_attempts = 3U,
        .has_max_attempts = 1U,
        .reserved = {0U, 0U, 0U},
    };
    assert(abi_policy.initial_delay_ms == 100U);
    assert(abi_policy.max_attempts == 3U);
    assert(abi_policy.has_max_attempts == 1U);

    const flowrt_backend_health_snapshot_t abi_snapshot{
        .state = FLOWRT_BACKEND_HEALTH_RECONNECTING,
        .attempt = 2U,
        .next_retry_unix_ms = 123456U,
        .last_error = label_view,
        .has_next_retry_unix_ms = 1U,
        .recoverable = 1U,
        .reserved = {0U, 0U, 0U, 0U, 0U, 0U},
    };
    assert(abi_snapshot.state == FLOWRT_BACKEND_HEALTH_RECONNECTING);
    assert(abi_snapshot.last_error.len == 3U);
    assert(abi_snapshot.recoverable == 1U);

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
    assert_capabilities_equal(inproc_backend.capabilities(), std::array<std::string_view, 24>{
                                                                 "abi:fixed_size_plain_data",
                                                                 "abi:variable_payload_frame",
                                                                 "layout:native_layout",
                                                                 "allocation:bounded",
                                                                 "allocation:unbounded_dynamic",
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

    static_assert(!flowrt::Iox2Backend::compiled_with_transport(),
                  "default build should not have iox2 transport");
    static_assert(!flowrt::ZenohBackend::compiled_with_transport(),
                  "default build should not have zenoh transport");

    flowrt::Iox2Backend iox2_backend;
    assert(iox2_backend.kind() == flowrt::BackendKind::Iox2);
    assert(iox2_backend.capabilities().contains("topology:multi_process"));
    assert_capabilities_equal(iox2_backend.capabilities(), std::array<std::string_view, 24>{
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
    assert_capabilities_equal(zenoh_backend.capabilities(), std::array<std::string_view, 22>{
                                                                "abi:fixed_size_plain_data",
                                                                "abi:variable_payload_frame",
                                                                "layout:native_layout",
                                                                "allocation:bounded",
                                                                "allocation:unbounded_dynamic",
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
    flowrt::ReconnectPolicy policy{100U, 1000U, std::optional<std::uint32_t>{3U}};
    assert(policy.initial_delay_ms == 100U);
    assert(policy.max_delay_ms == 1000U);
    assert(policy.max_attempts == std::optional<std::uint32_t>{3U});
    assert(policy.delay_for_attempt(0U) == 100U);
    assert(policy.delay_for_attempt(1U) == 200U);
    assert(policy.delay_for_attempt(4U) == 1000U);
    assert(policy.can_retry(2U));
    assert(!policy.can_retry(3U));

    flowrt::BackendHealthTracker tracker{policy};
    assert(tracker.snapshot().state == flowrt::BackendHealthState::Ready);
    assert(tracker.snapshot().attempt == 0U);
    assert(!tracker.snapshot().recoverable);
    tracker.mark_degraded("receive failed");
    assert(tracker.snapshot().state == flowrt::BackendHealthState::Degraded);
    assert(tracker.snapshot().last_error == std::optional<std::string>{"receive failed"});
    assert(tracker.snapshot().recoverable);
    tracker.mark_reconnecting(1U, 500U);
    assert(tracker.snapshot().state == flowrt::BackendHealthState::Reconnecting);
    assert(tracker.snapshot().attempt == 1U);
    assert(tracker.snapshot().next_retry_unix_ms == std::optional<std::uint64_t>{500U});
    tracker.mark_ready();
    assert(tracker.snapshot() == flowrt::BackendHealthSnapshot::ready());
    tracker.mark_failed("retry budget exhausted", 3U);
    assert(tracker.snapshot().state == flowrt::BackendHealthState::Failed);
    assert(tracker.snapshot().attempt == 3U);
    assert(tracker.snapshot().last_error == std::optional<std::string>{"retry budget exhausted"});
    assert(!tracker.snapshot().recoverable);

    assert(zenoh_backend.health().state == flowrt::BackendHealthState::Ready);
    assert(zenoh_backend.reconnect_policy().initial_delay_ms == 100U);
    assert(zenoh_backend.reconnect_policy().max_delay_ms == 5000U);
    assert(!zenoh_backend.reconnect_policy().max_attempts.has_value());

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

    std::size_t completed_ticks = 0;
    const auto completed_status = inproc_backend.scheduler().run_ticks(
        4, [&completed_ticks](std::size_t tick, flowrt::Context &) -> flowrt::Status {
            assert(tick == completed_ticks);
            ++completed_ticks;
            return flowrt::Status::Ok;
        });
    assert(completed_ticks == 4);
    assert(completed_status == flowrt::Status::Ok);

    std::size_t shutdown_ticks = 0;
    auto shutdown = flowrt::ShutdownToken::new_for_test();
    const auto shutdown_status = inproc_backend.scheduler().run_ticks_until_shutdown(
        10, shutdown,
        [&shutdown_ticks, &shutdown](std::size_t, flowrt::Context &) -> flowrt::Status {
            ++shutdown_ticks;
            shutdown.request();
            return flowrt::Status::Ok;
        });
    assert(shutdown_ticks == 1);
    assert(shutdown_status == flowrt::Status::Ok);

    flowrt::ScheduleWaiter data_waiter;
    auto data_shutdown = flowrt::ShutdownToken::new_for_test();
    std::thread data_notifier([&data_waiter]() {
        std::this_thread::sleep_for(std::chrono::milliseconds{5});
        data_waiter.notify_data();
    });
    assert(data_waiter.wait_until(std::chrono::steady_clock::now() + std::chrono::seconds{1},
                                  data_shutdown) == flowrt::ScheduleEvent::Data);
    data_notifier.join();

    const flowrt::ScheduleWaiter const_notifier = data_waiter;
    const auto before_const_notify = data_waiter.data_generation();
    const_notifier.notify_data();
    assert(data_waiter.data_generation() == before_const_notify + 1);

    flowrt::ScheduleWaiter barrier_waiter;
    auto barrier_shutdown = flowrt::ShutdownToken::new_for_test();
    barrier_waiter.notify_data();
    const auto seen_generation = barrier_waiter.data_generation();
    assert(barrier_waiter.wait_until_after(
               seen_generation, std::chrono::steady_clock::now() + std::chrono::milliseconds{1},
               barrier_shutdown) == flowrt::ScheduleEvent::Timer);

    flowrt::ScheduleWaiter timer_waiter;
    auto timer_shutdown = flowrt::ShutdownToken::new_for_test();
    assert(timer_waiter.wait_until(std::chrono::steady_clock::now(), timer_shutdown) ==
           flowrt::ScheduleEvent::Timer);

    flowrt::ScheduleWaiter shutdown_waiter;
    auto shutdown_for_waiter = flowrt::ShutdownToken::new_for_test();
    shutdown_for_waiter.request();
    assert(shutdown_waiter.wait_until(std::nullopt, shutdown_for_waiter) ==
           flowrt::ScheduleEvent::Shutdown);

    flowrt::DeterministicExecutor executor{1};
    executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 10});
    executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{2}, .lane = flowrt::LaneId{1}, .priority = 1});
    executor.wake(flowrt::TaskId{1});
    executor.wake(flowrt::TaskId{2});
    std::vector<flowrt::TaskId> executor_order;
    executor.run_ready([&executor_order](flowrt::TaskId task) {
        executor_order.push_back(task);
        return flowrt::Status::Ok;
    });
    assert((executor_order == std::vector<flowrt::TaskId>{flowrt::TaskId{2}, flowrt::TaskId{1}}));

    flowrt::DeterministicExecutor fair_executor{1};
    fair_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    fair_executor.add_lane(flowrt::LaneId{2}, flowrt::LaneKind::Serial);
    fair_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    fair_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{2}, .lane = flowrt::LaneId{1}, .priority = 1});
    fair_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{3}, .lane = flowrt::LaneId{2}, .priority = 99});
    fair_executor.wake(flowrt::TaskId{1});
    fair_executor.wake(flowrt::TaskId{2});
    fair_executor.wake(flowrt::TaskId{3});
    std::vector<flowrt::TaskId> fair_order;
    fair_executor.run_ready([&fair_order](flowrt::TaskId task) {
        fair_order.push_back(task);
        return flowrt::Status::Ok;
    });
    assert((fair_order ==
            std::vector<flowrt::TaskId>{flowrt::TaskId{1}, flowrt::TaskId{3}, flowrt::TaskId{2}}));

    flowrt::DeterministicExecutor timer_executor{1};
    timer_executor.add_lane(flowrt::LaneId{1}, flowrt::LaneKind::Serial);
    timer_executor.add_task(
        flowrt::TaskSpec{.id = flowrt::TaskId{1}, .lane = flowrt::LaneId{1}, .priority = 0});
    timer_executor.add_periodic(
        flowrt::PeriodicSpec{.task = flowrt::TaskId{1}, .period = std::chrono::milliseconds{10}});
    timer_executor.advance_to(std::chrono::milliseconds{35});
    std::vector<flowrt::TaskId> timer_order;
    timer_executor.run_ready([&timer_order](flowrt::TaskId task) {
        timer_order.push_back(task);
        return flowrt::Status::Ok;
    });
    assert((timer_order == std::vector<flowrt::TaskId>{flowrt::TaskId{1}}));
    assert(timer_executor.next_deadline(flowrt::TaskId{1}) == std::chrono::milliseconds{40});
    assert(timer_executor.missed_periods(flowrt::TaskId{1}) == 2U);

    flowrt::ManualExecutor coroutine_executor;
    bool coroutine_resumed = false;
    auto task = mark_after_schedule(coroutine_executor, coroutine_resumed);
    (void)task;
    assert(!coroutine_resumed);
    assert(coroutine_executor.run_ready() == 1U);
    assert(coroutine_resumed);

    flowrt::WorkerPool worker_pool{2};
    std::atomic<std::size_t> completed_jobs{0};
    for (std::size_t index = 0; index < 8; ++index) {
        assert(worker_pool.spawn([&completed_jobs]() {
            ++completed_jobs;
            return flowrt::Status::Ok;
        }));
    }
    assert(worker_pool.worker_threads() == 2U);
    assert(worker_pool.shutdown() == flowrt::Status::Ok);
    assert(completed_jobs.load() == 8U);
    assert(!worker_pool.spawn([]() { return flowrt::Status::Ok; }));

    flowrt::WorkerPool failing_pool{2};
    assert(failing_pool.spawn([]() { return flowrt::Status::Ok; }));
    assert(failing_pool.spawn([]() { return flowrt::Status::Error; }));
    assert(failing_pool.spawn([]() { return flowrt::Status::Ok; }));
    assert(failing_pool.shutdown() == flowrt::Status::Error);

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
    assert(latest_channel.revision() == 0U);
    latest_channel.publish(Sample{11U});
    assert(latest_channel.revision() == 1U);
    assert(latest_channel.view().present());
    assert(latest_channel.view().get()->value == 11U);
    assert(latest_channel.revision() == 1U);
    latest_channel.publish_at(Sample{12U}, 10U);
    assert(latest_channel.revision() == 2U);
    assert(latest_channel.take()->value == 12U);
    assert(latest_channel.revision() == 2U);

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
    assert(block_channel.revision() == 0U);
    const auto block_first = block_channel.push(Sample{3U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(block_first));
    assert(std::get<flowrt::ChannelWriteOutcome>(block_first) ==
           flowrt::ChannelWriteOutcome::Accepted);
    assert(block_channel.revision() == 1U);
    const auto block_second = block_channel.push(Sample{4U});
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(block_second));
    assert(std::get<flowrt::ChannelWriteOutcome>(block_second) ==
           flowrt::ChannelWriteOutcome::Backpressured);
    assert(block_channel.revision() == 1U);
    assert(block_channel.pop()->value == 3U);
    assert(block_channel.revision() == 1U);

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
    assert(iox2_endpoint.health().state == flowrt::BackendHealthState::Unsupported);
    assert(!iox2_endpoint.health().recoverable);
    const auto transport_write = iox2_endpoint.publish_at(Sample{23U}, 10U);
    assert(std::holds_alternative<flowrt::ChannelError>(transport_write));
    assert(std::get<flowrt::ChannelError>(transport_write) == flowrt::ChannelError::Unsupported);
    assert(iox2_endpoint.health().state == flowrt::BackendHealthState::Unsupported);
    assert(!iox2_endpoint.health().recoverable);
    const auto transport_read = iox2_endpoint.receive_latest_at(10U);
    assert(std::holds_alternative<flowrt::ChannelError>(transport_read));
    assert(std::get<flowrt::ChannelError>(transport_read) == flowrt::ChannelError::Unsupported);

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
    assert(zenoh_endpoint.health().state == flowrt::BackendHealthState::Unsupported);
    assert(!zenoh_endpoint.health().recoverable);
    const auto zenoh_transport_write = zenoh_endpoint.publish_at(TinyWireMessage{23U}, 10U);
    assert(std::holds_alternative<flowrt::ChannelError>(zenoh_transport_write));
    assert(std::get<flowrt::ChannelError>(zenoh_transport_write) ==
           flowrt::ChannelError::Unsupported);
    assert(zenoh_endpoint.health().state == flowrt::BackendHealthState::Unsupported);
    assert(!zenoh_endpoint.health().recoverable);
    const auto zenoh_transport_read = zenoh_endpoint.receive_latest_at(10U);
    assert(std::holds_alternative<flowrt::ChannelError>(zenoh_transport_read));
    assert(std::get<flowrt::ChannelError>(zenoh_transport_read) ==
           flowrt::ChannelError::Unsupported);

    return 0;
}
