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
    /// 传感器 sample-time（毫秒）。v0.18.0 起由声明 timestamp 源的消息在录制时填充；为空时
    /// event-time 回放回退到 `time_ms` receive-time，确保回放时间线格式向前兼容。
    pub sample_time_ms: Option<u64>,
}

impl ReplayEvent {
    /// event-time 回放使用的有效逻辑时间：有 sample-time 用之，否则回退 receive-time。
    pub fn effective_time_ms(&self) -> u64 {
        self.sample_time_ms.unwrap_or(self.time_ms)
    }
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
        let Some(next_event_ms) = self.timeline.front().map(|event| event.effective_time_ms())
        else {
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
                .is_some_and(|event| event.effective_time_ms() == target)
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

/// 时间驱动抽象：让调度循环对时间来源不可知。
///
/// realtime 暂仍走生成 shell 既有 wall-clock 路径（后续可迁移到本 trait）；[`ReplayDriver`]
/// 与 [`SteppedDriver`] 通过本 trait 统一接入：`next_step` 推进逻辑时钟并分类本步，
/// `take_pending_events` 取出 [`Step::Data`] 步需注入的回放事件（realtime 恒为空——其数据由
/// 真实 backend 经 `ScheduleWaiter` 自行注入）。
pub trait TimeDriver {
    /// 推进到下一个调度步。`next_periodic_deadline_ms` 是调度器算出的下一个 periodic
    /// deadline（无 periodic 待触发时为 `None`）；`shutdown` 触发时返回 [`Step::Shutdown`]。
    fn next_step(
        &mut self,
        next_periodic_deadline_ms: Option<u64>,
        shutdown: &crate::ShutdownToken,
    ) -> Step;

    /// 当前逻辑毫秒时间。
    fn now_ms(&self) -> u64;

    /// 取走上一个 [`Step::Data`] 暂存的待注入事件。默认空（realtime 数据由 backend 自注入）。
    fn take_pending_events(&mut self) -> Vec<ReplayEvent> {
        Vec::new()
    }
}

impl TimeDriver for ReplayDriver {
    fn next_step(
        &mut self,
        next_periodic_deadline_ms: Option<u64>,
        shutdown: &crate::ShutdownToken,
    ) -> Step {
        if shutdown.is_requested() {
            return Step::Shutdown;
        }
        self.step(next_periodic_deadline_ms)
    }

    fn now_ms(&self) -> u64 {
        ReplayDriver::now_ms(self)
    }

    fn take_pending_events(&mut self) -> Vec<ReplayEvent> {
        ReplayDriver::take_pending_events(self)
    }
}

/// external_stepped 时钟的外部步进控制器。
///
/// deterministic debug 场景下，外部控制面（introspection socket）按需 [`grant`](Self::grant)
/// 步进预算；[`SteppedDriver`] 在预算耗尽时阻塞等待，直到获得新预算、被 [`close`](Self::close)
/// 或收到 shutdown。控制器可克隆共享给控制面线程。
#[derive(Debug, Clone, Default)]
pub struct StepController {
    state: std::sync::Arc<(std::sync::Mutex<StepState>, std::sync::Condvar)>,
}

#[derive(Debug, Default)]
struct StepState {
    budget: u64,
    closed: bool,
}

impl StepController {
    /// 构造预算为 0 的控制器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加 `steps` 个步进预算并唤醒等待者。
    pub fn grant(&self, steps: u64) {
        let (mutex, condvar) = &*self.state;
        let mut state = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.budget = state.budget.saturating_add(steps);
        condvar.notify_all();
    }

    /// 关闭控制器：等待者将解除阻塞并结束回放。
    pub fn close(&self) {
        let (mutex, condvar) = &*self.state;
        let mut state = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.closed = true;
        condvar.notify_all();
    }

    /// 阻塞获取一个步进预算。返回 `false` 表示被关闭或 shutdown，应结束回放。
    fn acquire(&self, shutdown: &crate::ShutdownToken) -> bool {
        let (mutex, condvar) = &*self.state;
        let mut state = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let poll = std::time::Duration::from_millis(50);
        loop {
            if shutdown.is_requested() {
                return false;
            }
            if state.budget > 0 {
                state.budget -= 1;
                return true;
            }
            if state.closed {
                return false;
            }
            let (next, _) = condvar
                .wait_timeout(state, poll)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
        }
    }
}

/// external_stepped 时间驱动：拥有回放时间线，但每一步都等外部控制器授权后才推进。
#[derive(Debug)]
pub struct SteppedDriver {
    replay: ReplayDriver,
    controller: StepController,
}

impl SteppedDriver {
    /// 用回放时间线和共享控制器构造 driver。
    pub fn new(timeline: Vec<ReplayEvent>, controller: StepController) -> Self {
        Self {
            replay: ReplayDriver::new(timeline),
            controller,
        }
    }
}

impl TimeDriver for SteppedDriver {
    fn next_step(
        &mut self,
        next_periodic_deadline_ms: Option<u64>,
        shutdown: &crate::ShutdownToken,
    ) -> Step {
        if !self.controller.acquire(shutdown) {
            return Step::Shutdown;
        }
        self.replay.step(next_periodic_deadline_ms)
    }

    fn now_ms(&self) -> u64 {
        self.replay.now_ms()
    }

    fn take_pending_events(&mut self) -> Vec<ReplayEvent> {
        self.replay.take_pending_events()
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

    #[test]
    fn replay_driver_as_time_driver_short_circuits_on_shutdown() {
        let mut driver = ReplayDriver::new(vec![event(0, "a")]);
        let shutdown = crate::ShutdownToken::new_for_test();
        shutdown.request();
        assert_eq!(
            TimeDriver::next_step(&mut driver, Some(0), &shutdown),
            Step::Shutdown
        );
    }

    #[test]
    fn stepped_driver_advances_only_with_granted_budget() {
        let controller = StepController::new();
        let mut driver = SteppedDriver::new(vec![event(0, "a"), event(5, "b")], controller.clone());
        let shutdown = crate::ShutdownToken::new_for_test();
        // 授权两步：分别命中两个事件。
        controller.grant(2);
        assert_eq!(driver.next_step(Some(0), &shutdown), Step::Data);
        assert_eq!(driver.now_ms(), 0);
        assert_eq!(driver.take_pending_events().len(), 1);
        assert_eq!(driver.next_step(Some(100), &shutdown), Step::Data);
        assert_eq!(driver.now_ms(), 5);
        // 预算耗尽后关闭：driver 结束。
        controller.close();
        assert_eq!(driver.next_step(Some(100), &shutdown), Step::Shutdown);
    }

    #[test]
    fn stepped_driver_shutdown_unblocks_without_budget() {
        let controller = StepController::new();
        let mut driver = SteppedDriver::new(vec![event(0, "a")], controller);
        let shutdown = crate::ShutdownToken::new_for_test();
        shutdown.request();
        // 无预算但 shutdown 已请求：不应阻塞，直接结束。
        assert_eq!(driver.next_step(Some(0), &shutdown), Step::Shutdown);
    }

    #[test]
    fn replay_driver_steps_by_sample_time_when_present() {
        // 事件 receive-time 5/6，但 sample-time 100/200 → 按 sample-time（event-time）步进。
        let mut driver = ReplayDriver::new(vec![
            ReplayEvent {
                time_ms: 5,
                target: "a".to_string(),
                payload: vec![],
                sample_time_ms: Some(100),
            },
            ReplayEvent {
                time_ms: 6,
                target: "b".to_string(),
                payload: vec![],
                sample_time_ms: Some(200),
            },
        ]);
        assert_eq!(driver.step(Some(1000)), Step::Data);
        assert_eq!(driver.now_ms(), 100);
        assert_eq!(driver.take_pending_events().len(), 1);
        assert_eq!(driver.step(Some(1000)), Step::Data);
        assert_eq!(driver.now_ms(), 200);
        assert_eq!(driver.step(Some(1000)), Step::Shutdown);
    }
}
