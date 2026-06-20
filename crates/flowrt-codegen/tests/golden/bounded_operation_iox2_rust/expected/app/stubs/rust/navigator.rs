// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Navigator;

impl flowrt_app::components::Navigator for Navigator {
    fn on_plan_operation(
        &mut self,
        goal: &flowrt_app::messages::PlanGoal,
        cancel: flowrt::OperationCancelToken,
        progress: &mut flowrt::OperationProgressPublisher<flowrt_app::messages::PlanFeedback>,
    ) -> flowrt::OperationHandlerResult<flowrt_app::messages::PlanResult> {
        let _ = (goal, cancel, progress);
        flowrt::OperationHandlerResult::failed()
    }

    fn on_tick(&mut self) -> flowrt::Status {
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Navigator::default()))
}
