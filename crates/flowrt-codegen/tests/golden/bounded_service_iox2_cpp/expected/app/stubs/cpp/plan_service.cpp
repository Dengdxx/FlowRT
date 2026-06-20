// FlowRT 管理参考模板（C++）。可删除重建；复制到用户 app/ 后再修改。

#include "flowrt_app/runtime_shell.hpp"

#include <memory>

namespace {

class PlanService final : public flowrt_app::PlanServiceInterface {
public:
    flowrt::ServiceResult<flowrt_app::PlanResponse> on_plan_request(const flowrt_app::PlanRequest& request) override {
        (void)request;
        return flowrt::ServiceResult<flowrt_app::PlanResponse>::err(flowrt::ServiceError::HandlerError);
    }

    flowrt::Status on_tick() override {
        return flowrt::ok();
    }
};

}  // namespace

namespace flowrt_user {

flowrt_app::App build_app() {
    return flowrt_app::App(std::make_unique<PlanService>());
}

}  // namespace flowrt_user
