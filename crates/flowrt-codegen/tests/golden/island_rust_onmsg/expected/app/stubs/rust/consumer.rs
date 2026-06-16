// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Consumer;

impl flowrt_app::components::Consumer for Consumer {
    fn on_tick(
        &mut self,
        sample: flowrt::Latest<'_, flowrt_app::messages::Sample>,
        echo: &mut flowrt::Output<flowrt_app::messages::Sample>,
    ) -> flowrt::Status {
        let _ = sample;
        echo.write(flowrt_app::messages::Sample::default());
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Consumer::default()))
}
