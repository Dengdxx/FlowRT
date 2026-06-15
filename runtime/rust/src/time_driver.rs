//! 运行时原生确定性回放的时间驱动 (v0.17.0)。
//!
//! [`ReplayDriver`] 让 runtime 自己拥有回放事件时间线并按确定性网格步进，取代 v0.16.0 经
//! introspection socket 由外部 wall-clock 节奏逐事件注入的回放路径。给定 scheduler 计算出的
//! 下一个 periodic deadline，driver 在「下一个事件时间」与「下一个 periodic 网格点」之间取
//! 较早者推进逻辑时钟，从而在两个事件之间逐周期触发 periodic task（与 realtime 积分粒度对齐），
//! 回放结果只取决于事件序列，与回放物理快慢无关。
//!
//! driver 只对抽象时间线 [`ReplayEvent`] 操作，不解析 MCAP、不读 wall-clock；时间线来源
//! （MCAP 录制、fixture）由生成 shell 在更外层装配，保持本 runtime crate 精简。

use std::collections::VecDeque;

/// 单条回放事件：在某个逻辑毫秒把一段 wire payload 注入某个 boundary input 或 channel。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayEvent {
    /// 事件进入 runtime 的逻辑毫秒时间（record envelope 的 receive / monotonic 时间）。
    pub time_ms: u64,
    /// 注入目标 boundary input 或 channel 名称。
    pub target: String,
    /// 注入的 wire payload 字节。
    pub payload: Vec<u8>,
    /// 传感器 sample-time（毫秒）。v0.17.0 恒为 `None`；为后续 sensor event-time 与多传感器
    /// 同步预留，确保回放时间线格式向前兼容。
    pub sample_time_ms: Option<u64>,
}

/// 一次步进的分类结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// 推进到一个 periodic 网格点，本步无新数据。
    Timer,
    /// 推进到一个事件时间，已暂存该时刻全部待注入事件（用 [`ReplayDriver::take_pending_events`]
    /// 取走后注入对应 boundary/channel）。
    Data,
    /// 时间线已耗尽，回放结束。
    Shutdown,
}

/// 运行时原生回放驱动：拥有事件时间线并确定性步进逻辑时钟。
#[derive(Debug)]
pub struct ReplayDriver {
    timeline: VecDeque<ReplayEvent>,
    now_ms: u64,
    pending: Vec<ReplayEvent>,
}

impl ReplayDriver {
    /// 从按时间升序的事件时间线构造 driver。调用方负责保证时间线已排序。
    pub fn new(timeline: Vec<ReplayEvent>) -> Self {
        Self {
            timeline: timeline.into(),
            now_ms: 0,
            pending: Vec::new(),
        }
    }

    /// 当前逻辑毫秒时间。
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// 在「下一个事件时间」与传入的「下一个 periodic deadline」之间取较早者推进逻辑时钟。
    ///
    /// 命中事件时间返回 [`Step::Data`] 并暂存该时刻全部事件（用 [`Self::take_pending_events`]
    /// 取走）；命中 periodic 网格点返回 [`Step::Timer`]；时间线耗尽返回 [`Step::Shutdown`]。
    /// 逻辑时钟单调不退；同一时刻的多个事件一次性暂存。
    pub fn step(&mut self, next_periodic_deadline_ms: Option<u64>) -> Step {
        let Some(next_event_ms) = self.timeline.front().map(|event| event.time_ms) else {
            return Step::Shutdown;
        };
        let target = match next_periodic_deadline_ms {
            Some(periodic) => next_event_ms.min(periodic),
            None => next_event_ms,
        };
        self.now_ms = self.now_ms.max(target);
        if target == next_event_ms {
            while self
                .timeline
                .front()
                .is_some_and(|event| event.time_ms == target)
            {
                if let Some(event) = self.timeline.pop_front() {
                    self.pending.push(event);
                }
            }
            Step::Data
        } else {
            Step::Timer
        }
    }

    /// 取走上一次 [`Step::Data`] 暂存的待注入事件。
    pub fn take_pending_events(&mut self) -> Vec<ReplayEvent> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(time_ms: u64, target: &str) -> ReplayEvent {
        ReplayEvent {
            time_ms,
            target: target.to_string(),
            payload: vec![time_ms as u8],
            sample_time_ms: None,
        }
    }

    #[test]
    fn replay_driver_steps_periodic_grid_between_events() {
        // 事件在 t=0 与 t=15；scheduler 用 5ms periodic 网格。
        // 期望两事件之间逐周期 Timer，而非一次 catch-up 跳跃。
        let mut driver = ReplayDriver::new(vec![event(0, "a"), event(15, "b")]);
        let mut log: Vec<(Step, u64)> = Vec::new();
        let mut next_periodic = Some(0u64);
        loop {
            let step = driver.step(next_periodic);
            log.push((step, driver.now_ms()));
            match step {
                Step::Shutdown => break,
                Step::Data => {
                    assert!(!driver.take_pending_events().is_empty());
                    next_periodic = Some(driver.now_ms() + 5);
                }
                Step::Timer => next_periodic = Some(driver.now_ms() + 5),
            }
            assert!(log.len() <= 16, "runaway stepping: {log:?}");
        }
        assert_eq!(
            log,
            vec![
                (Step::Data, 0),
                (Step::Timer, 5),
                (Step::Timer, 10),
                (Step::Data, 15),
                (Step::Shutdown, 15),
            ]
        );
    }

    #[test]
    fn replay_driver_data_step_stages_events_at_same_time() {
        // 同一时刻的多个事件应一次性暂存。
        let mut driver = ReplayDriver::new(vec![event(7, "a"), event(7, "b")]);
        assert_eq!(driver.step(Some(100)), Step::Data);
        assert_eq!(driver.now_ms(), 7);
        assert_eq!(driver.take_pending_events().len(), 2);
        assert_eq!(driver.step(Some(100)), Step::Shutdown);
    }
}
