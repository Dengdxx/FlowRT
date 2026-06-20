// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Source;

impl flowrt_app::components::Source for Source {
    fn on_tick(
        &mut self,
        packet: &mut flowrt::Output<flowrt_app::messages::Packet>,
    ) -> flowrt::Status {
        packet.write(flowrt_app::messages::Packet::default());
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Source::default()))
}
