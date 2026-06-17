// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Producer;

impl flowrt_app::components::Producer for Producer {
    fn on_tick(
        &mut self,
        sample: &mut flowrt::Output<flowrt_app::messages::Sample>,
    ) -> flowrt::Status {
        sample.write(flowrt_app::messages::Sample::default());
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Producer::default()))
}
