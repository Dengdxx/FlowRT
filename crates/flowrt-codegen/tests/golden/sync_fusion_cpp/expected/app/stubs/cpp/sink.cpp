// FlowRT 管理参考模板（C++）。可删除重建；复制到用户 app/ 后再修改。

#include "flowrt_app/runtime_shell.hpp"

#include <memory>

namespace {

class Sink final : public flowrt_app::SinkInterface {
public:
    flowrt::Status on_tick(
        const flowrt::Latest<flowrt_app::Estimate>& estimate) override {
        (void)estimate;
        return flowrt::ok();
    }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(std::make_unique<Sink>());
}

}  // namespace flowrt_user
