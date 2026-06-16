#include <cassert>
#include <cstdint>
#include <flowrt/synchronizer.hpp>
#include <optional>
#include <vector>

namespace {

using Set = std::optional<std::vector<std::uint64_t>>;

// 以下用例与 Rust `synchronizer::tests` 共享 golden 向量：同一事件序列必须产出相同
// 的同步集序列（跨语言 conformance，参见 runtime/rust/src/synchronizer.rs）。

void aligned_latest_samples_emit_one_set() {
    flowrt::Synchronizer<std::uint64_t> sync{2, 8, 10};
    sync.push(0, 100, 100);
    sync.push(1, 105, 105);
    assert((sync.poll() == Set{std::vector<std::uint64_t>{100, 105}}));
    assert(sync.poll() == std::nullopt);
}

void spread_exceeded_drains_laggard_then_recovers() {
    flowrt::Synchronizer<std::uint64_t> sync{2, 8, 10};
    sync.push(0, 100, 100);
    sync.push(1, 130, 130);
    assert(sync.poll() == std::nullopt);
    assert(sync.buffered(0) == 0);
    assert(sync.buffered(1) == 1);
    sync.push(0, 128, 128);
    assert((sync.poll() == Set{std::vector<std::uint64_t>{128, 130}}));
}

void late_sample_is_dropped() {
    flowrt::Synchronizer<std::uint64_t> sync{2, 8, 10};
    sync.push(0, 100, 100);
    sync.push(1, 100, 100);
    assert((sync.poll() == Set{std::vector<std::uint64_t>{100, 100}}));
    sync.push(0, 90, 90);  // late: <= watermark 100
    sync.push(1, 105, 105);
    assert(sync.poll() == std::nullopt);
    assert(sync.buffered(0) == 0);
}

void full_buffer_drops_oldest() {
    flowrt::Synchronizer<std::uint64_t> sync{2, 2, 100};
    sync.push(0, 1, 1);
    sync.push(0, 2, 2);
    sync.push(0, 3, 3);
    assert(sync.buffered(0) == 2);
    sync.push(1, 3, 3);
    assert((sync.poll() == Set{std::vector<std::uint64_t>{3, 3}}));
}

void three_inputs_align_within_window() {
    flowrt::Synchronizer<std::uint64_t> sync{3, 8, 10};
    sync.push(0, 100, 100);
    sync.push(1, 108, 108);
    sync.push(2, 105, 105);
    assert((sync.poll() == Set{std::vector<std::uint64_t>{100, 108, 105}}));
}

void three_inputs_drain_lagging_input() {
    flowrt::Synchronizer<std::uint64_t> sync{3, 8, 5};
    sync.push(0, 100, 100);
    sync.push(1, 200, 200);
    sync.push(2, 202, 202);
    assert(sync.poll() == std::nullopt);
    assert(sync.buffered(0) == 0);
    assert(sync.buffered(1) == 1);
    assert(sync.buffered(2) == 1);
}

}  // namespace

int main() {
    aligned_latest_samples_emit_one_set();
    spread_exceeded_drains_laggard_then_recovers();
    late_sample_is_dropped();
    full_buffer_drops_oldest();
    three_inputs_align_within_window();
    three_inputs_drain_lagging_input();
    return 0;
}
