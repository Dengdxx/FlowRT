// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Plant;

impl flowrt_app::components::Plant for Plant {
    fn on_tick(
        &mut self,
        cmd: flowrt::Latest<'_, flowrt_app::messages::Cmd>,
        state: &mut flowrt::Output<flowrt_app::messages::State>,
    ) -> flowrt::Status {
        let _ = cmd;
        state.write(flowrt_app::messages::State::default());
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Plant::default()))
}
