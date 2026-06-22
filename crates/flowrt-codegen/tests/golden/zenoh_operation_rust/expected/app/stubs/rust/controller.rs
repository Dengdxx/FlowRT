// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Controller;

impl flowrt_app::components::Controller for Controller {
    fn on_tick(
        &mut self,
        _plan: &flowrt_app::components::OperationClient_controller_plan,
    ) -> flowrt::Status {
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Controller::default()))
}
