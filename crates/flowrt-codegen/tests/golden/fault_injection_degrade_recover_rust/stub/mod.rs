// 编译网用户代码 stub（crate:: 路径）。monitor 用 Monitor；注入门在 runtime shell 侧短路。
#[derive(Default)]
pub struct Monitor;

impl crate::components::Monitor for Monitor {
    fn on_tick(
        &mut self,
        sample: flowrt::Latest<'_, crate::messages::Sample>,
        echo: &mut flowrt::Output<crate::messages::Sample>,
    ) -> flowrt::Status {
        let _ = sample;
        echo.write(crate::messages::Sample::default());
        flowrt::Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(Box::new(Monitor::default()))
}
