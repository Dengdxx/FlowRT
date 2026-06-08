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
    if (value.find(expected) == std::string::npos) {
        std::fprintf(stderr, "expected substring: %s\nactual value: %s\n", expected.c_str(),
                     value.c_str());
    }
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

    flowrt::IntrospectionState probe_state;
    probe_state.register_channel("source.imu_to_sink.imu", "Imu");
    assert(!probe_state
                .try_probe_channel_publish_bytes(
                    "source.imu_to_sink.imu", "Imu",
                    std::vector<std::uint8_t>{std::uint8_t{9}, std::uint8_t{9}},
                    std::optional<std::uint64_t>{1U})
                .recorded);
    assert(probe_state.channel_snapshot("source.imu_to_sink.imu")->published_count == 0U);
    {
        const auto guard = probe_state.observe_channel("source.imu_to_sink.imu");
        assert(guard.has_value());
        assert(probe_state.active_probe_count("source.imu_to_sink.imu") ==
               std::optional<std::uint64_t>{1U});
        assert(probe_state
                   .try_probe_channel_publish_bytes(
                       "source.imu_to_sink.imu", "Imu",
                       std::vector<std::uint8_t>{std::uint8_t{8}, std::uint8_t{7}},
                       std::optional<std::uint64_t>{2U})
                   .recorded);
        const auto expected_probe_payload = std::optional<std::vector<std::uint8_t>>{
            std::vector<std::uint8_t>{std::uint8_t{8}, std::uint8_t{7}}};
        assert(probe_state.channel_snapshot("source.imu_to_sink.imu")->payload ==
               expected_probe_payload);
        assert(probe_state.channel_snapshot("source.imu_to_sink.imu")->published_count == 0U);
    }
    assert(probe_state.active_probe_count("source.imu_to_sink.imu") ==
           std::optional<std::uint64_t>{0U});
    const auto probe = probe_state.channel_probe("source.imu_to_sink.imu");
    assert(probe.has_value());
    probe->record_publish_event();
    probe->record_publish_event();
    assert(probe_state.channel_snapshot("source.imu_to_sink.imu")->published_count == 2U);

    flowrt::IntrospectionState recorder_state;
    recorder_state.register_channel("source.imu_to_sink.imu", "Imu");
    assert(!recorder_state.status().recorder.enabled);
    assert(!recorder_state
                .try_record_channel_sample_bytes(
                    "source.imu_to_sink.imu", "Imu",
                    std::vector<std::uint8_t>{std::uint8_t{1}, std::uint8_t{2}},
                    std::optional<std::uint64_t>{11U})
                .recorded);
    const auto started_recorder = recorder_state.start_recorder(flowrt::IntrospectionRecorderStart{
        .output = std::optional<std::string>{"memory://cpp.mcap"},
        .filters = {"channel"},
        .queue_depth = 1,
        .package = "robot_demo",
        .process = "main",
        .runtime_pid = 42,
        .self_description_hash = "abc123",
    });
    assert(started_recorder.enabled);
    assert(started_recorder.output == std::optional<std::string>{"memory://cpp.mcap"});
    assert(started_recorder.active_filters == std::vector<std::string>{"channel"});
    const auto recorded_sample = recorder_state.try_record_channel_sample_bytes(
        "source.imu_to_sink.imu", "Imu",
        std::vector<std::uint8_t>{std::uint8_t{3}, std::uint8_t{4}},
        std::optional<std::uint64_t>{12U});
    assert(recorded_sample.recorded);
    assert(!recorded_sample.dropped);
    const auto dropped_sample = recorder_state.try_record_channel_sample_bytes(
        "source.imu_to_sink.imu", "Imu",
        std::vector<std::uint8_t>{std::uint8_t{5}, std::uint8_t{6}},
        std::optional<std::uint64_t>{13U});
    assert(!dropped_sample.recorded);
    assert(dropped_sample.dropped);
    const auto recorder_status = recorder_state.status().recorder;
    assert(recorder_status.enabled);
    assert(recorder_status.queued_events == 1U);
    assert(recorder_status.dropped_count == 1U);
    assert(recorder_status.bytes_written == 2U);
    const auto recorder_events = recorder_state.drain_recorder_events();
    assert(recorder_events.size() == 1U);
    assert(recorder_events.front().schema_version == 1U);
    assert(recorder_events.front().event_kind == "channel_sample");
    assert(recorder_events.front().package == "robot_demo");
    assert(recorder_events.front().process == "main");
    assert(recorder_events.front().runtime_pid == 42U);
    assert(recorder_events.front().selfdesc_hash == "abc123");
    assert(recorder_events.front().sequence == 0U);
    assert(recorder_events.front().entity_kind == "channel");
    assert(recorder_events.front().entity_name == "source.imu_to_sink.imu");
    assert(recorder_events.front().entity_type_name == std::optional<std::string>{"Imu"});
    assert(recorder_events.front().payload == std::vector<std::uint8_t>({3U, 4U}));
    assert(recorder_state.status().recorder.queued_events == 0U);
    assert(!recorder_state.stop_recorder().enabled);

    flowrt::IntrospectionState service_recorder_state;
    service_recorder_state.start_recorder(flowrt::IntrospectionRecorderStart{
        .output = std::nullopt,
        .filters = {"all"},
        .queue_depth = 2,
        .package = "robot_demo",
        .process = "main",
        .runtime_pid = 42,
        .self_description_hash = "abc123",
    });
    service_recorder_state.record_service_health(flowrt::IntrospectionServiceStatus{
        .name = "planner.plan_to_executor.execute",
        .ready = true,
        .in_flight = 1,
        .queued = 0,
        .total_requests = 1,
    });
    const auto service_recorder_events = service_recorder_state.drain_recorder_events();
    assert(service_recorder_events.size() == 1U);
    assert(service_recorder_events.front().event_kind == "service_event");
    assert(service_recorder_events.front().entity_kind == "service");
    assert(service_recorder_events.front().entity_name == "planner.plan_to_executor.execute");

    flowrt::IntrospectionState service_ready_state;
    service_ready_state.register_service("planner.plan");
    auto service_ready_status = service_ready_state.status();
    assert(service_ready_status.services.size() == 1U);
    assert(!service_ready_status.services.front().ready);
    service_ready_state.mark_service_ready("planner.plan");
    service_ready_status = service_ready_state.status();
    assert(service_ready_status.services.front().ready);

    flowrt::IntrospectionState bounded_probe_state;
    bounded_probe_state.register_channel_with_probe_capacity("source.packet_to_sink.packet",
                                                             "Packet", std::size_t{4});
    {
        const auto guard = bounded_probe_state.observe_channel("source.packet_to_sink.packet");
        assert(guard.has_value());
        const auto record = bounded_probe_state.try_probe_channel_publish_bytes(
            "source.packet_to_sink.packet", "Packet",
            std::vector<std::uint8_t>{std::uint8_t{1}, std::uint8_t{2}, std::uint8_t{3},
                                      std::uint8_t{4}},
            std::optional<std::uint64_t>{3U});
        assert(record.recorded);
        assert(!record.dropped);
        const auto expected_bounded_payload =
            std::optional<std::vector<std::uint8_t>>{std::vector<std::uint8_t>{
                std::uint8_t{1}, std::uint8_t{2}, std::uint8_t{3}, std::uint8_t{4}}};
        assert(bounded_probe_state.channel_snapshot("source.packet_to_sink.packet")->payload ==
               expected_bounded_payload);
        assert(
            bounded_probe_state.channel_status("source.packet_to_sink.packet")->dropped_samples ==
            0U);
    }

    flowrt::IntrospectionState state;
    state.register_channel("source.imu_to_sink.imu", "Imu");
    for (std::size_t index = 0; index < 7; ++index) {
        state.record_tick();
    }
    state.record_task_health(flowrt::IntrospectionTaskHealth{
        .name = "imu_task",
        .lane = "sensor_lane",
        .deadline_missed = 3,
        .stale_input = 1,
        .backpressure = 0,
        .overflow = 0,
        .fairness_violations = 0,
        .run_count = 100,
        .success_count = 97,
        .consecutive_failures = 0,
        .last_run_ms = std::optional<std::uint64_t>{1000U},
        .last_success_ms = std::optional<std::uint64_t>{1000U},
    });
    state.record_lane_health(flowrt::IntrospectionLaneHealth{
        .name = "sensor_lane",
        .queue_depth = 2,
        .dispatched_count = 500,
        .fairness_violations = 0,
    });
    state.register_operation("controller.plan");
    state.record_operation_health(flowrt::IntrospectionOperationStatus{
        .name = "controller.plan",
        .ready = true,
        .running = 1,
        .queued = 2,
        .current_operation_ids = {"111:7:3"},
        .total_started = 9,
        .succeeded_count = 5,
        .failed_count = 1,
        .canceled_count = 0,
        .timeout_count = 1,
        .preempted_count = 0,
        .last_transition_ms = std::optional<std::uint64_t>{12345U},
    });
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
        const auto duplicate_server = flowrt::spawn_status_server_at(
            socket_path,
            flowrt::IntrospectionHandshake{
                .protocol_version = flowrt::INTROSPECTION_PROTOCOL_VERSION,
                .pid = 43,
                .started_at_unix_ms = 1001,
                .self_description_hash = "duplicate",
                .package = "robot_demo",
                .process = "main",
                .runtime = "cpp",
            },
            state);
        assert(!duplicate_server.has_value());

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
        assert_contains(status_response, R"("name":"imu_task")");
        assert_contains(status_response, R"("lane":"sensor_lane")");
        assert_contains(status_response, R"("deadline_missed":3)");
        assert_contains(status_response, R"("stale_input":1)");
        assert_contains(status_response, R"("run_count":100)");
        assert_contains(status_response, R"("success_count":97)");
        assert_contains(status_response, R"("queue_depth":2)");
        assert_contains(status_response, R"("dispatched_count":500)");
        assert_contains(status_response, R"("operations":[)");
        assert_contains(status_response, R"("name":"controller.plan")");
        assert_contains(status_response, R"("running":1)");
        assert_contains(status_response, R"("current_operation_ids":["111:7:3"])");
        assert_contains(status_response, R"("total_started":9)");
        assert_contains(status_response, R"("succeeded_count":5)");
        assert_contains(status_response, R"("timeout_count":1)");
        assert_contains(status_response, R"("last_transition_ms":12345)");
        assert_contains(status_response, R"("recorder":{"enabled":false)");

        const auto recorder_start_response = request_line(
            socket_path,
            R"({"command":"recorder_start","output":"memory://socket.mcap","filters":["channel"],"queue_depth":2})");
        assert_contains(recorder_start_response, R"("response":"recorder_value")");
        assert_contains(recorder_start_response, R"("enabled":true)");
        assert_contains(recorder_start_response, R"("output":"memory://socket.mcap")");
        assert_contains(recorder_start_response, R"("active_filters":["channel"])");
        state.try_record_channel_sample_bytes(
            "source.imu_to_sink.imu", "Imu",
            std::vector<std::uint8_t>{std::uint8_t{7}, std::uint8_t{8}},
            std::optional<std::uint64_t>{15U});
        const auto recorder_drain_response =
            request_line(socket_path, R"({"command":"recorder_drain"})");
        assert_contains(recorder_drain_response, R"("response":"recorder_events")");
        assert_contains(recorder_drain_response, R"("schema_version":1)");
        assert_contains(recorder_drain_response, R"("event_kind":"channel_sample")");
        assert_contains(recorder_drain_response, R"("package":"robot_demo")");
        assert_contains(recorder_drain_response, R"("runtime_pid":42)");
        assert_contains(recorder_drain_response, R"("selfdesc_hash":"abc123")");
        assert_contains(recorder_drain_response, R"("sequence":0)");
        assert_contains(recorder_drain_response,
                        R"("entity":{"kind":"channel","name":"source.imu_to_sink.imu")");
        assert_contains(recorder_drain_response, R"("type_name":"Imu")");
        assert_contains(recorder_drain_response, R"("payload":[7,8])");
        const auto recorder_stop_response =
            request_line(socket_path, R"({"command":"recorder_stop"})");
        assert_contains(recorder_stop_response, R"("response":"recorder_value")");
        assert_contains(recorder_stop_response, R"("enabled":false)");

        const auto operation_cancel_response =
            request_line(socket_path, R"({"command":"operation_cancel","operation_id":"111:7:3"})");
        assert_contains(operation_cancel_response, R"("response":"operation_value")");
        assert_contains(operation_cancel_response, R"("name":"controller.plan")");
        assert_contains(operation_cancel_response, R"("running":0)");
        assert_contains(operation_cancel_response, R"("canceled_count":1)");

        const auto operation_cancel_again_response =
            request_line(socket_path, R"({"command":"operation_cancel","operation_id":"111:7:3"})");
        assert_contains(operation_cancel_again_response, R"("response":"error")");
        assert_contains(operation_cancel_again_response,
                        R"("message":"unknown FlowRT operation `111:7:3`")");

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

        const auto selfdesc_missing_response =
            request_line(socket_path, R"({"command":"self_description"})");
        assert_contains(selfdesc_missing_response, R"("response":"error")");
        assert_contains(selfdesc_missing_response,
                        R"("message":"FlowRT self-description is not registered")");
        state.set_self_description_json(R"({"package":{"name":"robot_demo"}})");
        const auto selfdesc_response =
            request_line(socket_path, R"({"command":"self_description"})");
        assert_contains(selfdesc_response, R"("response":"self_description")");
        assert_contains(selfdesc_response, R"("json":"{\"package\":{\"name\":\"robot_demo\"}}")");

        {
            const int fd = ::socket(AF_UNIX, SOCK_STREAM, 0);
            assert(fd >= 0);
            sockaddr_un address{};
            address.sun_family = AF_UNIX;
            const auto path = socket_path.string();
            std::snprintf(address.sun_path, sizeof(address.sun_path), "%s", path.c_str());
            assert(::connect(fd, reinterpret_cast<sockaddr *>(&address), sizeof(address)) == 0);
            const std::string request =
                R"({"command":"observe_channel","channel":"source.imu_to_sink.imu","mode":"latest"})"
                "\n";
            assert(::write(fd, request.data(), request.size()) ==
                   static_cast<ssize_t>(request.size()));
            char byte = '\0';
            std::string observe_response;
            while (::read(fd, &byte, 1) == 1) {
                if (byte == '\n') {
                    break;
                }
                observe_response.push_back(byte);
            }
            assert_contains(observe_response, R"("response":"observe_ready")");
            assert(state.active_probe_count("source.imu_to_sink.imu") ==
                   std::optional<std::uint64_t>{1U});
            assert(state
                       .try_probe_channel_publish_bytes(
                           "source.imu_to_sink.imu", "Imu",
                           std::vector<std::uint8_t>{std::uint8_t{4}, std::uint8_t{5}},
                           std::optional<std::uint64_t>{10U})
                       .recorded);
            assert(::shutdown(fd, SHUT_RDWR) == 0);
            ::close(fd);
            for (std::size_t attempt = 0; attempt < 100; ++attempt) {
                if (state.active_probe_count("source.imu_to_sink.imu") ==
                    std::optional<std::uint64_t>{0U}) {
                    break;
                }
                std::this_thread::sleep_for(std::chrono::milliseconds{5});
            }
            assert(state.active_probe_count("source.imu_to_sink.imu") ==
                   std::optional<std::uint64_t>{0U});
        }

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
