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

struct Cmd {
    std::uint32_t value{};

    bool operator==(const Cmd&) const = default;
};

}  // namespace flowrt_app
