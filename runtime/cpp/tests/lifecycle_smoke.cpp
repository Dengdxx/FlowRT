#include "flowrt/introspection/state.hpp"
#include "flowrt/lifecycle.hpp"

#include <cassert>
#include <cstdint>
#include <string>

int main() {
    // 离散值与 Rust 镜像一致。
    static_assert(static_cast<std::uint8_t>(flowrt::LifecycleState::Uninitialized) == 0);
    static_assert(static_cast<std::uint8_t>(flowrt::LifecycleState::Initialized) == 1);
    static_assert(static_cast<std::uint8_t>(flowrt::LifecycleState::Running) == 2);
    static_assert(static_cast<std::uint8_t>(flowrt::LifecycleState::Stopped) == 3);
    static_assert(static_cast<std::uint8_t>(flowrt::LifecycleState::ShutDown) == 4);
    static_assert(static_cast<std::uint8_t>(flowrt::LifecycleState::Faulted) == 5);
    static_assert(static_cast<std::uint8_t>(flowrt::LifecycleState::Degraded) == 6);

    flowrt::IntrospectionState state;
    state.record_lifecycle_state("controller", flowrt::LifecycleState::Running);
    state.record_lifecycle_state("plant", flowrt::LifecycleState::Faulted);
    state.record_lifecycle_state("monitor", flowrt::LifecycleState::Degraded);

    const auto status = state.status();
    assert(status.instances.size() == 3);
    // std::map 迭代按 key canonical 排序。
    assert(status.instances[0].instance == "controller");
    assert(status.instances[0].lifecycle_state == "running");
    assert(status.instances[1].instance == "monitor");
    assert(status.instances[1].lifecycle_state == "degraded");
    assert(status.instances[2].instance == "plant");
    assert(status.instances[2].lifecycle_state == "faulted");

    std::size_t lifecycle_diagnostics = 0;
    bool plant_error = false;
    bool monitor_warn = false;
    for (const auto &diagnostic : status.diagnostics) {
        if (diagnostic.category != "lifecycle") {
            continue;
        }
        ++lifecycle_diagnostics;
        assert(diagnostic.entity_kind == "instance");
        if (diagnostic.entity_id == "plant") {
            assert(diagnostic.state == "faulted");
            assert(diagnostic.severity == "error");
            plant_error = true;
        }
        // degraded 降级续跑 → warn，与 Rust facts 镜像。
        if (diagnostic.entity_id == "monitor") {
            assert(diagnostic.state == "degraded");
            assert(diagnostic.severity == "warn");
            monitor_warn = true;
        }
    }
    assert(lifecycle_diagnostics == 3);
    assert(plant_error);
    assert(monitor_warn);

    // 图级 health = worst-of：存在 faulted 实例 → 图 faulted（error），与 Rust 镜像。
    assert(status.graph_health == "faulted");
    bool graph_faulted = false;
    for (const auto &diagnostic : status.diagnostics) {
        if (diagnostic.category != "graph_health") {
            continue;
        }
        assert(diagnostic.entity_kind == "graph");
        assert(diagnostic.entity_id == "graph");
        assert(diagnostic.state == "faulted");
        assert(diagnostic.severity == "error");
        graph_faulted = true;
    }
    assert(graph_faulted);

    state.record_failover(flowrt::IntrospectionFailoverEvent{
        .event = "failover",
        .group = "controller_ha",
        .old_active = "controller",
        .new_active = "monitor",
        .tick_id = 7,
        .reason = "critical_fault",
    });
    const auto failover_status = state.status();
    assert(failover_status.failovers.size() == 1);
    assert(failover_status.failovers[0].event == "failover");
    assert(failover_status.failovers[0].group == "controller_ha");
    assert(failover_status.failovers[0].old_active == "controller");
    assert(failover_status.failovers[0].new_active == "monitor");
    assert(failover_status.failovers[0].tick_id == 7);
    assert(failover_status.failovers[0].reason == "critical_fault");

    return 0;
}
