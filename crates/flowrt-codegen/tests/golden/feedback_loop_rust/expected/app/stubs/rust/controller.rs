// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Controller;

impl flowrt_app::components::Controller for Controller {
    fn on_tick(
        &mut self,
        state: flowrt::Latest<'_, flowrt_app::messages::State>,
        cmd: &mut flowrt::Output<flowrt_app::messages::Cmd>,
    ) -> flowrt::Status {
        let _ = state;
        cmd.write(flowrt_app::messages::Cmd::default());
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Controller::default()))
}
