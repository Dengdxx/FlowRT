use crate::components::Processor;
use crate::messages::{ProcessedSample, Sample};

/// 单功能单位处理器：从 boundary input 读取样本，并把结果写到 boundary output。
///
/// 该实现没有依赖完整 graph 的上游或下游组件，用于验证 Island Mode 下的
/// `flowrt pub -> component -> flowrt echo` 闭环。删除 boundary endpoint 并改回
/// strict graph 后，组件 trait 和算法代码保持不变。
#[derive(Default)]
pub struct ProcessorImpl;

impl Processor for ProcessorImpl {
    fn on_tick(
        &mut self,
        sample: flowrt::Latest<'_, Sample>,
        result: &mut flowrt::Output<ProcessedSample>,
    ) -> flowrt::Status {
        let Some(sample) = sample.as_ref() else {
            return flowrt::Status::Retry;
        };
        result.write(ProcessedSample {
            seq: sample.seq,
            doubled: sample.value.saturating_mul(2),
        });
        flowrt::Status::ok()
    }
}

/// 组装应用：Island Mode 只改变边界接线，不改变用户组件注入方式。
pub fn build_app() -> crate::App {
    crate::App::new(Box::new(ProcessorImpl))
}
