use crate::components::Source;
use crate::messages::TextFrame;

#[derive(Debug, Default)]
struct SourceNode {
    sequence: u64,
}

impl Source for SourceNode {
    fn on_tick(&mut self, text: &mut flowrt::Output<TextFrame>) -> flowrt::Status {
        self.sequence += 1;
        text.write(TextFrame {
            data: format!("flowrt-{}", self.sequence),
        });
        flowrt::Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(Box::new(SourceNode::default()))
}
