use std::collections::BTreeSet;

/// supervisor 发给 runtime participant 的一次逻辑 tick 授权。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickGrant {
    pub tick_id: u64,
    pub logical_time_ms: u64,
}

/// runtime participant 完成当前 tick 后上报的 barrier fact。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickDone {
    pub tick_id: u64,
    pub participant: String,
}

/// global tick coordinator 对外产生的状态事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickCoordinatorEvent {
    Grant {
        participant: String,
        grant: TickGrant,
    },
    Completed {
        tick_id: u64,
    },
    Fault {
        tick_id: u64,
        reason: String,
    },
}

/// 本机 supervisor 管理域内的 global tick barrier 状态机。
#[derive(Debug, Clone)]
pub struct GlobalTickCoordinator {
    participants: Vec<String>,
    tick_timeout_ms: u64,
    current_tick: u64,
    pending: BTreeSet<String>,
    faulted: bool,
}

impl GlobalTickCoordinator {
    pub fn new(mut participants: Vec<String>, tick_timeout_ms: u64) -> Self {
        participants.sort();
        participants.dedup();
        Self {
            participants,
            tick_timeout_ms,
            current_tick: 0,
            pending: BTreeSet::new(),
            faulted: false,
        }
    }

    pub fn tick_timeout_ms(&self) -> u64 {
        self.tick_timeout_ms
    }

    pub fn start_tick(&mut self, logical_time_ms: u64) -> Vec<TickCoordinatorEvent> {
        if self.faulted || !self.pending.is_empty() || self.participants.is_empty() {
            return Vec::new();
        }
        self.current_tick += 1;
        self.pending = self.participants.iter().cloned().collect();
        self.participants
            .iter()
            .map(|participant| TickCoordinatorEvent::Grant {
                participant: participant.clone(),
                grant: TickGrant {
                    tick_id: self.current_tick,
                    logical_time_ms,
                },
            })
            .collect()
    }

    pub fn mark_done(&mut self, done: TickDone) -> Vec<TickCoordinatorEvent> {
        if self.faulted || done.tick_id != self.current_tick {
            return Vec::new();
        }
        self.pending.remove(&done.participant);
        if self.pending.is_empty() {
            vec![TickCoordinatorEvent::Completed {
                tick_id: self.current_tick,
            }]
        } else {
            Vec::new()
        }
    }

    pub fn timeout(&mut self, reason: impl Into<String>) -> Option<TickCoordinatorEvent> {
        if self.faulted || self.current_tick == 0 || self.pending.is_empty() {
            return None;
        }
        self.faulted = true;
        Some(TickCoordinatorEvent::Fault {
            tick_id: self.current_tick,
            reason: reason.into(),
        })
    }
}
