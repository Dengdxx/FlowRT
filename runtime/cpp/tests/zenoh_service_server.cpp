/// 跨进程 smoke：C++ zenoh service server 端。
///
/// 用法：./zenoh_service_server
/// 环境变量 FLOWRT_ZENOH_SERVICE_NAME 可覆盖 service name。

#include <chrono>
#include <cstdint>
#include <cstdio>
#include <flowrt/runtime.hpp>
#include <string>
#include <thread>

struct AddRequest {
    std::int32_t a{};
    std::int32_t b{};

    static constexpr std::size_t wire_size() noexcept {
        return sizeof(std::int32_t) + sizeof(std::int32_t);
    }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0, a);
        flowrt::write_wire_le(output, sizeof(std::int32_t), b);
    }

    static AddRequest decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        return AddRequest{flowrt::read_wire_le<std::int32_t>(input, 0),
                          flowrt::read_wire_le<std::int32_t>(input, sizeof(std::int32_t))};
    }
};

struct AddResponse {
    std::int32_t sum{};

    static constexpr std::size_t wire_size() noexcept { return sizeof(std::int32_t); }

    void encode_wire(std::span<std::uint8_t> output) const {
        flowrt::ensure_wire_size(wire_size(), output.size());
        flowrt::write_wire_le(output, 0, sum);
    }

    static AddResponse decode_wire(std::span<const std::uint8_t> input) {
        flowrt::ensure_wire_size(wire_size(), input.size());
        return AddResponse{flowrt::read_wire_le<std::int32_t>(input, 0)};
    }
};

int main() {
    const char *name_env = std::getenv("FLOWRT_ZENOH_SERVICE_NAME");
    std::string name = name_env ? name_env : "flowrt/cross_lang/add";

    auto session = std::make_shared<::zenoh::Session>(flowrt::zenoh::open_zenoh_session_from_env());

    auto server = flowrt::zenoh::ZenohServiceServer<AddRequest, AddResponse>::open(
        name, session, [](const AddRequest &req) -> flowrt::ServiceResult<AddResponse> {
            return flowrt::ServiceResult<AddResponse>::ok(AddResponse{req.a + req.b});
        });

    std::fprintf(stderr, "[cpp-server] listening on service '%s'\n", name.c_str());
    std::fprintf(stderr, "[cpp-server] waiting for requests... (Ctrl+C to quit)\n");

    while (true) {
        std::this_thread::sleep_for(std::chrono::seconds{1});
    }
}
