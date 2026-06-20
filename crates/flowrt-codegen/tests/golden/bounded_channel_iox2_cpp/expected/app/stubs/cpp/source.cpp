// FlowRT 管理参考模板（C++）。可删除重建；复制到用户 app/ 后再修改。

#include "flowrt_app/runtime_shell.hpp"

#include <memory>

namespace {

class Source final : public flowrt_app::SourceInterface {
public:
    flowrt::Status on_tick(
        flowrt::Output<flowrt_app::Packet>& packet) override {
        packet.write(flowrt_app::Packet{});
        return flowrt::ok();
    }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(std::make_unique<Source>());
}

}  // namespace flowrt_user
