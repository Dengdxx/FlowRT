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

struct Estimate {
    double x{};

    bool operator==(const Estimate&) const = default;
};

struct Imu {
    double ax{};
    std::uint64_t stamp_ns{};

    bool operator==(const Imu&) const = default;
};

struct Odom {
    double vx{};
    std::uint64_t stamp_ns{};

    bool operator==(const Odom&) const = default;
};

}  // namespace flowrt_app
