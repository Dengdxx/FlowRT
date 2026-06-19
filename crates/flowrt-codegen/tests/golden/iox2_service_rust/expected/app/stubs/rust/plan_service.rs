// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct PlanService;

impl flowrt_app::components::PlanService for PlanService {
    fn on_plan_request(
        &mut self,
        request: &flowrt_app::messages::PlanRequest,
    ) -> flowrt::ServiceResult<flowrt_app::messages::PlanResponse> {
        let _ = request;
        flowrt::ServiceResult::err(flowrt::ServiceError::HandlerError)
    }

    fn on_tick(&mut self) -> flowrt::Status {
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(PlanService::default()))
}
