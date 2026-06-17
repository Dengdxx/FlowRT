// FlowRT 管理产物。不要手工修改。
#pragma once

#include <cstddef>
#include <string_view>

namespace flowrt_app {

std::string_view self_description_json() noexcept;

std::string_view self_description_hash() noexcept;

}  // namespace flowrt_app
