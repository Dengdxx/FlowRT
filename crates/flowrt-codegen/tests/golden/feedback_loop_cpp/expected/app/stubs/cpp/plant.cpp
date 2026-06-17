// FlowRT 管理参考模板（C++）。可删除重建；复制到用户 app/ 后再修改。

#include "flowrt_app/runtime_shell.hpp"

#include <memory>

namespace {

class Plant final : public flowrt_app::PlantInterface {
public:
    flowrt::Status on_tick(
        const flowrt::Latest<flowrt_app::Cmd>& cmd,
        flowrt::Output<flowrt_app::State>& state) override {
        (void)cmd;
        state.write(flowrt_app::State{});
        return flowrt::ok();
    }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(std::make_unique<Plant>());
}

}  // namespace flowrt_user
