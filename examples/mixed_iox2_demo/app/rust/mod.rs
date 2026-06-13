use crate::components::Source;
use crate::messages::Sample;

/// 数据源：每 tick 递增计数器并通过 iox2 发布到 C++ 进程的 Sink。
///
/// 这是最小的跨语言 iox2 demo——Rust Source 发布，C++ Sink 接收，
/// 验证 iox2 零拷贝传输在 Rust→C++ 方向的正确性。
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

/// 组装 Rust 侧应用：仅包含数据源组件。
pub fn build_app() -> crate::App {
    crate::App::new(Box::new(SourceNode::default()))
}
