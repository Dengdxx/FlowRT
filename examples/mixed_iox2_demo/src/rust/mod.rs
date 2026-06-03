use crate::components::Source;
use crate::messages::Sample;

#[derive(Debug, Default)]
struct SourceNode {
    value: u32,
}

impl Source for SourceNode {
    fn on_tick(&mut self, sample: &mut flowrt::Output<Sample>) -> flowrt::Status {
        self.value += 1;
        sample.write(Sample { value: self.value });
        flowrt::Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(Box::new(SourceNode::default()))
}
