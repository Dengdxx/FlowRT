use crate::components::{Actuator, Controller, Stimulus};
use crate::messages::Sample;
use flowrt::{Latest, Output, Status};

#[derive(Default)]
struct ControllerNode {
    seq: u32,
}

impl Controller for ControllerNode {
    fn on_tick(&mut self, cmd: &mut Output<Sample>) -> Status {
        self.seq = self.seq.saturating_add(1);
        cmd.write(Sample { value: self.seq });
        Status::ok()
    }
}

#[derive(Default)]
struct ActuatorNode;

impl Actuator for ActuatorNode {
    fn on_tick(&mut self, cmd: Latest<'_, Sample>) -> Status {
        if cmd.as_ref().is_none() {
            return Status::Retry;
        }
        Status::ok()
    }
}

#[derive(Default)]
struct StimulusNode;

impl Stimulus for StimulusNode {
    fn on_tick(&mut self, sample: Latest<'_, Sample>) -> Status {
        let _ = sample;
        Status::ok()
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(
        Box::new(ControllerNode::default()),
        Box::new(ActuatorNode),
        Box::new(ControllerNode::default()),
        Box::new(StimulusNode),
    )
}
