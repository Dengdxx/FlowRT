// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Sink;

impl flowrt_app::components::Sink for Sink {
    fn on_tick(
        &mut self,
        estimate: flowrt::Latest<'_, flowrt_app::messages::Estimate>,
    ) -> flowrt::Status {
        let _ = estimate;
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Sink::default()))
}
