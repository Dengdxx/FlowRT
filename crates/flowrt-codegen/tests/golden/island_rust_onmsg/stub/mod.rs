// 编译网用户代码 stub（crate:: 路径）。生成的参考 stub 用 flowrt_app:: 面向独立 crate；
// 临时 island 经 #[path] 内联 user 模块，须改 crate::。
#[derive(Default)]
pub struct Consumer;

impl crate::components::Consumer for Consumer {
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
    crate::App::new(Box::new(Consumer::default()))
}
