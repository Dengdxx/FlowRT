#include <cassert>
#include <cstdint>
#include <filesystem>
#include <flowrt/core.hpp>
#include <flowrt/replay.hpp>
#include <fstream>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <variant>
#include <vector>

namespace {

flowrt::ReplayEvent event(std::uint64_t time_ms, std::string target) {
    return flowrt::ReplayEvent{
        .time_ms = time_ms,
        .target = std::move(target),
        .payload = std::vector<std::uint8_t>{static_cast<std::uint8_t>(time_ms)},
        .sample_time_ms = std::nullopt,
    };
}

// 镜像 Rust time_driver 用例：事件在 t=0 与 t=15，5ms periodic 网格，两事件之间逐周期 Timer，
// 而非一次 catch-up 跳跃。
void replay_driver_steps_periodic_grid_between_events() {
    flowrt::ReplayDriver driver{std::vector<flowrt::ReplayEvent>{event(0, "a"), event(15, "b")}};
    std::optional<std::uint64_t> next_periodic{0U};
    std::vector<std::pair<flowrt::Step, std::uint64_t>> log;
    while (true) {
        const auto step = driver.step(next_periodic);
        log.emplace_back(step, driver.now_ms());
        if (step == flowrt::Step::Shutdown) {
            break;
        }
        if (step == flowrt::Step::Data) {
            assert(!driver.take_pending_events().empty());
        }
        next_periodic = std::optional<std::uint64_t>{driver.now_ms() + 5U};
        assert(log.size() <= 16U);
    }
    const std::vector<std::pair<flowrt::Step, std::uint64_t>> expected{
        {flowrt::Step::Data, 0U},  {flowrt::Step::Timer, 5U},     {flowrt::Step::Timer, 10U},
        {flowrt::Step::Data, 15U}, {flowrt::Step::Shutdown, 15U},
    };
    assert(log == expected);
}

// 同一时刻的多个事件应一次性暂存。
void replay_driver_data_step_stages_events_at_same_time() {
    flowrt::ReplayDriver driver{std::vector<flowrt::ReplayEvent>{event(7, "a"), event(7, "b")}};
    assert(driver.step(std::optional<std::uint64_t>{100U}) == flowrt::Step::Data);
    assert(driver.now_ms() == 7U);
    assert(driver.take_pending_events().size() == 2U);
    assert(driver.step(std::optional<std::uint64_t>{100U}) == flowrt::Step::Shutdown);
}

void replay_driver_next_step_short_circuits_on_shutdown() {
    flowrt::ReplayDriver driver{std::vector<flowrt::ReplayEvent>{event(0, "a")}};
    auto shutdown = flowrt::ShutdownToken::new_for_test();
    shutdown.request();
    assert(driver.next_step(std::optional<std::uint64_t>{0U}, shutdown) == flowrt::Step::Shutdown);
}

void boundary_replay_events_keeps_only_boundary_targets() {
    const std::vector<flowrt::ReplayTimelineEntry> entries{
        {.time_ms = 5U, .target = "sample_in", .payload = {1}},
        {.time_ms = 6U, .target = "internal.channel", .payload = {2}},
        {.time_ms = 7U, .target = "sample_in", .payload = {3}},
    };
    const std::set<std::string> boundary{"sample_in"};
    const auto events = flowrt::boundary_replay_events(entries, boundary);
    assert(events.size() == 2U);
    assert(events[0].time_ms == 5U);
    assert(events[0].target == "sample_in");
    assert(events[0].payload == std::vector<std::uint8_t>{1});
    assert(events[1].time_ms == 7U);
    for (const auto &replay_event : events) {
        assert(!replay_event.sample_time_ms.has_value());
    }
}

void replay_driver_from_timeline_file_reads_and_filters_boundary_stimuli() {
    const auto path = std::filesystem::temp_directory_path() / "flowrt-replay-cpp-smoke.jsonl";
    {
        std::ofstream out{path};
        // 内含空行，验证 reader 跳过空行。
        out << "{\"time_ms\":5,\"target\":\"sample_in\",\"payload\":[1]}\n";
        out << "\n";
        out << "{\"time_ms\":6,\"target\":\"internal.channel\",\"payload\":[2]}\n";
    }
    const std::set<std::string> boundary{"sample_in"};
    auto loaded = flowrt::replay_driver_from_timeline_file(path.string(), boundary);
    assert(std::holds_alternative<flowrt::ReplayDriver>(loaded));
    auto &driver = std::get<flowrt::ReplayDriver>(loaded);
    // 只有 boundary 事件参与：第一步命中 t=5 的 Data，且只暂存一个事件。
    assert(driver.step(std::optional<std::uint64_t>{1000U}) == flowrt::Step::Data);
    assert(driver.now_ms() == 5U);
    const auto pending = driver.take_pending_events();
    assert(pending.size() == 1U);
    assert(pending[0].target == "sample_in");
    assert(pending[0].payload == std::vector<std::uint8_t>{1});
    // 内部 channel 事件被过滤，时间线随即耗尽。
    assert(driver.step(std::optional<std::uint64_t>{1000U}) == flowrt::Step::Shutdown);
    std::filesystem::remove(path);
}

void replay_driver_from_timeline_file_reports_open_failure() {
    const std::set<std::string> boundary{"sample_in"};
    auto loaded =
        flowrt::replay_driver_from_timeline_file("/nonexistent/flowrt/replay.jsonl", boundary);
    assert(std::holds_alternative<std::string>(loaded));
}

}  // namespace

int main() {
    replay_driver_steps_periodic_grid_between_events();
    replay_driver_data_step_stages_events_at_same_time();
    replay_driver_next_step_short_circuits_on_shutdown();
    boundary_replay_events_keeps_only_boundary_targets();
    replay_driver_from_timeline_file_reads_and_filters_boundary_stimuli();
    replay_driver_from_timeline_file_reports_open_failure();
    return 0;
}
