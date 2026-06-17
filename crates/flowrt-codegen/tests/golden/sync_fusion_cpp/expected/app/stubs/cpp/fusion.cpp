// FlowRT 管理参考模板（C++）。可删除重建；复制到用户 app/ 后再修改。

#include "flowrt_app/runtime_shell.hpp"

#include <memory>

namespace {

class Fusion final : public flowrt_app::FusionInterface {
public:
    flowrt::Status on_tick(
        const flowrt::Latest<flowrt_app::Imu>& imu,
        const flowrt::Latest<flowrt_app::Odom>& odom,
        flowrt::Output<flowrt_app::Estimate>& estimate) override {
        (void)imu;
        (void)odom;
        estimate.write(flowrt_app::Estimate{});
        return flowrt::ok();
    }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(std::make_unique<Fusion>());
}

}  // namespace flowrt_user
