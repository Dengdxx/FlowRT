/// 跨进程 smoke：C++ zenoh service client 端。
///
/// 用法：./zenoh_service_client
/// 环境变量 FLOWRT_ZENOH_SERVICE_NAME 可覆盖 service name。

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <flowrt/runtime.hpp>
#include <string>

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

    auto client = flowrt::zenoh::ZenohServiceClient<AddRequest, AddResponse>::open(name, session);

    std::fprintf(stderr, "[cpp-client] calling service '%s'...\n", name.c_str());

    auto result = client.call(AddRequest{10, 20}, 5000);
    if (result.is_ok()) {
        std::fprintf(stderr, "[cpp-client] OK: 10 + 20 = %d\n", result.value()->sum);
        if (result.value()->sum != 30) {
            std::fprintf(stderr, "[cpp-client] FAIL: expected 30, got %d\n", result.value()->sum);
            return 1;
        }
        std::fprintf(stderr, "[cpp-client] PASS\n");
        return 0;
    } else {
        std::fprintf(stderr, "[cpp-client] FAIL: code=%d msg=%s\n",
                     static_cast<int>(result.error_code()),
                     result.error_message().value_or("").c_str());
        return 1;
    }
}
