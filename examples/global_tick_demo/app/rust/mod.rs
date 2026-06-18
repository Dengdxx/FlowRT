use crate::components::{Sink, Source};
use crate::messages::Sample;
use flowrt::{Latest, Output, Status};

#[derive(Default)]
struct SourceNode {
    seq: u64,
}

impl Source for SourceNode {
    fn on_tick(&mut self, sample: &mut Output<Sample>) -> Status {
        self.seq = self.seq.saturating_add(1);
        sample.write(Sample { value: self.seq });
        Status::Ok
    }
}

#[derive(Default)]
struct SinkNode {
    last_seen: u64,
}

impl Sink for SinkNode {
    fn on_tick(&mut self, sample: Latest<'_, Sample>) -> Status {
        if let Some(sample) = sample.as_ref() {
            self.last_seen = sample.value;
        }
        Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(SourceNode::default()),
        Box::new(SinkNode::default()),
    )
}
