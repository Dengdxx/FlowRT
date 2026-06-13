use crate::components::Worker;
use crate::messages::Counter;
use flowrt::{Output, Status};

/// 工作节点：每 tick 递增计数器并发布。
///
/// 本示例的 RSDL 定义了两个 profile（default 使用 inproc，iox2 使用 iox2），
/// 同一组件代码在不同 profile 下生成不同的 channel 实现和 backend 绑定。
/// 通过 `flowrt check --profile iox2` 切换验证。
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

/// 组装应用：注入工作节点。
pub fn build_app() -> crate::App {
    crate::App::new(Box::new(WorkerNode::default()))
}
