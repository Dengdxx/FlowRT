#include <flowrt_app/runtime_shell.hpp>

#include <iostream>
#include <memory>

namespace {

/// 在 C++ 主机上接收并打印 Rust 进程发布的跨机 frame。
class Sink final : public flowrt_app::SinkInterface {
public:
    auto on_tick(const flowrt::Latest<flowrt_app::CrossHostFrame> &frame)
        -> flowrt::Status override {
        if (!frame.present()) {
            return flowrt::Status::Ok;
        }

        const auto &value = *frame.as_ref();
        std::cout << std::boolalpha
                  << "valid=" << value.valid
                  << " label=" << value.label.view()
                  << " payload_bytes=" << value.payload.size()
                  << " samples=" << value.samples.size()
                  << " temperature=" << value.temperature
                  << '\n';
        return value.valid ? flowrt::Status::Ok : flowrt::Status::Error;
    }
};

}  // namespace

namespace flowrt_user {

/// 组装 C++ 侧应用：仅包含跨机数据消费端。
auto build_app() -> flowrt_app::App {
    return flowrt_app::App(std::make_unique<Sink>());
}

}  // namespace flowrt_user
