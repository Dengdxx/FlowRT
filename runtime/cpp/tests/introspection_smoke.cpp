#include <cassert>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <flowrt/runtime.hpp>
#include <optional>
#include <string>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include <vector>

namespace {

std::string request_line(const std::filesystem::path &socket_path, const std::string &request) {
    const int fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0);

    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    const auto path = socket_path.string();
    assert(path.size() < sizeof(address.sun_path));
    std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", path.c_str());
    assert(::connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) == 0);

    const std::string line = request + "\n";
    assert(::write(fd, line.data(), line.size()) == static_cast<ssize_t>(line.size()));

    std::string response;
    char byte = '\0';
    while (::read(fd, &byte, 1) == 1) {
        if (byte == '\n') {
            break;
        }
        response.push_back(byte);
    }
    ::close(fd);
    return response;
}

void send_line_and_close(const std::filesystem::path &socket_path, const std::string &request) {
    const int fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0);

    sockaddr_un address{};
    address.sun_family = AF_UNIX;
    const auto path = socket_path.string();
    assert(path.size() < sizeof(address.sun_path));
    std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", path.c_str());
    assert(::connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) == 0);

    const std::string line = request + "\n";
    assert(::write(fd, line.data(), line.size()) == static_cast<ssize_t>(line.size()));
    assert(::shutdown(fd, SHUT_RDWR) == 0);
    ::close(fd);
}

std::filesystem::path temp_socket_path(std::string_view name) {
    std::filesystem::path root = std::filesystem::temp_directory_path() /
                                 ("flowrt-cpp-introspection-test-" + std::to_string(::getpid()));
    std::filesystem::remove_all(root);
    std::filesystem::create_directories(root);
    return root / std::string{name};
}

void assert_contains(const std::string &value, const std::string &expected) {
    assert(value.find(expected) != std::string::npos);
}

}  // namespace

int main() {
    const char *original_runtime_dir = std::getenv("XDG_RUNTIME_DIR");
    const std::optional<std::string> saved_runtime_dir =
        original_runtime_dir == nullptr ? std::nullopt
                                        : std::optional<std::string>{original_runtime_dir};
    assert(::setenv("XDG_RUNTIME_DIR", "/tmp/flowrt-xdg-smoke", 1) == 0);
    assert(flowrt::runtime_socket_path_for_pid(1234) ==
           std::filesystem::path("/tmp/flowrt-xdg-smoke/flowrt/1234.sock"));

    assert(::unsetenv("XDG_RUNTIME_DIR") == 0);
    assert(flowrt::runtime_socket_path_for_pid(1234) ==
           std::filesystem::path("/tmp") /
               ("flowrt." + std::to_string(static_cast<unsigned int>(::getuid()))) / "1234.sock");

    if (saved_runtime_dir) {
        assert(::setenv("XDG_RUNTIME_DIR", saved_runtime_dir->c_str(), 1) == 0);
    } else {
        assert(::unsetenv("XDG_RUNTIME_DIR") == 0);
    }

    const auto default_path = flowrt::runtime_socket_path_for_pid(1234);
    assert(default_path.filename() == "1234.sock");
    assert(default_path.parent_path().filename() == "flowrt" ||
           default_path.parent_path().filename().string().starts_with("flowrt."));

    flowrt::IntrospectionHandshake handshake{
        .protocol_version = flowrt::INTROSPECTION_PROTOCOL_VERSION,
        .pid = 42,
        .started_at_unix_ms = 1000,
        .self_description_hash = "abc123",
        .package = "robot_demo",
        .process = "main",
        .runtime = "cpp",
    };

    flowrt::IntrospectionState state;
    state.register_channel("source.imu_to_sink.imu", "Imu");
    for (std::size_t index = 0; index < 7; ++index) {
        state.record_tick();
    }
    state.record_channel_publish_bytes(
        "source.imu_to_sink.imu", "Imu",
        std::vector<std::uint8_t>{std::uint8_t{1}, std::uint8_t{2}, std::uint8_t{3}},
        std::optional<std::uint64_t>{9U});
    state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.kp",
        .ty = "f32",
        .update = "on_tick",
        .current = "1.0",
        .min = "0.0",
        .max = "10.0",
        .choices = {},
    });
    state.register_param(flowrt::IntrospectionParamSchema{
        .name = "controller.mode",
        .ty = "string",
        .update = "startup",
        .current = "\"normal\"",
        .min = std::nullopt,
        .max = std::nullopt,
        .choices = {"\"normal\"", "\"safe\""},
    });

    const auto socket_path = temp_socket_path("worker.sock");
    {
        auto server =
            flowrt::spawn_status_server_at(socket_path, std::move(handshake), state).value();

        const auto status_response = request_line(socket_path, R"({"command":"status"})");
        assert_contains(status_response, R"("response":"status")");
        assert_contains(status_response, R"("protocol_version":"0.1")");
        assert_contains(status_response, R"("pid":42)");
        assert_contains(status_response, R"("started_at_unix_ms":1000)");
        assert_contains(status_response, R"("self_description_hash":"abc123")");
        assert_contains(status_response, R"("package":"robot_demo")");
        assert_contains(status_response, R"("process":"main")");
        assert_contains(status_response, R"("runtime":"cpp")");
        assert_contains(status_response, R"("tick_count":7)");
        assert_contains(status_response, R"("name":"source.imu_to_sink.imu")");
        assert_contains(status_response, R"("message_type":"Imu")");
        assert_contains(status_response, R"("published_count":1)");
        assert_contains(status_response, R"("last_payload_len":3)");

        const auto snapshot_response = request_line(
            socket_path, R"({"command":"channel_snapshot","channel":"source.imu_to_sink.imu"})");
        assert_contains(snapshot_response, R"("response":"channel_snapshot")");
        assert_contains(snapshot_response, R"("published_count":1)");
        assert_contains(snapshot_response, R"("payload":[1,2,3])");
        assert_contains(snapshot_response, R"("published_at_ms":9)");

        const auto unknown_response = request_line(
            socket_path, R"({"command":"channel_snapshot","channel":"missing.channel"})");
        assert_contains(unknown_response, R"("response":"error")");
        assert_contains(unknown_response, R"("message":"unknown FlowRT channel")");

        const auto param_list_response = request_line(socket_path, R"({"command":"param_list"})");
        assert_contains(param_list_response, R"("response":"param_list")");
        assert_contains(param_list_response, R"("name":"controller.kp")");
        assert_contains(param_list_response, R"("type":"f32")");
        assert_contains(param_list_response, R"("update":"on_tick")");
        assert_contains(param_list_response, R"("current":1.0)");

        const auto param_set_response = request_line(
            socket_path, R"({"command":"param_set","name":"controller.kp","value":2.5})");
        assert_contains(param_set_response, R"("response":"param_value")");
        assert_contains(param_set_response, R"("pending":2.5)");
        assert(state.pending_param("controller.kp") == std::optional<std::string>{"2.5"});
        state.record_param_applied("controller.kp", "2.5");
        assert(!state.pending_param("controller.kp").has_value());

        const auto startup_param_set_response = request_line(
            socket_path, R"({"command":"param_set","name":"controller.mode","value":"safe"})");
        assert_contains(startup_param_set_response, R"("response":"error")");
        assert_contains(startup_param_set_response,
                        R"("message":"FlowRT parameter `controller.mode` is startup-only")");

        const auto range_param_set_response = request_line(
            socket_path, R"({"command":"param_set","name":"controller.kp","value":12.0})");
        assert_contains(range_param_set_response, R"("response":"error")");
        assert_contains(range_param_set_response,
                        R"("message":"FlowRT parameter `controller.kp` is above maximum")");

        send_line_and_close(socket_path, R"({"command":"status"})");
        const auto status_after_early_close = request_line(socket_path, R"({"command":"status"})");
        assert_contains(status_after_early_close, R"("response":"status")");
        assert_contains(status_after_early_close, R"("tick_count":7)");
    }

    assert(!std::filesystem::exists(socket_path));
    std::filesystem::remove_all(socket_path.parent_path());
    return 0;
}
