use crate::components::Worker;
use crate::messages::Counter;
use flowrt::{Output, Status};

#[derive(Debug, Default)]
struct WorkerNode {
    value: u32,
}

impl Worker for WorkerNode {
    fn on_tick(&mut self, counter: &mut Output<Counter>) -> Status {
        self.value += 1;
        counter.write(Counter { value: self.value });
        Status::Ok
    }
}

pub fn build_app() -> crate::App {
    crate::App::new(Box::new(WorkerNode::default()))
}
