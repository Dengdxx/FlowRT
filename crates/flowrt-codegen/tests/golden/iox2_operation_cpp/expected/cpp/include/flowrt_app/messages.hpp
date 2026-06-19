// FlowRT 管理产物。不要手工修改。
#pragma once

#include <algorithm>
#include <array>
#include <cstddef>
#include <cstdint>
#include <limits>
#include <span>
#include <string>
#include <vector>

#include <flowrt/runtime.hpp>

namespace flowrt_app {

struct PlanFeedback {
    static constexpr const char* IOX2_TYPE_NAME = "PlanFeedback";
    float progress{};

    bool operator==(const PlanFeedback&) const = default;
};

struct PlanGoal {
    static constexpr const char* IOX2_TYPE_NAME = "PlanGoal";
    std::uint32_t target{};

    bool operator==(const PlanGoal&) const = default;
};

struct PlanResult {
    static constexpr const char* IOX2_TYPE_NAME = "PlanResult";
    bool accepted{};

    bool operator==(const PlanResult&) const = default;
};

}  // namespace flowrt_app
