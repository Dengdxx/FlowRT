/// inproc service request/response runtime C++ smoke 测试。
///
/// 覆盖：基本 request/response、timeout、server unavailable、queue 满 Busy、
/// late response 不污染、same-lane WouldDeadlock、非阻塞 poll/ready、
/// on_request_arrived 回调、统计计数、handler 异常和业务错误。

#include <cassert>
#include <cstdint>
#include <flowrt/runtime.hpp>
#include <memory>
#include <string>
#include <thread>
#include <vector>

struct AddRequest {
    std::int32_t a{};
    std::int32_t b{};
};

struct AddResponse {
    std::int32_t sum{};
};

struct EchoRequest {
    std::string payload;
};

struct EchoResponse {
    std::string payload;
};

int main() {
    auto &registry = flowrt::InprocServiceRegistry::instance();
    registry.clear();

    // ── 基本 request/response ────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "add_service",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        assert(registry.has_service("add_service"));

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("add_service", server);

        auto handle = client.start_call(AddRequest{3, 5});
        assert(server.pending_count() == 1);

        const auto processed = server.process_pending();
        assert(processed == 1);
        assert(server.pending_count() == 0);

        auto result = handle.wait();
        assert(result.is_ok());
        assert(result.value()->sum == 8);
    }

    registry.clear();

    // ── blocking call 返回 ServiceResult ───────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "blocking_add",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("blocking_add", server);

        std::thread worker([&server]() {
            while (server.pending_count() == 0) {
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            server.process_pending();
        });

        auto result = client.call(AddRequest{4, 9}, 1000);
        worker.join();

        assert(result.is_ok());
        assert(result.value()->sum == 13);
    }

    registry.clear();

    // ── 多请求批处理 ─────────────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "batch_add",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("batch_add", server);

        auto h1 = client.start_call(AddRequest{1, 2});
        auto h2 = client.start_call(AddRequest{10, 20});
        auto h3 = client.start_call(AddRequest{100, 200});
        assert(server.pending_count() == 3);

        const auto processed = server.process_pending();
        assert(processed == 3);

        assert(h1.wait().value()->sum == 3);
        assert(h2.wait().value()->sum == 30);
        assert(h3.wait().value()->sum == 300);
    }

    registry.clear();

    // ── timeout ──────────────────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 50;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "slow_service",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("slow_service", server);

        auto handle = client.start_call(AddRequest{1, 1}, 20);
        assert(!handle.ready());

        auto result = handle.wait();
        assert(result.is_err());
        assert(result.error_code() == flowrt::ServiceError::Timeout);

        // 超时后 done=true，late response 不会写入
        assert(handle.ready());
        assert(handle.poll());

        auto stats = server.stats();
        assert(stats.timeouts == 1);
    }

    registry.clear();

    // ── server 不可用（server 已销毁）─────────────────────────────────────────

    {
        std::shared_ptr<flowrt::detail::InprocServiceState> orphan_state;
        std::string service_name = "dead_service";

        {
            flowrt::InprocServiceConfig config;
            config.default_timeout_ms = 500;

            flowrt::InprocServiceServer<AddRequest, AddResponse> server(
                service_name,
                [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                    return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
                },
                config);

            orphan_state = server.shared_state();
            assert(registry.has_service(service_name));
        }

        assert(!registry.has_service(service_name));
        assert(!orphan_state->available);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client(service_name, orphan_state, 0,
                                                                    0);
        auto handle = client.start_call(AddRequest{1, 1});
        assert(handle.poll());
        auto result = handle.wait();
        assert(result.is_err());
        assert(result.error_code() == flowrt::ServiceError::Unavailable);

        auto stats = orphan_state->stats;
        assert(stats.unavailable == 1);
    }

    registry.clear();

    // ── queue 满返回 Busy ────────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.queue_depth = 2;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "tiny_queue",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("tiny_queue", server);

        auto h1 = client.start_call(AddRequest{1, 1});
        auto h2 = client.start_call(AddRequest{2, 2});
        assert(server.pending_count() == 2);

        auto h3 = client.start_call(AddRequest{3, 3});
        assert(h3.poll());
        auto busy_result = h3.wait();
        assert(busy_result.is_err());
        assert(busy_result.error_code() == flowrt::ServiceError::Busy);

        auto stats = server.stats();
        assert(stats.busy == 1);
    }

    registry.clear();

    // ── max_in_flight 满返回 Busy ────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.queue_depth = 10;
        config.max_in_flight = 1;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "low_concurrency",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("low_concurrency", server);

        auto h1 = client.start_call(AddRequest{1, 1});
        assert(server.in_flight_count() == 1);

        auto h2 = client.start_call(AddRequest{2, 2});
        assert(h2.poll());
        auto busy_result = h2.wait();
        assert(busy_result.error_code() == flowrt::ServiceError::Busy);

        server.process_pending();
        assert(server.in_flight_count() == 0);

        auto result1 = h1.wait();
        assert(result1.is_ok());
        assert(result1.value()->sum == 2);
    }

    registry.clear();

    // ── late response 不污染下一次 request，late_dropped 计数 ──────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "no_pollution",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("no_pollution", server);

        auto h1 = client.start_call(AddRequest{10, 20}, 1);
        auto r1 = h1.wait();
        assert(r1.is_err());
        assert(r1.error_code() == flowrt::ServiceError::Timeout);
        assert(h1.ready());

        // server 处理：h1 的 deliver 会检测到 done=true 并递增 late_dropped
        server.process_pending();
        auto stats1 = server.stats();
        assert(stats1.late_dropped == 1);

        auto h2 = client.start_call(AddRequest{100, 200});
        server.process_pending();
        auto r2 = h2.wait();
        assert(r2.is_ok());
        assert(r2.value()->sum == 300);
    }

    registry.clear();

    // ── same-lane blocking call 返回 WouldDeadlock ───────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "lane_bound",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client(
            "lane_bound", server, /*caller_lane=*/5, /*server_lane=*/5);

        auto handle = client.start_call(AddRequest{1, 1});
        assert(handle.poll());
        auto result = handle.wait();
        assert(result.is_err());
        assert(result.error_code() == flowrt::ServiceError::WouldDeadlock);

        auto stats = server.stats();
        assert(stats.deadlocks == 1);
    }

    registry.clear();

    // ── 非阻塞 poll / ready ──────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "poll_service",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("poll_service", server);

        auto handle = client.start_call(AddRequest{7, 8});
        assert(!handle.ready());
        assert(!handle.poll());

        server.process_pending();
        assert(handle.ready());

        assert(handle.poll());
        auto polled = handle.wait();
        assert(polled.is_ok());
        assert(polled.value()->sum == 15);
    }

    registry.clear();

    // ── ready error handle ──────────────────────────────────────────────────

    {
        auto handle =
            flowrt::InprocServiceHandle<AddResponse>::ready_error(flowrt::ServiceError::Backend);
        assert(handle.ready());
        assert(handle.poll());
        auto result = handle.wait();
        assert(result.is_err());
        assert(result.error_code() == flowrt::ServiceError::Backend);
    }

    registry.clear();

    // ── on_request_arrived 回调 ───────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;
        std::size_t callback_count = 0;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "callback_service",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config, [&callback_count]() { ++callback_count; });

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("callback_service", server);

        client.start_call(AddRequest{1, 1});
        assert(callback_count == 1);

        client.start_call(AddRequest{2, 2});
        assert(callback_count == 2);

        server.process_pending();
        assert(callback_count == 2);
    }

    registry.clear();

    // ── handler 抛异常返回 HandlerError ──────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "throwing_service",
            [](const AddRequest &) -> flowrt::ServiceResult<AddResponse> {
                throw std::runtime_error("handler exploded");
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("throwing_service", server);

        auto handle = client.start_call(AddRequest{1, 1});
        server.process_pending();

        auto result = handle.wait();
        assert(result.is_err());
        assert(result.error_code() == flowrt::ServiceError::HandlerError);
        assert(result.error_message().has_value());
        assert(result.error_message()->find("exploded") != std::string::npos);
    }

    registry.clear();

    // ── handler 返回业务错误 ──────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "biz_error_service",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                if (req.a < 0) {
                    return flowrt::ServiceResult<AddResponse>::err_with_message(
                        flowrt::ServiceError::Rejected, "negative input");
                }
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("biz_error_service", server);

        auto h1 = client.start_call(AddRequest{-1, 5});
        server.process_pending();
        auto r1 = h1.wait();
        assert(r1.is_err());
        assert(r1.error_code() == flowrt::ServiceError::Rejected);

        auto h2 = client.start_call(AddRequest{3, 5});
        server.process_pending();
        auto r2 = h2.wait();
        assert(r2.is_ok());
        assert(r2.value()->sum == 8);
    }

    registry.clear();

    // ── 异步线程中 wait ───────────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "async_service",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("async_service", server);

        auto handle = client.start_call(AddRequest{40, 60});

        std::thread waiter([&handle]() {
            auto result = handle.wait();
            assert(result.is_ok());
            assert(result.value()->sum == 100);
        });

        std::this_thread::sleep_for(std::chrono::milliseconds(5));
        server.process_pending();
        waiter.join();
    }

    registry.clear();

    // ── Echo 类型（非平凡类型）─────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<EchoRequest, EchoResponse> server(
            "echo_service",
            [](const EchoRequest &req) -> flowrt::ServiceResult<EchoResponse> {
                return flowrt::ServiceResult<EchoResponse>::ok(EchoResponse{req.payload});
            },
            config);

        flowrt::InprocServiceClient<EchoRequest, EchoResponse> client("echo_service", server);

        auto handle = client.start_call(EchoRequest{"hello world"});
        server.process_pending();
        auto result = handle.wait();
        assert(result.is_ok());
        assert(result.value()->payload == "hello world");
    }

    registry.clear();

    // ── 统计计数 ──────────────────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.queue_depth = 2;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "stats_service",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        // 不同 lane 的 client：正常调用
        flowrt::InprocServiceClient<AddRequest, AddResponse> normal_client(
            "stats_service", server, /*caller_lane=*/1, /*server_lane=*/2);

        auto h1 = normal_client.start_call(AddRequest{1, 1});
        auto h2 = normal_client.start_call(AddRequest{2, 2});

        server.process_pending();
        h1.wait();
        h2.wait();

        // 同 lane 的 client：触发死锁检测
        flowrt::InprocServiceClient<AddRequest, AddResponse> same_lane_client(
            "stats_service", server, /*caller_lane=*/3, /*server_lane=*/3);

        auto deadlock_handle = same_lane_client.start_call(AddRequest{4, 4});
        assert(deadlock_handle.poll());

        auto stats = server.stats();
        assert(stats.completed == 2);
        assert(stats.deadlocks == 1);
        assert(stats.busy == 0);
    }

    registry.clear();

    // ── 默认配置验证 ──────────────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        assert(config.queue_depth == 32);
        assert(config.max_in_flight == 64);
        assert(config.default_timeout_ms == 5000);
    }

    {
        flowrt::InprocServiceStats stats;
        assert(stats.completed == 0);
        assert(stats.timeouts == 0);
        assert(stats.unavailable == 0);
        assert(stats.busy == 0);
        assert(stats.late_dropped == 0);
        assert(stats.deadlocks == 0);
        stats.completed = 5;
        stats.reset();
        assert(stats.completed == 0);
    }

    registry.clear();

    // ── queue_depth=1 边界 ───────────────────────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.queue_depth = 1;
        config.max_in_flight = 1;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "single_slot",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("single_slot", server);

        auto h1 = client.start_call(AddRequest{1, 1});
        assert(server.pending_count() == 1);

        auto h2 = client.start_call(AddRequest{2, 2});
        assert(h2.poll());
        auto busy = h2.wait();
        assert(busy.error_code() == flowrt::ServiceError::Busy);

        server.process_pending();
        auto r1 = h1.wait();
        assert(r1.is_ok());
        assert(r1.value()->sum == 2);

        // 处理完成后可以再次调用
        auto h3 = client.start_call(AddRequest{10, 20});
        server.process_pending();
        auto r3 = h3.wait();
        assert(r3.is_ok());
        assert(r3.value()->sum == 30);
    }

    registry.clear();

    // ── timeout=0 回退到 default_timeout_ms ──────────────────────────────────

    {
        flowrt::InprocServiceConfig config;
        config.default_timeout_ms = 1000;

        flowrt::InprocServiceServer<AddRequest, AddResponse> server(
            "default_timeout",
            [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
                return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
            },
            config);

        flowrt::InprocServiceClient<AddRequest, AddResponse> client("default_timeout", server);

        // timeout_ms=0 应使用 default_timeout_ms（1000ms），不会立即超时
        auto handle = client.start_call(AddRequest{5, 5}, 0);
        server.process_pending();
        auto result = handle.wait();
        assert(result.is_ok());
        assert(result.value()->sum == 10);
    }

    registry.clear();

    return 0;
}
