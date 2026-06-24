#[path = "../perception/rust/processor.rs"]
mod perception_processor;

pub fn build_app() -> crate::App {
    crate::App::new(Box::new(perception_processor::PerceptionNode::default()))
}
