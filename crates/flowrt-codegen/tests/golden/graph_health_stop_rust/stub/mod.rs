// 编译网用户代码 stub（crate:: 路径）。flaky(restart)/guard(isolate) 用 Producer，consumer 用 Sink。
#[derive(Default)]
pub struct Producer;

impl crate::components::Producer for Producer {
    fn on_tick(&mut self, sample: &mut flowrt::Output<crate::messages::Sample>) -> flowrt::Status {
        sample.write(crate::messages::Sample::default());
        flowrt::Status::Ok
    }
}

#[derive(Default)]
pub struct Sink;

impl crate::components::Sink for Sink {
    fn on_tick(&mut self, sample: flowrt::Latest<'_, crate::messages::Sample>) -> flowrt::Status {
        let _ = sample;
        flowrt::Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(Producer::default()),
        Box::new(Sink::default()),
        Box::new(Producer::default()),
    )
}
