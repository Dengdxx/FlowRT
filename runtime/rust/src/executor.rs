use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    task::{Context as TaskContext, Poll, Wake, Waker},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LaneId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneKind {
    Serial,
    Parallel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub id: TaskId,
    pub lane: LaneId,
    pub priority: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeriodicSpec {
    pub task: TaskId,
    pub period_ms: u64,
}

/// 一次 task admission 的调度元数据。
///
/// `scheduled_time_ms` 表示 runtime 计划该次运行的时间，`observed_time_ms` 表示
/// scheduler 实际准入该 task 的时间。generated shell 应把这些字段转换成
/// `TaskTiming` 后交给用户 `Context`，并在 completion 回来后显式调用
/// `DeterministicExecutor::complete_task` 释放 inflight token。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskAdmission {
    pub task: TaskId,
    pub lane: LaneId,
    pub scheduled_time_ms: u64,
    pub observed_time_ms: u64,
    pub period_ms: Option<u64>,
    pub deadline_ms: Option<u64>,
    pub missed_periods: u64,
    pub lateness_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FutureHandle(TaskId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRunResult {
    pub task: TaskId,
    pub status: Status,
}

/// 单个 task 回调返回的状态和 task-local 输出集合。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRunOutcome<O> {
    /// 用户回调返回的 FlowRT 状态。
    pub status: Status,
    /// 回调结束后由 generated shell 收集的输出集合。
    pub outputs: O,
}

impl<O> TaskRunOutcome<O> {
    /// 构造一个带指定状态的 task 结果。
    pub fn new(status: Status, outputs: O) -> Self {
        Self { status, outputs }
    }

    /// 构造成功结果；scheduler 后续可以提交其中输出。
    pub fn ok(outputs: O) -> Self {
        Self::new(Status::Ok, outputs)
    }

    /// 构造重试结果；runtime 会丢弃其中输出。
    pub fn retry(outputs: O) -> Self {
        Self::new(Status::Retry, outputs)
    }

    /// 构造错误结果；runtime 会丢弃其中输出。
    pub fn error(outputs: O) -> Self {
        Self::new(Status::Error, outputs)
    }
}

/// scheduler 线程可观察到的 task 执行结果。
///
/// `outputs` 只在 `status == Status::Ok` 时保留；非 `Ok` 或 panic 路径统一为 `None`，
/// 防止 generated shell 误提交失败回调写入的样本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRunOutput<O> {
    /// 被执行的 task id。
    pub task: TaskId,
    /// task 执行状态。
    pub status: Status,
    /// 可提交输出集合；只有成功 task 会保留。
    pub outputs: Option<O>,
}

impl<O> TaskRunOutput<O> {
    /// 从用户回调正常返回的结果构造 scheduler 可见结果。
    pub fn from_outcome(task: TaskId, outcome: TaskRunOutcome<O>) -> Self {
        let status = outcome.status;
        let outputs = if status == Status::Ok {
            Some(outcome.outputs)
        } else {
            None
        };
        Self {
            task,
            status,
            outputs,
        }
    }

    /// 构造不携带输出的结果，用于非 `Ok`、panic 或 worker 异常路径。
    pub fn without_outputs(task: TaskId, status: Status) -> Self {
        Self {
            task,
            status,
            outputs: None,
        }
    }

    /// 消费结果并返回可提交输出；非成功结果返回 `None`。
    pub fn into_outputs_if_ok(self) -> Option<O> {
        if self.status == Status::Ok {
            self.outputs
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
struct TaskState {
    spec: TaskSpec,
    deadline_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct PeriodicState {
    period_ms: u64,
    next_deadline_ms: u64,
    missed_periods: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingAdmission {
    scheduled_time_ms: u64,
    period_ms: Option<u64>,
    missed_periods: u64,
}

#[derive(Debug, Clone)]
pub struct DeterministicExecutor {
    worker_threads: usize,
    now_ms: u64,
    current_tick: u64,
    admission_open: bool,
    lanes: BTreeMap<LaneId, LaneKind>,
    tasks: BTreeMap<TaskId, TaskState>,
    ready: BTreeSet<TaskId>,
    pending: BTreeMap<TaskId, PendingAdmission>,
    periodic: BTreeMap<TaskId, PeriodicState>,
    inflight_tasks: BTreeSet<TaskId>,
    inflight_serial_lanes: BTreeSet<LaneId>,
    /// 被故障隔离暂停的 task：不进 ready、不被 admit；resume 后恢复。
    suspended: BTreeSet<TaskId>,
    /// 记录每个 lane 最近一次被调度执行的 tick 编号。
    lane_last_dispatched_tick: BTreeMap<LaneId, u64>,
}

type WorkerJobFn = Box<dyn FnOnce() -> Status + Send + 'static>;
type WorkerCompletion = Box<dyn FnOnce(Status) + Send + 'static>;

struct WorkerJob {
    run: WorkerJobFn,
    on_complete: Option<WorkerCompletion>,
}

impl WorkerJob {
    fn new(run: WorkerJobFn) -> Self {
        Self {
            run,
            on_complete: None,
        }
    }

    fn with_completion(run: WorkerJobFn, on_complete: WorkerCompletion) -> Self {
        Self {
            run,
            on_complete: Some(on_complete),
        }
    }
}

/// worker job 未进入执行队列时返回的提交失败信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerSubmitError {
    /// 被拒绝提交的 task id。
    pub task: TaskId,
}

struct WorkerCompletionQueueInner<O> {
    completed: Mutex<VecDeque<TaskRunOutput<O>>>,
    wake: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

/// worker 线程完成后交还 scheduler 线程的 typed completion queue。
///
/// `drain_completed` 和 `try_drain_completed` 只取回当前已完成结果，空队列立即返回。
pub struct WorkerCompletionQueue<O> {
    inner: Arc<WorkerCompletionQueueInner<O>>,
}

impl<O> Clone for WorkerCompletionQueue<O> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<O> Default for WorkerCompletionQueue<O> {
    fn default() -> Self {
        Self::new()
    }
}

impl<O> WorkerCompletionQueue<O> {
    /// 创建空 completion queue。
    pub fn new() -> Self {
        Self {
            inner: Arc::new(WorkerCompletionQueueInner {
                completed: Mutex::new(VecDeque::new()),
                wake: Mutex::new(None),
            }),
        }
    }

    /// 设置 completion 入队后的唤醒回调。
    ///
    /// generated scheduler 用它把 worker completion 合并进 scheduler wake 路径；回调不拥有
    /// completion 数据，只负责唤醒调度线程。
    pub fn set_wake_callback<F>(&self, wake: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        *lock_recover(&self.inner.wake) = Some(Box::new(wake));
    }

    /// 取回当前所有已完成结果；空队列立即返回空列表。
    pub fn drain_completed(&self) -> Vec<TaskRunOutput<O>> {
        let mut completed = lock_recover(&self.inner.completed);
        completed.drain(..).collect()
    }

    /// 非阻塞取回当前所有已完成结果。
    pub fn try_drain_completed(&self) -> Vec<TaskRunOutput<O>> {
        self.drain_completed()
    }

    fn push(&self, result: TaskRunOutput<O>) {
        let mut completed = lock_recover(&self.inner.completed);
        completed.push_back(result);
        if let Some(wake) = lock_recover(&self.inner.wake).as_ref() {
            wake();
        }
    }
}

struct WorkerPoolState {
    admission_open: bool,
    queue: VecDeque<WorkerJob>,
    active: usize,
    status: Status,
}

struct WorkerPoolInner {
    state: Mutex<WorkerPoolState>,
    ready: Condvar,
    drained: Condvar,
}

/// 面向后续异步/并发 generated shell 的有界 worker 基础。
///
/// 该 pool 只提供固定 worker 数、关闭准入、drain/join、completion 回传和首个错误聚合。
/// scheduler 负责 admission、completion drain 与 backend commit；pool 不等待整批 task 完成。
pub struct WorkerPool {
    worker_threads: usize,
    inner: Arc<WorkerPoolInner>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyBatch {
    tasks: Vec<TaskId>,
    admissions: Vec<TaskAdmission>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleEvent {
    Data,
    Timer,
    Shutdown,
}

#[derive(Debug, Default)]
struct ScheduleWaitState {
    data_generation: u64,
    data_time_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ScheduleWaiter {
    state: Arc<(Mutex<ScheduleWaitState>, Condvar)>,
}

impl ScheduleWaiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn notify_data(&self) {
        self.notify_data_with_time(None);
    }

    pub fn notify_data_at_ms(&self, time_ms: u64) {
        self.notify_data_with_time(Some(time_ms));
    }

    fn notify_data_with_time(&self, time_ms: Option<u64>) {
        let (mutex, condvar) = &*self.state;
        let mut state = lock_recover(mutex);
        state.data_generation = state.data_generation.saturating_add(1);
        if let Some(time_ms) = time_ms {
            state.data_time_ms = Some(
                state
                    .data_time_ms
                    .map_or(time_ms, |known| known.max(time_ms)),
            );
        }
        condvar.notify_all();
    }

    pub fn data_generation(&self) -> u64 {
        let (mutex, _) = &*self.state;
        lock_recover(mutex).data_generation
    }

    pub fn take_data_time_ms(&self) -> Option<u64> {
        let (mutex, _) = &*self.state;
        lock_recover(mutex).data_time_ms.take()
    }

    pub fn wait_until(
        &self,
        deadline: Option<Instant>,
        shutdown: &crate::ShutdownToken,
    ) -> ScheduleEvent {
        self.wait_until_after(self.data_generation(), deadline, shutdown)
    }

    pub fn wait_until_after(
        &self,
        seen_generation: u64,
        deadline: Option<Instant>,
        shutdown: &crate::ShutdownToken,
    ) -> ScheduleEvent {
        let (mutex, condvar) = &*self.state;
        let mut state = lock_recover(mutex);
        let shutdown_poll = Duration::from_millis(50);
        loop {
            if shutdown.is_requested() {
                return ScheduleEvent::Shutdown;
            }
            if state.data_generation != seen_generation {
                return ScheduleEvent::Data;
            }

            match deadline {
                Some(deadline) => {
                    let now = Instant::now();
                    if now >= deadline {
                        return ScheduleEvent::Timer;
                    }
                    let timeout = deadline.saturating_duration_since(now).min(shutdown_poll);
                    let (next_state, timed_out) = wait_timeout_recover(condvar, state, timeout);
                    state = next_state;
                    if timed_out && Instant::now() >= deadline {
                        return ScheduleEvent::Timer;
                    }
                }
                None => {
                    let (next_state, _) = wait_timeout_recover(condvar, state, shutdown_poll);
                    state = next_state;
                }
            }
        }
    }
}

impl WorkerPool {
    pub fn new(worker_threads: usize) -> Self {
        let worker_threads = worker_threads.max(1);
        let inner = Arc::new(WorkerPoolInner {
            state: Mutex::new(WorkerPoolState {
                admission_open: true,
                queue: VecDeque::new(),
                active: 0,
                status: Status::Ok,
            }),
            ready: Condvar::new(),
            drained: Condvar::new(),
        });
        let mut workers = Vec::with_capacity(worker_threads);
        for _ in 0..worker_threads {
            let worker_inner = Arc::clone(&inner);
            workers.push(thread::spawn(move || worker_loop(worker_inner)));
        }

        Self {
            worker_threads,
            inner,
            workers: Mutex::new(workers),
        }
    }

    pub fn worker_threads(&self) -> usize {
        self.worker_threads
    }

    pub fn spawn<F>(&self, job: F) -> Option<()>
    where
        F: FnOnce() -> Status + Send + 'static,
    {
        let mut state = lock_recover(&self.inner.state);
        if !state.admission_open {
            return None;
        }
        state.queue.push_back(WorkerJob::new(Box::new(job)));
        self.inner.ready.notify_one();
        Some(())
    }

    pub fn close_admission(&self) {
        let mut state = lock_recover(&self.inner.state);
        state.admission_open = false;
        self.inner.ready.notify_all();
    }

    pub fn drain(&self) -> Status {
        let mut state = lock_recover(&self.inner.state);
        while !state.queue.is_empty() || state.active != 0 {
            state = wait_recover(&self.inner.drained, state);
        }
        state.status
    }

    pub fn spawn_with_completion<F, C>(&self, job: F, on_complete: C) -> Option<()>
    where
        F: FnOnce() -> Status + Send + 'static,
        C: FnOnce(Status) + Send + 'static,
    {
        let mut state = lock_recover(&self.inner.state);
        if !state.admission_open {
            return None;
        }
        state.queue.push_back(WorkerJob::with_completion(
            Box::new(job),
            Box::new(on_complete),
        ));
        self.inner.ready.notify_one();
        Some(())
    }

    /// 非阻塞提交带 typed output 的 task job。
    ///
    /// 调用成功后立即返回，worker 完成时向 `completions` 写入一条
    /// `TaskRunOutput`。panic 会转换为 `Status::Error`，且不保留输出。
    pub fn submit_collect<O, F>(
        &self,
        task: TaskId,
        completions: &WorkerCompletionQueue<O>,
        job: F,
    ) -> Result<(), WorkerSubmitError>
    where
        O: Send + 'static,
        F: FnOnce() -> TaskRunOutcome<O> + Send + 'static,
    {
        let completion_queue = completions.clone();
        let output_slot = Arc::new(Mutex::new(None));
        let job_slot = Arc::clone(&output_slot);
        let completion_slot = Arc::clone(&output_slot);
        self.spawn_with_completion(
            move || {
                let outcome = job();
                let status = outcome.status;
                *lock_recover(&job_slot) = Some(TaskRunOutput::from_outcome(task, outcome));
                status
            },
            move |status| {
                let result = lock_recover(&completion_slot)
                    .take()
                    .unwrap_or_else(|| TaskRunOutput::without_outputs(task, status));
                completion_queue.push(result);
            },
        )
        .ok_or(WorkerSubmitError { task })
    }

    pub fn shutdown(&self) -> Status {
        self.close_admission();
        let _ = self.drain();

        let mut workers = lock_recover(&self.workers);
        while let Some(worker) = workers.pop() {
            let _ = worker.join();
        }

        self.drain()
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn worker_loop(inner: Arc<WorkerPoolInner>) {
    loop {
        let job = {
            let mut state = lock_recover(&inner.state);
            loop {
                if let Some(job) = state.queue.pop_front() {
                    state.active += 1;
                    break job;
                }
                if !state.admission_open {
                    return;
                }
                state = wait_recover(&inner.ready, state);
            }
        };

        let WorkerJob { run, on_complete } = job;
        let status =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or(Status::Error);
        if let Some(on_complete) = on_complete {
            let completion_status = status;
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                on_complete(completion_status);
            }));
        }
        let mut state = lock_recover(&inner.state);
        if status == Status::Error {
            state.status = Status::Error;
        }
        state.active = state.active.saturating_sub(1);
        if state.queue.is_empty() && state.active == 0 {
            inner.drained.notify_all();
        }
    }
}

impl DeterministicExecutor {
    pub fn new(worker_threads: usize) -> Self {
        Self {
            worker_threads: worker_threads.max(1),
            now_ms: 0,
            current_tick: 0,
            admission_open: true,
            lanes: BTreeMap::new(),
            tasks: BTreeMap::new(),
            ready: BTreeSet::new(),
            pending: BTreeMap::new(),
            periodic: BTreeMap::new(),
            inflight_tasks: BTreeSet::new(),
            inflight_serial_lanes: BTreeSet::new(),
            suspended: BTreeSet::new(),
            lane_last_dispatched_tick: BTreeMap::new(),
        }
    }

    pub fn add_lane(&mut self, lane: LaneId, kind: LaneKind) {
        self.lanes.insert(lane, kind);
    }

    pub fn worker_threads(&self) -> usize {
        self.worker_threads
    }

    pub fn add_task(&mut self, spec: TaskSpec) {
        self.tasks.insert(
            spec.id,
            TaskState {
                spec,
                deadline_ms: None,
            },
        );
    }

    /// 设置 task 的相对 deadline 元数据，单位为毫秒。
    ///
    /// executor 不在这里判定 deadline 失败；generated scheduler 用 admission metadata 构造
    /// `TaskTiming`，并由 status/introspection 解释 lateness 与 deadline。
    pub fn set_task_deadline_ms(&mut self, task: TaskId, deadline_ms: Option<u64>) {
        if let Some(state) = self.tasks.get_mut(&task) {
            state.deadline_ms = deadline_ms;
        }
    }

    pub fn add_periodic(&mut self, spec: PeriodicSpec) {
        let period_ms = spec.period_ms.max(1);
        self.periodic.insert(
            spec.task,
            PeriodicState {
                period_ms,
                next_deadline_ms: self.now_ms.saturating_add(period_ms),
                missed_periods: 0,
            },
        );
    }

    pub fn wake(&mut self, task: TaskId) {
        if self.admission_open && !self.suspended.contains(&task) && self.tasks.contains_key(&task)
        {
            self.ready.insert(task);
            self.pending.entry(task).or_insert(PendingAdmission {
                scheduled_time_ms: self.now_ms,
                period_ms: None,
                missed_periods: 0,
            });
        }
    }

    /// 故障隔离：暂停某 task 的调度。把它移出 ready/pending，后续 `wake`/periodic due
    /// 也不再让它进入 ready，直到 `resume_task`。已 inflight 的提交不受影响（completion 仍回来）。
    pub fn suspend_task(&mut self, task: TaskId) {
        self.suspended.insert(task);
        self.ready.remove(&task);
        self.pending.remove(&task);
    }

    /// 重启成功：恢复某 task 的调度资格。下次 `wake` 或 periodic due 起重新进入 ready。
    pub fn resume_task(&mut self, task: TaskId) {
        self.suspended.remove(&task);
    }

    pub fn close_admission(&mut self) {
        self.admission_open = false;
    }

    pub fn is_drained(&self) -> bool {
        self.ready.is_empty() && self.inflight_tasks.is_empty()
    }

    pub fn advance_to_ms(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        let mut due = Vec::new();
        for (task, state) in &mut self.periodic {
            if now_ms < state.next_deadline_ms {
                continue;
            }
            let elapsed = now_ms - state.next_deadline_ms;
            let missed = elapsed / state.period_ms;
            let scheduled_time_ms = state
                .next_deadline_ms
                .saturating_add(missed.saturating_mul(state.period_ms));
            state.next_deadline_ms = scheduled_time_ms.saturating_add(state.period_ms);
            due.push((*task, scheduled_time_ms, state.period_ms, missed));
        }
        for (task, scheduled_time_ms, period_ms, missed) in due {
            self.record_periodic_due(task, scheduled_time_ms, period_ms, missed);
        }
    }

    pub fn next_deadline_ms(&self, task: TaskId) -> Option<u64> {
        self.periodic.get(&task).map(|state| state.next_deadline_ms)
    }

    pub fn missed_periods(&self, task: TaskId) -> u64 {
        self.periodic
            .get(&task)
            .map(|state| state.missed_periods)
            .unwrap_or(0)
    }

    /// 准入当前 ready set 中可并行 dispatch 的 task。
    ///
    /// 该方法只修改 scheduler admission 状态：移除 ready、写入 inflight task/lane token，
    /// 并返回每个 task 的 timing metadata。它不运行用户回调，也不等待 completion。
    pub fn take_ready_batch(&mut self) -> ReadyBatch {
        let mut admissions = Vec::new();
        for task in self.ready_parallel_order() {
            if let Some(admission) = self.admit_task(task) {
                admissions.push(admission);
            }
        }
        ReadyBatch::from_admissions(admissions)
    }

    fn ready_parallel_order(&self) -> Vec<TaskId> {
        let mut by_lane = BTreeMap::<LaneId, Vec<TaskId>>::new();
        for task in &self.ready {
            if !self.can_admit(*task) {
                continue;
            }
            if let Some(state) = self.tasks.get(task) {
                by_lane.entry(state.spec.lane).or_default().push(*task);
            }
        }
        for lane_tasks in by_lane.values_mut() {
            lane_tasks.sort_by(|left, right| self.compare_ready_tasks(*left, *right));
        }

        let mut tasks = Vec::new();
        let mut reserved_serial_lanes = BTreeSet::new();
        for (lane, lane_tasks) in by_lane {
            match self.lane_kind(lane) {
                LaneKind::Serial => {
                    if self.inflight_serial_lanes.contains(&lane)
                        || reserved_serial_lanes.contains(&lane)
                    {
                        continue;
                    }
                    if let Some(task) = lane_tasks.into_iter().next() {
                        reserved_serial_lanes.insert(lane);
                        tasks.push(task);
                    }
                }
                LaneKind::Parallel => tasks.extend(lane_tasks),
            }
        }
        tasks
    }

    fn record_periodic_due(
        &mut self,
        task: TaskId,
        scheduled_time_ms: u64,
        period_ms: u64,
        missed_from_deadline: u64,
    ) {
        let counted_pending = self
            .pending
            .get(&task)
            .filter(|pending| pending.period_ms == Some(period_ms))
            .map_or(0, |pending| pending.missed_periods);
        let pending_same_period = self
            .pending
            .get(&task)
            .filter(|pending| pending.period_ms == Some(period_ms));
        let missed_periods = pending_same_period
            .and_then(|pending| {
                scheduled_time_ms
                    .checked_sub(pending.scheduled_time_ms)
                    .map(|delta| {
                        pending
                            .missed_periods
                            .saturating_add(delta / period_ms.max(1))
                    })
            })
            .unwrap_or_else(|| {
                missed_from_deadline.saturating_add(u64::from(self.inflight_tasks.contains(&task)))
            });
        if let Some(state) = self.periodic.get_mut(&task) {
            let additional_missed = missed_periods.saturating_sub(counted_pending);
            state.missed_periods = state.missed_periods.saturating_add(additional_missed);
        }
        if self.admission_open && !self.suspended.contains(&task) && self.tasks.contains_key(&task)
        {
            self.ready.insert(task);
            self.pending.insert(
                task,
                PendingAdmission {
                    scheduled_time_ms,
                    period_ms: Some(period_ms),
                    missed_periods,
                },
            );
        }
    }

    /// 标记 task completion，并释放它实际持有的 inflight token。
    ///
    /// 返回 `true` 表示 task 本次确实处于 inflight 状态且已释放；重复 completion 或错误
    /// task id 返回 `false`，且不会释放其他 task 持有的 serial lane token。
    pub fn complete_task(&mut self, task: TaskId) -> bool {
        if !self.inflight_tasks.remove(&task) {
            return false;
        }
        if let Some(state) = self.tasks.get(&task) {
            if self.lane_kind(state.spec.lane) == LaneKind::Serial {
                self.inflight_serial_lanes.remove(&state.spec.lane);
            }
        }
        true
    }

    fn admit_task(&mut self, task: TaskId) -> Option<TaskAdmission> {
        if !self.can_admit(task) {
            return None;
        }
        let admission = self.admission_for(task)?;
        self.ready.remove(&task);
        self.pending.remove(&task);
        self.inflight_tasks.insert(task);
        if self.lane_kind(admission.lane) == LaneKind::Serial {
            self.inflight_serial_lanes.insert(admission.lane);
        }
        self.lane_last_dispatched_tick
            .insert(admission.lane, self.current_tick);
        Some(admission)
    }

    fn admission_for(&self, task: TaskId) -> Option<TaskAdmission> {
        let state = self.tasks.get(&task)?;
        let pending = self
            .pending
            .get(&task)
            .copied()
            .unwrap_or(PendingAdmission {
                scheduled_time_ms: self.now_ms,
                period_ms: None,
                missed_periods: 0,
            });
        Some(TaskAdmission {
            task,
            lane: state.spec.lane,
            scheduled_time_ms: pending.scheduled_time_ms,
            observed_time_ms: self.now_ms,
            period_ms: pending.period_ms,
            deadline_ms: state.deadline_ms,
            missed_periods: pending.missed_periods,
            lateness_ms: self.now_ms.saturating_sub(pending.scheduled_time_ms),
        })
    }

    fn can_admit(&self, task: TaskId) -> bool {
        if self.inflight_tasks.contains(&task) || self.suspended.contains(&task) {
            return false;
        }
        let Some(state) = self.tasks.get(&task) else {
            return false;
        };
        match self.lane_kind(state.spec.lane) {
            LaneKind::Serial => !self.inflight_serial_lanes.contains(&state.spec.lane),
            LaneKind::Parallel => true,
        }
    }

    fn lane_kind(&self, lane: LaneId) -> LaneKind {
        self.lanes.get(&lane).copied().unwrap_or(LaneKind::Serial)
    }

    fn compare_ready_tasks(&self, left: TaskId, right: TaskId) -> std::cmp::Ordering {
        let Some(left_state) = self.tasks.get(&left) else {
            return left.cmp(&right);
        };
        let Some(right_state) = self.tasks.get(&right) else {
            return left.cmp(&right);
        };
        left_state
            .spec
            .priority
            .cmp(&right_state.spec.priority)
            .then_with(|| left_state.spec.id.cmp(&right_state.spec.id))
    }

    /// 设置当前调度 tick 编号，用于 lane 饥饿检测。
    pub fn set_current_tick(&mut self, tick: u64) {
        self.current_tick = tick;
    }

    /// 返回当前调度 tick 编号。
    pub fn current_tick(&self) -> u64 {
        self.current_tick
    }

    /// 返回指定 lane 距离上次被调度已经过的 tick 数。
    ///
    /// 如果该 lane 从未被调度，返回 `u64::MAX`。
    pub fn lane_starvation_ticks(&self, lane: LaneId) -> u64 {
        match self.lane_last_dispatched_tick.get(&lane) {
            Some(last) => self.current_tick.saturating_sub(*last),
            None => u64::MAX,
        }
    }
}

impl ReadyBatch {
    fn from_admissions(admissions: Vec<TaskAdmission>) -> Self {
        let tasks = admissions
            .iter()
            .map(|admission| admission.task)
            .collect::<Vec<_>>();
        Self { tasks, admissions }
    }

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn tasks(&self) -> &[TaskId] {
        &self.tasks
    }

    /// 返回本批 admission metadata，顺序即 generated scheduler 应使用的 canonical commit 顺序。
    pub fn admissions(&self) -> &[TaskAdmission] {
        &self.admissions
    }
}

type BoxedStatusFuture = Pin<Box<dyn Future<Output = Status> + Send + 'static>>;

struct FutureInner {
    next_task: u64,
    admission_open: bool,
    futures: BTreeMap<TaskId, BoxedStatusFuture>,
    ready: BTreeSet<TaskId>,
    finished: BTreeMap<TaskId, Status>,
}

#[derive(Clone)]
pub struct FutureExecutor {
    inner: Arc<Mutex<FutureInner>>,
}

impl FutureExecutor {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(FutureInner {
                next_task: 1,
                admission_open: true,
                futures: BTreeMap::new(),
                ready: BTreeSet::new(),
                finished: BTreeMap::new(),
            })),
        }
    }

    pub fn spawn<F>(&self, future: F) -> Option<FutureHandle>
    where
        F: Future<Output = Status> + Send + 'static,
    {
        let mut inner = lock_recover(&self.inner);
        if !inner.admission_open {
            return None;
        }
        let task = TaskId(inner.next_task);
        inner.next_task = inner.next_task.saturating_add(1);
        inner.futures.insert(task, Box::pin(future));
        inner.ready.insert(task);
        Some(FutureHandle(task))
    }

    pub fn close_admission(&self) {
        lock_recover(&self.inner).admission_open = false;
    }

    pub fn is_finished(&self, handle: FutureHandle) -> bool {
        lock_recover(&self.inner).finished.contains_key(&handle.0)
    }

    pub fn run_ready(&self) -> usize {
        let mut polled = 0;
        let ready = {
            let mut inner = lock_recover(&self.inner);
            std::mem::take(&mut inner.ready)
        };
        for task in ready {
            let mut future = {
                let mut inner = lock_recover(&self.inner);
                let Some(future) = inner.futures.remove(&task) else {
                    continue;
                };
                future
            };

            let waker = Waker::from(Arc::new(FutureWake {
                task,
                inner: Arc::clone(&self.inner),
            }));
            let mut context = TaskContext::from_waker(&waker);
            match future.as_mut().poll(&mut context) {
                Poll::Ready(status) => {
                    let mut inner = lock_recover(&self.inner);
                    inner.finished.insert(task, status);
                }
                Poll::Pending => {
                    let mut inner = lock_recover(&self.inner);
                    inner.futures.insert(task, future);
                }
            }
            polled += 1;
        }
        polled
    }
}

impl Default for FutureExecutor {
    fn default() -> Self {
        Self::new()
    }
}

struct FutureWake {
    task: TaskId,
    inner: Arc<Mutex<FutureInner>>,
}

impl Wake for FutureWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let mut inner = lock_recover(&self.inner);
        if !inner.finished.contains_key(&self.task) {
            inner.ready.insert(self.task);
        }
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_recover<'a, T>(condvar: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    match condvar.wait(guard) {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn wait_timeout_recover<'a, T>(
    condvar: &Condvar,
    guard: MutexGuard<'a, T>,
    timeout: Duration,
) -> (MutexGuard<'a, T>, bool) {
    match condvar.wait_timeout(guard, timeout) {
        Ok((guard, result)) => (guard, result.timed_out()),
        Err(poisoned) => {
            let (guard, result) = poisoned.into_inner();
            (guard, result.timed_out())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        task::{Context, Poll},
    };

    fn add_task(executor: &mut DeterministicExecutor, id: u64, lane: u64, priority: u32) {
        executor.add_task(TaskSpec {
            id: TaskId(id),
            lane: LaneId(lane),
            priority,
        });
    }

    fn admitted_tasks(batch: &ReadyBatch) -> Vec<TaskId> {
        batch
            .admissions()
            .iter()
            .map(|admission| admission.task)
            .collect()
    }

    #[test]
    fn serial_lane_admits_one_task_by_priority_until_completion() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        add_task(&mut executor, 1, 1, 10);
        add_task(&mut executor, 2, 1, 1);
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        let first = executor.take_ready_batch();
        assert_eq!(admitted_tasks(&first), vec![TaskId(2)]);
        assert!(!executor.is_drained());
        assert!(executor.take_ready_batch().is_empty());

        assert!(executor.complete_task(TaskId(2)));
        let second = executor.take_ready_batch();
        assert_eq!(admitted_tasks(&second), vec![TaskId(1)]);
        assert!(executor.complete_task(TaskId(1)));
        assert!(executor.is_drained());
    }

    #[test]
    fn wrong_or_duplicate_completion_does_not_release_serial_lane() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        add_task(&mut executor, 1, 1, 0);
        add_task(&mut executor, 2, 1, 1);
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(1)]
        );
        assert!(!executor.complete_task(TaskId(2)));
        assert!(executor.take_ready_batch().is_empty());
        assert!(executor.complete_task(TaskId(1)));
        assert!(!executor.complete_task(TaskId(1)));
        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(2)]
        );
    }

    #[test]
    fn serial_lanes_admit_one_task_per_lane_in_same_batch() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_lane(LaneId(2), LaneKind::Serial);
        add_task(&mut executor, 1, 1, 0);
        add_task(&mut executor, 2, 1, 1);
        add_task(&mut executor, 3, 2, 99);
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));
        executor.wake(TaskId(3));

        let batch = executor.take_ready_batch();
        assert_eq!(admitted_tasks(&batch), vec![TaskId(1), TaskId(3)]);
        assert!(executor.complete_task(TaskId(1)));
        assert!(executor.complete_task(TaskId(3)));
        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(2)]
        );
    }

    #[test]
    fn parallel_lane_admits_all_ready_tasks_from_same_lane() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Parallel);
        add_task(&mut executor, 1, 1, 0);
        add_task(&mut executor, 2, 1, 1);
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(1), TaskId(2)]
        );
    }

    #[test]
    fn repeated_wake_is_coalesced_while_task_is_inflight() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        add_task(&mut executor, 1, 1, 0);
        executor.wake(TaskId(1));
        executor.wake(TaskId(1));

        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(1)]
        );
        executor.wake(TaskId(1));
        assert!(executor.take_ready_batch().is_empty());
        assert!(executor.complete_task(TaskId(1)));
        executor.wake(TaskId(1));
        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(1)]
        );
    }

    #[test]
    fn periodic_admission_records_scheduled_slot_and_lateness() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        add_task(&mut executor, 1, 1, 0);
        executor.set_task_deadline_ms(TaskId(1), Some(7));
        executor.add_periodic(PeriodicSpec {
            task: TaskId(1),
            period_ms: 10,
        });

        executor.advance_to_ms(35);

        let batch = executor.take_ready_batch();
        assert_eq!(batch.admissions().len(), 1);
        assert_eq!(
            batch.admissions()[0],
            TaskAdmission {
                task: TaskId(1),
                lane: LaneId(1),
                scheduled_time_ms: 30,
                observed_time_ms: 35,
                period_ms: Some(10),
                deadline_ms: Some(7),
                missed_periods: 2,
                lateness_ms: 5,
            }
        );
        assert_eq!(executor.next_deadline_ms(TaskId(1)), Some(40));
        assert_eq!(executor.missed_periods(TaskId(1)), 2);
    }

    #[test]
    fn periodic_due_is_coalesced_while_task_is_inflight() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        add_task(&mut executor, 1, 1, 0);
        executor.add_periodic(PeriodicSpec {
            task: TaskId(1),
            period_ms: 10,
        });

        executor.advance_to_ms(10);
        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(1)]
        );
        executor.advance_to_ms(35);
        assert!(executor.take_ready_batch().is_empty());
        assert_eq!(executor.missed_periods(TaskId(1)), 2);
        assert!(executor.complete_task(TaskId(1)));

        let batch = executor.take_ready_batch();
        assert_eq!(admitted_tasks(&batch), vec![TaskId(1)]);
        assert_eq!(batch.admissions()[0].scheduled_time_ms, 30);
        assert_eq!(batch.admissions()[0].missed_periods, 2);
    }

    #[test]
    fn suspend_task_skips_admission_until_resume() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Parallel);
        add_task(&mut executor, 1, 1, 0);
        add_task(&mut executor, 2, 1, 0);
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        // 隔离 task 1：本批只准入 task 2，且 task 1 不再被 wake 进 ready。
        executor.suspend_task(TaskId(1));
        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(2)]
        );
        assert!(executor.complete_task(TaskId(2)));
        executor.wake(TaskId(1));
        assert!(executor.take_ready_batch().is_empty());

        // 恢复后下次 wake 正常准入。
        executor.resume_task(TaskId(1));
        executor.wake(TaskId(1));
        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(1)]
        );
    }

    #[test]
    fn suspended_periodic_task_does_not_admit_on_advance() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        add_task(&mut executor, 1, 1, 0);
        executor.add_periodic(PeriodicSpec {
            task: TaskId(1),
            period_ms: 10,
        });
        executor.suspend_task(TaskId(1));

        executor.advance_to_ms(35);
        assert!(executor.take_ready_batch().is_empty());

        executor.resume_task(TaskId(1));
        executor.advance_to_ms(45);
        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(1)]
        );
    }

    #[test]
    fn shutdown_closes_new_admission_but_keeps_existing_inflight() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        add_task(&mut executor, 1, 1, 0);
        executor.wake(TaskId(1));
        assert_eq!(
            admitted_tasks(&executor.take_ready_batch()),
            vec![TaskId(1)]
        );
        executor.close_admission();
        executor.wake(TaskId(1));

        assert!(!executor.is_drained());
        assert!(executor.complete_task(TaskId(1)));
        assert!(executor.is_drained());
    }

    /// 退避公式：与生成 shell 的 `rust_backoff_expr` 同一契约
    /// `min(initial << consecutive.min(31), max)`，单位 clock-ms。
    fn restart_backoff_ms(initial_delay_ms: u64, max_delay_ms: u64, consecutive: u32) -> u64 {
        (initial_delay_ms << consecutive.min(31)).min(max_delay_ms)
    }

    /// 复刻生成 shell 的 restart driver，按 clock-ms 推进故障注入组件，记录每次重启尝试
    /// 发生的 clock-ms，并返回是否终态 Faulted。`attempt_ok(n)` 决定第 n 次重启是否成功。
    ///
    /// 该 driver 把 executor 的 `suspend_task`/`resume_task` 与 clock 驱动退避组合起来，
    /// 锁定生成 shell 容错时序的确定性边界：相同事件序列必产生相同重启 clock-ms。
    fn drive_restart_sequence(
        initial_delay_ms: u64,
        max_delay_ms: u64,
        max_restarts: u32,
        fault_at_ms: u64,
        horizon_ms: u64,
        attempt_ok: impl Fn(u32) -> bool,
    ) -> (Vec<u64>, bool) {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        add_task(&mut executor, 1, 1, 0);
        executor.add_periodic(PeriodicSpec {
            task: TaskId(1),
            period_ms: 10,
        });

        let mut consecutive = 0u32;
        let mut terminal = false;
        let mut next_restart_ms: Option<u64> = None;
        let mut attempts = 0u32;
        let mut restart_clock_ms = Vec::new();

        // 故障注入：fault_at_ms 时回调返 Error → 隔离 task 并安排首次重启。
        executor.advance_to_ms(fault_at_ms);
        executor.suspend_task(TaskId(1));
        if !terminal {
            next_restart_ms =
                Some(fault_at_ms + restart_backoff_ms(initial_delay_ms, max_delay_ms, consecutive));
        }

        // 按 clock-ms 单步推进，命中 next_restart_ms 即重跑生命周期。
        let mut now = fault_at_ms;
        while now <= horizon_ms {
            if let Some(due) = next_restart_ms {
                if now >= due {
                    next_restart_ms = None;
                    restart_clock_ms.push(now);
                    attempts += 1;
                    if attempt_ok(attempts) {
                        consecutive = 0;
                        executor.resume_task(TaskId(1));
                    } else {
                        consecutive += 1;
                        if consecutive >= max_restarts {
                            terminal = true;
                        } else {
                            next_restart_ms = Some(
                                now + restart_backoff_ms(
                                    initial_delay_ms,
                                    max_delay_ms,
                                    consecutive,
                                ),
                            );
                        }
                    }
                }
            }
            now += 1;
        }
        (restart_clock_ms, terminal)
    }

    #[test]
    fn restart_backoff_sequence_is_deterministic_until_terminal() {
        // initial=10, max=40, max_restarts=3，全程失败：退避 10/20/40 → 重启 clock-ms 10/30/70，
        // 第 3 次失败后终态 Faulted。
        let (seq, terminal) = drive_restart_sequence(10, 40, 3, 0, 200, |_| false);
        assert_eq!(seq, vec![10, 30, 70]);
        assert!(terminal);

        // 相同输入再跑一次：序列与终态完全一致（确定性回放边界）。
        let (replay, replay_terminal) = drive_restart_sequence(10, 40, 3, 0, 200, |_| false);
        assert_eq!(replay, seq);
        assert_eq!(replay_terminal, terminal);
    }

    #[test]
    fn restart_recovers_and_resets_consecutive_on_success() {
        // 前两次重启失败、第 3 次成功：重启 clock-ms 10/30/70，成功后 resume、不终态。
        let (seq, terminal) = drive_restart_sequence(10, 40, 5, 0, 200, |attempt| attempt == 3);
        assert_eq!(seq, vec![10, 30, 70]);
        assert!(!terminal);
    }

    #[test]
    fn restart_backoff_saturates_at_max_delay() {
        // 退避左移不得越过 max_delay_ms，且 consecutive 移位上限 31 防止 UB。
        assert_eq!(restart_backoff_ms(10, 40, 0), 10);
        assert_eq!(restart_backoff_ms(10, 40, 1), 20);
        assert_eq!(restart_backoff_ms(10, 40, 2), 40);
        assert_eq!(restart_backoff_ms(10, 40, 9), 40);
        assert_eq!(restart_backoff_ms(100, 1000, 60), 1000);
    }

    struct YieldOnce {
        yielded: bool,
    }

    impl Future for YieldOnce {
        type Output = Status;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.yielded {
                Poll::Ready(Status::Ok)
            } else {
                self.yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    #[test]
    fn future_executor_requeues_woken_future_without_tokio() {
        let executor = FutureExecutor::new();
        let handle = executor
            .spawn(YieldOnce { yielded: false })
            .expect("admission should be open");

        assert_eq!(executor.run_ready(), 1);
        assert!(!executor.is_finished(handle));
        assert_eq!(executor.run_ready(), 1);
        assert!(executor.is_finished(handle));
    }

    #[test]
    fn future_executor_close_admission_rejects_new_future() {
        let executor = FutureExecutor::new();
        executor.close_admission();

        assert!(executor.spawn(YieldOnce { yielded: false }).is_none());
    }

    #[test]
    fn worker_pool_runs_jobs_on_bounded_workers_and_joins_on_shutdown() {
        let pool = WorkerPool::new(2);
        let completed = Arc::new(AtomicUsize::new(0));
        for _ in 0..8 {
            let completed = Arc::clone(&completed);
            assert!(
                pool.spawn(move || {
                    completed.fetch_add(1, Ordering::SeqCst);
                    Status::Ok
                })
                .is_some()
            );
        }

        assert_eq!(pool.worker_threads(), 2);
        pool.shutdown();

        assert_eq!(completed.load(Ordering::SeqCst), 8);
        assert!(pool.spawn(|| Status::Ok).is_none());
    }

    #[test]
    fn worker_pool_reports_error_but_keeps_draining_ready_work() {
        let pool = WorkerPool::new(2);
        assert!(pool.spawn(|| Status::Ok).is_some());
        assert!(pool.spawn(|| Status::Error).is_some());
        assert!(pool.spawn(|| Status::Ok).is_some());

        let status = pool.shutdown();

        assert_eq!(status, Status::Error);
    }

    #[test]
    fn worker_pool_submit_collect_returns_before_completion_and_drains_outputs() {
        let pool = WorkerPool::new(1);
        let completions = WorkerCompletionQueue::new();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let submitted = pool.submit_collect(TaskId(7), &completions, move || {
            let _ = entered_tx.send(());
            match release_rx.recv() {
                Ok(()) => TaskRunOutcome::ok(vec![7_u64]),
                Err(_) => TaskRunOutcome::error(Vec::new()),
            }
        });

        assert_eq!(submitted, Ok(()));
        assert_eq!(entered_rx.recv_timeout(Duration::from_millis(200)), Ok(()));
        assert!(completions.try_drain_completed().is_empty());
        assert_eq!(release_tx.send(()), Ok(()));
        assert_eq!(pool.drain(), Status::Ok);
        assert_eq!(
            completions.drain_completed(),
            vec![TaskRunOutput {
                task: TaskId(7),
                status: Status::Ok,
                outputs: Some(vec![7_u64]),
            }]
        );
        assert!(completions.drain_completed().is_empty());
        assert_eq!(pool.shutdown(), Status::Ok);
    }

    #[test]
    fn worker_pool_submit_collect_converts_panic_to_error_without_outputs() {
        let pool = WorkerPool::new(1);
        let completions = WorkerCompletionQueue::<Vec<u64>>::new();

        assert_eq!(
            pool.submit_collect(TaskId(9), &completions, || {
                std::panic::panic_any("completion job failed");
            }),
            Ok(())
        );

        assert_eq!(pool.drain(), Status::Error);
        assert_eq!(
            completions.drain_completed(),
            vec![TaskRunOutput::<Vec<u64>>::without_outputs(
                TaskId(9),
                Status::Error,
            )]
        );
        assert_eq!(pool.shutdown(), Status::Error);
    }

    #[test]
    fn worker_pool_shutdown_preserves_completed_queue_and_rejects_submit() {
        let pool = WorkerPool::new(1);
        let completions = WorkerCompletionQueue::new();

        assert_eq!(
            pool.submit_collect(TaskId(10), &completions, || TaskRunOutcome::ok(vec![
                10_u64
            ])),
            Ok(())
        );
        assert_eq!(pool.shutdown(), Status::Ok);
        assert_eq!(
            completions.drain_completed(),
            vec![TaskRunOutput {
                task: TaskId(10),
                status: Status::Ok,
                outputs: Some(vec![10_u64]),
            }]
        );
        assert_eq!(
            pool.submit_collect(TaskId(11), &completions, || TaskRunOutcome::ok(Vec::new())),
            Err(WorkerSubmitError { task: TaskId(11) })
        );
    }

    #[test]
    fn worker_pool_close_admission_rejects_new_jobs_but_drains_inflight() {
        let pool = WorkerPool::new(2);
        let release = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(AtomicUsize::new(0));
        assert!(
            pool.spawn({
                let release = Arc::clone(&release);
                let entered = Arc::clone(&entered);
                move || {
                    entered.fetch_add(1, Ordering::SeqCst);
                    while release.load(Ordering::SeqCst) == 0 {
                        thread::yield_now();
                    }
                    Status::Ok
                }
            })
            .is_some()
        );
        while entered.load(Ordering::SeqCst) == 0 {
            thread::yield_now();
        }

        pool.close_admission();
        assert!(pool.spawn(|| Status::Ok).is_none());
        release.store(1, Ordering::SeqCst);

        assert_eq!(pool.drain(), Status::Ok);
        assert_eq!(pool.shutdown(), Status::Ok);
    }

    #[test]
    fn worker_pool_shutdown_with_empty_queue_returns_ok() {
        let pool = WorkerPool::new(4);
        assert_eq!(pool.shutdown(), Status::Ok);
        assert!(pool.spawn(|| Status::Ok).is_none());
    }

    #[test]
    fn worker_pool_job_panic_marks_error_and_releases_active_slot() {
        let mut pool = std::mem::ManuallyDrop::new(WorkerPool::new(1));
        let attempted = Arc::new(AtomicUsize::new(0));
        let attempted_for_job = Arc::clone(&attempted);
        assert!(
            pool.spawn(move || -> Status {
                attempted_for_job.fetch_add(1, Ordering::SeqCst);
                panic!("worker job panicked");
            })
            .is_some()
        );

        for _ in 0..100 {
            let drained = {
                let state = lock_recover(&pool.inner.state);
                attempted.load(Ordering::SeqCst) == 1 && state.queue.is_empty() && state.active == 0
            };
            if drained {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        let state = lock_recover(&pool.inner.state);
        assert_eq!(state.active, 0);
        assert_eq!(state.status, Status::Error);
        drop(state);
        let status = pool.shutdown();
        assert_eq!(status, Status::Error);
        // SAFETY: shutdown 已经 join worker，显式 drop 只释放容器字段。
        unsafe {
            std::mem::ManuallyDrop::drop(&mut pool);
        }
    }

    #[test]
    fn schedule_waiter_wakes_on_data_before_timer_deadline() {
        let waiter = ScheduleWaiter::new();
        let notifier = waiter.clone();
        let started = Arc::new(AtomicUsize::new(0));
        let started_for_thread = Arc::clone(&started);
        thread::spawn(move || {
            started_for_thread.store(1, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(10));
            notifier.notify_data();
        });
        while started.load(Ordering::SeqCst) == 0 {
            thread::yield_now();
        }

        let event = waiter.wait_until(
            Some(Instant::now() + Duration::from_secs(5)),
            &crate::ShutdownToken::new_for_test(),
        );

        assert_eq!(event, ScheduleEvent::Data);
    }

    #[test]
    fn schedule_waiter_wait_until_after_uses_generation_barrier() {
        let waiter = ScheduleWaiter::new();
        waiter.notify_data();
        let seen = waiter.data_generation();

        let event = waiter.wait_until_after(
            seen,
            Some(Instant::now() + Duration::from_millis(1)),
            &crate::ShutdownToken::new_for_test(),
        );

        assert_eq!(event, ScheduleEvent::Timer);
    }

    #[test]
    fn schedule_waiter_carries_latest_data_time_for_replay_clock() {
        let waiter = ScheduleWaiter::new();

        waiter.notify_data_at_ms(42);
        waiter.notify_data_at_ms(7);

        assert_eq!(waiter.take_data_time_ms(), Some(42));
        assert_eq!(waiter.take_data_time_ms(), None);
    }

    #[test]
    fn schedule_waiter_reports_timer_deadline() {
        let waiter = ScheduleWaiter::new();

        let event = waiter.wait_until(
            Some(Instant::now() + Duration::from_millis(1)),
            &crate::ShutdownToken::new_for_test(),
        );

        assert_eq!(event, ScheduleEvent::Timer);
    }
}
