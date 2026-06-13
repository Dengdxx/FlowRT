use crate::components::{ControlProcessor, PerceptionProcessor};
use crate::messages::{ControlSample, PerceptionSample};

#[derive(Debug, Default)]
struct PerceptionNode {
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

#[derive(Debug, Default)]
struct ControlNode;

impl ControlProcessor for ControlNode {
    fn on_tick(
        &mut self,
        sample: flowrt::Latest<'_, PerceptionSample>,
        command: &mut flowrt::Output<ControlSample>,
    ) -> flowrt::Status {
        let Some(sample) = sample.as_ref() else {
            return flowrt::Status::Retry;
        };
        command.write(ControlSample {
            command: sample.ax * 2.0,
        });
        flowrt::Status::ok()
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(Box::new(PerceptionNode::default()), Box::new(ControlNode))
}
