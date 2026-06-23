#include <cassert>
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <flowrt/runtime.hpp>
#include <mutex>
#include <optional>
#include <span>
#include <stdexcept>
#include <string>
#include <thread>
#include <variant>
#include <vector>

struct WireProbe {
    std::uint8_t tag{};
    std::uint32_t value{};

    static inline std::size_t encode_calls = 0;
    static inline std::size_t decode_calls = 0;

    static constexpr std::size_t wire_size() noexcept {
        return sizeof(std::uint8_t) + sizeof(std::uint32_t);
    }

    void encode_wire(std::span<std::uint8_t> output) const {
        ++encode_calls;
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0, tag);
        flowrt::write_wire_le(output, sizeof(std::uint8_t), value);
    }

    static WireProbe decode_wire(std::span<const std::uint8_t> input) {
        ++decode_calls;
        flowrt::ensure_wire_size(wire_size(), input.size());
        const auto tag = flowrt::read_wire_le<std::uint8_t>(input, 0);
        if (tag == 0xFFU) {
            throw std::runtime_error("intentional decode failure");
        }
        return WireProbe{tag, flowrt::read_wire_le<std::uint32_t>(input, sizeof(std::uint8_t))};
    }
};

#ifdef FLOWRT_HAS_ZENOH_CXX
std::optional<std::string> query_json(::zenoh::Session &session, std::string_view key_expr,
                                      std::string_view payload) {
    struct ReplyState {
        std::mutex mutex;
        std::condition_variable cv;
        std::optional<std::string> response;
        bool done = false;
    };

    auto state = std::make_shared<ReplyState>();
    auto on_reply = [state](::zenoh::Reply &reply) {
        if (reply.is_ok()) {
            auto bytes = reply.get_ok().get_payload().as_vector();
            std::string response{reinterpret_cast<const char *>(bytes.data()), bytes.size()};
            {
                std::lock_guard lock(state->mutex);
                state->response = std::move(response);
                state->done = true;
            }
            state->cv.notify_one();
        }
    };
    auto on_drop = [state]() {
        {
            std::lock_guard lock(state->mutex);
            state->done = true;
        }
        state->cv.notify_one();
    };

    auto opts = ::zenoh::Session::GetOptions::create_default();
    opts.timeout_ms = 5000;
    opts.payload = ::zenoh::Bytes(std::vector<std::uint8_t>(payload.begin(), payload.end()));
    session.get(::zenoh::KeyExpr(std::string{key_expr}), "", std::move(on_reply),
                std::move(on_drop), std::move(opts));

    std::unique_lock lock(state->mutex);
    if (!state->cv.wait_for(lock, std::chrono::milliseconds{5000},
                            [&state]() { return state->done; })) {
        return std::nullopt;
    }
    return state->response;
}
#endif

int main() {
    static_assert(flowrt::ZenohBackend::compiled_with_transport(),
                  "zenoh smoke requires zenoh transport");
    static_assert(!flowrt::Iox2Backend::compiled_with_transport(),
                  "zenoh smoke should not have iox2 transport");

    static_assert(flowrt::CanonicalTransportMessage<WireProbe>);
    static_assert(sizeof(WireProbe) > WireProbe::wire_size());

    auto config = flowrt::zenoh::ZenohChannelConfig::latest().with_stale_config(
        flowrt::StaleConfig{std::chrono::milliseconds{5}, flowrt::StalePolicy::Warn});
    auto endpoint = flowrt::zenoh::ZenohPubSub<WireProbe>::open_with_config(
        "flowrt/runtime/cpp/zenoh_smoke", config);
    assert(endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Ready);

    std::this_thread::sleep_for(std::chrono::milliseconds{200});

    const auto first_write = endpoint.publish_at(WireProbe{1U, 11U}, 100U);
    const auto second_write = endpoint.publish_at(WireProbe{2U, 22U}, 102U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(first_write));
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(second_write));
    assert(WireProbe::encode_calls == 2U);

    for (std::size_t attempt = 0; attempt < 100U; ++attempt) {
        const auto read = endpoint.receive_latest_at(103U);
        if (std::holds_alternative<flowrt::Latest<WireProbe>>(read)) {
            const auto latest = std::get<flowrt::Latest<WireProbe>>(read);
            if (latest.present()) {
                assert(!latest.stale());
                assert(latest.get()->tag == 2U);
                assert(latest.get()->value == 22U);
                break;
            }
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{20});
    }

    assert(WireProbe::decode_calls == 1U);
    const auto stale_read = endpoint.receive_latest_at(108U);
    assert(std::holds_alternative<flowrt::Latest<WireProbe>>(stale_read));
    const auto stale = std::get<flowrt::Latest<WireProbe>>(stale_read);
    assert(stale.present());
    assert(stale.stale());
    assert(stale.get()->value == 22U);

#ifdef FLOWRT_ENABLE_TEST_HOOKS
    endpoint.close_session_for_test();
    assert(!endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Degraded);
    const auto recovered_write = endpoint.publish_at(WireProbe{3U, 33U}, 120U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(recovered_write));
    assert(endpoint.ready());
    assert(endpoint.health().state == flowrt::BackendHealthState::Ready);
    for (std::size_t attempt = 0; attempt < 100U; ++attempt) {
        const auto read = endpoint.receive_latest_at(121U);
        assert(!std::holds_alternative<flowrt::ChannelError>(read));
        const auto latest = std::get<flowrt::Latest<WireProbe>>(read);
        if (latest.present() && latest.get()->tag == 3U) {
            assert(!latest.stale());
            assert(latest.get()->value == 33U);
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{20});
    }
#endif

    {
        auto session =
            std::make_shared<::zenoh::Session>(flowrt::zenoh::open_zenoh_session_from_env());
        auto key_expr = flowrt::zenoh::operation_key_expr("robot", "hash", 42U);
        flowrt::IntrospectionState state;
        state.register_operation_start_handler(
            "controller.plan",
            [state](std::vector<std::uint8_t> payload, std::optional<std::uint64_t> timeout_ms,
                    std::optional<std::string> owner)
                -> std::variant<flowrt::IntrospectionOperationStartStatus, std::string> {
                assert((payload == std::vector<std::uint8_t>{10U, 20U}));
                assert(timeout_ms == std::optional<std::uint64_t>{77U});
                assert(owner == std::optional<std::string>{"cli"});
                state.record_operation_transition(
                    "controller.plan", "222:8:4", "accepted",
                    std::optional<std::string_view>{"controller.plan"},
                    std::optional<std::uint64_t>{123456U});
                return flowrt::IntrospectionOperationStartStatus{
                    .operation_id = "222:8:4",
                    .operation =
                        flowrt::IntrospectionOperationStatus{
                            .name = "controller.plan",
                            .ready = true,
                            .running = 1,
                            .queued = 0,
                            .current_operation_ids = {"222:8:4"},
                            .total_started = 1,
                            .current_state = std::optional<std::string>{"accepted"},
                            .current_owner = std::optional<std::string>{"controller.plan"},
                            .current_deadline_ms = std::optional<std::uint64_t>{123456U},
                        },
                };
            });
        state.register_operation_status_handler(
            "controller.plan",
            [](std::string_view operation_id)
                -> std::variant<flowrt::IntrospectionOperationStatus, std::string> {
                assert(operation_id == "222:8:4");
                return flowrt::IntrospectionOperationStatus{
                    .name = "controller.plan",
                    .ready = true,
                    .running = 1,
                    .queued = 0,
                    .current_operation_ids = {"222:8:4"},
                    .total_started = 1,
                    .current_state = std::optional<std::string>{"running"},
                };
            });
        state.register_operation_cancel_handler(
            "controller.plan",
            [](std::string_view operation_id)
                -> std::variant<flowrt::IntrospectionOperationStatus, std::string> {
                assert(operation_id == "222:8:4");
                return flowrt::IntrospectionOperationStatus{
                    .name = "controller.plan",
                    .ready = true,
                    .running = 0,
                    .queued = 0,
                    .current_operation_ids = {},
                    .total_started = 1,
                    .canceled_count = 1,
                    .current_state = std::optional<std::string>{"cancel_requested"},
                };
            });
        state.record_operation_transition("controller.plan", "333:9:5", "running",
                                          std::optional<std::string_view>{"controller.plan"},
                                          std::optional<std::uint64_t>{50U});
        state.record_operation_progress_payload(
            "controller.plan", "333:9:5", 1U,
            std::optional<std::vector<std::uint8_t>>{std::vector<std::uint8_t>{7U, 6U}});
        state.record_operation_result_payload(
            "controller.plan", "333:9:5", "succeeded", std::nullopt,
            std::optional<std::vector<std::uint8_t>>{std::vector<std::uint8_t>{9U, 8U}});
        auto server = flowrt::zenoh::ZenohOperationServer::open(
            key_expr, session,
            flowrt::IntrospectionHandshake{
                .protocol_version = flowrt::INTROSPECTION_PROTOCOL_VERSION,
                .pid = 42U,
                .started_at_unix_ms = 0U,
                .self_description_hash = "hash",
                .package = "robot",
                .process = "planner",
                .runtime = "cpp",
            },
            state);
        assert(server.ready());

        const auto status_response = query_json(*session, key_expr, R"({"command":"status"})");
        assert(status_response.has_value());
        assert(status_response->find(R"("response":"status")") != std::string::npos);
        assert(status_response->find(R"("runtime":"cpp")") != std::string::npos);

        const auto start_response = query_json(
            *session, key_expr,
            R"({"command":"operation_start","operation":"controller.plan","payload":[10,20],"timeout_ms":77,"owner":"cli"})");
        assert(start_response.has_value());
        assert(start_response->find(R"("response":"operation_started")") != std::string::npos);
        assert(start_response->find(R"("operation_id":"222:8:4")") != std::string::npos);

        const auto operation_status_response = query_json(
            *session, key_expr, R"({"command":"operation_status","operation_id":"222:8:4"})");
        assert(operation_status_response.has_value());
        assert(operation_status_response->find(R"("response":"operation_value")") !=
               std::string::npos);
        assert(operation_status_response->find(R"("current_state":"running")") !=
               std::string::npos);

        const auto cancel_response = query_json(
            *session, key_expr, R"({"command":"operation_cancel","operation_id":"222:8:4"})");
        assert(cancel_response.has_value());
        assert(cancel_response->find(R"("response":"operation_value")") != std::string::npos);
        assert(cancel_response->find(R"("current_state":"cancel_requested")") != std::string::npos);

        const auto result_response = query_json(
            *session, key_expr, R"({"command":"operation_result","operation_id":"333:9:5"})");
        assert(result_response.has_value());
        assert(result_response->find(R"("response":"operation_result")") != std::string::npos);
        assert(result_response->find(R"("state":"succeeded")") != std::string::npos);
        assert(result_response->find(R"("payload":[9,8])") != std::string::npos);

        const auto observe_response = query_json(
            *session, key_expr,
            R"({"command":"operation_observe","operation_id":"333:9:5","after_sequence":0,"limit":8})");
        assert(observe_response.has_value());
        assert(observe_response->find(R"("response":"operation_events")") != std::string::npos);
        assert(observe_response->find(R"("kind":"state")") != std::string::npos);
        assert(observe_response->find(R"("kind":"progress")") != std::string::npos);
        assert(observe_response->find(R"("progress_sequence":1)") != std::string::npos);
        assert(observe_response->find(R"("kind":"result")") != std::string::npos);
        assert(observe_response->find(R"("terminal":true)") != std::string::npos);
    }

    const auto invalid_write = endpoint.publish_at(WireProbe{0xFFU, 99U}, 500U);
    assert(std::holds_alternative<flowrt::ChannelWriteOutcome>(invalid_write));
    bool saw_decode_error = false;
    for (std::size_t attempt = 0; attempt < 100U; ++attempt) {
        const auto read = endpoint.receive_latest_at(126U);
        if (std::holds_alternative<flowrt::ChannelError>(read)) {
            saw_decode_error = true;
            break;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds{20});
    }
    assert(saw_decode_error);
    assert(endpoint.health().state == flowrt::BackendHealthState::Ready);

    const auto preserved_read = endpoint.receive_latest_at(126U);
    assert(std::holds_alternative<flowrt::Latest<WireProbe>>(preserved_read));
    const auto preserved = std::get<flowrt::Latest<WireProbe>>(preserved_read);
    assert(preserved.present());
    assert(preserved.stale());
    assert(preserved.get()->value == 33U);

    return 0;
}
