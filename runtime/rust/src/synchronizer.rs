//! 多传感器同步原语：把 N 路输入按 sample-time 对齐成同步集。
//!
//! v1 采用 latest-aligned approx-window 策略：当所有输入的最新样本落在一个
//! `tolerance` 窗口内时发射一组对齐样本，否则丢弃落后输入的陈旧 backlog 等待其
//! 追平。算法只依赖样本 sample-time，不关心时间来源，因此 realtime 与 replay
//! 行为一致、跨语言（Rust/C++）位级确定。时间与 tolerance 使用同一整数单位
//! （codegen 传入 ns）。
//!
//! v1 限制：非最优匹配（不做 ROS2 ApproximateTime 式全局最优配对），late 样本
//! 一律丢弃（DropLate）。

use std::collections::VecDeque;

/// N 路输入的同步器。`T` 为对齐投递给用户回调的 typed 样本。
#[derive(Debug)]
pub struct Synchronizer<T> {
    tolerance: u64,
    capacity: usize,
    buffers: Vec<VecDeque<(u64, T)>>,
    watermark: Vec<Option<u64>>,
}

impl<T: Clone> Synchronizer<T> {
    /// 构造一个 `input_count` 路、每路 buffer 容量 `capacity`、窗口宽度 `tolerance`
    /// （与样本时间同单位）的同步器。
    pub fn new(input_count: usize, capacity: usize, tolerance: u64) -> Self {
        assert!(input_count >= 1, "synchronizer requires at least one input");
        assert!(
            capacity >= 1,
            "synchronizer buffer capacity must be positive"
        );
        Self {
            tolerance,
            capacity,
            buffers: (0..input_count).map(|_| VecDeque::new()).collect(),
            watermark: vec![None; input_count],
        }
    }

    /// 输入路数。
    pub fn input_count(&self) -> usize {
        self.buffers.len()
    }

    /// 第 `input` 路当前缓冲样本数（用于诊断与测试）。
    pub fn buffered(&self, input: usize) -> usize {
        self.buffers[input].len()
    }

    /// 接收第 `input` 路一个 sample-time 为 `time` 的样本。
    ///
    /// late 样本（时间不晚于该路上次发射窗口）按 DropLate 丢弃；buffer 满则丢最旧。
    pub fn push(&mut self, input: usize, time: u64, value: T) {
        if let Some(watermark) = self.watermark[input]
            && time <= watermark
        {
            return;
        }
        let buffer = &mut self.buffers[input];
        if buffer.len() == self.capacity {
            buffer.pop_front();
        }
        buffer.push_back((time, value));
    }

    /// 尝试发射一组对齐样本。返回 `Some(每路一个样本)` 或 `None`（暂无可发集）。
    pub fn poll(&mut self) -> Option<Vec<T>> {
        loop {
            if self.buffers.iter().any(VecDeque::is_empty) {
                return None;
            }
            let latest: Vec<u64> = self
                .buffers
                .iter()
                .map(|buffer| buffer.back().expect("non-empty checked above").0)
                .collect();
            let max = *latest.iter().max().expect("input_count >= 1");
            let min = *latest.iter().min().expect("input_count >= 1");
            if max - min <= self.tolerance {
                let mut set = Vec::with_capacity(self.buffers.len());
                for (input, buffer) in self.buffers.iter_mut().enumerate() {
                    let (time, value) = buffer.back().expect("non-empty checked above").clone();
                    self.watermark[input] = Some(time);
                    buffer.clear();
                    set.push(value);
                }
                return Some(set);
            }
            let laggard = latest
                .iter()
                .enumerate()
                .min_by_key(|(index, time)| (**time, *index))
                .map(|(index, _)| index)
                .expect("input_count >= 1");
            self.buffers[laggard].pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 以下用例即跨语言 conformance golden 向量：C++ synchronizer 单测必须对同一
    // 事件序列产出相同的同步集序列（参见 runtime/cpp/tests/synchronizer_smoke.cpp）。

    #[test]
    fn aligned_latest_samples_emit_one_set() {
        let mut sync = Synchronizer::new(2, 8, 10);
        sync.push(0, 100, 100);
        sync.push(1, 105, 105);
        assert_eq!(sync.poll(), Some(vec![100, 105]));
        assert_eq!(sync.poll(), None);
    }

    #[test]
    fn spread_exceeded_drains_laggard_then_recovers() {
        let mut sync = Synchronizer::new(2, 8, 10);
        sync.push(0, 100, 100);
        sync.push(1, 130, 130);
        assert_eq!(sync.poll(), None);
        assert_eq!(sync.buffered(0), 0);
        assert_eq!(sync.buffered(1), 1);
        sync.push(0, 128, 128);
        assert_eq!(sync.poll(), Some(vec![128, 130]));
    }

    #[test]
    fn late_sample_is_dropped() {
        let mut sync = Synchronizer::new(2, 8, 10);
        sync.push(0, 100, 100);
        sync.push(1, 100, 100);
        assert_eq!(sync.poll(), Some(vec![100, 100]));
        sync.push(0, 90, 90); // late: <= watermark 100
        sync.push(1, 105, 105);
        assert_eq!(sync.poll(), None);
        assert_eq!(sync.buffered(0), 0);
    }

    #[test]
    fn full_buffer_drops_oldest() {
        let mut sync = Synchronizer::new(2, 2, 100);
        sync.push(0, 1, 1);
        sync.push(0, 2, 2);
        sync.push(0, 3, 3);
        assert_eq!(sync.buffered(0), 2);
        sync.push(1, 3, 3);
        assert_eq!(sync.poll(), Some(vec![3, 3]));
    }

    #[test]
    fn three_inputs_align_within_window() {
        let mut sync = Synchronizer::new(3, 8, 10);
        sync.push(0, 100, 100);
        sync.push(1, 108, 108);
        sync.push(2, 105, 105);
        assert_eq!(sync.poll(), Some(vec![100, 108, 105]));
    }

    #[test]
    fn three_inputs_drain_lagging_input() {
        let mut sync = Synchronizer::new(3, 8, 5);
        sync.push(0, 100, 100);
        sync.push(1, 200, 200);
        sync.push(2, 202, 202);
        assert_eq!(sync.poll(), None);
        assert_eq!(sync.buffered(0), 0);
        assert_eq!(sync.buffered(1), 1);
        assert_eq!(sync.buffered(2), 1);
    }
}
