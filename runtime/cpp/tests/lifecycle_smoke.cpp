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

    const auto status = state.status();
    assert(status.instances.size() == 2);
    // std::map 迭代按 key canonical 排序。
    assert(status.instances[0].instance == "controller");
    assert(status.instances[0].lifecycle_state == "running");
    assert(status.instances[1].instance == "plant");
    assert(status.instances[1].lifecycle_state == "faulted");

    std::size_t lifecycle_diagnostics = 0;
    bool plant_error = false;
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
    }
    assert(lifecycle_diagnostics == 2);
    assert(plant_error);
    return 0;
}
