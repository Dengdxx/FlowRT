use crate::components::Source;
use crate::messages::CrossHostFrame;

/// 在 Rust 主机上生成变长 frame，由 generated shell 通过 zenoh 发布。
#[derive(Debug, Default)]
struct SourceNode {
    sequence: u32,
}

impl Source for SourceNode {
    fn on_tick(&mut self, frame: &mut flowrt::Output<CrossHostFrame>) -> flowrt::Status {
        self.sequence += 1;
        let label = format!("frame-{}", self.sequence);
        let payload = vec![
            self.sequence as u8,
            self.sequence.saturating_add(1) as u8,
            self.sequence.saturating_add(2) as u8,
        ];
        let samples = vec![
            self.sequence,
            self.sequence + 1,
            self.sequence + 2,
        ];
        frame.write(CrossHostFrame {
            valid: true,
            label,
            payload,
            samples,
            temperature: 20.0 + self.sequence as f32 * 0.25,
        });
        flowrt::Status::Ok
    }
}

/// 组装 Rust 侧应用：仅包含跨机数据源组件。
pub fn build_app() -> crate::App {
    crate::App::new(Box::new(SourceNode::default()))
}
