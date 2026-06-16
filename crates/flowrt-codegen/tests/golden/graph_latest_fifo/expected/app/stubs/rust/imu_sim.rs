// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct ImuSim;

impl flowrt_app::components::ImuSim for ImuSim {
    fn on_tick(
        &mut self,
        imu: &mut flowrt::Output<flowrt_app::messages::Imu>,
    ) -> flowrt::Status {
        imu.write(flowrt_app::messages::Imu::default());
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(ImuSim::default()))
}
