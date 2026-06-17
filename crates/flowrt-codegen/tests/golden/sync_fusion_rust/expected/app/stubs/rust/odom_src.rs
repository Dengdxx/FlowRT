// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct OdomSrc;

impl flowrt_app::components::OdomSrc for OdomSrc {
    fn on_tick(
        &mut self,
        odom: &mut flowrt::Output<flowrt_app::messages::Odom>,
    ) -> flowrt::Status {
        odom.write(flowrt_app::messages::Odom::default());
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(OdomSrc::default()))
}
