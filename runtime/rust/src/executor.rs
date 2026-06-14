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
}

#[derive(Debug, Clone)]
struct PeriodicState {
    period_ms: u64,
    next_deadline_ms: u64,
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
    periodic: BTreeMap<TaskId, PeriodicState>,
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
    ready: Condvar,
}

/// worker 线程完成后交还 scheduler 线程的 typed completion queue。
///
/// `drain_completed` 和 `try_drain_completed` 只取回当前已完成结果，空队列立即返回；
/// `wait_completed` 供同步兼容封装在不忙等的情况下等待下一条 completion。
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
                ready: Condvar::new(),
            }),
        }
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

    /// 等待并取回下一条 completion，供同步兼容封装使用。
    pub fn wait_completed(&self) -> TaskRunOutput<O> {
        let mut completed = lock_recover(&self.inner.completed);
        loop {
            if let Some(result) = completed.pop_front() {
                return result;
            }
            completed = wait_recover(&self.inner.ready, completed);
        }
    }

    fn push(&self, result: TaskRunOutput<O>) {
        let mut completed = lock_recover(&self.inner.completed);
        completed.push_back(result);
        self.inner.ready.notify_all();
    }
}

struct TaskBatchState {
    remaining: usize,
    results: Vec<Option<TaskRunResult>>,
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
/// 当前 generated shell 仍同步调用用户回调。该 pool 只提供固定 worker 数、关闭准入、
/// drain/join 和首个错误聚合，用来让 Rust runtime 与 C++ runtime 先共享同一执行边界形状，
/// 但不把线程模型暴露给普通组件实现。
pub struct WorkerPool {
    worker_threads: usize,
    inner: Arc<WorkerPoolInner>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyBatch {
    tasks: Vec<TaskId>,
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
            periodic: BTreeMap::new(),
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
        self.tasks.insert(spec.id, TaskState { spec });
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
        if self.admission_open && self.tasks.contains_key(&task) {
            self.ready.insert(task);
        }
    }

    pub fn close_admission(&mut self) {
        self.admission_open = false;
    }

    pub fn is_drained(&self) -> bool {
        self.ready.is_empty()
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
            state.missed_periods = state.missed_periods.saturating_add(missed);
            state.next_deadline_ms = state
                .next_deadline_ms
                .saturating_add((missed + 1).saturating_mul(state.period_ms));
            due.push(*task);
        }
        for task in due {
            self.wake(task);
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

    pub fn run_ready<R>(&mut self, mut run: impl FnMut(TaskId) -> R) -> Vec<R> {
        let mut output = Vec::new();
        for task in self.ready_order() {
            self.ready.remove(&task);
            if let Some(state) = self.tasks.get(&task) {
                self.lane_last_dispatched_tick
                    .insert(state.spec.lane, self.current_tick);
            }
            output.push(run(task));
        }
        output
    }

    pub fn take_ready_batch(&mut self) -> ReadyBatch {
        let tasks = self.ready_parallel_order();
        for task in &tasks {
            self.ready.remove(task);
            if let Some(state) = self.tasks.get(task) {
                self.lane_last_dispatched_tick
                    .insert(state.spec.lane, self.current_tick);
            }
        }
        ReadyBatch { tasks }
    }

    pub fn run_ready_parallel(
        &mut self,
        worker_pool: &WorkerPool,
        run: impl Fn(TaskId) -> Status + Send + Sync + 'static,
    ) -> Vec<TaskRunResult> {
        self.take_ready_batch().run(worker_pool, run)
    }

    fn ready_order(&self) -> Vec<TaskId> {
        let mut by_lane = BTreeMap::<LaneId, Vec<TaskId>>::new();
        for task in &self.ready {
            let spec = &self.tasks[task].spec;
            by_lane.entry(spec.lane).or_default().push(*task);
        }
        for lane_tasks in by_lane.values_mut() {
            lane_tasks.sort_by(|left, right| {
                let left_spec = &self.tasks[left].spec;
                let right_spec = &self.tasks[right].spec;
                left_spec
                    .priority
                    .cmp(&right_spec.priority)
                    .then_with(|| left_spec.id.cmp(&right_spec.id))
            });
        }

        let mut tasks = Vec::with_capacity(self.ready.len());
        while by_lane.values().any(|lane_tasks| !lane_tasks.is_empty()) {
            for lane_tasks in by_lane.values_mut() {
                if !lane_tasks.is_empty() {
                    tasks.push(lane_tasks.remove(0));
                }
            }
        }
        tasks
    }

    fn ready_parallel_order(&self) -> Vec<TaskId> {
        let mut by_lane = BTreeMap::<LaneId, Vec<TaskId>>::new();
        for task in &self.ready {
            let spec = &self.tasks[task].spec;
            by_lane.entry(spec.lane).or_default().push(*task);
        }
        for lane_tasks in by_lane.values_mut() {
            lane_tasks.sort_by(|left, right| {
                let left_spec = &self.tasks[left].spec;
                let right_spec = &self.tasks[right].spec;
                left_spec
                    .priority
                    .cmp(&right_spec.priority)
                    .then_with(|| left_spec.id.cmp(&right_spec.id))
            });
        }

        by_lane
            .into_values()
            .filter_map(|lane_tasks| lane_tasks.into_iter().next())
            .collect()
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
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    pub fn tasks(&self) -> &[TaskId] {
        &self.tasks
    }

    pub fn run(
        self,
        worker_pool: &WorkerPool,
        run: impl Fn(TaskId) -> Status + Send + Sync + 'static,
    ) -> Vec<TaskRunResult> {
        if self.tasks.is_empty() {
            return Vec::new();
        }

        let task_count = self.tasks.len();
        let state = Arc::new((
            Mutex::new(TaskBatchState {
                remaining: task_count,
                results: vec![None; task_count],
            }),
            Condvar::new(),
        ));
        let run = Arc::new(run);
        let mut fallback = Vec::new();

        for (index, task) in self.tasks.into_iter().enumerate() {
            let batch_state = Arc::clone(&state);
            let run_fn = Arc::clone(&run);
            let submitted = worker_pool.spawn_with_completion(
                move || run_fn(task),
                move |status| {
                    let (mutex, ready) = &*batch_state;
                    let mut state = lock_recover(mutex);
                    state.results[index] = Some(TaskRunResult { task, status });
                    state.remaining = state.remaining.saturating_sub(1);
                    if state.remaining == 0 {
                        ready.notify_all();
                    }
                },
            );

            if submitted.is_none() {
                fallback.push((index, task));
            }
        }

        if !fallback.is_empty() {
            let (mutex, ready) = &*state;
            let mut batch_state = lock_recover(mutex);
            for (index, task) in fallback {
                let status = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(task)))
                    .unwrap_or(Status::Error);
                batch_state.results[index] = Some(TaskRunResult { task, status });
                batch_state.remaining = batch_state.remaining.saturating_sub(1);
            }
            if batch_state.remaining == 0 {
                ready.notify_all();
            }
        }

        let (mutex, ready) = &*state;
        let mut batch_state = lock_recover(mutex);
        while batch_state.remaining != 0 {
            batch_state = wait_recover(ready, batch_state);
        }

        batch_state
            .results
            .iter()
            .map(|result| result.expect("ready batch result should be recorded"))
            .collect()
    }

    /// 通过 worker pool 执行 ready batch，并收集可提交输出。
    ///
    /// 该路径会把输出集合从 worker 线程交还 scheduler 线程，因此要求 `O: Send`。非
    /// `Ok`、panic 或 worker completion 异常路径会记录状态但丢弃输出。
    pub fn run_collect<O>(
        self,
        worker_pool: &WorkerPool,
        run: impl Fn(TaskId) -> TaskRunOutcome<O> + Send + Sync + 'static,
    ) -> Vec<TaskRunOutput<O>>
    where
        O: Send + 'static,
    {
        let tasks = self.tasks;
        if tasks.is_empty() {
            return Vec::new();
        }

        let task_count = tasks.len();
        let completions = WorkerCompletionQueue::new();
        let run = Arc::new(run);
        let mut task_indexes = BTreeMap::new();

        for (index, task) in tasks.iter().copied().enumerate() {
            task_indexes.insert(task, index);
            let run_fn = Arc::clone(&run);
            if worker_pool
                .submit_collect(task, &completions, move || run_fn(task))
                .is_ok()
            {
                continue;
            }

            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(task)))
                .map(|outcome| TaskRunOutput::from_outcome(task, outcome))
                .unwrap_or_else(|_| TaskRunOutput::without_outputs(task, Status::Error));
            completions.push(result);
        }

        let mut ordered = std::iter::repeat_with(|| None)
            .take(task_count)
            .collect::<Vec<_>>();
        let mut recorded = 0;
        while recorded < task_count {
            let completion = completions.wait_completed();
            if let Some(index) = task_indexes.get(&completion.task).copied() {
                if ordered[index].is_none() {
                    ordered[index] = Some(completion);
                    recorded += 1;
                }
            }
        }

        ordered
            .into_iter()
            .enumerate()
            .map(|(index, result)| match result {
                Some(result) => result,
                None => TaskRunOutput::without_outputs(tasks[index], Status::Error),
            })
            .collect()
    }

    /// 在当前 scheduler 线程内执行 ready batch。
    ///
    /// Rust 侧部分 backend（目前主要是 iox2）持有 thread-affine SDK handle，
    /// 不能被安全移动到 worker 线程。generated shell 在发现 App 状态包含这类
    /// backend 时应选择该路径；纯 `Send + Sync` graph 仍可使用 `run` 进入
    /// `WorkerPool`。
    pub fn run_local(self, mut run: impl FnMut(TaskId) -> Status) -> Vec<TaskRunResult> {
        self.tasks
            .into_iter()
            .map(|task| {
                let status = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(task)))
                    .unwrap_or(Status::Error);
                TaskRunResult { task, status }
            })
            .collect()
    }

    /// 在当前 scheduler 线程内执行 ready batch，并收集可提交输出。
    ///
    /// 该路径不要求输出集合实现 `Send`，适用于 thread-affine backend 或只在 scheduler
    /// 线程内完成 commit 的 generated shell。非 `Ok` 和 panic 路径会丢弃输出。
    pub fn run_local_collect<O>(
        self,
        mut run: impl FnMut(TaskId) -> TaskRunOutcome<O>,
    ) -> Vec<TaskRunOutput<O>> {
        self.tasks
            .into_iter()
            .map(|task| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(task)))
                    .map(|outcome| TaskRunOutput::from_outcome(task, outcome))
                    .unwrap_or_else(|_| TaskRunOutput::without_outputs(task, Status::Error))
            })
            .collect()
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
        cell::Cell,
        future::Future,
        pin::Pin,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        task::{Context, Poll},
    };

    fn update_max(target: &AtomicUsize, value: usize) {
        let mut observed = target.load(Ordering::SeqCst);
        while observed < value {
            match target.compare_exchange(observed, value, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(actual) => observed = actual,
            }
        }
    }

    #[test]
    fn serial_lane_runs_one_task_at_a_time_in_priority_order() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 10,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(1),
            priority: 1,
        });

        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        assert_eq!(executor.run_ready(|task| task), vec![TaskId(2), TaskId(1)]);
    }

    #[test]
    fn ready_order_is_fair_across_lanes_then_priority_within_lane() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_lane(LaneId(2), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(1),
            priority: 1,
        });
        executor.add_task(TaskSpec {
            id: TaskId(3),
            lane: LaneId(2),
            priority: 99,
        });

        executor.wake(TaskId(1));
        executor.wake(TaskId(2));
        executor.wake(TaskId(3));

        assert_eq!(
            executor.run_ready(|task| task),
            vec![TaskId(1), TaskId(3), TaskId(2)]
        );
    }

    #[test]
    fn repeated_wake_is_coalesced_until_task_runs() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });

        executor.wake(TaskId(1));
        executor.wake(TaskId(1));
        executor.wake(TaskId(1));

        assert_eq!(executor.run_ready(|task| task), vec![TaskId(1)]);
    }

    #[test]
    fn periodic_task_skips_missed_periods_without_catch_up_storm() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_periodic(PeriodicSpec {
            task: TaskId(1),
            period_ms: 10,
        });

        executor.advance_to_ms(35);

        assert_eq!(executor.run_ready(|task| task), vec![TaskId(1)]);
        assert_eq!(executor.next_deadline_ms(TaskId(1)), Some(40));
        assert_eq!(executor.missed_periods(TaskId(1)), 2);
    }

    #[test]
    fn shutdown_closes_admission_but_allows_inflight_to_finish() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.wake(TaskId(1));
        executor.close_admission();
        executor.wake(TaskId(1));

        assert_eq!(executor.run_ready(|_| Status::Ok), vec![Status::Ok]);
        assert!(executor.is_drained());
    }

    #[test]
    fn parallel_ready_batch_overlaps_across_lanes() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_lane(LaneId(2), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(2),
            priority: 0,
        });
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        let worker_pool = WorkerPool::new(2);
        let started = Arc::new(AtomicUsize::new(0));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let results = executor.run_ready_parallel(&worker_pool, {
            let started = Arc::clone(&started);
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            move |task| {
                let current_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                update_max(&max_active, current_active);
                started.fetch_add(1, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_millis(200);
                while started.load(Ordering::SeqCst) < 2 {
                    if Instant::now() >= deadline {
                        active.fetch_sub(1, Ordering::SeqCst);
                        return if task == TaskId(1) {
                            Status::Error
                        } else {
                            Status::Retry
                        };
                    }
                    thread::yield_now();
                }
                thread::sleep(Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
                Status::Ok
            }
        });

        assert_eq!(
            results,
            vec![
                TaskRunResult {
                    task: TaskId(1),
                    status: Status::Ok,
                },
                TaskRunResult {
                    task: TaskId(2),
                    status: Status::Ok,
                }
            ]
        );
        assert!(max_active.load(Ordering::SeqCst) >= 2);
        assert_eq!(worker_pool.shutdown(), Status::Ok);
    }

    #[test]
    fn local_ready_batch_runs_without_worker_send_bounds() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_lane(LaneId(2), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(2),
            priority: 0,
        });
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        let mut seen = Vec::new();
        let results = executor.take_ready_batch().run_local(|task| {
            seen.push(task);
            Status::Ok
        });

        assert_eq!(seen, vec![TaskId(1), TaskId(2)]);
        assert_eq!(
            results,
            vec![
                TaskRunResult {
                    task: TaskId(1),
                    status: Status::Ok,
                },
                TaskRunResult {
                    task: TaskId(2),
                    status: Status::Ok,
                }
            ]
        );
    }

    #[test]
    fn local_ready_batch_collects_outputs_without_send_bound_and_discards_non_ok() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_lane(LaneId(2), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(2),
            priority: 0,
        });
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        let marker = Rc::new(Cell::new(0u32));
        let results = executor.take_ready_batch().run_local_collect({
            let marker = Rc::clone(&marker);
            move |task| {
                marker.set(marker.get() + 1);
                if task == TaskId(1) {
                    TaskRunOutcome::ok(Rc::clone(&marker))
                } else {
                    TaskRunOutcome::retry(Rc::clone(&marker))
                }
            }
        });

        assert_eq!(marker.get(), 2);
        assert_eq!(results[0].task, TaskId(1));
        assert_eq!(results[0].status, Status::Ok);
        assert!(results[0].outputs.is_some());
        assert_eq!(results[1].task, TaskId(2));
        assert_eq!(results[1].status, Status::Retry);
        assert!(results[1].outputs.is_none());
    }

    #[test]
    fn worker_ready_batch_collects_outputs_and_keeps_result_order() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_lane(LaneId(2), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(2),
            priority: 0,
        });
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        let worker_pool = WorkerPool::new(2);
        let results = executor
            .take_ready_batch()
            .run_collect(&worker_pool, |task| {
                let mut output = crate::Output::new();
                output.write(task.0);
                let record = output
                    .take_commit_record(task, format!("task.{}", task.0), "value", 120, 100)
                    .expect("task wrote output");
                TaskRunOutcome::ok(vec![record])
            });

        assert_eq!(results[0].task, TaskId(1));
        assert_eq!(results[0].status, Status::Ok);
        assert_eq!(
            results[0]
                .outputs
                .as_ref()
                .map(|records| records[0].payload),
            Some(1)
        );
        assert_eq!(results[1].task, TaskId(2));
        assert_eq!(results[1].status, Status::Ok);
        assert_eq!(
            results[1]
                .outputs
                .as_ref()
                .map(|records| records[0].payload),
            Some(2)
        );
        assert_eq!(worker_pool.shutdown(), Status::Ok);
    }

    #[test]
    fn parallel_ready_batch_keeps_same_lane_serial() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(1),
            priority: 1,
        });
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        let worker_pool = WorkerPool::new(2);
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let executed = Arc::new(Mutex::new(Vec::new()));
        let run = {
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            let executed = Arc::clone(&executed);
            move |task| {
                let current_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                update_max(&max_active, current_active);
                thread::sleep(Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
                lock_recover(&executed).push(task);
                Status::Ok
            }
        };

        let first = executor.run_ready_parallel(&worker_pool, run);
        assert_eq!(
            first,
            vec![TaskRunResult {
                task: TaskId(1),
                status: Status::Ok,
            }]
        );
        let second = executor.run_ready_parallel(&worker_pool, {
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            let executed = Arc::clone(&executed);
            move |task| {
                let current_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                update_max(&max_active, current_active);
                thread::sleep(Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
                lock_recover(&executed).push(task);
                Status::Ok
            }
        });
        assert_eq!(
            second,
            vec![TaskRunResult {
                task: TaskId(2),
                status: Status::Ok,
            }]
        );
        assert_eq!(*lock_recover(&executed), vec![TaskId(1), TaskId(2)]);
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
        assert_eq!(worker_pool.shutdown(), Status::Ok);
    }

    #[test]
    fn parallel_ready_batch_degrades_to_serial_with_single_worker() {
        let mut executor = DeterministicExecutor::new(1);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_lane(LaneId(2), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(2),
            priority: 0,
        });
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        let worker_pool = WorkerPool::new(1);
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let results = executor.run_ready_parallel(&worker_pool, {
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            move |task| {
                let current_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                update_max(&max_active, current_active);
                thread::sleep(Duration::from_millis(10));
                active.fetch_sub(1, Ordering::SeqCst);
                if task == TaskId(1) {
                    Status::Ok
                } else {
                    Status::Retry
                }
            }
        });

        assert_eq!(
            results,
            vec![
                TaskRunResult {
                    task: TaskId(1),
                    status: Status::Ok,
                },
                TaskRunResult {
                    task: TaskId(2),
                    status: Status::Retry,
                }
            ]
        );
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
        assert_eq!(worker_pool.shutdown(), Status::Ok);
    }

    #[test]
    fn parallel_ready_batch_converts_panic_to_error_and_shutdown_still_drains() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_lane(LaneId(2), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(2),
            priority: 0,
        });
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        let worker_pool = WorkerPool::new(2);
        let results = executor.run_ready_parallel(&worker_pool, |task| {
            if task == TaskId(1) {
                panic!("parallel task panicked");
            }
            Status::Ok
        });

        assert_eq!(
            results,
            vec![
                TaskRunResult {
                    task: TaskId(1),
                    status: Status::Error,
                },
                TaskRunResult {
                    task: TaskId(2),
                    status: Status::Ok,
                }
            ]
        );
        assert_eq!(worker_pool.shutdown(), Status::Error);
    }

    #[test]
    fn worker_ready_batch_discards_output_when_task_panics() {
        let mut executor = DeterministicExecutor::new(2);
        executor.add_lane(LaneId(1), LaneKind::Serial);
        executor.add_lane(LaneId(2), LaneKind::Serial);
        executor.add_task(TaskSpec {
            id: TaskId(1),
            lane: LaneId(1),
            priority: 0,
        });
        executor.add_task(TaskSpec {
            id: TaskId(2),
            lane: LaneId(2),
            priority: 0,
        });
        executor.wake(TaskId(1));
        executor.wake(TaskId(2));

        let worker_pool = WorkerPool::new(2);
        let results = executor
            .take_ready_batch()
            .run_collect(&worker_pool, |task| {
                let mut output = crate::Output::new();
                output.write(task.0);
                if task == TaskId(1) {
                    panic!("task panicked after writing output");
                }
                let record = output
                    .take_commit_record(task, format!("task.{}", task.0), "value", 120, 100)
                    .expect("task wrote output");
                TaskRunOutcome::ok(vec![record])
            });

        assert_eq!(results[0].task, TaskId(1));
        assert_eq!(results[0].status, Status::Error);
        assert!(results[0].outputs.is_none());
        assert_eq!(results[1].task, TaskId(2));
        assert_eq!(results[1].status, Status::Ok);
        assert_eq!(
            results[1]
                .outputs
                .as_ref()
                .map(|records| records[0].payload),
            Some(2)
        );
        assert_eq!(worker_pool.shutdown(), Status::Error);
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
