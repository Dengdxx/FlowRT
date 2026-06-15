#pragma once

#include <algorithm>
#include <cerrno>
#include <cstdint>
#include <cstdlib>
#include <flowrt/introspection/json.hpp>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace flowrt {

namespace detail {

inline std::optional<std::size_t> find_json_string_value(std::string_view input,
                                                         std::string_view key, std::string &value) {
    const std::string needle = "\"" + std::string(key) + "\"";
    const auto key_pos = input.find(needle);
    if (key_pos == std::string_view::npos) {
        return std::nullopt;
    }
    std::size_t index = key_pos + needle.size();
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size() || input[index] != ':') {
        return std::nullopt;
    }
    ++index;
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size() || input[index] != '"') {
        return std::nullopt;
    }
    ++index;

    value.clear();
    while (index < input.size()) {
        const char byte = input[index++];
        if (byte == '"') {
            return index;
        }
        if (byte != '\\') {
            value.push_back(byte);
            continue;
        }
        if (index >= input.size()) {
            return std::nullopt;
        }
        const char escape = input[index++];
        switch (escape) {
            case '"':
            case '\\':
            case '/':
                value.push_back(escape);
                break;
            case 'b':
                value.push_back('\b');
                break;
            case 'f':
                value.push_back('\f');
                break;
            case 'n':
                value.push_back('\n');
                break;
            case 'r':
                value.push_back('\r');
                break;
            case 't':
                value.push_back('\t');
                break;
            default:
                return std::nullopt;
        }
    }
    return std::nullopt;
}

enum class IntrospectionRequestKind : std::uint8_t {
    Status = 0,
    SelfDescription = 1,
    ChannelSnapshot = 2,
    ObserveChannel = 3,
    ParamList = 4,
    ParamGet = 5,
    ParamSet = 6,
    OperationCancel = 7,
    RecorderStart = 8,
    RecorderStop = 9,
    RecorderDrain = 10,
    BoundaryPublish = 11,
};

struct ParsedIntrospectionRequest {
    IntrospectionRequestKind kind = IntrospectionRequestKind::Status;
    std::string channel;
    std::string param_name;
    std::string param_value;
    std::string operation_id;
    std::string boundary_endpoint;
    std::vector<std::uint8_t> boundary_payload;
    std::optional<std::uint64_t> boundary_published_at_ms;
    std::optional<std::string> recorder_output;
    std::vector<std::string> recorder_filters;
    std::optional<std::size_t> recorder_queue_depth;
};

inline std::optional<std::size_t> find_json_value_fragment(std::string_view input,
                                                           std::string_view key,
                                                           std::string &value) {
    const std::string needle = "\"" + std::string(key) + "\"";
    const auto key_pos = input.find(needle);
    if (key_pos == std::string_view::npos) {
        return std::nullopt;
    }
    std::size_t index = key_pos + needle.size();
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size() || input[index] != ':') {
        return std::nullopt;
    }
    ++index;
    while (index < input.size() && json_whitespace(input[index])) {
        ++index;
    }
    if (index >= input.size()) {
        return std::nullopt;
    }
    const std::size_t start = index;
    bool in_string = false;
    bool escaped = false;
    int object_depth = 0;
    int array_depth = 0;
    while (index < input.size()) {
        const char byte = input[index];
        if (in_string) {
            if (escaped) {
                escaped = false;
            } else if (byte == '\\') {
                escaped = true;
            } else if (byte == '"') {
                in_string = false;
            }
            ++index;
            continue;
        }
        if (byte == '"') {
            in_string = true;
            ++index;
            continue;
        }
        if (byte == '{') {
            ++object_depth;
        } else if (byte == '}') {
            if (object_depth == 0 && array_depth == 0) {
                break;
            }
            --object_depth;
        } else if (byte == '[') {
            ++array_depth;
        } else if (byte == ']') {
            --array_depth;
        } else if (byte == ',' && object_depth == 0 && array_depth == 0) {
            break;
        }
        ++index;
    }
    value = std::string{input.substr(start, index - start)};
    while (!value.empty() && json_whitespace(value.back())) {
        value.pop_back();
    }
    return index;
}

inline std::optional<std::size_t> find_json_unsigned_value(std::string_view input,
                                                           std::string_view key) {
    std::string fragment;
    if (!find_json_value_fragment(input, key, fragment)) {
        return std::nullopt;
    }
    if (fragment.empty() || std::any_of(fragment.begin(), fragment.end(),
                                        [](char byte) { return byte < '0' || byte > '9'; })) {
        return std::nullopt;
    }
    errno = 0;
    char *end = nullptr;
    const auto parsed = std::strtoull(fragment.c_str(), &end, 10);
    if (errno != 0 || end == fragment.c_str() || *end != '\0') {
        return std::nullopt;
    }
    return static_cast<std::size_t>(parsed);
}

inline std::optional<std::vector<std::string>> find_json_string_array_value(std::string_view input,
                                                                            std::string_view key) {
    std::string fragment;
    if (!find_json_value_fragment(input, key, fragment)) {
        return std::nullopt;
    }
    std::size_t index = 0;
    while (index < fragment.size() && json_whitespace(fragment[index])) {
        ++index;
    }
    if (index >= fragment.size() || fragment[index] != '[') {
        return std::nullopt;
    }
    ++index;
    std::vector<std::string> values;
    while (index < fragment.size()) {
        while (index < fragment.size() && json_whitespace(fragment[index])) {
            ++index;
        }
        if (index < fragment.size() && fragment[index] == ']') {
            ++index;
            while (index < fragment.size() && json_whitespace(fragment[index])) {
                ++index;
            }
            return index == fragment.size() ? std::optional<std::vector<std::string>>{values}
                                            : std::nullopt;
        }
        if (index >= fragment.size() || fragment[index] != '"') {
            return std::nullopt;
        }
        ++index;
        std::string value;
        while (index < fragment.size()) {
            const char byte = fragment[index++];
            if (byte == '"') {
                break;
            }
            if (byte != '\\') {
                value.push_back(byte);
                continue;
            }
            if (index >= fragment.size()) {
                return std::nullopt;
            }
            const char escape = fragment[index++];
            switch (escape) {
                case '"':
                case '\\':
                case '/':
                    value.push_back(escape);
                    break;
                case 'b':
                    value.push_back('\b');
                    break;
                case 'f':
                    value.push_back('\f');
                    break;
                case 'n':
                    value.push_back('\n');
                    break;
                case 'r':
                    value.push_back('\r');
                    break;
                case 't':
                    value.push_back('\t');
                    break;
                default:
                    return std::nullopt;
            }
        }
        values.push_back(std::move(value));
        while (index < fragment.size() && json_whitespace(fragment[index])) {
            ++index;
        }
        if (index < fragment.size() && fragment[index] == ',') {
            ++index;
            continue;
        }
        if (index < fragment.size() && fragment[index] == ']') {
            continue;
        }
        return std::nullopt;
    }
    return std::nullopt;
}

inline std::optional<std::vector<std::uint8_t>> find_json_u8_array_value(std::string_view input,
                                                                         std::string_view key) {
    std::string fragment;
    if (!find_json_value_fragment(input, key, fragment)) {
        return std::nullopt;
    }
    std::size_t index = 0;
    while (index < fragment.size() && json_whitespace(fragment[index])) {
        ++index;
    }
    if (index >= fragment.size() || fragment[index] != '[') {
        return std::nullopt;
    }
    ++index;
    std::vector<std::uint8_t> values;
    while (index < fragment.size()) {
        while (index < fragment.size() && json_whitespace(fragment[index])) {
            ++index;
        }
        if (index < fragment.size() && fragment[index] == ']') {
            ++index;
            while (index < fragment.size() && json_whitespace(fragment[index])) {
                ++index;
            }
            return index == fragment.size() ? std::optional<std::vector<std::uint8_t>>{values}
                                            : std::nullopt;
        }
        if (index >= fragment.size() || fragment[index] < '0' || fragment[index] > '9') {
            return std::nullopt;
        }
        std::uint64_t value = 0;
        while (index < fragment.size() && fragment[index] >= '0' && fragment[index] <= '9') {
            value = value * 10U + static_cast<std::uint64_t>(fragment[index] - '0');
            if (value > 255U) {
                return std::nullopt;
            }
            ++index;
        }
        values.push_back(static_cast<std::uint8_t>(value));
        while (index < fragment.size() && json_whitespace(fragment[index])) {
            ++index;
        }
        if (index < fragment.size() && fragment[index] == ',') {
            ++index;
            continue;
        }
        if (index < fragment.size() && fragment[index] == ']') {
            continue;
        }
        return std::nullopt;
    }
    return std::nullopt;
}

inline std::optional<ParsedIntrospectionRequest> parse_introspection_request(
    std::string_view line) {
    std::string command;
    if (!find_json_string_value(line, "command", command)) {
        return std::nullopt;
    }
    if (command == "status") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::Status, {}};
    }
    if (command == "self_description") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::SelfDescription, {}};
    }
    if (command == "channel_snapshot") {
        std::string channel;
        if (!find_json_string_value(line, "channel", channel)) {
            return std::nullopt;
        }
        ParsedIntrospectionRequest request{IntrospectionRequestKind::ChannelSnapshot};
        request.channel = std::move(channel);
        return request;
    }
    if (command == "observe_channel") {
        std::string channel;
        if (!find_json_string_value(line, "channel", channel)) {
            return std::nullopt;
        }
        ParsedIntrospectionRequest request{IntrospectionRequestKind::ObserveChannel};
        request.channel = std::move(channel);
        return request;
    }
    if (command == "param_list") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::ParamList};
    }
    if (command == "param_get") {
        std::string name;
        if (!find_json_string_value(line, "name", name)) {
            return std::nullopt;
        }
        ParsedIntrospectionRequest request{IntrospectionRequestKind::ParamGet};
        request.param_name = std::move(name);
        return request;
    }
    if (command == "param_set") {
        std::string name;
        std::string value;
        if (!find_json_string_value(line, "name", name) ||
            !find_json_value_fragment(line, "value", value)) {
            return std::nullopt;
        }
        ParsedIntrospectionRequest request{IntrospectionRequestKind::ParamSet};
        request.param_name = std::move(name);
        request.param_value = std::move(value);
        return request;
    }
    if (command == "boundary_publish") {
        std::string endpoint;
        const auto payload = find_json_u8_array_value(line, "payload");
        if (!find_json_string_value(line, "endpoint", endpoint) || !payload.has_value()) {
            return std::nullopt;
        }
        ParsedIntrospectionRequest request{IntrospectionRequestKind::BoundaryPublish};
        request.boundary_endpoint = std::move(endpoint);
        request.boundary_payload = *payload;
        if (const auto published_at_ms = find_json_unsigned_value(line, "published_at_ms")) {
            request.boundary_published_at_ms = static_cast<std::uint64_t>(*published_at_ms);
        }
        return request;
    }
    if (command == "operation_cancel") {
        std::string operation_id;
        if (!find_json_string_value(line, "operation_id", operation_id)) {
            return std::nullopt;
        }
        ParsedIntrospectionRequest request{IntrospectionRequestKind::OperationCancel};
        request.operation_id = std::move(operation_id);
        return request;
    }
    if (command == "recorder_start") {
        std::string output;
        const auto output_end = find_json_string_value(line, "output", output);
        auto filters =
            find_json_string_array_value(line, "filters").value_or(std::vector<std::string>{});
        ParsedIntrospectionRequest request{IntrospectionRequestKind::RecorderStart};
        if (output_end) {
            request.recorder_output = std::move(output);
        }
        request.recorder_filters = std::move(filters);
        request.recorder_queue_depth = find_json_unsigned_value(line, "queue_depth");
        return request;
    }
    if (command == "recorder_stop") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::RecorderStop};
    }
    if (command == "recorder_drain") {
        return ParsedIntrospectionRequest{IntrospectionRequestKind::RecorderDrain};
    }
    return std::nullopt;
}

}  // namespace detail

}  // namespace flowrt
