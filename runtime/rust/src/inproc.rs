use crate::{Context, Scheduler, Status, backend::InprocScheduler};

/// 使用默认 inproc scheduler 连续运行固定数量的步骤。
///
/// 这是测试和最小 demo 的便捷入口。它会按 tick 顺序调用 `step`，并在第一个非 OK 状态处停止。
pub fn run_ticks<F>(ticks: usize, mut step: F) -> Status
where
    F: FnMut(usize, &mut Context) -> Status,
{
    InprocScheduler.run_ticks(ticks, &mut step)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_on_first_failure() {
        let mut seen = 0usize;
        let status = run_ticks(5, |tick, _| {
            seen += 1;
            if tick == 2 { Status::Error } else { Status::Ok }
        });
        assert_eq!(seen, 3);
        assert_eq!(status, Status::Error);
    }
}
