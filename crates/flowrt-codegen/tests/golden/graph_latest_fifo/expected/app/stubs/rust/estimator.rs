// FlowRT 管理参考模板（Rust）。可删除重建；复制到用户 app/ 后再修改。

#[derive(Default)]
pub struct Estimator;

impl flowrt_app::components::Estimator for Estimator {
    fn on_tick(
        &mut self,
        imu: flowrt::Latest<'_, flowrt_app::messages::Imu>,
        odom: &mut flowrt::Output<flowrt_app::messages::Odom>,
    ) -> flowrt::Status {
        let _ = imu;
        odom.write(flowrt_app::messages::Odom::default());
        flowrt::Status::Ok
    }
}

pub fn build_app() -> flowrt_app::runtime_shell::App {
    flowrt_app::runtime_shell::App::new(Box::new(Estimator::default()))
}
