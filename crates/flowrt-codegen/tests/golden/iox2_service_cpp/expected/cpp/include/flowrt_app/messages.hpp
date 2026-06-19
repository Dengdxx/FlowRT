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

struct PlanRequest {
    static constexpr const char* IOX2_TYPE_NAME = "PlanRequest";
    std::uint32_t goal{};

    bool operator==(const PlanRequest&) const = default;
};

struct PlanResponse {
    static constexpr const char* IOX2_TYPE_NAME = "PlanResponse";
    bool accepted{};

    bool operator==(const PlanResponse&) const = default;
};

}  // namespace flowrt_app
