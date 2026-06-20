// FlowRT 管理参考模板（C++）。可删除重建；复制到用户 app/ 后再修改。

#include "flowrt_app/runtime_shell.hpp"

#include <memory>

namespace {

class Navigator final : public flowrt_app::NavigatorInterface {
public:
    flowrt::OperationHandlerResult<flowrt_app::PlanResult> on_plan_operation(
        const flowrt_app::PlanGoal& goal,
        flowrt::OperationCancelToken cancel,
        flowrt::OperationProgressPublisher<flowrt_app::PlanFeedback>& progress) override {
        (void)goal;
        (void)cancel;
        (void)progress;
        return flowrt::OperationHandlerResult<flowrt_app::PlanResult>::failed();
    }

    flowrt::Status on_tick() override {
        return flowrt::ok();
    }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(std::make_unique<Navigator>());
}

}  // namespace flowrt_user
