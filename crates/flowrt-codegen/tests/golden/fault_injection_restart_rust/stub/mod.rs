// 编译网用户代码 stub（crate:: 路径）。flaky 用 Flaky；注入门在 runtime shell 侧短路，
// 用户回调本身只做最小写出。
#[derive(Default)]
pub struct Flaky;

impl crate::components::Flaky for Flaky {
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
    crate::App::new(Box::new(Flaky::default()))
}
