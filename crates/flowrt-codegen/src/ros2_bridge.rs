use flowrt_ir::{ContractIr, GraphIr, TypeExpr};

use crate::messages::cpp_type;
use crate::runtime_plan::{BridgeRuntimePlan, bridge_runtime_plans};
use crate::{
    cpp_string_literal, managed_header, ros2_bridge_key_expr, sanitize_package_name, type_by_name,
};

pub(crate) fn emit_ros2_bridge_adapter(contract: &ContractIr) -> String {
    let graph = contract
        .graphs
        .first()
        .expect("normalized contract must contain at least one graph");
    let bridges = bridge_runtime_plans(contract, graph);
    let mut output = managed_header();
    output.push_str(
        r#"#include "flowrt_app/messages.hpp"

#include <flowrt/runtime.hpp>

#include <rclcpp/rclcpp.hpp>
#include <std_msgs/msg/string.hpp>
#include <zenoh.hxx>

#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <iostream>
#include <memory>
#include <optional>
#include <string>
#include <thread>
#include <type_traits>
#include <variant>
#include <vector>

using namespace std::chrono_literals;

namespace {

std::size_t parse_run_ticks(int argc, char** argv) {
    std::size_t run_ticks = 0;
    for (int index = 1; index < argc; ++index) {
        const std::string arg{argv[index]};
        if (arg == "--flowrt-run-ticks" && index + 1 < argc) {
            run_ticks = static_cast<std::size_t>(std::stoull(argv[++index]));
            continue;
        }
        std::cerr << "unknown FlowRT ROS2 bridge argument: " << arg << '\n';
        std::exit(2);
    }
    return run_ticks;
}

bool ensure_rmw_zenoh() {
    setenv("RMW_IMPLEMENTATION", "rmw_zenoh_cpp", 0);
    const char* value = std::getenv("RMW_IMPLEMENTATION");
    return value != nullptr && std::string{value} == "rmw_zenoh_cpp";
}

std::string json_string(std::string_view value) {
    std::string output = "\"";
    for (const char ch : value) {
        switch (ch) {
            case '\\':
                output += "\\\\";
                break;
            case '"':
                output += "\\\"";
                break;
            case '\n':
                output += "\\n";
                break;
            case '\r':
                output += "\\r";
                break;
            case '\t':
                output += "\\t";
                break;
            default:
                output += ch;
                break;
        }
    }
    output += "\"";
    return output;
}

std::vector<std::string> endpoint_list_items(std::string_view raw) {
    std::vector<std::string> endpoints;
    std::size_t start = 0;
    while (start <= raw.size()) {
        const auto comma = raw.find(',', start);
        const auto end = comma == std::string_view::npos ? raw.size() : comma;
        auto item = raw.substr(start, end - start);
        while (!item.empty() && (item.front() == ' ' || item.front() == '\t')) {
            item.remove_prefix(1);
        }
        while (!item.empty() && (item.back() == ' ' || item.back() == '\t')) {
            item.remove_suffix(1);
        }
        if (!item.empty()) {
            endpoints.emplace_back(item);
        }
        if (comma == std::string_view::npos) {
            break;
        }
        start = comma + 1;
    }
    return endpoints;
}

std::string endpoint_list_json(std::string_view raw) {
    const auto endpoints = endpoint_list_items(raw);
    if (endpoints.empty()) {
        return {};
    }

    std::string json = "[";
    for (std::size_t index = 0; index < endpoints.size(); ++index) {
        if (index != 0U) {
            json += ",";
        }
        json += json_string(endpoints[index]);
    }
    json += "]";
    return json;
}

bool env_flag_enabled(const char* value) noexcept {
    if (value == nullptr) {
        return false;
    }
    const auto flag = std::string_view{value};
    return flag == "1" || flag == "true" || flag == "TRUE" || flag == "yes" || flag == "on";
}

::zenoh::Config bridge_zenoh_config_from_environment() {
    auto config = ::zenoh::Config::create_default();
    if (const auto* mode = std::getenv("FLOWRT_ZENOH_MODE")) {
        config.insert_json5(Z_CONFIG_MODE_KEY, json_string(std::string_view{mode}));
    }
    if (const auto* listen = std::getenv("FLOWRT_ZENOH_LISTEN")) {
        if (const auto json = endpoint_list_json(std::string_view{listen}); !json.empty()) {
            config.insert_json5(Z_CONFIG_LISTEN_KEY, json);
        }
    }
    if (const auto* connect = std::getenv("FLOWRT_ZENOH_CONNECT")) {
        if (const auto json = endpoint_list_json(std::string_view{connect}); !json.empty()) {
            config.insert_json5(Z_CONFIG_CONNECT_KEY, json);
        }
    }
    if (const auto* no_multicast = std::getenv("FLOWRT_ZENOH_NO_MULTICAST");
        env_flag_enabled(no_multicast)) {
        config.insert_json5(Z_CONFIG_MULTICAST_SCOUTING_KEY, "false");
    }
    return config;
}

template <flowrt::CanonicalTransportMessage T>
class BridgeZenohLatest {
   public:
    using Subscriber =
        ::zenoh::Subscriber<::zenoh::channels::RingChannel::HandlerType<::zenoh::Sample>>;

    explicit BridgeZenohLatest(std::string_view key_expr)
        : key_expr_(key_expr),
          session_(::zenoh::Session::open(bridge_zenoh_config_from_environment())),
          subscriber_(session_->declare_subscriber(::zenoh::KeyExpr(key_expr_),
                                                   ::zenoh::channels::RingChannel(1))) {}

    std::optional<T> receive_latest() {
        std::optional<T> latest;
        for (;;) {
            auto result = subscriber_->handler().try_recv();
            if (std::holds_alternative<::zenoh::channels::RecvError>(result)) {
                break;
            }
            auto sample = std::get<::zenoh::Sample>(std::move(result));
            const auto frame = sample.get_payload().as_vector();
            if (frame.size() < sizeof(std::uint64_t)) {
                continue;
            }
            try {
                latest = flowrt::detail::decode_frame<T>(
                    std::span<const std::uint8_t>{frame}.subspan(sizeof(std::uint64_t)));
            } catch (...) {
                continue;
            }
        }
        return latest;
    }

   private:
    std::string key_expr_;
    std::optional<::zenoh::Session> session_;
    std::optional<Subscriber> subscriber_;
};

"#,
    );

    for bridge in &bridges {
        output.push_str(&emit_bridge_context(contract, graph, bridge));
    }

    output.push_str(
        r#"}  // namespace

int main(int argc, char** argv) {
    if (!ensure_rmw_zenoh()) {
        std::cerr << "FlowRT ROS2 bridge requires RMW_IMPLEMENTATION=rmw_zenoh_cpp\n";
        return 2;
    }

    const std::size_t run_ticks = parse_run_ticks(argc, argv);
    rclcpp::init(argc, argv);
    auto node = std::make_shared<rclcpp::Node>("flowrt_ros2_bridge");

"#,
    );

    for bridge in &bridges {
        output.push_str(&format!(
            "    auto {name} = {context}::make(node);\n",
            name = bridge.field_name,
            context = bridge_context_name(bridge)
        ));
    }

    output.push_str(
        r#"
    std::size_t ticks = 0;
    while (rclcpp::ok() && (run_ticks == 0 || ticks < run_ticks)) {
        rclcpp::spin_some(node);
"#,
    );
    for bridge in &bridges {
        output.push_str(&format!(
            "        {name}.poll();\n",
            name = bridge.field_name
        ));
    }
    output.push_str(
        r#"        ++ticks;
        std::this_thread::sleep_for(1ms);
    }

    rclcpp::shutdown();
    return 0;
}
"#,
    );
    output
}

fn emit_bridge_context(
    contract: &ContractIr,
    graph: &GraphIr,
    bridge: &BridgeRuntimePlan,
) -> String {
    let message_type = cpp_type(&bridge.source_type);
    let context = bridge_context_name(bridge);
    let key_expr = ros2_bridge_key_expr(contract, graph, bridge);
    let field = &bridge.field;
    let ros2_topic = &bridge.ros2_topic;
    let field_type = bridge_field_type(contract, bridge);

    format!(
        r#"using flowrt_app::{message_type};

struct {context} {{
    BridgeZenohLatest<{message_type}> endpoint;
    rclcpp::Publisher<std_msgs::msg::String>::SharedPtr publisher;

    static {context} make(const std::shared_ptr<rclcpp::Node>& node) {{
        return {context}{{
            BridgeZenohLatest<{message_type}>({key_expr}),
            node->create_publisher<std_msgs::msg::String>({ros2_topic}, 10),
        }};
    }}

    void poll() {{
        auto latest = endpoint.receive_latest();
        if (!latest.has_value()) {{
            return;
        }}
        const auto& value = *latest;
        std_msgs::msg::String message;
        message.data = value.{field};
        publisher->publish(message);
    }}
}};

static_assert(std::is_same_v<{field_type}, std::string>, "ROS2 std_msgs/String bridge field must be string");

"#,
        key_expr = cpp_string_literal(&key_expr),
        ros2_topic = cpp_string_literal(ros2_topic),
    )
}

fn bridge_context_name(bridge: &BridgeRuntimePlan) -> String {
    format!("BridgeContext{}", bridge.index)
}

fn bridge_field_type(contract: &ContractIr, bridge: &BridgeRuntimePlan) -> String {
    let TypeExpr::Named { name } = &bridge.source_type else {
        return "void".to_string();
    };
    let ty = type_by_name(contract, name);
    let field = ty
        .fields
        .iter()
        .find(|field| field.name == bridge.field)
        .expect("validated ROS2 bridge must reference known message field");
    cpp_type(&field.ty)
}

pub(crate) fn ros2_bridge_stem(contract: &ContractIr) -> String {
    format!(
        "{}_ros2_bridge",
        sanitize_package_name(&contract.package.name).replace('-', "_")
    )
}
