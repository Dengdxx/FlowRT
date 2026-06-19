#pragma once

#include <flowrt/introspection/json.hpp>
#include <flowrt/introspection/model.hpp>
#include <flowrt/introspection/probe.hpp>
#include <flowrt/introspection/request_parser.hpp>
#include <flowrt/introspection/server.hpp>
#include <flowrt/introspection/socket.hpp>
#include <flowrt/introspection/state.hpp>

namespace flowrt {

/// 将完整 introspection status snapshot 编码为 JSON。
inline std::string introspection_status_json(const IntrospectionStatus &status) {
    return detail::status_json(status);
}

}  // namespace flowrt
