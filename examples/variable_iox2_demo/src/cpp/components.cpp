#include <flowrt_app/runtime_shell.hpp>
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
        if (!value.valid || value.label.empty() || value.payload.empty() || value.samples.empty()) {
            return flowrt::Status::Error;
        }
        return flowrt::Status::Ok;
    }
};

}  // namespace

namespace flowrt_user {

/// 组装 C++ 侧应用：仅包含 iox2 变长消息消费端。
auto build_app() -> flowrt_app::App { return flowrt_app::App(std::make_unique<Sink>()); }

}  // namespace flowrt_user
