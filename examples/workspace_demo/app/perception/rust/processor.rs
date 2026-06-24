use crate::components::PerceptionProcessor;
use crate::messages::PerceptionSample;

#[derive(Debug, Default)]
pub(crate) struct PerceptionNode {
    tick: u64,
}

impl PerceptionProcessor for PerceptionNode {
    fn on_tick(&mut self, sample: &mut flowrt::Output<PerceptionSample>) -> flowrt::Status {
        self.tick += 1;
        sample.write(PerceptionSample {
            timestamp: self.tick,
            ax: self.tick as f32 * 0.1,
        });
        flowrt::Status::ok()
    }
}
