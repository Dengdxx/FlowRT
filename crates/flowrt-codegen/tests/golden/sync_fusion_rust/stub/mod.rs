// 编译网用户代码 stub（crate:: 路径）。生成的参考 stub 用 flowrt_app:: 面向独立 crate；
// 临时内联 user 模块经 #[path] 引入，须改 crate::。
#[derive(Default)]
pub struct ImuSrc;

impl crate::components::ImuSrc for ImuSrc {
    fn on_tick(&mut self, imu: &mut flowrt::Output<crate::messages::Imu>) -> flowrt::Status {
        imu.write(crate::messages::Imu::default());
        flowrt::Status::Ok
    }
}

#[derive(Default)]
pub struct OdomSrc;

impl crate::components::OdomSrc for OdomSrc {
    fn on_tick(&mut self, odom: &mut flowrt::Output<crate::messages::Odom>) -> flowrt::Status {
        odom.write(crate::messages::Odom::default());
        flowrt::Status::Ok
    }
}

#[derive(Default)]
pub struct Fusion;

impl crate::components::Fusion for Fusion {
    fn on_tick(
        &mut self,
        imu: flowrt::Latest<'_, crate::messages::Imu>,
        odom: flowrt::Latest<'_, crate::messages::Odom>,
        estimate: &mut flowrt::Output<crate::messages::Estimate>,
    ) -> flowrt::Status {
        let _ = (imu, odom);
        estimate.write(crate::messages::Estimate::default());
        flowrt::Status::Ok
    }
}

#[derive(Default)]
pub struct Sink;

impl crate::components::Sink for Sink {
    fn on_tick(&mut self, estimate: flowrt::Latest<'_, crate::messages::Estimate>) -> flowrt::Status {
        let _ = estimate;
        flowrt::Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(ImuSrc::default()),
        Box::new(OdomSrc::default()),
        Box::new(Fusion::default()),
        Box::new(Sink::default()),
    )
}
