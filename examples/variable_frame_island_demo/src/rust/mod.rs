use crate::components::Summarizer;
use crate::messages::{ScanFrame, ScanSummary};

/// 变长帧迁移验证组件：接收 `sequence<f32>` 输入，输出固定大小摘要。
///
/// 示例刻意只输出 count/min/max/mean 这类稳定字段，方便 `flowrt pub --file`
/// 注入 JSONL 后用 `flowrt echo` 做确定性断言。真实系统迁移时可以先采用同样的
/// island 脚手架验证行为，再拆掉 boundary endpoint 回到 strict graph。
#[derive(Default)]
pub struct SummarizerImpl;

impl Summarizer for SummarizerImpl {
    fn on_tick(
        &mut self,
        scan: flowrt::Latest<'_, ScanFrame>,
        summary: &mut flowrt::Output<ScanSummary>,
    ) -> flowrt::Status {
        let Some(scan) = scan.as_ref() else {
            return flowrt::Status::Retry;
        };
        if scan.ranges.is_empty() {
            summary.write(ScanSummary {
                seq: scan.seq,
                count: 0,
                min_milli: 0,
                max_milli: 0,
                mean_milli: 0,
            });
            return flowrt::Status::ok();
        }

        let mut min = scan.ranges[0];
        let mut max = scan.ranges[0];
        let mut sum = 0.0f32;
        for value in &scan.ranges {
            min = min.min(*value);
            max = max.max(*value);
            sum += *value;
        }
        let count = scan.ranges.len() as u32;
        let mean = sum / count as f32;
        summary.write(ScanSummary {
            seq: scan.seq,
            count,
            min_milli: meters_to_milli(min),
            max_milli: meters_to_milli(max),
            mean_milli: meters_to_milli(mean),
        });
        flowrt::Status::ok()
    }
}

fn meters_to_milli(value: f32) -> i32 {
    (value * 1000.0).round() as i32
}

/// 组装应用：用户算法只实现 generated trait，不依赖 boundary 或 backend API。
pub fn build_app() -> crate::App {
    crate::App::new(Box::new(SummarizerImpl))
}
