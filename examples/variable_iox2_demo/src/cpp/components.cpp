#include <cstdlib>
#include <cstdint>
#include <flowrt_app/runtime_shell.hpp>
#include <fstream>
#include <memory>

namespace {

/// C++ 消费端：接收 Rust 进程通过 iox2 fixed slot 携带的 bounded variable frame。
class Sink final : public flowrt_app::SinkInterface {
   public:
    auto on_tick(const flowrt::Latest<flowrt_app::Packet> &packet) -> flowrt::Status override {
        if (!packet.present()) {
            return flowrt::Status::Ok;
        }

        const auto &value = *packet.as_ref();
        const auto payload = value.payload.as_span();
        const auto samples = value.samples.as_span();
        if (!value.valid || value.label.view().rfind("packet-", 0) != 0 || payload.size() != 3 ||
            samples.size() != 3 || samples[1] != samples[0] + 1 || samples[2] != samples[0] + 2 ||
            payload[0] != static_cast<std::uint8_t>(samples[0])) {
            return flowrt::Status::Error;
        }
        if (const char *path = std::getenv("FLOWRT_VARIABLE_IOX2_SAW_PACKET_PATH")) {
            std::ofstream marker(path);
            if (!marker) {
                return flowrt::Status::Error;
            }
            marker << value.label.view() << '\n';
        }
        return flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

/// 组装 C++ 侧应用：仅包含 iox2 变长消息消费端。
auto build_app() -> flowrt_app::App { return flowrt_app::App(std::make_unique<Sink>()); }

}  // namespace flowrt_user
