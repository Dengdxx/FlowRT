use crate::components::Source;
use crate::messages::Packet;

/// Rust 数据源：生成 bounded variable frame，由 generated shell 编码进 iox2 fixed slot。
#[derive(Debug, Default)]
struct SourceNode {
    sequence: u32,
}

impl Source for SourceNode {
    fn on_tick(&mut self, packet: &mut flowrt::Output<Packet>) -> flowrt::Status {
        self.sequence += 1;
        let label = flowrt::BoundedString::<64>::try_from_str(&format!("packet-{}", self.sequence))
            .unwrap();
        let payload = flowrt::BoundedBytes::<32>::try_from_slice(&[
            self.sequence as u8,
            self.sequence.saturating_add(1) as u8,
            self.sequence.saturating_add(2) as u8,
        ])
        .unwrap();
        let samples = flowrt::BoundedSequence::<u32, 8>::try_from_vec(vec![
            self.sequence,
            self.sequence + 1,
            self.sequence + 2,
        ])
        .unwrap();
        packet.write(Packet {
            valid: true,
            label,
            payload,
            samples,
            temperature: 18.0 + self.sequence as f32 * 0.5,
        });
        flowrt::Status::Ok
    }
}

/// 组装 Rust 侧应用：仅包含 iox2 变长消息数据源。
pub fn build_app() -> crate::App {
    crate::App::new(Box::new(SourceNode::default()))
}
